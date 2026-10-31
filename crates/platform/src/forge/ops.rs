//! Typed forge operations (M169, #1215): `POST /api/forge/ops` takes a
//! [`wire::Call`] (an operation's name, its version and its arguments) and
//! answers a [`wire::Answer`]. What each does is the routes' it's built on
//! ([`items`], [`drafts`]), with their viewer checks: this only finds the
//! item or project a call names by `OWNER/NAME` and number.
//!
//! - **Versions:** a call for one this platform doesn't speak is a 400.
//! - **Sends** are a person's (aru-041): a machine is refused. A person's
//!   goes out as them through one of their machines ([`send`], M168): the
//!   CLI has no browser for GitHub's authorization. One that isn't a draft
//!   yet is made one first, so it waits when it can't go.
//! - **The inbox** (M171): `inbox.list` is `GET /api/inbox`'s, and
//!   `inbox.mark` a person's, as `POST /api/inbox/{item}` is: a machine is
//!   refused.
//! - A repo the reader may not read is no linked repo to them, as
//!   [`drafts::repo`] says it, and an item they may not read no item.

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal, reply};
use arugula_control_wire::forge_ops::{self as wire, Op};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Serialize;
use serde_json::Value;

use arugula_control_wire::projects::NewDraft;

use super::{
    drafts::{self, Drafter, DraftsQuery},
    inbox,
    items::{self, ItemsQuery},
    send,
    viewer::{self, Viewer},
};
use crate::{
    Platform,
    db::forge::{ItemFilter, LinkedRepo},
};

fn not_linked(repo: &str) -> ApiError {
    err(StatusCode::NOT_FOUND, &format!("{repo} isn't linked to a project you can read it in"))
}

fn answer<T: Serialize>(op: &Op, v: u32, result: T) -> Result<Json<Value>, ApiError> {
    reply(&wire::Answer { op: op.name().to_owned(), v, result })
}

/// `POST /api/forge/ops` (module docs).
pub async fn call(
    State(app): State<Arc<Platform>>,
    d: Drafter,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let name = body["op"].as_str().unwrap_or_default().to_owned();
    if !wire::NAMES.contains(&name.as_str()) {
        return Err(err(StatusCode::BAD_REQUEST, &format!("no forge operation {name:?}: {}", wire::NAMES.join(", "))));
    }
    let c: wire::Call =
        serde_json::from_value(body).map_err(|e| err(StatusCode::BAD_REQUEST, &format!("{name}: {e}")))?;
    let (op, v) = (c.op, c.v);
    if v == 0 || v > op.version() {
        return Err(err(StatusCode::BAD_REQUEST, &format!("{name} v{v}: this control speaks v{}", op.version())));
    }
    if op.sends() {
        if d.machine.is_some() {
            return Err(refusal(
                StatusCode::FORBIDDEN,
                wire::AGENTS_DRAFT,
                &format!("{name} sends, and only a person sends: draft it (draft.create) for them to send"),
            ));
        }
        let sent = sends(&app, d, &op).await?;
        return answer(&op, v, sent);
    }
    if let (Op::InboxMark(_), Some(_)) = (&op, &d.machine) {
        return Err(err(StatusCode::FORBIDDEN, "inbox.mark is a person's: an agent reads the inbox (inbox.list)"));
    }
    if let Op::DraftCreate(new) = &op {
        let Json(made) = drafts::create(State(app.clone()), d, Json(new.clone())).await?;
        return answer(&op, v, made);
    }
    let viewer = Viewer::of(&app, &d.account).await?;
    let st = || State(app.clone());
    match &op {
        Op::PrGet(r) | Op::IssueGet(r) => {
            let kind = if matches!(op, Op::PrGet(_)) { "pr" } else { "issue" };
            let id = item_id(&app, &viewer, r, Some(kind)).await?;
            let Json(item) = items::get(st(), viewer.clone(), Path(id.clone())).await?;
            let Json(events) = items::events(st(), viewer.clone(), Path(id.clone())).await?;
            let Json(drafts) = drafts::item(st(), viewer, Path(id), Query(DraftsQuery::default())).await?;
            answer(&op, v, wire::ItemView { item, events: events.events, drafts: drafts.drafts, note: None })
        }
        Op::TrackerGet(r) => {
            let project = r.project.as_deref().unwrap_or("personal");
            let item = items::plane_get(&app, &viewer, project, &r.key, r.number).await?;
            let note = "A Plane item's body and comments aren't mirrored: open its url in Plane to read them.";
            answer(&op, v, wire::ItemView { item, events: vec![], drafts: vec![], note: Some(note.to_owned()) })
        }
        Op::ChecksGet(r) => {
            let id = item_id(&app, &viewer, r, None).await?;
            let Json(checks) = items::checks(st(), viewer, Path(id)).await?;
            answer(&op, v, checks)
        }
        Op::ItemList(q) => {
            let project = match (&q.project, &q.repo) {
                (Some(p), _) => p.clone(),
                (None, Some(repo)) => readable_link(&app, &viewer, repo).await?.0.project_id,
                (None, None) => "personal".into(),
            };
            let query = ItemsQuery {
                repo: q.repo.clone(),
                kind: q.kind.clone(),
                state: q.state.clone(),
                author: q.author.clone(),
                review_requested: q.review_requested.clone(),
                q: q.q.clone(),
                limit: q.limit,
                provider: q.provider.clone(),
                tracker: q.tracker.clone(),
            };
            let Json(listed) = items::list(st(), viewer, Path(project), Query(query)).await?;
            answer(&op, v, listed)
        }
        Op::DraftList(q) => {
            let query = DraftsQuery {
                repo: q.repo.clone(),
                number: q.number,
                mine: q.mine,
                state: q.state.clone(),
                limit: q.limit,
            };
            let project = match (&q.project, &q.repo) {
                (Some(p), _) => Some(p.clone()),
                (None, Some(repo)) => Some(readable_link(&app, &viewer, repo).await?.0.project_id),
                (None, None) => None,
            };
            let Json(listed) = match project {
                Some(p) => drafts::project(st(), viewer, Path(p), Query(query)).await?,
                None => drafts::mine(st(), viewer, Query(query)).await?,
            };
            answer(&op, v, listed)
        }
        Op::InboxList(q) => {
            let query = inbox::InboxQuery { done: q.done, limit: q.limit };
            let who = Drafter { account: d.account.clone(), machine: d.machine.clone() };
            let Json(listed) = inbox::list(st(), who, Query(query)).await?;
            answer(&op, v, listed)
        }
        Op::InboxMark(m) => {
            let at = wire::ItemRef { repo: m.repo.clone(), number: m.number };
            let id = item_id(&app, &viewer, &at, None).await?;
            let b = arugula_control_wire::projects::InboxMark { reason: m.reason.clone(), seen: m.seen, done: m.done };
            answer(&op, v, inbox::mark_for(&app, &d.account, id, &b).await?)
        }
        Op::DraftGet(r) => {
            let Json(one) = drafts::get(st(), viewer, Path(r.draft.clone())).await?;
            answer(&op, v, one)
        }
        Op::DraftDiscard(r) => {
            let Json(gone) = drafts::discard(st(), viewer, Path(r.draft.clone())).await?;
            answer(&op, v, gone)
        }
        // Answered above.
        Op::DraftCreate(_)
        | Op::DraftSend(_)
        | Op::PrComment(_)
        | Op::PrReview(_)
        | Op::IssueCreate(_)
        | Op::IssueComment(_) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, "unreachable")),
    }
}

/// A person's send (M168): a draft's, through [`send::send`] by one of
/// their machines (a CLI has no browser to authorize in). A comment,
/// review or issue that isn't a draft yet is made one first, so one that
/// can't go now waits, and the refusal says which.
async fn sends(app: &Arc<Platform>, d: Drafter, op: &Op) -> Result<wire::Sent, ApiError> {
    let account = d.account.clone();
    let new = match op {
        Op::DraftSend(_) => None,
        Op::PrComment(c) | Op::IssueComment(c) => Some(NewDraft {
            repo: c.repo.clone(),
            number: Some(c.number),
            kind: "comment".into(),
            body: c.body.clone(),
            ..Default::default()
        }),
        Op::PrReview(r) => Some(NewDraft {
            repo: r.repo.clone(),
            number: Some(r.number),
            kind: "review".into(),
            event: Some(r.event.clone()),
            body: r.body.clone(),
            ..Default::default()
        }),
        Op::IssueCreate(i) => Some(NewDraft {
            repo: i.repo.clone(),
            kind: "issue".into(),
            title: Some(i.title.clone()),
            body: i.body.clone(),
            ..Default::default()
        }),
        _ => return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "not a send")),
    };
    let (id, made) = match (op, new) {
        (Op::DraftSend(r), _) => (r.draft.clone(), false),
        (_, Some(new)) => {
            let Json(made) = drafts::create(State(app.clone()), d, Json(new)).await?;
            (made.draft.id, true)
        }
        _ => return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "not a send")),
    };
    match send::send(app, &account, &id, send::How::Machine).await {
        Ok(s) => Ok(wire::Sent { url: s.draft.sent_url, draft: Some(id) }),
        Err(ApiError(status, msg, reason)) if made => {
            let msg = format!("{msg} It waits as draft {id}: `arugula draft send {id}` sends it.");
            Err(ApiError(status, msg, reason))
        }
        Err(e) => Err(e),
    }
}

/// A linked repo the viewer may read, by name, and the check that says so.
async fn readable_link(app: &Platform, v: &Viewer, repo: &str) -> Result<(LinkedRepo, viewer::Readable), ApiError> {
    let name = repo.trim();
    if !super::good_repo(name) {
        return Err(err(StatusCode::BAD_REQUEST, "repo: OWNER/NAME"));
    }
    let link = app.db.linked_repo_named(name)?.ok_or_else(|| not_linked(name))?;
    let c = viewer::check(app, v, std::slice::from_ref(&link)).await?;
    if !c.readable.has(link.repo_id) {
        return Err(not_linked(name));
    }
    Ok((link, c.readable))
}

/// The mirror's id for `OWNER/NAME#N`, when the viewer may read it (and
/// it's a `kind`, when one is asked).
async fn item_id(app: &Platform, v: &Viewer, r: &wire::ItemRef, kind: Option<&str>) -> Result<String, ApiError> {
    let (link, readable) = readable_link(app, v, &r.repo).await?;
    let f = ItemFilter { number: Some(r.number), limit: 1, ..Default::default() };
    let at = format!("{}#{}", link.full_name, r.number);
    let i = app
        .db
        .forge_items(&readable, &f)?
        .into_iter()
        .next()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, &format!("{at} isn't in the mirror (yet)")))?;
    match kind {
        Some(k) if i.kind != k => {
            let (is, op) = if i.kind == "pr" { ("a PR", "pr.get") } else { ("an issue", "issue.get") };
            Err(err(StatusCode::NOT_FOUND, &format!("{at} is {is}: {op} reads it")))
        }
        _ => Ok(i.id),
    }
}
