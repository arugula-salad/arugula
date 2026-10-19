//! `threads` (M61): what the people working on a pane or a session say, read,
//! posted and marked read. Over HTTP (the web, `arugula`'s clients) each
//! person reads and posts as themselves. They're people's only: agents are
//! out of chat (#1095), so MCP has no tools for them.

use arugula_proto::{
    ThreadTarget,
    api::{Empty, Invitable, ThreadMessages, ThreadPostRequest, ThreadPosted, ThreadReadRequest, Unreached},
    op::ops::{ThreadGet, ThreadPost, ThreadRead},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError, Via};
use crate::{
    acl::Principal,
    mux::{Api, Cmd},
    server::App,
};

fn thread_err(e: crate::mux::ThreadError) -> OpError {
    OpError::Status(StatusCode::from_u16(e.0).unwrap_or(StatusCode::BAD_REQUEST), e.1)
}

fn gone() -> OpError {
    OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())
}

/// The person asking, as `Via::Http` has them; an agent's call is refused.
fn person<'a>(cx: &'a Cx<'_>) -> Result<(&'a Principal, &'a Option<Principal>, bool), OpError> {
    match &cx.via {
        Via::Http { who, acting, owner, .. } => Ok((who, acting, *owner)),
        Via::Mcp(_) => Err(OpError::Forbidden("threads are people's: an agent doesn't read or post in them".into())),
    }
}

impl Handle for ThreadGet {
    /// A thread's messages, as the caller may read them.
    async fn handle(cx: &Cx<'_>, target: ThreadTarget, _: Empty) -> Result<ThreadMessages, OpError> {
        let who = person(cx)?.0.clone();
        let messages =
            cx.app.mux.api(|r| Api::ThreadGet(target, who, r)).await.ok_or_else(gone)?.map_err(thread_err)?;
        Ok(ThreadMessages { target, messages })
    }
}

impl Handle for ThreadPost {
    /// Post in a thread. An `@agent` in it is text like any other `@`.
    async fn handle(cx: &Cx<'_>, target: ThreadTarget, req: ThreadPostRequest) -> Result<ThreadPosted, OpError> {
        let (who, acting, owner) = person(cx)?;
        let post = crate::mux::ThreadPost {
            target,
            who: who.clone(),
            acting: acting.clone(),
            text: req.text,
            quote: req.quote,
        };
        let (msg, mut unreached) =
            cx.app.mux.api(|r| Api::ThreadPost(post, r)).await.ok_or_else(gone)?.map_err(thread_err)?;
        // The owner, who sees every grant and roster already, is offered to
        // invite whom an @ named but who can't read the thread (#297). Nobody
        // else's post does any of this, so theirs says nothing about who exists.
        let invitable = if owner { Some(invitable(cx.app, target, &mut unreached).await) } else { None };
        Ok(ThreadPosted { message: msg, unreached, invitable })
    }
}

impl Handle for ThreadRead {
    async fn handle(cx: &Cx<'_>, target: ThreadTarget, req: ThreadReadRequest) -> Result<Empty, OpError> {
        let (who, acting, _) = person(cx)?;
        crate::labs::chat().map_err(|e| OpError::Status(StatusCode::NOT_IMPLEMENTED, e))?;
        cx.app.mux.send(Cmd::Api(Api::ThreadRead(target, who.clone(), acting.clone(), req.upto)));
        Ok(Empty {})
    }
}

/// Whom an owner's `@`s that reached nobody name, of those an invite may
/// name, who can't read the thread and could once invited (not on a
/// private pane's), or for a conversation, added. Their tokens leave
/// `unreached`: the offer says it.
async fn invitable(app: &App, target: ThreadTarget, unreached: &mut Vec<Unreached>) -> Vec<Invitable> {
    let tokens: Vec<String> = unreached.iter().map(|u| u.token.clone()).collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    // A conversation's member must reach this machine already (#855):
    // adding one doesn't share anything with them.
    let conv = matches!(target, ThreadTarget::Conv(_));
    let named: Vec<(String, crate::invite::Person)> = crate::invite::nameable(app)
        .into_iter()
        .filter(|p| !conv || app.acl.knows(&Principal::User { id: p.id.clone(), name: String::new(), pic: None }))
        .filter_map(|p| {
            let t = tokens.iter().find(|t| crate::labs::thread_names(t, &p.id, &p.name))?;
            Some((t.clone(), p))
        })
        .collect();
    if named.is_empty() {
        return Vec::new();
    }
    let ids = named.iter().map(|(_, p)| p.id.clone()).collect();
    let Some(reads) = app.mux.api(|r| Api::CanRead(target, ids, r)).await.flatten() else { return Vec::new() };
    let out: Vec<(String, crate::invite::Person)> =
        named.into_iter().zip(reads).filter(|(_, reads)| !reads).map(|(n, _)| n).collect();
    unreached.retain(|u| !out.iter().any(|(t, _)| *t == u.token));
    // One taken for another (a login by an account's name) says whom.
    out.into_iter().map(|(token, p)| Invitable { token, who: p.id, name: p.name, merged: p.merged }).collect()
}
