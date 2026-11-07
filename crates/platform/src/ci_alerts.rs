//! CI alerts as native issues (native #774): chant's tick posts a
//! project's `chant.notify/v1` CI events to `POST /ci/notify/{project}`,
//! and they open, comment on and close one "main is red" issue per branch.
//!
//! **The key.** The platform makes it (32 random bytes, hex) when a
//! project's owner or editor turns alerts on, shows it once
//! ([`CiAlertsKey`]) and seals it in `project_secrets` under
//! [`KEY_NAME`], as a Plane connection's webhook secret is: it has to be
//! read back to check a signature, so it can't be a hash. Making a new one
//! (rotating) replaces it at once; turning alerts off removes it.
//!
//! **A delivery** is chant's (`@intentius/chant/notify`): the body, and
//! - `x-chant-signature: sha256=<hex>`, HMAC-SHA256 with the key over
//!   `<x-chant-timestamp>.<body>`;
//! - `x-chant-timestamp`, seconds since the epoch: one more than
//!   [`TOLERANCE_S`] from now either way is refused, as chant's own
//!   `verifyWebhook` refuses it;
//! - `x-chant-event` and `x-chant-delivery` (the event's `id`), which this
//!   reads from the body instead, since the signature covers only the body.
//!
//! A project without alerts on, or with no key to check with, is a 404, the
//! same for both; a bad signature or an old timestamp is a 401 before
//! anything is read or written. Each project has its own key, so one
//! project's key never posts to another's URL. A signed event is
//! deduplicated by its `id` in the GitHub webhook's table
//! ([`Db::first_delivery`], key `ci:<project>:<id>`, kept 7 days); one that
//! fails after that is forgotten, so chant's retry is taken. Only `ci.red`,
//! `ci.culprit`, `ci.stale` and `ci.green` are acted on; any other event
//! (`waiting`, `ci.quarantine-passed`...) is a 200 that does nothing.
//!
//! **One open issue per branch** ([`Db::ci_alert_open`]), held by a unique
//! index on the open rows, since the platform's machines take events side
//! by side: the one that loses the race to open it comments on the winner's
//! ([`Db::create_ci_issue`]):
//! - `ci.red` opens it, titled with the branch and the red commit, or
//!   comments on the open one;
//! - `ci.culprit` comments with the commit, and its author and pull
//!   request when the repo is linked to the project and the GitHub App can
//!   read it;
//! - `ci.stale` comments with how long the branch has been red;
//! - `ci.green` comments with the green commit and closes the issue as
//!   completed. A `ci.red` after that opens a new one.
//!
//! `ci.culprit` and `ci.stale` with none open open one (its first red was
//! missed, or a person closed it); `ci.green` with none open does nothing.
//!
//! **Who wrote it.** Not a person and not anyone's agent: the issue's
//! author is empty with `ci:<source>` as its agent (`ci:chant`), its
//! comments have no author and `via` `ci:<source>`, as an agent session's
//! issues and GitHub's comments are kept (aru-115, M205). Both are shown as
//! `chant CI` ([`ci_name`]). Only this endpoint writes a `ci:` agent or
//! `via`: a person's post can't set `via`, and a product's (`boxes::post`)
//! always has its person as the author.
//!
//! **Who hears it.** A new issue mentions the people the project's setting
//! names (and assigns them, with `assign`): the inbox's `mentioned` and
//! `assigned`, with their push. `ci.culprit`, `ci.stale` and `ci.green`
//! mention them again in the thread. Push preferences are the inbox's
//! ([`crate::inbox`]): quiet hours, muted threads and its limits apply.
//! Text from the event is never a mention: an `@` in it is broken.
//!
//! [`Db::first_delivery`]: crate::db::Db::first_delivery
//! [`Db::ci_alert_open`]: crate::db::Db::ci_alert_open
//! [`Db::create_ci_issue`]: crate::db::Db::create_ci_issue

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects::{CI_VIA, CiAlertOpen, CiAlerts, CiAlertsKey, CiAlertsSettings, ci_name};
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
        ci_alerts::{Alerts, KEY_NAME},
        issues::{self as idb, Actor, CloseReason, Issue, State as IssueState},
    },
    forge::{Caller, Purpose, Repo},
    issues::place,
    keys,
};

/// How far from now a delivery's timestamp may be, in seconds: chant's
/// `DEFAULT_TOLERANCE_SECONDS`.
pub const TOLERANCE_S: u64 = 300;

/// The events acted on.
pub const EVENTS: [&str; 4] = ["ci.red", "ci.culprit", "ci.stale", "ci.green"];

/// What the body's `schema` is.
pub const SCHEMA: &str = "chant.notify/v1";

/// Most people a project's alerts name.
pub const PEOPLE_MAX: usize = 20;

/// The URL chant posts `project`'s events to.
pub fn url(app: &Platform, project: &str) -> String {
    format!("{}/ci/notify/{project}", app.cfg.public_url)
}

fn wire(app: &Platform, project: &str, a: Option<&Alerts>) -> Result<CiAlerts, ApiError> {
    let mut open = Vec::new();
    for (branch, o) in app.db.ci_alert_opens(project)? {
        if let Some(i) = app.db.issue(&o.issue)? {
            open.push(CiAlertOpen { branch, issue: i.id, number: i.number, opened_at: o.opened_at });
        }
    }
    Ok(CiAlerts {
        project: project.to_owned(),
        on: a.is_some(),
        url: url(app, project),
        people: a.map(|a| a.people.clone()).unwrap_or_default(),
        assign: a.is_some_and(|a| a.assign),
        key_made_by: a.map(|a| a.key_made_by.clone()),
        key_made_at: a.map(|a| a.key_made_at),
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
    let id = p.project.id;
    Ok(Json(wire(&app, &id, app.db.ci_alerts(&id)?.as_ref())?))
}

/// `PATCH /api/projects/{project}/ci-alerts`: whom a new red issue
/// mentions, whether it assigns them, or off. An owner or editor.
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
            return Ok(Json(wire(&app, &id, None)?));
        }
        Some(true) if app.db.ci_alerts(&id)?.is_none() => {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "CI alerts are turned on by making their key"));
        }
        _ => {}
    }
    let people = match &b.people {
        Some(v) => {
            let mut out: Vec<String> = Vec::new();
            for a in v.iter().map(|a| a.trim()) {
                if !p.has(a) {
                    return Err(err(StatusCode::UNPROCESSABLE_ENTITY, &format!("{a} isn't in this project")));
                }
                if !out.iter().any(|x| x == a) {
                    out.push(a.to_owned());
                }
            }
            if out.len() > PEOPLE_MAX {
                return Err(err(StatusCode::UNPROCESSABLE_ENTITY, &format!("at most {PEOPLE_MAX} people")));
            }
            Some(out)
        }
        None => None,
    };
    if !app.db.set_ci_alerts(&id, people.as_deref(), b.assign)? {
        return Err(err(StatusCode::NOT_FOUND, "CI alerts aren't on in this project"));
    }
    info!(%account, project = %id, people = ?people, assign = ?b.assign, "changed CI alerts");
    Ok(Json(wire(&app, &id, app.db.ci_alerts(&id)?.as_ref())?))
}

/// `POST /api/projects/{project}/ci-alerts/key`: alerts on with a new key,
/// or the key rotated. An owner or editor; the key is in this answer only.
pub async fn make_key(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<CiAlertsKey>, ApiError> {
    let p = place(&app, Actor::person(&account), &project, true).await?;
    let id = p.project.id;
    let Some(master) = app.secret_key.as_deref() else {
        return Err(err(StatusCode::NOT_IMPLEMENTED, "this control can't keep a key (it has no key to seal it with)"));
    };
    let key = hex::encode(arugula_e2e::random::<32>());
    keys::seal(&app.db, master, &id, KEY_NAME, &key)?;
    app.db.ci_alerts_key_made(&id, &account, now_ms())?;
    info!(%account, project = %id, "made a CI alerts key");
    let alerts = wire(&app, &id, app.db.ci_alerts(&id)?.as_ref())?;
    Ok(Json(CiAlertsKey { key, alerts }))
}

// ---------------------------------------------------------------- the endpoint

fn nothing() -> Response {
    err(StatusCode::NOT_FOUND, "no such webhook").into_response()
}

/// `sha256=<hex>` of `<timestamp>.<body>` with `key`, as chant's
/// `signWebhook` makes it.
pub fn sign(key: &str, timestamp: &str, body: &[u8]) -> String {
    use hmac::{KeyInit, Mac};
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes()).expect("hmac");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
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
    use hmac::{KeyInit, Mac};
    let Some(signature) = signature else { return Some("no x-chant-signature header") };
    let Some(ts) = timestamp.filter(|t| !t.is_empty() && t.len() <= 20 && t.bytes().all(|b| b.is_ascii_digit())) else {
        return Some("no x-chant-timestamp header, or not a number of seconds");
    };
    let Some(given) = signature.trim().strip_prefix("sha256=").and_then(|h| hex::decode(h).ok()) else {
        return Some("the signature does not match");
    };
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes()).expect("hmac");
    mac.update(ts.as_bytes());
    mac.update(b".");
    mac.update(body);
    if mac.verify_slice(&given).is_err() {
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
    if app.db.ci_alerts(project)?.is_none() {
        return Ok(None);
    }
    let Some(master) = app.secret_key.as_deref() else { return Ok(None) };
    keys::open(&app.db, master, project, KEY_NAME)
}

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
        Ok(None) => return nothing(),
        Err(e) => {
            warn!(%project, error = format!("{e:#}"), "checking a CI alerts webhook's key");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't check the webhook").into_response();
        }
    };
    let now = now_ms();
    if let Some(why) = refused(&key, h("x-chant-signature"), h("x-chant-timestamp"), &body, now / 1000) {
        warn!(%project, why, "a CI alerts event refused");
        return err(StatusCode::UNAUTHORIZED, why).into_response();
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
        return err(StatusCode::BAD_REQUEST, "a ci event needs ci.branch and ci.commit").into_response();
    };
    let delivery = format!("ci:{project}:{id}");
    match app.db.first_delivery(&delivery, event, now) {
        Ok(true) => {}
        Ok(false) => return Json(json!({ "duplicate": true })).into_response(),
        Err(e) => {
            warn!(%project, event, error = %e, "recording a CI alerts event");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't record the event").into_response();
        }
    }
    match apply(&app, &project, &e).await {
        Ok(Some((did, issue))) => {
            info!(%project, event, did, issue = %issue.id, "a CI alerts event taken");
            Json(json!({ "did": did, "issue": issue.id, "number": issue.number })).into_response()
        }
        Ok(None) => Json(json!({ "ignored": true })).into_response(),
        Err(er) => {
            // Forgotten, so chant's retry is taken.
            warn!(%project, event, error = format!("{er:#}"), "a CI alerts event couldn't be applied");
            let _ = app.db.forget_delivery(&delivery);
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
    /// `ci:<source>`.
    via: String,
    summary: String,
    branch: String,
    commit: String,
    phase: Option<String>,
    failed_run: Option<(String, Option<String>, Option<String>)>,
    culprit: Option<String>,
    red: Option<String>,
    since: Option<String>,
    last_green: Option<String>,
    hours: Option<u64>,
    /// `owner/name` on GitHub, from `run`.
    repo: Option<String>,
    pr: Option<u64>,
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

/// A URL that can go in a link: http(s), no spaces or parentheses.
fn link(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    let ok = (s.starts_with("https://") || s.starts_with("http://"))
        && s.len() <= 512
        && !s.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '(' | ')' | '<' | '>'));
    ok.then(|| s.to_owned())
}

impl Event {
    fn of(v: &Value) -> Option<Self> {
        let ci = &v["ci"];
        let branch = text(&ci["branch"], 200)?;
        let commit = sha(&ci["commit"])?;
        let source: String = v["source"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .take(32)
            .collect::<String>()
            .to_lowercase();
        let source = if source.is_empty() { "chant".to_owned() } else { source };
        let run = &v["run"];
        let repo = text(&run["repository"], 140).filter(|r| {
            let github = run["forge"].as_str().is_none_or(|f| f == "github");
            github
                && r.split('/').count() == 2
                && r.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b))
                && !r.starts_with('/')
                && !r.ends_with('/')
        });
        let fr = &ci["failedRun"];
        let failed_run = text(&fr["name"], 200).map(|n| (n, text(&fr["conclusion"], 40), link(&fr["url"])));
        Some(Self {
            event: v["event"].as_str().unwrap_or_default().to_owned(),
            via: format!("{CI_VIA}{source}"),
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

    fn short(sha: &str) -> &str {
        &sha[..sha.len().min(12)]
    }

    /// A commit, linked on GitHub when the repo is known.
    fn commit_md(&self, sha: &str) -> String {
        match &self.repo {
            Some(r) => format!("[`{}`](https://github.com/{r}/commit/{sha})", Self::short(sha)),
            None => format!("`{}`", Self::short(sha)),
        }
    }

    /// The red issue's title: the branch and the commit it went red at.
    fn title(&self) -> String {
        let at = match self.event.as_str() {
            "ci.stale" => self.since.as_deref().unwrap_or(&self.commit),
            "ci.culprit" => self.culprit.as_deref().unwrap_or(&self.commit),
            _ => &self.commit,
        };
        format!("{} is red at {}", self.branch, Self::short(at))
    }

    /// What `ci.red` says: the summary, the phase and its failed run.
    fn red_lines(&self) -> Vec<String> {
        let mut out = vec![format!("**{}** is red at {}.", self.branch, self.commit_md(&self.commit))];
        if let Some(p) = &self.phase {
            let run = match &self.failed_run {
                Some((name, conclusion, url)) => {
                    let what =
                        format!("{name}{}", conclusion.as_deref().map(|c| format!(" ({c})")).unwrap_or_default());
                    match url {
                        Some(u) => format!(": [{what}]({u})"),
                        None => format!(": {what}"),
                    }
                }
                None => String::new(),
            };
            out.push(format!("Phase {p}{run}."));
        }
        out
    }
}

/// Text from an event, with no `@` that could name someone.
fn quiet(s: &str) -> String {
    s.replace('@', "@\u{2060}")
}

/// The open red issue of `branch`, if the row names one that's still open
/// in this project. One a person closed (or that's gone) stops being the branch's here, so
/// the next event opens another.
fn open_issue(app: &Platform, project: &str, branch: &str) -> anyhow::Result<Option<Issue>> {
    let Some(o) = app.db.ci_alert_open(project, branch)? else { return Ok(None) };
    match app.db.issue(&o.issue)? {
        Some(i) if i.project == project && i.state == IssueState::Open => Ok(Some(i)),
        _ => {
            app.db.ci_alert_closed(project, branch, &o.issue, now_ms())?;
            Ok(None)
        }
    }
}

/// Do what `e` says for `project`: what was done and to which issue, or
/// `None` when it changes nothing.
async fn apply(app: &Platform, project: &str, e: &Event) -> anyhow::Result<Option<(&'static str, Issue)>> {
    let Some(alerts) = app.db.ci_alerts(project)? else { return Ok(None) };
    let by = Actor { account: "", agent: Some(&e.via) };
    let people = told(app, project, &alerts).await?;
    let issue = match open_issue(app, project, &e.branch)? {
        Some(issue) => issue,
        None if e.event == "ci.green" => return Ok(None),
        None => match open_new(app, project, e, &alerts, &people, by).await? {
            Some(issue) => return Ok(Some(("opened", issue))),
            // Another machine opened the branch's issue just now: this
            // event is said on that one.
            None => match open_issue(app, project, &e.branch)? {
                Some(issue) => issue,
                None => anyhow::bail!("the branch's open issue was taken, and it isn't open now"),
            },
        },
    };
    on_open(app, project, e, issue, &people, by).await.map(Some)
}

/// Say `e` on `branch`'s open issue: a comment, and for green, closed.
async fn on_open(
    app: &Platform,
    project: &str,
    e: &Event,
    issue: Issue,
    people: &[String],
    by: Actor<'_>,
) -> anyhow::Result<(&'static str, Issue)> {
    match e.event.as_str() {
        "ci.red" => {
            let mut lines = e.red_lines();
            lines.push("Still red after its retry.".to_owned());
            comment(app, &issue, e, &lines.join("\n\n"), &[]).await?;
            Ok(("commented", issue))
        }
        "ci.green" => {
            let mut lines = vec![format!("**{}** is green again at {}.", e.branch, e.commit_md(&e.commit))];
            if let Some(r) = &e.red {
                lines.push(format!("That answers the red at {}.", e.commit_md(r)));
            }
            comment(app, &issue, e, &lines.join("\n\n"), people).await?;
            let patch = idb::Patch { state: Some(IssueState::Closed(CloseReason::Completed)), ..Default::default() };
            let old = issue.clone();
            let now = now_ms();
            let (closed, events) =
                app.db.patch_issue(&issue.id, &patch, by, now)?.ok_or_else(|| anyhow::anyhow!("the issue is gone"))?;
            app.db.ci_alert_closed(project, &e.branch, &issue.id, now)?;
            crate::inbox::issue_changed(app, by, &old, &closed).await;
            if !events.is_empty() {
                app.chat.object_changed(&app.db, &format!("project:{project}"), &closed.id);
                crate::boards::issue_changed(app, &closed.id);
            }
            Ok(("closed", closed))
        }
        _ => {
            let text = match e.event.as_str() {
                "ci.culprit" => culprit_text(app, project, e).await,
                _ => stale_text(e),
            };
            comment(app, &issue, e, &text, people).await?;
            Ok(("commented", issue))
        }
    }
}

/// The people `alerts` names who are in the project now.
async fn told(app: &Platform, project: &str, alerts: &Alerts) -> anyhow::Result<Vec<String>> {
    if alerts.people.is_empty() {
        return Ok(vec![]);
    }
    let members = crate::forge::inbox::members(app, project).await?;
    Ok(alerts.people.iter().filter(|a| members.contains(a)).cloned().collect())
}

/// Open `branch`'s red issue, mentioning (and with `assign`, assigning) the
/// people. `None` when the branch has an open one already
/// ([`Db::create_ci_issue`]).
///
/// [`Db::create_ci_issue`]: crate::db::Db::create_ci_issue
async fn open_new(
    app: &Platform,
    project: &str,
    e: &Event,
    alerts: &Alerts,
    people: &[String],
    by: Actor<'_>,
) -> anyhow::Result<Option<Issue>> {
    let mut lines = match e.event.as_str() {
        "ci.culprit" => vec![culprit_text(app, project, e).await],
        "ci.stale" => vec![stale_text(e)],
        _ => e.red_lines(),
    };
    if !e.summary.is_empty() {
        lines.push(format!("> {}", quiet(&e.summary)));
    }
    lines.push(format!(
        "{} opened this, and closes it when {} is green again.",
        ci_name(&e.via).unwrap_or_default(),
        e.branch
    ));
    if !people.is_empty() {
        let accounts = app.cache.accounts(&*app.hub, people).await?;
        let at: Vec<String> =
            people.iter().filter_map(|a| accounts.get(a)).map(|a| format!("@{}", a.login.to_lowercase())).collect();
        if !at.is_empty() {
            lines.push(format!("cc {}", at.join(" ")));
        }
    }
    let body = lines.join("\n\n");
    let assignees: Vec<String> = if alerts.assign { people.to_vec() } else { vec![] };
    let new =
        idb::NewIssue { project, title: &e.title(), body: &body, labels: &["ci".to_owned()], assignees: &assignees };
    let red_commit = match e.event.as_str() {
        "ci.stale" => e.since.as_deref().unwrap_or(&e.commit),
        "ci.culprit" => e.culprit.as_deref().unwrap_or(&e.commit),
        _ => &e.commit,
    };
    let Some(issue) = app.db.create_ci_issue(&new, by, &e.branch, red_commit, now_ms())? else { return Ok(None) };
    crate::inbox::issue_opened(app, by, &issue).await;
    Ok(Some(issue))
}

/// Post `text` in the issue's thread as CI, and mention `people` there.
async fn comment(app: &Platform, issue: &Issue, e: &Event, text: &str, people: &[String]) -> anyhow::Result<()> {
    let seq = crate::features::chat().ci_comment(app, &issue.id, &e.via, text).await?;
    if people.is_empty() {
        return Ok(());
    }
    let scope = format!("project:{}", issue.project);
    let Some(room) = app.db.chat_room_on(&scope, &issue.id)? else { return Ok(()) };
    let me = ci_name(&e.via).unwrap_or_default();
    crate::inbox::thread_mentioned(app, &room, &me, (seq, text), people, &[])?;
    Ok(())
}

/// What `ci.stale` says: how long the branch has been red.
fn stale_text(e: &Event) -> String {
    let hours = match e.hours {
        Some(1) => "1 hour".to_owned(),
        Some(h) => format!("{h} hours"),
        None => "a while".to_owned(),
    };
    let mut s = format!("**{}** has had no green commit for {hours}", e.branch);
    if let Some(since) = &e.since {
        s.push_str(&format!(", since {}", e.commit_md(since)));
    }
    if let Some(g) = &e.last_green {
        s.push_str(&format!(" (the last green one is {})", e.commit_md(g)));
    }
    s.push_str(&format!(". Its newest commit is {}.", e.commit_md(&e.commit)));
    s
}

/// What `ci.culprit` says: the commit that broke the branch, its author and
/// its pull request, as far as GitHub says.
async fn culprit_text(app: &Platform, project: &str, e: &Event) -> String {
    let Some(culprit) = &e.culprit else {
        return format!("A bisect below {} named no commit.", e.commit_md(&e.commit));
    };
    let mut s = format!("The bisect names {} as the commit that turned **{}** red", e.commit_md(culprit), e.branch);
    let facts = match &e.repo {
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
        s.push_str(&format!(", by {}", quiet(a)));
    }
    let pr = facts.pr.or(e.pr.map(|n| (n, String::new())));
    match (pr, &e.repo) {
        (Some((n, title)), Some(repo)) => {
            let title = if title.is_empty() { String::new() } else { format!(" ({})", quiet(&title)) };
            s.push_str(&format!(", in [{repo}#{n}](https://github.com/{repo}/pull/{n}){title}"));
        }
        (Some((n, _)), None) => s.push_str(&format!(", in pull request {n}")),
        _ => {}
    }
    s.push('.');
    s
}

/// What GitHub says of a commit: its author's login (else name) and the
/// pull request it came in (number, title).
#[derive(Debug, Default)]
struct Facts {
    author: Option<String>,
    pr: Option<(u64, String)>,
}

/// [`Facts`] of `sha` in `repo`, when it's linked to `project` and this
/// platform has its GitHub App; else nothing.
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
        .map(|a| a.chars().take(100).collect());
    let pr = match get(format!("commits/{sha}/pulls")).await {
        Ok(v) => v.as_array().and_then(|l| l.first()).and_then(|p| {
            Some((p["number"].as_u64()?, p["title"].as_str().unwrap_or_default().chars().take(200).collect()))
        }),
        Err(er) => {
            warn!(%project, repo, error = format!("{er:#}"), "CI alerts: couldn't read the culprit's pull request");
            None
        }
    };
    Ok(Facts { author, pr })
}
