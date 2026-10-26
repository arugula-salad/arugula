//! Sending a draft as its person (M168, #1214; "Sending as a person uses a
//! one-use user token, falling back to their machine", aru-085). The
//! comment, review or issue goes out on GitHub as the person, never as
//! `arugula[bot]`, so the App's installation tokens ([`super::WriteToken`])
//! aren't used here: every send has a person behind it.
//!
//! - **Who sends:** the draft's own person, a person or a CLI device
//!   ([`Sender`]), while they may still read its repo (M165). A machine is
//!   refused, grant or not: "Agents draft, people send" (aru-041). An
//!   agent's draft is sent by its person like any other.
//! - **A one-use user token** ([`How::Browser`], then [`How::Token`]): the
//!   page asks for GitHub's authorization of the App, which the hub runs
//!   (it owns the App's OAuth client, aru-078): silent once the person has
//!   authorized the App, which signing in with GitHub did. GitHub comes back
//!   to the hub's `/auth/github/callback`, which swaps the code for a user
//!   token and hands it here (`PlatformPort::send_draft`) for one call.
//!   Before it, `GET /user` must be the account's own GitHub id. The mint
//!   is a `github_token_mints` row (kind `user`, never the token); the
//!   token is never stored, logged, cached or put in a URL, and the hub
//!   revokes it once this answers.
//! - **Their machine** ([`How::Machine`]): the CLI's way, and the way when
//!   the browser hop fails or the token may not write. One of the person's
//!   own machines on control's relay sends it with their own `gh`, as a
//!   typed op through the hub (`HubPort::daemon_op`, `forge.send`). A
//!   machine that's away, too old to know the op, or has no `gh` login is
//!   passed over for the next.
//! - **Otherwise** it's refused with [`wire::NO_WAY`], and the draft waits,
//!   saying so. A draft is never lost: GitHub's refusal leaves it `failed`,
//!   to send again.
//!
//! A send holds its draft while it goes ([`crate::db::forge::SENDING_MS`]), so two at once
//! (the phone and a block's card) never post it twice.
//!
//! Machines that send at a person's word from a block's card say so here
//! ([`machine`]): the platform only records it.

use std::sync::Arc;

use arugula_control_common::{ApiError, Principal, err, refusal};
use arugula_control_wire::{
    forge::{DaemonOp, ForgeSend, OpAnswer},
    projects as wire,
};
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::{
    drafts::{no_draft, readable, wire_drafts},
    viewer::Viewer,
};
use crate::{
    Platform,
    auth::{Caller, Machine},
    db::forge::{DraftRow, MintKind, NewMint, SendEnd},
};

/// What a machine that asks to send is told.
pub const AGENTS_DRAFT: &str = "agents draft, people send: a machine can't send a draft; its person sends it from the platform page, the CLI or the block's card";

/// A send's way.
#[derive(Debug)]
pub enum How {
    /// GitHub's authorization in the person's browser, coming back to the
    /// platform page's `back`.
    Browser { back: String },
    /// The user token GitHub gave the hub for this send.
    Token(UserToken),
    /// One of their online machines.
    Machine,
}

/// A person's GitHub user token, for one call. Not `Clone`, never printed,
/// and it crosses from the hub only inside a signed internal call.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserToken(String);

impl UserToken {
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    pub(crate) fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for UserToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UserToken(…)")
    }
}

/// What a send did, as the hub's callback hears it: the draft sent, or why
/// not (the draft says it too).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(default)]
    pub sent: Option<wire::DraftSent>,
    #[serde(default)]
    pub refused: Option<String>,
    /// A session's PR (M259) that was opened: its URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<String>,
}

impl From<Result<wire::DraftSent, ApiError>> for Outcome {
    fn from(r: Result<wire::DraftSent, ApiError>) -> Self {
        match r {
            Ok(s) => Outcome { sent: Some(s), refused: None, pr: None },
            Err(e) => Outcome { sent: None, refused: Some(e.1), pr: None },
        }
    }
}

/// A person, or a CLI device for its account: who may send. A machine is
/// refused ([`AGENTS_DRAFT`]), whatever grants it holds.
pub struct Sender {
    pub account: String,
}

impl FromRequestParts<Arc<Platform>> for Sender {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<Platform>) -> Result<Self, ApiError> {
        let Caller(who) = Caller::from_request_parts(parts, app).await?;
        match who {
            Principal::Person { account } | Principal::Cli { account, .. } => Ok(Sender { account }),
            Principal::Daemon { account, daemon } => {
                warn!(what = "draft_send", %account, %daemon, why = "a machine", "refused");
                Err(refusal(StatusCode::FORBIDDEN, "agents_draft", AGENTS_DRAFT))
            }
        }
    }
}

/// `POST /api/forge/drafts/{id}/send` ([`wire::SendDraft`]): module docs.
pub async fn route(
    State(app): State<Arc<Platform>>,
    s: Sender,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<wire::DraftSent>, ApiError> {
    let b: wire::SendDraft = if body.is_empty() {
        Default::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?
    };
    let how = match b.via.as_deref() {
        Some("browser") => How::Browser { back: b.back.unwrap_or_default() },
        None | Some("machine") => How::Machine,
        Some(_) => return Err(err(StatusCode::BAD_REQUEST, "via: browser or machine")),
    };
    send(&app, &s.account, &id, how).await.map(Json)
}

/// A path on the platform page to come back to: only a path, never
/// `//elsewhere`.
fn back_path(back: &str) -> String {
    let ok = back.starts_with('/') && !back.starts_with("//") && !back.contains('\\') && back.len() <= 512;
    if ok { back.to_owned() } else { "/".into() }
}

/// Send `account`'s draft `id` as them, `how`: the one way in, for the
/// route, the hub's callback and the typed ops (M169's `draft.send`).
pub async fn send(app: &Platform, account: &str, id: &str, how: How) -> Result<wire::DraftSent, ApiError> {
    let v = Viewer::of(app, account).await?;
    let (repo_id, author) = app.db.draft_target(id)?.ok_or_else(no_draft)?;
    let r = readable(app, &v, repo_id).await?.ok_or_else(no_draft)?;
    let row = app.db.draft(&r, id)?.ok_or_else(no_draft)?;
    if matches!(how, How::Token(_)) {
        // Written down as it arrives (GitHub minted it, whatever becomes
        // of the send): who, what for, never the token.
        app.db.log_mint(&NewMint {
            kind: MintKind::User,
            installation_id: None,
            repo_id: Some(row.repo_id),
            permissions: &json!({}),
            caller: account,
            purpose: purpose(&row.kind),
            at: now_ms(),
        })?;
    }
    if author != account {
        return Err(err(StatusCode::FORBIDDEN, "only its person sends a draft: it goes out as them"));
    }
    let now = now_ms();
    match row.state.as_str() {
        "sent" => return Err(err(StatusCode::CONFLICT, "it was sent already")),
        "discarded" => return Err(err(StatusCode::CONFLICT, "it was discarded")),
        _ if row.sending(now) => return Err(err(StatusCode::CONFLICT, "it's being sent")),
        _ => {}
    }
    let how = match how {
        How::Browser { back } => match app.hub.send_authorization(account, id, &back_path(&back)).await {
            Ok(url) => {
                let draft = wire_drafts(app, vec![row]).await?.remove(0);
                return Ok(wire::DraftSent { draft, authorize: Some(url), via: None, machine: None });
            }
            Err(e) => {
                info!(draft = id, error = format!("{e:#}"), "no browser hop for a send: trying a machine");
                How::Machine
            }
        },
        how => how,
    };
    let held = app.db.hold_draft(&r, id, account, now)?.ok_or_else(|| err(StatusCode::CONFLICT, "it's being sent"))?;
    let at = held.sent_at.unwrap_or(now);
    match how {
        How::Token(t) => with_token(app, &v, held, at, t).await,
        _ => by_machine(app, account, held, at, Vec::new()).await,
    }
}

/// End the send holding `id` since `at`, and the draft as the wire says it.
async fn end(app: &Platform, id: &str, at: u64, e: SendEnd<'_>) -> Result<wire::ForgeDraft, ApiError> {
    let row = app.db.end_send(id, at, e, now_ms())?.ok_or_else(|| err(StatusCode::CONFLICT, "another send took it"))?;
    Ok(wire_drafts(app, vec![row]).await?.remove(0))
}

/// What a draft's call writes, for the mint's log.
fn purpose(kind: &str) -> &'static str {
    match kind {
        "review" => "send:review",
        "issue" => "send:issue",
        _ => "send:comment",
    }
}

/// The token's GitHub user id (`GET /user`), or why not. The answer's
/// error never carries the token.
pub(crate) async fn user_of(app: &Platform, api: &str, t: &UserToken) -> Result<u64, String> {
    let r = github(app, reqwest::Method::GET, api, "user", t)
        .send()
        .await
        .map_err(|e| format!("asking GitHub who the token is: {}", e.without_url()))?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    match (status.is_success(), v["id"].as_u64()) {
        (true, Some(id)) => Ok(id),
        _ => Err(format!("GitHub said {status} for the token's user")),
    }
}

pub(crate) fn github(
    app: &Platform,
    method: reqwest::Method,
    api: &str,
    path: &str,
    t: &UserToken,
) -> reqwest::RequestBuilder {
    app.http
        .request(method, format!("{api}/{path}"))
        .bearer_auth(t.secret())
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "arugula-control")
}

/// Why a call failed, and whether this token may not write there (the
/// App's permissions, or the person's own), so a machine may.
struct CallFailed {
    error: String,
    denied: bool,
}

/// The send's one call with the user token: the comment's, review's or
/// issue's URL.
async fn the_call(app: &Platform, api: &str, d: &DraftRow, t: &UserToken) -> Result<Option<String>, CallFailed> {
    let repo = &d.full_name;
    let n = d.number.unwrap_or(0);
    let (path, body) = match d.kind.as_str() {
        "review" => {
            let mut b = json!({ "event": d.event.as_deref().unwrap_or("COMMENT") });
            if !d.body.trim().is_empty() {
                b["body"] = json!(d.body);
            }
            (format!("repos/{repo}/pulls/{n}/reviews"), b)
        }
        "issue" => (format!("repos/{repo}/issues"), json!({ "title": d.title, "body": d.body })),
        _ => (format!("repos/{repo}/issues/{n}/comments"), json!({ "body": d.body })),
    };
    let r = github(app, reqwest::Method::POST, api, &path, t)
        .json(&body)
        .send()
        .await
        .map_err(|e| CallFailed { error: format!("GitHub couldn't be reached: {}", e.without_url()), denied: false })?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    if status.is_success() {
        return Ok(v["html_url"].as_str().map(str::to_owned));
    }
    let said = v["message"].as_str().unwrap_or("");
    Err(CallFailed {
        error: format!("GitHub said {}: {said}", status.as_u16()),
        // GitHub answers 404 for what a token may not see.
        denied: matches!(status.as_u16(), 401 | 403 | 404),
    })
}

/// [`How::Token`]: check who the token is, log its mint, make the one call,
/// and drop it. A token that may not write there leaves it to a machine.
async fn with_token(
    app: &Platform,
    v: &Viewer,
    d: DraftRow,
    at: u64,
    t: UserToken,
) -> Result<wire::DraftSent, ApiError> {
    let Some(api) = app.github.as_ref().map(|g| g.api.clone()) else {
        drop(t);
        return by_machine(app, &v.account, d, at, vec!["this control has no GitHub App".into()]).await;
    };
    // The person's own GitHub account, by id: the one they signed in with.
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
        warn!(draft = d.id, account = v.account, why, "a user token refused for a send");
        end(app, &d.id, at, SendEnd::Waits(&why)).await?;
        return Err(err(StatusCode::FORBIDDEN, &why));
    }
    let r = the_call(app, &api, &d, &t).await;
    drop(t);
    match r {
        Ok(url) => {
            info!(draft = d.id, account = v.account, repo = d.repo_id, via = "token", "a draft sent as its person");
            let draft = end(app, &d.id, at, SendEnd::Sent(url.as_deref())).await?;
            Ok(wire::DraftSent { draft, authorize: None, via: Some("token".into()), machine: None })
        }
        Err(f) if f.denied => {
            info!(draft = d.id, error = f.error, "the user token can't send it: trying a machine");
            by_machine(app, &v.account, d, at, vec![format!("as you through GitHub: {}", f.error)]).await
        }
        Err(f) => {
            warn!(draft = d.id, error = f.error, "a send failed");
            end(app, &d.id, at, SendEnd::Failed(&f.error)).await?;
            Err(refusal(StatusCode::BAD_GATEWAY, "github_refused", &f.error))
        }
    }
}

/// The op a machine is asked to do for a draft.
fn op_of(d: &DraftRow, account: &str) -> DaemonOp {
    DaemonOp::ForgeSend(ForgeSend {
        draft: d.id.clone(),
        account: account.to_owned(),
        repo: d.full_name.clone(),
        number: d.number,
        kind: d.kind.clone(),
        event: d.event.clone(),
        title: d.title.clone(),
        body: d.body.clone(),
        ..Default::default()
    })
}

/// [`How::Machine`]: each of the person's own machines in turn (the one
/// that drafted it first) until one sends it, GitHub refuses it, or none
/// is left. `tried` says what was tried before.
async fn by_machine(
    app: &Platform,
    account: &str,
    d: DraftRow,
    at: u64,
    mut tried: Vec<String>,
) -> Result<wire::DraftSent, ApiError> {
    let ids = app.hub.machines_for(account).await?;
    let mut mine: Vec<_> =
        app.cache.daemons(&*app.hub, &ids).await?.into_values().filter(|m| m.account == account).collect();
    mine.sort_by_key(|m| (d.agent_machine.as_deref() != Some(m.id.as_str()), m.name.clone()));
    let op = op_of(&d, account);
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
                    "a draft sent as its person"
                );
                let draft = end(app, &d.id, at, SendEnd::Sent(url.as_deref())).await?;
                return Ok(wire::DraftSent {
                    draft,
                    authorize: None,
                    via: Some("machine".into()),
                    machine: Some(m.name),
                });
            }
            OpAnswer::Failed { error } => {
                let error = format!("{}: {error}", m.name);
                warn!(draft = d.id, machine = m.id, error, "a send failed");
                end(app, &d.id, at, SendEnd::Failed(&error)).await?;
                return Err(refusal(StatusCode::BAD_GATEWAY, "github_refused", &error));
            }
            OpAnswer::Cant { why } => tried.push(format!("{}: {why}", m.name)),
            OpAnswer::Unknown => tried.push(format!("{}: its Arugula is too old to send drafts", m.name)),
            OpAnswer::Offline => {}
        }
    }
    info!(draft = d.id, account, tried = tried.join("; "), "no way to send a draft: it waits");
    end(app, &d.id, at, SendEnd::Waits(wire::NO_WAY)).await?;
    let msg =
        if tried.is_empty() { wire::NO_WAY.to_owned() } else { format!("{} ({})", wire::NO_WAY, tried.join("; ")) };
    Err(refusal(StatusCode::CONFLICT, "no_way", &msg))
}

/// `POST /api/forge/drafts/{id}/machine` ([`wire::MachineSending`]): a
/// machine sends its owner's draft with `gh` because a person answered its
/// block's card, and says so. `start` holds it, `sent` and `failed` end
/// that. Only the draft's own person's machine, while they may read it.
pub async fn machine(
    State(app): State<Arc<Platform>>,
    m: Machine,
    Path(id): Path<String>,
    Json(b): Json<wire::MachineSending>,
) -> Result<Json<wire::ForgeDraft>, ApiError> {
    let v = Viewer::of(&app, &m.account).await?;
    let (repo_id, author) = app.db.draft_target(&id)?.ok_or_else(no_draft)?;
    let r = readable(&app, &v, repo_id).await?.ok_or_else(no_draft)?;
    if author != m.account {
        return Err(err(StatusCode::FORBIDDEN, "it's another person's draft: they send it as themselves"));
    }
    let now = now_ms();
    let draft = match b.step.as_str() {
        "start" => {
            let held = app
                .db
                .hold_draft(&r, &id, &author, now)?
                .ok_or_else(|| err(StatusCode::CONFLICT, "it isn't waiting: sent, discarded or being sent already"))?;
            wire_drafts(&app, vec![held]).await?.remove(0)
        }
        "sent" | "failed" => {
            let row = app.db.draft(&r, &id)?.ok_or_else(no_draft)?;
            let at = row
                .sent_at
                .filter(|_| row.sending(now))
                .ok_or_else(|| err(StatusCode::CONFLICT, "no send holds it"))?;
            let error = b.error.as_deref().unwrap_or("the machine couldn't send it");
            let e = if b.step == "sent" { SendEnd::Sent(b.url.as_deref()) } else { SendEnd::Failed(error) };
            info!(draft = id, account = m.account, machine = m.daemon, step = b.step, "a machine's send from a block");
            end(&app, &id, at, e).await?
        }
        _ => return Err(err(StatusCode::BAD_REQUEST, "step: start, sent or failed")),
    };
    Ok(Json(draft))
}
