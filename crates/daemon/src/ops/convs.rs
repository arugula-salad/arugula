//! `convs` (#855): conversations, threads of their own kept on this machine
//! (their home), with members of their own. Their messages go through the
//! thread operations (`threads.rs`, `conv-c-…` as the target); these make,
//! rename and delete them and add and take out members. The mux checks each
//! against the record (`arugula_mux::labs::convs`): the owner changes it, a
//! member may leave. They're for people: an agent's token is refused.
//!
//! Links (#856): a pane's machine keeps the grant that lets a conversation's
//! home drive it (`conv.grant`, on the pane), and the home keeps the link
//! (`conv.link`); `labs::links` has both halves.

use arugula_proto::{
    ConvGrant, ConvId, ConvLinkStatus, Conversation, PaneId,
    api::{
        ConvActRequest, ConvCreateRequest, ConvGrantRequest, ConvGrantRevokeRequest, ConvLinkRequest,
        ConvMemberRemoveRequest, ConvMemberRequest, ConvRenameRequest, ConvUnlinkRequest, Empty,
    },
    op::ops::{
        ConvAct, ConvCreate, ConvDelete, ConvGrantMake, ConvGrantRevoke, ConvGrantsList, ConvLeadStart, ConvLinkAdd,
        ConvLinkClose, ConvLinksGet, ConvMemberAdd, ConvMemberRemove, ConvRename,
    },
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError, Via};
use crate::{
    acl::Principal,
    labs::{events, lead, links},
    mux::{Api, ConvReq},
};

/// Who asks, if it's a person (and chat is in this build): conversations,
/// their links and the grants for them are people's, not agents'.
fn person(cx: &Cx<'_>) -> Result<Principal, OpError> {
    let who = cx.principal()?;
    if let Via::Http { agent: true, .. } = cx.via {
        // The owner's CLI with an agent at it.
        return Err(OpError::Forbidden("an agent doesn't make or change conversations or their links".into()));
    }
    crate::labs::chat().map_err(|e| OpError::Status(StatusCode::NOT_IMPLEMENTED, e))?;
    Ok(who)
}

fn refused((code, why): links::Refusal) -> OpError {
    OpError::Status(StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST), why)
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
        let c = conv(cx, ConvReq::Create { title: req.title }).await?;
        // #858: its lead starts with it. One that can't start (no Claude
        // Code here) leaves the conversation without one, to start later.
        match lead::start(cx.app, c.id).await {
            Ok(c) => Ok(c),
            Err((_, why)) => {
                tracing::warn!(conv = %c.id, why, "a new conversation's lead didn't start");
                Ok(c)
            }
        }
    }
}

impl Handle for ConvLeadStart {
    /// Start its lead, or a new one in place of a lost one: for anyone in
    /// it, as anyone in it may drive through the lead.
    async fn handle(cx: &Cx<'_>, id: ConvId, _: Empty) -> Result<Conversation, OpError> {
        conv(cx, ConvReq::Get { id }).await?;
        lead::start(cx.app, id).await.map_err(refused)
    }
}

impl Handle for ConvRename {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvRenameRequest) -> Result<Conversation, OpError> {
        conv(cx, ConvReq::Rename { id, title: req.title }).await
    }
}

impl Handle for ConvDelete {
    async fn handle(cx: &Cx<'_>, id: ConvId, _: Empty) -> Result<Empty, OpError> {
        let c = conv(cx, ConvReq::Delete { id }).await?;
        events::stop(id, None);
        // Its lead goes with it.
        lead::stop(id);
        if let Some(l) = c.lead {
            let _ = cx.app.mux.api(|r| Api::Close(l.block, r)).await;
        }
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

impl Handle for ConvLinkAdd {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvLinkRequest) -> Result<Conversation, OpError> {
        let who = person(cx)?;
        let c = links::link(cx.app, id, req, &who).await.map_err(refused)?;
        // #857: what it does shows up in the conversation.
        if let Some(l) = c.links.last() {
            events::watch(cx.app, id, l);
        }
        Ok(c)
    }
}

impl Handle for ConvLinkClose {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvUnlinkRequest) -> Result<Conversation, OpError> {
        let c = conv(cx, ConvReq::Unlink { id, name: req.name.clone() }).await?;
        events::stop(id, Some(&req.name));
        Ok(c)
    }
}

impl Handle for ConvLinksGet {
    async fn handle(cx: &Cx<'_>, id: ConvId, _: Empty) -> Result<Vec<ConvLinkStatus>, OpError> {
        let who = person(cx)?;
        links::statuses(cx.app, id, &who).await.map_err(refused)
    }
}

impl Handle for ConvGrantMake {
    async fn handle(cx: &Cx<'_>, pane: PaneId, req: ConvGrantRequest) -> Result<ConvGrant, OpError> {
        let who = person(cx)?;
        links::grant(cx.app, pane, req, &who).await.map_err(refused)
    }
}

impl Handle for ConvGrantsList {
    async fn handle(cx: &Cx<'_>, pane: PaneId, _: Empty) -> Result<Vec<ConvGrant>, OpError> {
        person(cx)?;
        Ok(links::grants(cx.app, pane).await)
    }
}

impl Handle for ConvGrantRevoke {
    async fn handle(cx: &Cx<'_>, pane: PaneId, req: ConvGrantRevokeRequest) -> Result<Vec<ConvGrant>, OpError> {
        person(cx)?;
        let left = links::revoke(cx.app, pane, &req.device, req.conv).await.map_err(refused)?;
        // A machine let in only by that grant is shut out again.
        cx.app.control.poke();
        Ok(left)
    }
}

impl Handle for ConvAct {
    async fn handle(cx: &Cx<'_>, id: ConvId, req: ConvActRequest) -> Result<Empty, OpError> {
        let who = person(cx)?;
        events::act(cx.app, id, req, &who).await.map_err(refused)?;
        Ok(Empty {})
    }
}
