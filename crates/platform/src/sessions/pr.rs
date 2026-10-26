//! A session's pull request (M259, thread = session, Arugula #129): once a
//! turn pushed the session's branch, its asker drafts the PR from the
//! session's card and sends it, as themselves ("agents draft, people
//! send", aru-041, and "Sending as a person uses a one-use user token,
//! falling back to their machine", aru-085). The routes are
//! `arugula_control_wire::sessions`' `SESSION_PR` and `SESSION_PR_SEND`;
//! the rows are `db::pr_drafts`'.
//!
//! - **Who:** the session's asker, a person (or their CLI), while they may
//!   read its repo (M165). Anyone else is refused, and a machine is
//!   refused whatever it holds ([`Sender`]).
//! - **Prefilled** ([`show`]): until the asker keeps one, the draft is
//!   the repo's PR template (`.github/pull_request_template.md` and the
//!   other places GitHub looks, read from the base branch with the App's
//!   read token) and what the session's pushed turns left: the commits,
//!   and the last pushed turn's diffstat. A later turn that didn't push
//!   (its finish step failed, M261, or its push did) isn't on GitHub, so
//!   it's left out. The title is the one commit's subject, or the
//!   session's slug in words. The base is the repo's default branch. It's
//!   a draft PR on GitHub unless the asker says not.
//! - **Sent** ([`send`]) as a forge draft is: the browser hop for a
//!   one-use user token (the hub's GitHub callback hands it back through
//!   `PlatformPort::send_draft`, which knows a PR draft by its `pdr_`),
//!   whose `GET /user` must be the asker's own GitHub id, then the one
//!   call, `POST repos/{repo}/pulls`, as them. A token that may not write
//!   there, or none, leaves it to one of their machines (`forge.send`, kind
//!   `pr`, with their `gh`). Only once a turn pushed the branch.
//! - **After:** the session's `pr` is the PR's URL, so its card shows it,
//!   and the PR's thread says where it came from once the mirror has the
//!   PR ([`noted`], `chat::session_card::note_pr`): now, if the PR's
//!   webhook came first, or when it comes.

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::{
    forge::{DaemonOp, ForgeSend, OpAnswer},
    projects::{MAX_DRAFT_BODY, NO_WAY},
    sessions::{SessionPr, SessionPrEdit, SessionPrSend, SessionPrSent},
};
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use base64::Engine as _;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    Platform,
    auth::Person,
    db::{
        forge::{LinkedRepo, MintKind, NewMint},
        pr_drafts::{NewPrDraft, NotSaved, PrDraft, PrEnd},
        sessions::{Kind, Session, SessionDetail},
    },
    forge::{
        Caller, Purpose, Repo,
        drafts::readable,
        send::{How, Sender, UserToken, github, user_of},
        viewer::Viewer,
    },
};

/// A PR draft's id starts so.
pub const PREFIX: &str = "pdr_";

/// Where GitHub looks for a repo's PR template, in its order.
const TEMPLATES: &[&str] = &[
    ".github/pull_request_template.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "pull_request_template.md",
    "PULL_REQUEST_TEMPLATE.md",
    "docs/pull_request_template.md",
];

/// The longest template taken into a body.
const TEMPLATE_MAX: usize = 32 * 1024;
/// The longest title GitHub takes.
const TITLE_MAX: usize = 256;

fn no_session() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such session")
}

fn no_draft() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such PR draft")
}

/// Session `ses` for its asker, `account`: the session and its linked repo,
/// while they may read it. Anyone else who may read it is told only its
/// asker drafts its PR; anyone who may not, that there's no such session.
async fn asker_of(app: &Platform, account: &str, ses: &str) -> Result<(Session, LinkedRepo, Viewer), ApiError> {
    let s = app.db.session(ses)?.ok_or_else(no_session)?;
    let repo = s.repo.as_deref().ok_or_else(|| err(StatusCode::CONFLICT, "the session works in no repo"))?;
    let link = app
        .db
        .linked_repo_named(repo)?
        .filter(|l| l.project_id == s.project && l.unlinked_at.is_none())
        .ok_or_else(|| err(StatusCode::CONFLICT, &format!("{repo} isn't linked to the session's project now")))?;
    let v = Viewer::of(app, account).await?;
    readable(app, &v, link.repo_id).await?.ok_or_else(no_session)?;
    if s.asker.is_empty() || s.asker != account {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only the session's asker drafts and sends its PR: it goes out as them",
        ));
    }
    Ok((s, link, v))
}

/// Every turn's detail, oldest first.
fn turns(app: &Platform, s: &Session) -> anyhow::Result<Vec<SessionDetail>> {
    Ok(app
        .db
        .session_events(&s.id)?
        .into_iter()
        .filter(|e| e.kind == Kind::Turn)
        .map(|e| serde_json::from_str(&e.detail).unwrap_or_default())
        .collect())
}

/// Whether a turn pushed the session's branch.
fn pushed(app: &Platform, s: &Session) -> anyhow::Result<bool> {
    Ok(turns(app, s)?.iter().any(|d| d.pushed == Some(true)))
}

/// The turns up to the last one that pushed: what's on GitHub. A turn
/// after it didn't push, so its commits aren't in the PR.
fn on_github(mut turns: Vec<SessionDetail>) -> Vec<SessionDetail> {
    let kept = turns.iter().rposition(|d| d.pushed == Some(true)).map_or(0, |i| i + 1);
    turns.truncate(kept);
    turns
}

/// A ref GitHub takes as a branch's name.
fn good_branch(b: &str) -> bool {
    !b.is_empty()
        && b.len() <= 200
        && !b.starts_with(['-', '/', '.'])
        && !b.ends_with(['/', '.'])
        && !b.contains("..")
        && !b.contains("//")
        && !b.ends_with(".lock")
        && b.bytes().all(|c| c.is_ascii_graphic() && !b"~^:?*[\\@{".contains(&c))
}

/// GET `path` on GitHub's API with the App's read token for `link`.
async fn app_get(app: &Platform, link: &LinkedRepo, path: &str) -> Option<Value> {
    let gh = app.github.as_ref()?;
    let t = gh
        .read_token(&Caller::of(app, "platform"), link.installation_id, Repo::Id(link.repo_id), Purpose::Read)
        .await
        .map_err(|e| warn!(repo = link.full_name, error = format!("{e:#}"), "no read token for a session's PR"))
        .ok()?;
    let r = app
        .http
        .get(format!("{}/{path}", gh.api.trim_end_matches('/')))
        .bearer_auth(t.secret())
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "arugula-control")
        .send()
        .await
        .ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.json().await.ok()
}

/// The repo's default branch, as GitHub says (`main` when it doesn't).
async fn default_branch(app: &Platform, link: &LinkedRepo) -> String {
    app_get(app, link, &format!("repos/{}", link.full_name))
        .await
        .and_then(|v| v["default_branch"].as_str().map(str::to_owned))
        .filter(|b| good_branch(b))
        .unwrap_or_else(|| "main".into())
}

/// The repo's PR template on `base`, and where it was.
async fn template(app: &Platform, link: &LinkedRepo, base: &str) -> Option<(String, String)> {
    for path in TEMPLATES {
        let Some(v) = app_get(app, link, &format!("repos/{}/contents/{path}?ref={base}", link.full_name)).await else {
            continue;
        };
        let Some(b64) = v["content"].as_str() else { continue };
        let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(clean) else { continue };
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if text.len() > TEMPLATE_MAX {
            let mut at = TEMPLATE_MAX;
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            text.truncate(at);
        }
        return Some(((*path).to_owned(), text));
    }
    None
}

/// The title the draft starts with: the one commit's subject, else the
/// session's slug in words.
fn title_for(s: &Session, commits: &[String]) -> String {
    let subject = |c: &String| c.split_once(' ').map(|(_, m)| m.trim().to_owned());
    if let [one] = commits
        && let Some(t) = subject(one).filter(|t| !t.is_empty())
    {
        return t.chars().take(TITLE_MAX).collect();
    }
    let short: String = s.id.trim_start_matches("ses_").chars().take(8).collect();
    let slug = s.branch.strip_prefix("arugula/").and_then(|b| b.strip_prefix(&short)).unwrap_or("");
    let words = slug.trim_start_matches('-').replace(['-', '_'], " ");
    let mut c = words.trim().chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => format!("Changes from agent session {short}"),
    }
}

/// What the (pushed) turns left, for the body: each commit once (newest
/// first), and the last turn's diffstat.
fn summary(s: &Session, asker: &str, turns: &[SessionDetail]) -> (Vec<String>, String) {
    let mut commits: Vec<String> = vec![];
    for d in turns.iter().rev() {
        for c in &d.commits {
            if !commits.contains(c) {
                commits.push(c.clone());
            }
        }
    }
    let short: String = s.id.trim_start_matches("ses_").chars().take(8).collect();
    let by = if asker.is_empty() { String::new() } else { format!(", asked for by {asker}") };
    let mut out = format!("Opened from Arugula agent session `{short}`{by}.\n");
    if !commits.is_empty() {
        out.push_str("\nCommits:\n");
        for c in &commits {
            out.push_str(&format!("- {c}\n"));
        }
    }
    if let Some(stat) = turns.iter().rev().find_map(|d| d.diffstat.clone()).filter(|d| !d.trim().is_empty()) {
        out.push_str(&format!("\nThe last turn's changes:\n\n```text\n{}\n```\n", stat.trim_end()));
    }
    (commits, out)
}

/// The draft as the asker sees it: kept, or prefilled now.
async fn draft_view(app: &Platform, s: &Session, link: &LinkedRepo, account: &str) -> anyhow::Result<SessionPr> {
    if let Some(d) = app.db.pr_draft_of(&s.id)? {
        return Ok(wire(&d, now_ms()));
    }
    let base = default_branch(app, link).await;
    let turns = on_github(turns(app, s)?);
    let names = app.cache.accounts(&*app.hub, &[account.to_owned()]).await?;
    let asker = names.get(account).map(|a| a.name.clone()).unwrap_or_default();
    let (commits, said) = summary(s, &asker, &turns);
    let tpl = template(app, link, &base).await;
    let body = match &tpl {
        Some((_, t)) if !t.trim().is_empty() => format!("{}\n\n---\n\n{said}", t.trim_end()),
        _ => said,
    };
    Ok(SessionPr {
        session: s.id.clone(),
        id: None,
        repo: link.full_name.clone(),
        head: s.branch.clone(),
        base,
        title: title_for(s, &commits),
        body,
        draft: true,
        state: "new".into(),
        error: None,
        url: None,
        number: None,
        template: tpl.map(|(p, _)| p),
    })
}

fn wire(d: &PrDraft, now: u64) -> SessionPr {
    SessionPr {
        session: d.session.clone(),
        id: Some(d.id.clone()),
        repo: d.full_name.clone(),
        head: d.head.clone(),
        base: d.base.clone(),
        title: d.title.clone(),
        body: d.body.clone(),
        draft: d.draft,
        state: if d.sending(now) { "sending".into() } else { d.state.clone() },
        error: d.error.clone(),
        url: d.sent_url.clone(),
        number: d.number,
        template: None,
    }
}

/// `GET /api/sessions/{ses}/pr`: module docs.
pub async fn show(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(ses): Path<String>,
) -> Result<Json<SessionPr>, ApiError> {
    let (s, link, _) = asker_of(&app, &account, &ses).await?;
    Ok(Json(draft_view(&app, &s, &link, &account).await?))
}

/// Keep what the asker wrote, checked.
async fn keep(app: &Platform, s: &Session, link: &LinkedRepo, e: &SessionPrEdit) -> Result<PrDraft, ApiError> {
    let title = e.title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX {
        return Err(err(StatusCode::BAD_REQUEST, "a PR needs a title of at most 256 characters"));
    }
    if e.body.len() > MAX_DRAFT_BODY {
        return Err(err(StatusCode::BAD_REQUEST, "too long for GitHub"));
    }
    let was = app.db.pr_draft_of(&s.id)?;
    let base = match (e.base.as_deref().map(str::trim), &was) {
        (Some(b), _) if !good_branch(b) || b == s.branch => {
            return Err(err(StatusCode::BAD_REQUEST, "base: a branch of the repo other than the session's"));
        }
        (Some(b), _) => b.to_owned(),
        (None, Some(w)) => w.base.clone(),
        (None, None) => default_branch(app, link).await,
    };
    let draft = e.draft.or(was.as_ref().map(|w| w.draft)).unwrap_or(true);
    let n = NewPrDraft {
        session: &s.id,
        project: &s.project,
        author: &s.asker,
        repo_id: link.repo_id,
        head: &s.branch,
        base: &base,
        title,
        body: &e.body,
        draft,
    };
    match app.db.save_pr_draft(&n, now_ms())? {
        Ok(d) => Ok(d),
        Err(NotSaved::Sent) => Err(err(StatusCode::CONFLICT, "it was sent already")),
        Err(NotSaved::Sending) => Err(err(StatusCode::CONFLICT, "it's being sent")),
    }
}

/// `PUT /api/sessions/{ses}/pr` ([`SessionPrEdit`]): module docs.
pub async fn save(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(ses): Path<String>,
    Json(e): Json<SessionPrEdit>,
) -> Result<Json<SessionPr>, ApiError> {
    let (s, link, _) = asker_of(&app, &account, &ses).await?;
    let d = keep(&app, &s, &link, &e).await?;
    Ok(Json(wire(&d, now_ms())))
}

/// A path on the platform page to come back to: only a path.
fn back_path(back: &str) -> String {
    let ok = back.starts_with('/') && !back.starts_with("//") && !back.contains('\\') && back.len() <= 512;
    if ok { back.to_owned() } else { "/".into() }
}

/// `POST /api/sessions/{ses}/pr/send` ([`SessionPrSend`]): keep what it
/// says, then send it as the asker. A machine is refused ([`Sender`]).
pub async fn route(
    State(app): State<Arc<Platform>>,
    snd: Sender,
    Path(ses): Path<String>,
    body: Bytes,
) -> Result<Json<SessionPrSent>, ApiError> {
    let b: SessionPrSend = if body.is_empty() {
        Default::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?
    };
    let how = match b.via.as_deref() {
        Some("browser") => How::Browser { back: b.back.unwrap_or_default() },
        None | Some("machine") => How::Machine,
        Some(_) => return Err(err(StatusCode::BAD_REQUEST, "via: browser or machine")),
    };
    let (s, link, _) = asker_of(&app, &snd.account, &ses).await?;
    let id = match &b.edit {
        Some(e) => keep(&app, &s, &link, e).await?.id,
        None => {
            app.db
                .pr_draft_of(&s.id)?
                .ok_or_else(|| err(StatusCode::CONFLICT, "nothing to send yet: write its PR first"))?
                .id
        }
    };
    send(&app, &snd.account, &id, how).await.map(Json)
}

/// Send `account`'s PR draft `id` as them, `how`: the one way in, for the
/// route and the hub's GitHub callback (`PlatformPort::send_draft`).
pub async fn send(app: &Platform, account: &str, id: &str, how: How) -> Result<SessionPrSent, ApiError> {
    let d = app.db.pr_draft(id)?.ok_or_else(no_draft)?;
    let (s, _, v) = asker_of(app, account, &d.session).await.map_err(|e| match e.0 {
        StatusCode::NOT_FOUND => no_draft(),
        _ => e,
    })?;
    if matches!(how, How::Token(_)) {
        // Written down as it arrives: who, what for, never the token.
        app.db.log_mint(&NewMint {
            kind: MintKind::User,
            installation_id: None,
            repo_id: Some(d.repo_id),
            permissions: &json!({}),
            caller: account,
            purpose: "send:pr",
            at: now_ms(),
        })?;
    }
    if d.author != account {
        return Err(err(StatusCode::FORBIDDEN, "only the session's asker sends its PR: it goes out as them"));
    }
    let now = now_ms();
    match d.state.as_str() {
        "sent" => return Err(err(StatusCode::CONFLICT, "it was sent already")),
        _ if d.sending(now) => return Err(err(StatusCode::CONFLICT, "it's being sent")),
        _ => {}
    }
    if !pushed(app, &s)? {
        return Err(err(StatusCode::CONFLICT, "its branch isn't on GitHub yet: a turn that commits pushes it"));
    }
    let how = match how {
        How::Browser { back } => match app.hub.send_authorization(account, id, &back_path(&back)).await {
            Ok(url) => return Ok(SessionPrSent { pr: wire(&d, now), authorize: Some(url), via: None, machine: None }),
            Err(e) => {
                info!(draft = id, error = format!("{e:#}"), "no browser hop for a PR's send: trying a machine");
                How::Machine
            }
        },
        how => how,
    };
    let held = app.db.hold_pr_draft(id, account, now)?.ok_or_else(|| err(StatusCode::CONFLICT, "it's being sent"))?;
    let at = held.sent_at.unwrap_or(now);
    match how {
        How::Token(t) => with_token(app, &v, held, at, t).await,
        _ => by_machine(app, account, held, at, Vec::new()).await,
    }
}

/// End the send holding `id` since `at`.
fn end(app: &Platform, id: &str, at: u64, e: PrEnd<'_>) -> Result<PrDraft, ApiError> {
    app.db.end_pr_send(id, at, e, now_ms())?.ok_or_else(|| err(StatusCode::CONFLICT, "another send took it"))
}

/// The PR's number, from its URL (`…/pull/12`).
fn number_of(url: &str) -> Option<u64> {
    url.trim_end_matches('/').rsplit_once("/pull/").and_then(|(_, n)| n.parse().ok())
}

/// It's open: the draft says so, the session links it, its card is told,
/// and its thread hears where it came from if the mirror has it.
fn opened(app: &Platform, d: &PrDraft, url: Option<&str>, number: Option<u64>) -> Result<PrDraft, ApiError> {
    let number = number.or(url.and_then(number_of));
    let at = d.sent_at.unwrap_or_default();
    let d = end(app, &d.id, at, PrEnd::Sent { number, url })?;
    if let Some(url) = &d.sent_url {
        app.db.link_session_pr(&d.session, url, now_ms())?;
    }
    if let Some(s) = app.db.session(&d.session)? {
        app.chat.session(&app.db, &s);
    }
    if let Err(e) = crate::chat::session_card::note_pr(app, &d) {
        warn!(draft = d.id, error = format!("{e:#}"), "couldn't say in a PR's thread where it came from");
    }
    Ok(d)
}

/// [`How::Token`]: the token's own GitHub account must be the asker's,
/// then the one call. A token that may not write there leaves it to a
/// machine.
async fn with_token(app: &Platform, v: &Viewer, d: PrDraft, at: u64, t: UserToken) -> Result<SessionPrSent, ApiError> {
    let Some(api) = app.github.as_ref().map(|g| g.api.clone()) else {
        drop(t);
        return by_machine(app, &v.account, d, at, vec!["this control has no GitHub App".into()]).await;
    };
    let theirs = match (user_of(app, &api, &t).await, &v.github) {
        (Ok(id), Some(g)) if id == g.id => Ok(()),
        (Ok(_), Some(g)) => Err(format!(
            "that GitHub account isn't {}, which you signed in to Arugula with: switch accounts on GitHub, then send it again",
            g.login
        )),
        (Ok(_), None) => Err("sign in to Arugula with GitHub to send as yourself".into()),
        (Err(e), _) => Err(e),
    };
    if let Err(why) = theirs {
        drop(t);
        warn!(draft = d.id, account = v.account, why, "a user token refused for a PR's send");
        end(app, &d.id, at, PrEnd::Waits(&why))?;
        return Err(err(StatusCode::FORBIDDEN, &why));
    }
    let body = json!({ "title": d.title, "body": d.body, "head": d.head, "base": d.base, "draft": d.draft });
    let r =
        github(app, reqwest::Method::POST, &api, &format!("repos/{}/pulls", d.full_name), &t).json(&body).send().await;
    drop(t);
    let (status, v2) = match r {
        Ok(r) => {
            let status = r.status();
            (status, r.json::<Value>().await.unwrap_or_default())
        }
        Err(e) => {
            let why = format!("GitHub couldn't be reached: {}", e.without_url());
            end(app, &d.id, at, PrEnd::Failed(&why))?;
            return Err(refusal(StatusCode::BAD_GATEWAY, "github_refused", &why));
        }
    };
    if status.is_success() {
        let url = v2["html_url"].as_str();
        info!(draft = d.id, account = v.account, repo = d.repo_id, via = "token", "a session's PR opened as its asker");
        let d = opened(app, &d, url, v2["number"].as_u64())?;
        return Ok(SessionPrSent { pr: wire(&d, now_ms()), authorize: None, via: Some("token".into()), machine: None });
    }
    let said = format!("GitHub said {}: {}", status.as_u16(), v2["message"].as_str().unwrap_or(""));
    // GitHub answers 404 for what a token may not see.
    if matches!(status.as_u16(), 401 | 403 | 404) {
        info!(draft = d.id, error = said, "the user token can't open the PR: trying a machine");
        return by_machine(app, &v.account, d, at, vec![format!("as you through GitHub: {said}")]).await;
    }
    // 422 says why in its errors (a PR from that branch is open already,
    // no commits between the two).
    let why = match v2["errors"][0]["message"].as_str() {
        Some(m) => format!("{said} ({m})"),
        None => said,
    };
    warn!(draft = d.id, error = why, "a session's PR wasn't opened");
    end(app, &d.id, at, PrEnd::Failed(&why))?;
    Err(refusal(StatusCode::BAD_GATEWAY, "github_refused", &why))
}

/// [`How::Machine`]: each of the asker's own machines in turn until one
/// opens it, GitHub refuses it, or none is left.
async fn by_machine(
    app: &Platform,
    account: &str,
    d: PrDraft,
    at: u64,
    mut tried: Vec<String>,
) -> Result<SessionPrSent, ApiError> {
    let ids = app.hub.machines_for(account).await?;
    let mut mine: Vec<_> =
        app.cache.daemons(&*app.hub, &ids).await?.into_values().filter(|m| m.account == account).collect();
    mine.sort_by_key(|m| m.name.clone());
    let op = DaemonOp::ForgeSend(ForgeSend {
        draft: d.id.clone(),
        account: account.to_owned(),
        repo: d.full_name.clone(),
        number: None,
        kind: "pr".into(),
        event: None,
        title: Some(d.title.clone()),
        body: d.body.clone(),
        head: Some(d.head.clone()),
        base: Some(d.base.clone()),
        draft_pr: d.draft,
    });
    for m in mine {
        let answer = match app.hub.daemon_op(&m.id, &op).await {
            Ok(a) => a,
            Err(e) => {
                warn!(error = format!("{e:#}"), "the hub can't ask machines to send");
                break;
            }
        };
        match answer {
            OpAnswer::Done { url } => {
                info!(
                    draft = d.id,
                    account,
                    machine = m.id,
                    repo = d.repo_id,
                    via = "machine",
                    "a session's PR opened as its asker"
                );
                let d = opened(app, &d, url.as_deref(), None)?;
                return Ok(SessionPrSent {
                    pr: wire(&d, now_ms()),
                    authorize: None,
                    via: Some("machine".into()),
                    machine: Some(m.name),
                });
            }
            OpAnswer::Failed { error } => {
                let error = format!("{}: {error}", m.name);
                warn!(draft = d.id, machine = m.id, error, "a session's PR wasn't opened");
                end(app, &d.id, at, PrEnd::Failed(&error))?;
                return Err(refusal(StatusCode::BAD_GATEWAY, "github_refused", &error));
            }
            OpAnswer::Cant { why } => tried.push(format!("{}: {why}", m.name)),
            OpAnswer::Unknown => tried.push(format!("{}: its Arugula is too old to send drafts", m.name)),
            OpAnswer::Offline => {}
        }
    }
    info!(draft = d.id, account, tried = tried.join("; "), "no way to open a session's PR: it waits");
    end(app, &d.id, at, PrEnd::Waits(NO_WAY))?;
    let msg = if tried.is_empty() { NO_WAY.to_owned() } else { format!("{NO_WAY} ({})", tried.join("; ")) };
    Err(refusal(StatusCode::CONFLICT, "no_way", &msg))
}

/// The mirror wrote `repo_id`'s items: the sessions' PRs it has now say
/// where they came from in their threads (module docs).
pub fn noted(app: &Platform, repo_id: u64) {
    let drafts = match app.db.pr_drafts_to_note(repo_id) {
        Ok(d) => d,
        Err(e) => return warn!(repo = repo_id, error = format!("{e:#}"), "reading sessions' PRs to note"),
    };
    for d in drafts {
        match crate::chat::session_card::note_pr(app, &d) {
            Ok(Some((room, _))) => {
                info!(draft = d.id, session = d.session, room, "a session's PR's thread says where it came from")
            }
            Ok(None) => {}
            Err(e) => warn!(draft = d.id, error = format!("{e:#}"), "couldn't say in a PR's thread where it came from"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_and_numbers() {
        for ok in ["main", "release/1.2", "arugula/abcd1234-fix"] {
            assert!(good_branch(ok), "{ok}");
        }
        for bad in ["", "-x", "a..b", "a b", "a/", "x.lock", "a~1", "/main", "a@{1}"] {
            assert!(!good_branch(bad), "{bad}");
        }
        assert_eq!(number_of("https://github.com/acme/app/pull/12"), Some(12));
        assert_eq!(number_of("https://github.com/acme/app/issues/12"), None);
        assert_eq!(back_path("//evil"), "/");
    }
}
