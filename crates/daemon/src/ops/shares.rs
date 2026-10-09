//! `shares` and `guests`: the read-only links and the ssh invites the owner
//! makes, lists and ends (`arugula share`, `arugula shares`, `arugula guests`).
//! All are the owner's (`serve` checks that before the handler runs), and
//! both lists are credentials. A share is small enough to be done here;
//! guests are Labs', so each goes through `crate::labs`, whose twin in a
//! build without it answers the 501 every method of `/api/guests` gave.

use arugula_proto::{
    BlockType,
    api::{Empty, GuestInvite, GuestInviteRequest, Share, ShareRequest},
    op::ops::{GuestMint, GuestRevoke, GuestsList, ShareMint, ShareRevoke, SharesList},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{mux::Api, share::DEFAULT_TTL_SECS};

impl Handle for SharesList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<Share>, OpError> {
        Ok(cx.app.shares.list())
    }
}

impl Handle for ShareMint {
    async fn handle(cx: &Cx<'_>, _: (), req: ShareRequest) -> Result<Share, OpError> {
        let app = cx.app;
        let summaries = app.mux.api(Api::Panes).await.unwrap_or_default();
        match summaries.iter().find(|p| p.info.id == req.pane) {
            None => return Err(OpError::NoPane(req.pane)),
            Some(p) if p.info.kind != BlockType::Terminal => {
                return Err(OpError::Status(
                    StatusCode::BAD_REQUEST,
                    format!("%{} isn't a terminal; only terminals can be shared", req.pane),
                ));
            }
            Some(_) => {}
        }
        let mut share = app.shares.mint(req.pane, req.ttl_secs.unwrap_or(DEFAULT_TTL_SECS).max(1));
        share.url = share.path.as_ref().map(|p| format!("{}{p}", app.access.page_origin()));
        Ok(share)
    }
}

impl Handle for ShareRevoke {
    async fn handle(cx: &Cx<'_>, id: u32, _: Empty) -> Result<Empty, OpError> {
        if cx.app.shares.revoke(id) {
            Ok(Empty {})
        } else {
            Err(OpError::Status(StatusCode::NOT_FOUND, format!("no share {id}")))
        }
    }
}

impl Handle for GuestsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<GuestInvite>, OpError> {
        Ok(crate::labs::guest_list(cx.app)?)
    }
}

impl Handle for GuestMint {
    async fn handle(cx: &Cx<'_>, _: (), req: GuestInviteRequest) -> Result<GuestInvite, OpError> {
        Ok(crate::labs::guest_mint(cx.app, req).await?)
    }
}

impl Handle for GuestRevoke {
    async fn handle(cx: &Cx<'_>, id: u32, _: Empty) -> Result<Empty, OpError> {
        crate::labs::guest_revoke(cx.app, id).await?;
        Ok(Empty {})
    }
}
