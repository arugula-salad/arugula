//! `convs` (#855): conversations, threads of their own kept on this machine
//! (their home), with members of their own. Their messages go through the
//! thread operations (`threads.rs`, `conv-c-…` as the target); these make,
//! rename and delete them and add and take out members. The mux checks each
//! against the record (`arugula_mux::labs::convs`): the owner changes it, a
//! member may leave. They're for people: an agent's token is refused.

use arugula_proto::{
    ConvId, Conversation,
    api::{ConvCreateRequest, ConvMemberRemoveRequest, ConvMemberRequest, ConvRenameRequest, Empty},
    op::ops::{ConvCreate, ConvDelete, ConvMemberAdd, ConvMemberRemove, ConvRename},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError, Via};
use crate::{
    acl::Principal,
    mux::{Api, ConvReq},
};

/// Who asks, if it's a person (and chat is in this build): conversations
/// are people's, not agents'.
fn person(cx: &Cx<'_>) -> Result<Principal, OpError> {
    let who = cx.principal()?;
    if let Via::Http { agent: true, .. } = cx.via {
        // The owner's CLI with an agent at it.
        return Err(OpError::Forbidden("an agent doesn't make or change conversations".into()));
    }
    crate::labs::chat().map_err(|e| OpError::Status(StatusCode::NOT_IMPLEMENTED, e))?;
    Ok(who)
}

async fn conv(cx: &Cx<'_>, req: ConvReq) -> Result<Conversation, OpError> {
    let who = person(cx)?;
    let r = cx
        .app
        .mux
        .api(|r| Api::Conv(req, who, r))
        .await
        .ok_or_else(|| OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into()))?;
    r.map_err(|e| OpError::Status(StatusCode::from_u16(e.0).unwrap_or(StatusCode::BAD_REQUEST), e.1))
}

impl Handle for ConvCreate {
    async fn handle(cx: &Cx<'_>, _: (), req: ConvCreateRequest) -> Result<Conversation, OpError> {
        conv(cx, ConvReq::Create { title: req.title }).await
    }
}

impl Handle for ConvRename {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvRenameRequest) -> Result<Conversation, OpError> {
        conv(cx, ConvReq::Rename { id, title: req.title }).await
    }
}

impl Handle for ConvDelete {
    async fn handle(cx: &Cx<'_>, id: ConvId, _: Empty) -> Result<Empty, OpError> {
        conv(cx, ConvReq::Delete { id }).await?;
        cx.app.control.poke();
        Ok(Empty {})
    }
}

impl Handle for ConvMemberAdd {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvMemberRequest) -> Result<Conversation, OpError> {
        // #856: an account that doesn't reach this machine yet comes in by
        // being a member, by the root a checked roster here names for it, or
        // else the one the owner's browser pinned.
        let reach = req.who.strip_prefix("account:").and_then(|account| {
            let root = cx.app.control.enrolled().and_then(|e| e.known_root(account)).or(req.root.clone())?;
            Some((req.name.clone().unwrap_or_default(), root))
        });
        let c = conv(cx, ConvReq::AddMember { id, member: req.who, history: req.history, reach }).await?;
        // Let them in (and have control route them) now.
        cx.app.control.poke();
        Ok(c)
    }
}

impl Handle for ConvMemberRemove {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvMemberRemoveRequest) -> Result<Conversation, OpError> {
        let c = conv(cx, ConvReq::RemoveMember { id, member: req.who }).await?;
        cx.app.control.poke();
        Ok(c)
    }
}
