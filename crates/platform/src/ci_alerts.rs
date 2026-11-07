//! CI alerts as native issues (native #774): chant's tick posts a
//! project's `chant.notify/v1` CI events to `POST /ci/notify/{project}`,
//! and they open, comment on and close one "main is red" issue per branch.
//!
//! **The key.** The platform makes it (32 random bytes, hex) when a
//! project's owner or editor turns alerts on, shows it once
//! ([`CiAlertsKey`]) and seals it in `project_secrets` under
//! [`KEY_NAME`], as a Plane connection's webhook secret is: it has to be
//! read back to check a signature, so it can't be a hash. Making a new one
//! (rotating) replaces it at once. Turning alerts off removes the key and
//! only the key: their people and open issues carry on when a new key is
//! made.
//!
//! **A delivery** is chant's (`@intentius/chant/notify`): the body (at most
//! [`BODY_MAX`]), and
//! - `x-chant-signature: sha256=<hex>`, HMAC-SHA256 with the key over
//!   `<x-chant-timestamp>.<body>`;
//! - `x-chant-timestamp`, seconds since the epoch: one more than
//!   [`TOLERANCE_S`] from now either way is refused, as chant's own
//!   `verifyWebhook` refuses it;
//! - `x-chant-event` and `x-chant-delivery` (the event's `id`), which this
//!   reads from the body instead, since the signature covers only the body.
//!
//! A project that isn't there, has alerts off or has no key, and a bad
//! signature or timestamp, all get the same 401, so nobody learns which
//! projects have alerts on; the log says which. Each project has its own
//! key, so one project's key never posts to another's URL. Signed events
//! count against [`PER_PROJECT`]; the hub limits deliveries per client
//! address before they get here (its `limit::CI_NOTIFY`), as only it knows
//! the address. Only `ci.red`, `ci.culprit`, `ci.stale` and `ci.green` are
//! acted on; any other event (`waiting`, `ci.quarantine-passed`...) is a
//! 200 that does nothing.
//!
//! **Once.** An event is noted by its `id` in the GitHub webhook's table
//! (`ci:<project>:<id>`, kept 7 days) in the same transaction as its first
//! write (the issue opened, the comment, the close), so a retry or a
//! second machine finds it noted and writes nothing. What follows that
//! write (mentions and pushes, the green's comment) is best-effort and
//! logged: a retry never repeats it. A failure before the first write
//! leaves nothing noted, so chant's retry is taken.
//!
//! **One open issue per branch** ([`Db::ci_alert_open`]), held by a unique
//! index on the open rows, since the platform's machines take events side
//! by side: the one that loses the race to open it comments on the winner's
//! ([`Db::create_ci_issue`]).
//! - `ci.red` opens it, titled with the branch and the red commit, or
//!   comments on the open one;
//! - `ci.culprit` comments with the commit, and its author and pull
//!   request when the repo is linked to the project and the GitHub App can
//!   read it;
//! - `ci.stale` comments with how long the branch has been red;
//! - `ci.green` closes it as completed and comments with the green commit.
//!   A `ci.red` after that opens a new one.
//!
//! **Order** is by the event's `at`, when chant sent it: the platform has
//! no git history, so it can't ask which commit is the newer. The newest
//! green per branch is noted ([`Db::ci_alert_green`]). A red, culprit or
//! stale event from no later than it is late and changes nothing; a green
//! closes only an issue whose first red came before it. `ci.culprit` and
//! `ci.stale` only ever comment on an open issue, and never open one.
//!
//! **Who wrote it.** Not a person and not anyone's agent: the issue's
//! author is empty with [`CI_AUTHOR`] as its agent, its comments have no
//! author and that `via`, as an agent session's issues and GitHub's
//! comments are kept (aru-115, M205). Both are shown as `chant CI`
//! ([`ci_name`]), whatever the event's `source`. Only this endpoint writes
//! a `ci:` agent or `via`: a person's post can't set `via`, and a
//! product's (`boxes::post`) always has its person as the author.
//!
//! **What it says.** Everything that comes from the event (the summary,
//! branch, phase, run's name and conclusion) or from GitHub (an author, a
//! pull request's title) is inert: a code span, its backticks handled,
//! with any `@` broken so it names nobody ([`inert`]). The only links are
//! the platform's own, built from validated parts on the project's linked
//! repo: a commit, a pull request, and a run on github.com under that repo.
//!
//! **Who hears it.** A new issue mentions the people the project's setting
//! names (and assigns them, with `assign`): the inbox's `mentioned` and
//! `assigned`, with their push. `ci.culprit`, `ci.stale` and `ci.green`
//! mention them again in the thread. Push preferences are the inbox's
//! ([`crate::inbox`]): quiet hours, muted threads and its limits apply.
//! Someone who left the project is dropped: when it's read, when it's
//! set, and when a team's roster changes ([`roster_changed`]).
//!
//! [`Db::ci_alert_open`]: crate::db::Db::ci_alert_open
//! [`Db::ci_alert_green`]: crate::db::Db::ci_alert_green
//! [`Db::create_ci_issue`]: crate::db::Db::create_ci_issue

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects::{CiAlertOpen, CiAlerts, CiAlertsKey, CiAlertsSettings, ci_name};
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::{
        ci_alerts::{self as rows, Alerts, Created, Green, KEY_NAME, Noted},
        issues::{self as idb, Actor, CloseReason, Issue, State as IssueState},
    },
    forge::{Caller, Purpose, Repo},
    issues::{Place, place},
    keys,
};

/// How far from now a delivery's timestamp may be, in seconds: chant's
/// `DEFAULT_TOLERANCE_SECONDS`.
pub const TOLERANCE_S: u64 = 300;

/// The events acted on.
pub const EVENTS: [&str; 4] = ["ci.red", "ci.culprit", "ci.stale", "ci.green"];

/// What the body's `schema` is.
pub const SCHEMA: &str = "chant.notify/v1";

/// The most a delivery's body may be: a `chant.notify/v1` event is a few
/// KiB at most.
pub const BODY_MAX: usize = 64 * 1024;

/// Signed events per project, per hour: a tick sends a few a day.
pub const PER_PROJECT: (&str, usize) = ("ci-notify-project", 600);

/// Most people a project's alerts name.
pub const PEOPLE_MAX: usize = 20;

/// The agent and `via` of everything CI alerts write.
pub const CI_AUTHOR: &str = "ci:chant";

/// The URL chant posts `project`'s events to.
pub fn url(app: &Platform, project: &str) -> String {
    format!("{}/ci/notify/{project}", app.cfg.public_url)
}

fn wire(app: &Platform, p: &Place, a: Option<&Alerts>) -> Result<CiAlerts, ApiError> {
    let project = &p.project.id;
    let mut open = Vec::new();
    for (branch, o) in app.db.ci_alert_opens(project)? {
        if let Some(i) = app.db.issue(&o.issue)? {
            open.push(CiAlertOpen { branch, issue: i.id, number: i.number, opened_at: o.opened_at });
        }
    }
    let on = a.filter(|a| a.on);
    Ok(CiAlerts {
        project: project.clone(),
        on: on.is_some(),
        url: url(app, project),
        // Only those in the project now.
        people: a.map(|a| a.people.iter().filter(|x| p.has(x)).cloned().collect()).unwrap_or_default(),
        assign: a.is_some_and(|a| a.assign),
        key_made_by: on.map(|a| a.key_made_by.clone()),
        key_made_at: on.and_then(|a| a.key_made_at),
        last_event_at: a.and_then(|a| a.last_event_at),
        open,
    })
}

/// `GET /api/projects/{project}/ci-alerts`: for its members. Never the key.
pub async fn show(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
) -> Result<Json<CiAlerts>, ApiError> {
    let p = place(&app, Actor::person(&account), &project, false).await?;
    Ok(Json(wire(&app, &p, app.db.ci_alerts(&p.project.id)?.as_ref())?))
}

/// `PATCH /api/projects/{project}/ci-alerts`: whom a new red issue
/// mentions (anyone not in the project is left out), whether it assigns
/// them, or off. An owner or editor.
pub async fn settings(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<CiAlertsSettings>,
) -> Result<Json<CiAlerts>, ApiError> {
    let p = place(&app, Actor::person(&account), &project, true).await?;
    let id = p.project.id.clone();
    if b.on.is_none() && b.people.is_none() && b.assign.is_none() {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "on, people or assign"));
    }
    match b.on {
        Some(false) => {
            keys::remove(&app.db, &id, KEY_NAME)?;
            app.db.ci_alerts_off(&id)?;
            info!(%account, project = %id, "turned CI alerts off");
        }
        Some(true) if !app.db.ci_alerts(&id)?.is_some_and(|a| a.on) => {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "CI alerts are turned on by making their key"));
        }
        _ => {}
    }
    let people = b.people.as_ref().map(|v| {
        let mut out: Vec<String> = Vec::new();
        for a in v.iter().map(|a| a.trim()).filter(|a| p.has(a)) {
            if !out.iter().any(|x| x == a) {
                out.push(a.to_owned());
            }
        }
        out
    });
    if people.as_ref().is_some_and(|v| v.len() > PEOPLE_MAX) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, &format!("at most {PEOPLE_MAX} people")));
    }
    if (people.is_some() || b.assign.is_some()) && !app.db.set_ci_alerts(&id, people.as_deref(), b.assign)? {
        return Err(err(StatusCode::NOT_FOUND, "CI alerts aren't on in this project"));
    }
    info!(%account, project = %id, people = ?people, assign = ?b.assign, "changed CI alerts");
    Ok(Json(wire(&app, &p, app.db.ci_alerts(&id)?.as_ref())?))
}

/// `POST /api/projects/{project}/ci-alerts/key`: alerts on with a new key,
/// or the key rotated. An owner or editor; the key is in this answer only.
pub async fn make_key(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<CiAlertsKey>, ApiError> {
    let p = place(&app, Actor::person(&account), &project, true).await?;
    let id = p.project.id.clone();
    let Some(master) = app.secret_key.as_deref() else {
        return Err(err(StatusCode::NOT_IMPLEMENTED, "this control can't keep a key (it has no key to seal it with)"));
    };
    let key = hex::encode(arugula_e2e::random::<32>());
    keys::seal(&app.db, master, &id, KEY_NAME, &key)?;
    app.db.ci_alerts_key_made(&id, &account, now_ms())?;
    info!(%account, project = %id, "made a CI alerts key");
    let alerts = wire(&app, &p, app.db.ci_alerts(&id)?.as_ref())?;
    Ok(Json(CiAlertsKey { key, alerts }))
}

/// A team's roster changed: whoever left it comes off its projects'
/// alerts. Logged, never failed.
pub async fn roster_changed(app: &Platform, team: &str) {
    let go = async {
        for project in app.db.ci_alert_team_projects(team)? {
            let members = crate::forge::inbox::members(app, &project).await?;
            let gone = app.db.ci_alert_keep_people(&project, &members)?;
            if gone > 0 {
                info!(%team, %project, gone, "took people who left off CI alerts");
            }
        }
        anyhow::Ok(())
    };
    if let Err(e) = go.await {
        warn!(%team, error = format!("{e:#}"), "CI alerts: pruning who left the team");
    }
}

// ---------------------------------------------------------------- the endpoint

/// The one answer for an unknown project, alerts off, and a bad signature
/// or timestamp.
fn refusal() -> Response {
    err(StatusCode::UNAUTHORIZED, "refused").into_response()
}

fn mac(key: &str, timestamp: &str, body: &[u8]) -> hmac::Hmac<sha2::Sha256> {
    use hmac::{KeyInit, Mac};
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes()).expect("hmac");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac
}

/// `sha256=<hex>` of `<timestamp>.<body>` with `key`, as chant's
/// `signWebhook` makes it.
pub fn sign(key: &str, timestamp: &str, body: &[u8]) -> String {
    use hmac::Mac;
    format!("sha256={}", hex::encode(mac(key, timestamp, body).finalize().into_bytes()))
}

/// Why a delivery isn't taken, or `None`: the signature over the timestamp
/// and the body (in constant time), then the timestamp within
/// [`TOLERANCE_S`] of `now_s`.
pub fn refused(
    key: &str,
    signature: Option<&str>,
    timestamp: Option<&str>,
    body: &[u8],
    now_s: u64,
) -> Option<&'static str> {
    use hmac::Mac;
    let Some(signature) = signature else { return Some("no x-chant-signature header") };
    let Some(ts) = timestamp.filter(|t| !t.is_empty() && t.len() <= 20 && t.bytes().all(|b| b.is_ascii_digit())) else {
        return Some("no x-chant-timestamp header, or not a number of seconds");
    };
    let Some(given) = signature.trim().strip_prefix("sha256=").and_then(|h| hex::decode(h).ok()) else {
        return Some("the signature does not match");
    };
    if mac(key, ts, body).verify_slice(&given).is_err() {
        return Some("the signature does not match");
    }
    let at: u64 = ts.parse().unwrap_or(0);
    if at.abs_diff(now_s) > TOLERANCE_S {
        return Some("the timestamp is more than 300s from now");
    }
    None
}

/// The project's key, while its alerts are on and it has one.
fn key_of(app: &Platform, project: &str) -> anyhow::Result<Option<String>> {
    if !app.db.ci_alerts(project)?.is_some_and(|a| a.on) {
        return Ok(None);
    }
    let Some(master) = app.secret_key.as_deref() else { return Ok(None) };
    keys::open(&app.db, master, project, KEY_NAME)
}

/// For the tests: fail what follows an event's first write, as a push or a
/// comment after it could.
#[cfg(any(test, feature = "testing"))]
pub static FAIL_AFTER_WRITE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `POST /ci/notify/{project}`: see the module docs.
pub async fn notify(
    State(app): State<Arc<Platform>>,
    Path(project): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    let key = match key_of(&app, &project) {
        Ok(Some(k)) => k,
        Ok(None) => {
            warn!(%project, "a CI alerts event for a project without them on");
            return refusal();
        }
        Err(e) => {
            warn!(%project, error = format!("{e:#}"), "checking a CI alerts webhook's key");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't check the webhook").into_response();
        }
    };
    let now = now_ms();
    if let Some(why) = refused(&key, h("x-chant-signature"), h("x-chant-timestamp"), &body, now / 1000) {
        warn!(%project, why, "a CI alerts event refused");
        return refusal();
    }
    if let Err(e) = app.limits.check_client(PER_PROJECT, &project) {
        warn!(%project, "CI alerts: too many events for this project");
        return e.into_response();
    }
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v @ Value::Object(_)) => v,
        _ => return err(StatusCode::BAD_REQUEST, "the body isn't a JSON object").into_response(),
    };
    if v["schema"] != SCHEMA {
        return err(StatusCode::BAD_REQUEST, "not a chant.notify/v1 event").into_response();
    }
    let (Some(id), Some(event)) = (v["id"].as_str().filter(|s| !s.is_empty()), v["event"].as_str()) else {
        return err(StatusCode::BAD_REQUEST, "an event needs its id and event").into_response();
    };
    let id: String = id.chars().take(128).collect();
    if let Err(e) = app.db.ci_alerts_heard(&project, now) {
        warn!(%project, error = %e, "noting a CI alerts event");
    }
    if !EVENTS.contains(&event) {
        info!(%project, event, "a CI alerts event ignored");
        return Json(json!({ "ignored": true })).into_response();
    }
    let Some(e) = Event::of(&v) else {
        return err(StatusCode::BAD_REQUEST, "a ci event needs its at, ci.branch and ci.commit").into_response();
    };
    let delivery = format!("ci:{project}:{id}");
    match app.db.delivery_event(&delivery) {
        Ok(Some(_)) => return Json(json!({ "duplicate": true })).into_response(),
        Ok(None) => {}
        Err(er) => {
            warn!(%project, event, error = %er, "reading a CI alerts event's delivery");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't check the event").into_response();
        }
    }
    let noted = Noted { delivery: &delivery, event, now };
    match apply(&app, &project, &e, noted).await {
        Ok(Did::Wrote(did, issue)) => {
            info!(%project, event, did, issue = %issue.id, "a CI alerts event taken");
            Json(json!({ "did": did, "issue": issue.id, "number": issue.number })).into_response()
        }
        Ok(Did::Nothing(why)) => {
            info!(%project, event, why, "a CI alerts event changed nothing");
            Json(json!({ "ignored": true, "why": why })).into_response()
        }
        Ok(Did::Applied) => Json(json!({ "duplicate": true })).into_response(),
        // Nothing was written, nor the event noted: chant's retry is taken.
        Err(er) => {
            warn!(%project, event, error = format!("{er:#}"), "a CI alerts event couldn't be applied");
            err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't apply the event").into_response()
        }
    }
}

// ---------------------------------------------------------------- what an event says

/// What a CI event says, from its body: the strings are cut short, and
/// only what's used is read.
#[derive(Debug, Clone)]
struct Event {
    event: String,
    /// When chant sent it (`at`), ms.
    at: u64,
    summary: String,
    branch: String,
    commit: String,
    phase: Option<String>,
    failed_run: Option<FailedRun>,
    culprit: Option<String>,
    red: Option<String>,
    since: Option<String>,
    last_green: Option<String>,
    hours: Option<u64>,
    /// `owner/name` on GitHub, as the event names it (checked against the
    /// project's linked repos before anything links to it).
    repo: Option<String>,
    pr: Option<u64>,
}

#[derive(Debug, Clone)]
struct FailedRun {
    name: String,
    conclusion: Option<String>,
    url: Option<String>,
}

fn text(v: &Value, max: usize) -> Option<String> {
    let s: String = v.as_str()?.chars().filter(|c| !c.is_control()).take(max).collect();
    let s = s.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// A commit's id: hex, 7 to 64 characters.
fn sha(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| (7..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_lowercase)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

impl Event {
    fn of(v: &Value) -> Option<Self> {
        let ci = &v["ci"];
        let at = v["at"].as_str()?.parse::<jiff::Timestamp>().ok()?.as_millisecond();
        let branch = text(&ci["branch"], 200)?;
        let commit = sha(&ci["commit"])?;
        let run = &v["run"];
        let repo = text(&run["repository"], 140).filter(|_| run["forge"].as_str().is_none_or(|f| f == "github"));
        let fr = &ci["failedRun"];
        let failed_run = text(&fr["name"], 200).map(|name| FailedRun {
            name,
            conclusion: text(&fr["conclusion"], 40),
            url: text(&fr["url"], 512),
        });
        Some(Self {
            event: v["event"].as_str().unwrap_or_default().to_owned(),
            at: u64::try_from(at).ok()?,
            summary: text(&v["summary"], 500).unwrap_or_default(),
            branch,
            commit,
            phase: text(&ci["phase"], 100),
            failed_run,
            culprit: sha(&ci["culprit"]),
            red: sha(&ci["red"]),
            since: sha(&ci["since"]),
            last_green: sha(&ci["lastGreen"]),
            hours: ci["hours"].as_u64(),
            repo,
            pr: run["pr"].as_u64(),
        })
    }

    /// The commit the branch went red at, as this event tells it.
    fn red_commit(&self) -> &str {
        match self.event.as_str() {
            "ci.stale" => self.since.as_deref().unwrap_or(&self.commit),
            "ci.culprit" => self.culprit.as_deref().unwrap_or(&self.commit),
            _ => &self.commit,
        }
    }

    /// The red issue's title (plain text): the branch and the commit it
    /// went red at.
    fn title(&self) -> String {
        format!("{} is red at {}", self.branch, short(self.red_commit()))
    }
}

/// Text from an event or from GitHub, inert in markdown: a code span (its
/// fence longer than any run of backticks in it, padded where it starts or
/// ends with one), so it can't link, show an image or emphasise; and every
/// `@` broken by a word joiner, so nothing names anyone.
pub fn inert(s: &str) -> String {
    let s = s.replace('@', "@\u{2060}");
    let (mut longest, mut run) = (0, 0);
    for c in s.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if s.is_empty() || s.starts_with('`') || s.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{s}{pad}{fence}")
}

/// What an event's words are said with: the project's linked repo it
/// names, if any, which is all its links are built on.
struct Say<'a> {
    e: &'a Event,
    /// `owner/name` as the platform has it linked to this project.
    repo: Option<String>,
}

impl<'a> Say<'a> {
    fn new(app: &Platform, project: &str, e: &'a Event) -> Self {
        let repo = e
            .repo
            .as_deref()
            .and_then(|r| app.db.linked_repo_named(r).ok().flatten())
            .filter(|l| l.project_id == project && l.unlinked_at.is_none())
            .map(|l| l.full_name);
        Self { e, repo }
    }

    /// A commit: a link on the linked repo, else its id.
    fn commit(&self, sha: &str) -> String {
        match &self.repo {
            Some(r) => format!("[{}](https://github.com/{r}/commit/{sha})", inert(short(sha))),
            None => inert(short(sha)),
        }
    }

    /// A run's page, only when it's under the linked repo on github.com:
    /// `https://github.com/<repo>/actions/runs/<n>`, with a `/job/<n>` or
    /// `/attempts/<n>` after it, rebuilt from the numbers.
    fn run_url(&self, url: &str) -> Option<String> {
        let repo = self.repo.as_deref()?;
        let rest = url.strip_prefix("https://github.com/")?;
        let (r, rest) = rest.split_at_checked(repo.len())?;
        if !r.eq_ignore_ascii_case(repo) {
            return None;
        }
        let parts: Vec<&str> = rest.strip_prefix("/actions/runs/")?.split('/').collect();
        let num = |s: &str| (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())).then_some(());
        num(parts[0])?;
        let tail = match parts.as_slice() {
            [_] => String::new(),
            [_, kind @ ("job" | "attempts"), n] => {
                num(n)?;
                format!("/{kind}/{n}")
            }
            _ => return None,
        };
        Some(format!("https://github.com/{repo}/actions/runs/{}{tail}", parts[0]))
    }

    /// A pull request on the linked repo.
    fn pr(&self, n: u64) -> String {
        match &self.repo {
            Some(r) => format!("[{r}#{n}](https://github.com/{r}/pull/{n})"),
            None => format!("pull request {n}"),
        }
    }

    /// `ci.red`: the branch, the commit, its phase and failed run.
    fn red(&self) -> Vec<String> {
        let e = self.e;
        let mut out = vec![format!("{} is red at {}.", inert(&e.branch), self.commit(&e.commit))];
        if let Some(p) = &e.phase {
            let mut line = format!("Phase {}", inert(p));
            if let Some(r) = &e.failed_run {
                line.push_str(&format!(": {}", inert(&r.name)));
                if let Some(c) = &r.conclusion {
                    line.push_str(&format!(" ({})", inert(c)));
                }
                if let Some(u) = r.url.as_deref().and_then(|u| self.run_url(u)) {
                    line.push_str(&format!(", [the failed run]({u})"));
                }
            }
            line.push('.');
            out.push(line);
        }
        out
    }

    /// The event's own summary, inert.
    fn summary(&self) -> Option<String> {
        (!self.e.summary.is_empty()).then(|| format!("chant says: {}", inert(&self.e.summary)))
    }

    /// `ci.stale`: how long the branch has been red.
    fn stale(&self) -> String {
        let e = self.e;
        let hours = match e.hours {
            Some(1) => "1 hour".to_owned(),
            Some(h) => format!("{h} hours"),
            None => "a while".to_owned(),
        };
        let mut s = format!("{} has had no green commit for {hours}", inert(&e.branch));
        if let Some(since) = &e.since {
            s.push_str(&format!(", since {}", self.commit(since)));
        }
        if let Some(g) = &e.last_green {
            s.push_str(&format!(" (the last green one is {})", self.commit(g)));
        }
        s.push_str(&format!(". Its newest commit is {}.", self.commit(&e.commit)));
        s
    }

    /// `ci.green`: the green commit, and the red it answers.
    fn green(&self) -> String {
        let e = self.e;
        let mut s = format!("{} is green again at {}.", inert(&e.branch), self.commit(&e.commit));
        if let Some(r) = &e.red {
            s.push_str(&format!(" That answers the red at {}.", self.commit(r)));
        }
        s
    }

    /// `ci.culprit`: the commit that broke the branch, its author and its
    /// pull request, as far as GitHub says.
    async fn culprit(&self, app: &Platform, project: &str) -> String {
        let e = self.e;
        let Some(culprit) = &e.culprit else {
            return format!("A bisect below {} named no commit.", self.commit(&e.commit));
        };
        let mut s =
            format!("The bisect names {} as the commit that turned {} red", self.commit(culprit), inert(&e.branch));
        let facts = match &self.repo {
            Some(repo) => match commit_facts(app, project, repo, culprit).await {
                Ok(f) => f,
                Err(er) => {
                    warn!(%project, repo, error = format!("{er:#}"), "CI alerts: couldn't read the culprit on GitHub");
                    Facts::default()
                }
            },
            None => Facts::default(),
        };
        if let Some(a) = &facts.author {
            s.push_str(&format!(", by {}", inert(a)));
        }
        match facts.pr.or(e.pr.map(|n| (n, String::new()))) {
            Some((n, title)) if title.is_empty() => s.push_str(&format!(", in {}", self.pr(n))),
            Some((n, title)) => s.push_str(&format!(", in {} ({})", self.pr(n), inert(&title))),
            None => {}
        }
        s.push('.');
        s
    }
}

// ---------------------------------------------------------------- applying it

/// What an event did.
enum Did {
    /// What, to which issue.
    Wrote(&'static str, Box<Issue>),
    /// Nothing, and why: noted as taken.
    Nothing(&'static str),
    /// It was applied already.
    Applied,
}

/// The open red issue of `branch`, if the row names one that's still open
/// in this project. One a person closed (or that's gone) stops being the
/// branch's here, so the next red opens another.
fn open_issue(app: &Platform, project: &str, branch: &str) -> anyhow::Result<Option<(rows::Open, Issue)>> {
    let Some(o) = app.db.ci_alert_open(project, branch)? else { return Ok(None) };
    match app.db.issue(&o.issue)? {
        Some(i) if i.project == project && i.state == IssueState::Open => Ok(Some((o, i))),
        _ => {
            app.db.ci_alert_closed(project, branch, &o.issue, now_ms())?;
            Ok(None)
        }
    }
}

/// Note the event as taken, with nothing else written.
fn nothing(app: &Platform, noted: Noted, why: &'static str) -> anyhow::Result<Did> {
    Ok(if app.db.first_delivery(noted.delivery, noted.event, noted.now)? { Did::Nothing(why) } else { Did::Applied })
}

/// What follows an event's first write: logged when it fails, never
/// failing the event, which is noted already.
fn after(project: &str, what: &str, r: anyhow::Result<()>) {
    #[cfg(any(test, feature = "testing"))]
    let r = if FAIL_AFTER_WRITE.load(std::sync::atomic::Ordering::SeqCst) {
        Err(anyhow::anyhow!("failing what follows the write, for the test"))
    } else {
        r
    };
    if let Err(e) = r {
        warn!(%project, what, error = format!("{e:#}"), "CI alerts: after an event's write");
    }
}

/// Do what `e` says for `project`. See the module docs.
async fn apply(app: &Platform, project: &str, e: &Event, noted: Noted<'_>) -> anyhow::Result<Did> {
    let Some(alerts) = app.db.ci_alerts(project)? else { return nothing(app, noted, "alerts are off") };
    let by = Actor { account: "", agent: Some(CI_AUTHOR) };
    let say = Say::new(app, project, e);
    let green = app.db.ci_alert_green(project, &e.branch)?;
    let late = green.as_ref().is_some_and(|g| e.at <= g.at);
    let open = open_issue(app, project, &e.branch)?;
    match (e.event.as_str(), open) {
        ("ci.green", open) => {
            let g = Green { commit: e.commit.clone(), at: e.at };
            match open {
                // Opened before this green: it closes it.
                Some((o, issue)) if o.red_at < e.at => close(app, project, &say, &alerts, issue, &g, noted, by).await,
                _ => Ok(if app.db.ci_green_noted(project, &e.branch, &g, noted)? {
                    Did::Nothing("no issue open from before this green")
                } else {
                    Did::Applied
                }),
            }
        }
        (_, _) if late => nothing(app, noted, "from before the branch's newest green"),
        ("ci.red", None) => open_new(app, project, &say, &alerts, noted, by).await,
        (_, None) => nothing(app, noted, "no open issue to say it on"),
        (_, Some((_, issue))) => {
            let (text, people) = match e.event.as_str() {
                "ci.red" => {
                    let mut lines = say.red();
                    lines.push("Still red after its retry.".to_owned());
                    (lines.join("\n\n"), vec![])
                }
                "ci.culprit" => (say.culprit(app, project).await, told(app, project, &alerts).await?),
                _ => (say.stale(), told(app, project, &alerts).await?),
            };
            comment_on(app, project, issue, &text, &people, noted).await
        }
    }
}

/// Comment `text` on `issue` as the event's first write, then mention
/// `people` there.
async fn comment_on(
    app: &Platform,
    project: &str,
    issue: Issue,
    text: &str,
    people: &[String],
    noted: Noted<'_>,
) -> anyhow::Result<Did> {
    let mark = (noted.delivery, noted.event);
    let Some(seq) = crate::features::chat().ci_comment(app, &issue.id, CI_AUTHOR, text, Some(mark)).await? else {
        return Ok(Did::Applied);
    };
    after(project, "mentions", mention(app, &issue, seq, text, people));
    Ok(Did::Wrote("commented", Box::new(issue)))
}

/// Close `issue` for green `g` (with its row, the green and the event, in
/// one write), then comment and mention.
#[allow(clippy::too_many_arguments)]
async fn close(
    app: &Platform,
    project: &str,
    say: &Say<'_>,
    alerts: &Alerts,
    issue: Issue,
    g: &Green,
    noted: Noted<'_>,
    by: Actor<'_>,
) -> anyhow::Result<Did> {
    let branch = &say.e.branch;
    let patch = idb::Patch { state: Some(IssueState::Closed(CloseReason::Completed)), ..Default::default() };
    let (closed, events) = app
        .db
        .patch_issue_and(&issue.id, &patch, by, noted.now, |tx| {
            rows::green_closes(tx, project, branch, &issue.id, g, noted)
        })?
        .ok_or_else(|| anyhow::anyhow!("the issue is gone"))?;
    if events.is_empty() {
        // Applied already, or closed a moment ago by someone else.
        return Ok(if app.db.delivery_event(noted.delivery)?.is_some() {
            Did::Applied
        } else {
            nothing(app, noted, "closed already")?
        });
    }
    crate::inbox::issue_changed(app, by, &issue, &closed).await;
    app.chat.object_changed(&app.db, &format!("project:{project}"), &closed.id);
    crate::boards::issue_changed(app, &closed.id);
    let people = told(app, project, alerts).await.unwrap_or_default();
    let text = say.green();
    let said = async {
        let seq = crate::features::chat()
            .ci_comment(app, &closed.id, CI_AUTHOR, &text, None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no comment"))?;
        mention(app, &closed, seq, &text, &people)
    };
    after(project, "the green's comment", said.await);
    Ok(Did::Wrote("closed", Box::new(closed)))
}

/// The people `alerts` names who are in the project now.
async fn told(app: &Platform, project: &str, alerts: &Alerts) -> anyhow::Result<Vec<String>> {
    if alerts.people.is_empty() {
        return Ok(vec![]);
    }
    let members = crate::forge::inbox::members(app, project).await?;
    Ok(alerts.people.iter().filter(|a| members.contains(a)).cloned().collect())
}

/// Open the branch's red issue, mentioning (and with `assign`, assigning)
/// the people. When the branch has one open already (another machine just
/// opened it), the event is said on that one.
async fn open_new(
    app: &Platform,
    project: &str,
    say: &Say<'_>,
    alerts: &Alerts,
    noted: Noted<'_>,
    by: Actor<'_>,
) -> anyhow::Result<Did> {
    let e = say.e;
    let people = told(app, project, alerts).await?;
    let mut lines = say.red();
    lines.extend(say.summary());
    lines.push(format!(
        "{} opened this, and closes it when the branch is green again.",
        ci_name(CI_AUTHOR).unwrap_or_default()
    ));
    if !people.is_empty() {
        let accounts = app.cache.accounts(&*app.hub, &people).await?;
        let at: Vec<String> =
            people.iter().filter_map(|a| accounts.get(a)).map(|a| format!("@{}", a.login.to_lowercase())).collect();
        if !at.is_empty() {
            lines.push(format!("cc {}", at.join(" ")));
        }
    }
    let body = lines.join("\n\n");
    let assignees: Vec<String> = if alerts.assign { people.clone() } else { vec![] };
    let new =
        idb::NewIssue { project, title: &e.title(), body: &body, labels: &["ci".to_owned()], assignees: &assignees };
    match app.db.create_ci_issue(&new, by, &e.branch, (e.red_commit(), e.at), noted)? {
        Created::Opened(issue) => {
            crate::inbox::issue_opened(app, by, &issue).await;
            after(project, "the new issue's mentions", Ok(()));
            Ok(Did::Wrote("opened", issue))
        }
        Created::Applied => Ok(Did::Applied),
        Created::Taken => {
            let Some((_, issue)) = open_issue(app, project, &e.branch)? else {
                anyhow::bail!("the branch's open issue was taken, and it isn't open now");
            };
            let mut lines = say.red();
            lines.push("Still red after its retry.".to_owned());
            comment_on(app, project, issue, &lines.join("\n\n"), &[], noted).await
        }
    }
}

/// Mention `people` in the issue's thread, at its message `seq`.
fn mention(app: &Platform, issue: &Issue, seq: i64, text: &str, people: &[String]) -> anyhow::Result<()> {
    if people.is_empty() {
        return Ok(());
    }
    let scope = format!("project:{}", issue.project);
    let Some(room) = app.db.chat_room_on(&scope, &issue.id)? else { return Ok(()) };
    let me = ci_name(CI_AUTHOR).unwrap_or_default();
    crate::inbox::thread_mentioned(app, &room, &me, (seq, text), people, &[])?;
    Ok(())
}

/// What GitHub says of a commit: its author's login (else name) and the
/// pull request it came in (number, title).
#[derive(Debug, Default)]
struct Facts {
    author: Option<String>,
    pr: Option<(u64, String)>,
}

/// [`Facts`] of `sha` in `repo`, linked to `project`, when this platform
/// has its GitHub App; else nothing.
async fn commit_facts(app: &Platform, project: &str, repo: &str, sha: &str) -> anyhow::Result<Facts> {
    let Some(gh) = app.github.as_ref() else { return Ok(Facts::default()) };
    let Some(link) = app.db.linked_repo_named(repo)?.filter(|l| l.project_id == project) else {
        return Ok(Facts::default());
    };
    let token = gh
        .read_token(&Caller::of(app, "platform"), link.installation_id, Repo::Id(link.repo_id), Purpose::Read)
        .await?;
    let api = gh.api.trim_end_matches('/');
    let get = async |path: String| -> anyhow::Result<Value> {
        let r = app
            .http
            .get(format!("{api}/repos/{}/{path}", link.full_name))
            .bearer_auth(token.secret())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "arugula-platform")
            .send()
            .await?
            .error_for_status()?;
        Ok(r.json().await?)
    };
    let c = get(format!("commits/{sha}")).await?;
    let author = c["author"]["login"]
        .as_str()
        .map(|l| format!("{l} on GitHub"))
        .or_else(|| c["commit"]["author"]["name"].as_str().map(str::to_owned))
        .map(|a| a.chars().filter(|c| !c.is_control()).take(100).collect());
    let pr = match get(format!("commits/{sha}/pulls")).await {
        Ok(v) => v.as_array().and_then(|l| l.first()).and_then(|p| {
            Some((
                p["number"].as_u64()?,
                p["title"].as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).take(200).collect(),
            ))
        }),
        Err(er) => {
            warn!(%project, repo, error = format!("{er:#}"), "CI alerts: couldn't read the culprit's pull request");
            None
        }
    };
    Ok(Facts { author, pr })
}
