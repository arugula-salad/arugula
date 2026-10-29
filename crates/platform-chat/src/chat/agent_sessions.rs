//! A person's agent starts, answers and prompts agent sessions (Fleet 3,
//! aru-121): the routes MCP's `session_start` and `session_reply` call,
//! signed by the agent's machine.
//!
//! - **By grant, asked every call** ([`agent_object_room`],
//!   [`agent_room`], [`agent_may`]): `sessions.start` over the issue's or
//!   the channel's project to start one, `sessions.reply` over the
//!   session's project to reply. A revoked grant refuses the next call.
//!   Neither needs `threads.post`: the message is the ask or the reply, and
//!   nothing else.
//! - **Exactly as the owner's own words.** Each posts one message as the
//!   owner, labelled with the machine ("<owner>'s agent on <machine>"),
//!   and [`super::sessions`] takes it as it takes the owner's: the same
//!   checks (an owner or editor, on the hosted beta's list), the same box,
//!   repo and which-repo ask, the owner as the asker and the payer. What's
//!   different is the card's "started by" ([`super::sessions::agent_asker`])
//!   and that an agent's reply answers a session's open question.
//! - **Says what happened.** A start answers with the session it made, or
//!   the platform's lines since in the thread (why not, or the which-repo
//!   question, which waits on the owner, or on this agent's reply naming
//!   one); a reply with whether it answered, was queued, or was refused.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::{
    chat::{ChatPost, SYSTEM_VIA},
    sessions::{AgentSessionReplied, AgentSessionReply, AgentSessionStart, AgentSessionStarted, ask_via},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{
    J, agent_object_room, agent_room, post_replied,
    session_card::threaded,
    sessions::{AGENT_ON_THIS, MENTION, NOT_HERE, Replied, asks, works_in},
};
use crate::{
    Platform,
    auth::Machine,
    db::projects::{SESSIONS_REPLY, SESSIONS_START},
    grants::{AgentScope, agent_may, owner_reads},
    sessions::hydrate,
};

/// The longest ask or reply an agent sends, in characters: a person's
/// message's limit is chat's, and the thread is where the rest goes.
const TEXT_MAX: usize = 20_000;

fn text_of(t: &str) -> Result<&str, ApiError> {
    let t = t.trim();
    if t.chars().count() > TEXT_MAX {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "at most 20000 characters"));
    }
    Ok(t)
}

/// `POST /api/daemon/sessions` [`AgentSessionStart`]: the agent's
/// `@arugula` on a native issue or in a project's channel, as its owner
/// (module docs).
pub async fn start(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Json(b): Json<AgentSessionStart>,
) -> J<AgentSessionStarted> {
    let prompt = text_of(b.prompt.as_deref().unwrap_or_default())?;
    let issue = match (&b.issue, b.number, &b.project) {
        (Some(_), Some(_), _) => return Err(err(StatusCode::BAD_REQUEST, "an issue's id or its number, not both")),
        (Some(id), None, _) => Some(id.trim().to_owned()),
        (None, Some(n), Some(p)) => {
            Some(app.db.issue_by_number(p.trim(), n)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such issue"))?.id)
        }
        (None, Some(_), None) => return Err(err(StatusCode::BAD_REQUEST, "an issue's number needs its project")),
        (None, None, _) => None,
    };
    let (room, principal, parent) = match (issue, b.room.as_deref().map(str::trim)) {
        (Some(_), Some(_)) => return Err(err(StatusCode::BAD_REQUEST, "an issue or a room, not both")),
        (None, None) => return Err(err(StatusCode::BAD_REQUEST, "say where: an issue, or a room")),
        (Some(id), None) => {
            if !id.starts_with("iss_") {
                return Err(err(StatusCode::NOT_FOUND, "no such issue"));
            }
            if b.thread.is_some() {
                return Err(err(StatusCode::BAD_REQUEST, "an issue's thread is the issue: no thread"));
            }
            let (room, _, principal) = agent_object_room(&app, &d, SESSIONS_START, &id).await?;
            (room, principal, None)
        }
        (None, Some(room)) => {
            let (room, _, principal) = agent_room(&app, &d, SESSIONS_START, room).await?;
            if !works_in(&room) || room.kind != "channel" {
                return Err(err(StatusCode::BAD_REQUEST, NOT_HERE));
            }
            (room, principal, b.thread)
        }
    };
    // The thread it asks in: a channel's message is its own thread's first.
    let key = |seq: i64| match (parent, room.kind.as_str()) {
        (Some(p), _) => p,
        (None, "object") => 0,
        (None, _) => seq,
    };
    if let Some(p) = parent.or((room.kind == "object").then_some(0))
        && let Some(s) = app.db.session_on_thread(&room.id, p as u64)?
    {
        return Err(err(
            StatusCode::CONFLICT,
            &format!("an agent session is going there ({}): session_reply prompts it", s.id),
        ));
    }
    let text = match (prompt.is_empty(), room.kind.as_str()) {
        (true, "object") => AGENT_ON_THIS.to_owned(),
        (true, _) => return Err(err(StatusCode::BAD_REQUEST, "say what to ask @arugula")),
        (false, _) if asks(prompt) => prompt.to_owned(),
        (false, _) => format!("@{MENTION} {prompt}"),
    };
    let post = ChatPost { text, parent, ..Default::default() };
    let (posted, _) = post_replied(&app, &d.account, Some(&principal), None, &room.id, post).await?;
    let thread = key(posted.seq);
    let session = app
        .db
        .session_on_thread(&room.id, thread as u64)?
        .filter(|s| s.asker == d.account && s.asker_agent.as_deref() == Some(d.daemon.as_str()))
        .map(|s| s.id);
    // Without one, what the platform said since: why, or which repo.
    let said = match session {
        Some(_) => vec![],
        None => {
            let ask = ask_via(crate::sessions::asks::REPO);
            hydrate::messages(&app, &room, thread as u64)?
                .into_iter()
                .filter(|m| {
                    m.seq > posted.seq
                        && m.author.is_empty()
                        && m.deleted.is_none()
                        && matches!(m.via.as_deref(), Some(v) if v == SYSTEM_VIA || v == ask)
                })
                .map(|m| m.text)
                .collect()
        }
    };
    Ok(Json(AgentSessionStarted { room: room.id, seq: posted.seq, thread, session, said }))
}

/// `POST /api/daemon/sessions/{ses}/reply` [`AgentSessionReply`]: the
/// agent's reply in a session's thread, as its owner (module docs).
pub async fn reply(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(ses): Path<String>,
    Json(b): Json<AgentSessionReply>,
) -> J<AgentSessionReplied> {
    let none = || err(StatusCode::NOT_FOUND, "no such session");
    let s = app.db.session(&ses)?.ok_or_else(none)?;
    let scope = AgentScope::Project(s.project.clone());
    // Outside its owner's projects it isn't there, as for the owner.
    if owner_reads(&app, &d.account, &scope, false).await?.is_none() {
        return Err(none());
    }
    let principal = agent_may(&app, &d, SESSIONS_REPLY, &scope).await?;
    let text = text_of(&b.text)?;
    if text.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "say something"));
    }
    if s.state.terminal() {
        return Err(err(StatusCode::CONFLICT, &format!("session {} is {}: it takes no more", s.id, s.state.as_str())));
    }
    // Only the thread's own session takes its replies: not a Try 3's
    // sibling nobody picked, nor a review.
    if app.db.session_on_thread(&s.room, s.parent_seq)?.is_none_or(|t| t.id != s.id) {
        return Err(err(
            StatusCode::CONFLICT,
            &format!("session {} isn't its thread's (a sibling or a review): reply to the thread's own", s.id),
        ));
    }
    let room = app.db.chat_room(&s.room)?.ok_or_else(none)?;
    let parent = i64::try_from(s.parent_seq).ok().filter(|p| *p > 0 && threaded(&room.kind));
    let post = ChatPost { text: text.to_owned(), parent, ..Default::default() };
    let (posted, replied) = post_replied(&app, &d.account, Some(&principal), None, &room.id, post).await?;
    let mut out = AgentSessionReplied {
        session: s.id.clone(),
        room: room.id,
        seq: posted.seq,
        did: "chat".into(),
        prompt: None,
        permission: false,
        why: None,
    };
    match replied {
        Some(Replied::Answered) => out.did = "answered".into(),
        Some(Replied::Queued { prompt, permission }) => {
            out.did = "queued".into();
            out.prompt = Some(prompt);
            out.permission = permission;
        }
        Some(Replied::Refused(why)) => {
            out.did = "refused".into();
            out.why = Some(why);
        }
        Some(Replied::Chat) | None => {}
    }
    Ok(Json(out))
}
