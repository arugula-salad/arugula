//! The host list's routes (#578): who this daemon is, the other daemons it
//! lists, adding and removing them, and their tokens. HTTP only, on
//! `hosts.rs`'s router. `GET /api/host` is anyone's (the owner also hears how
//! the machine stands with control); the rest are the owner's. `invite`
//! (its `ttl` is a query on a POST) and `join` (a token is its credential)
//! stay written out.

use std::time::Duration;

use arugula_proto::{
    api::Empty,
    hosts::{AddHost, Host, HostAnswer, HostList, HostToken},
    op::ops::{HostAdd, HostGet, HostRemove, HostTokenMint, HostTokenRevoke, HostsList},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError, Via};

impl Handle for HostGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<HostAnswer, OpError> {
        // Only the owner hears how this machine stands with control (#325).
        let owner = match &cx.via {
            Via::Http { owner, .. } => *owner,
            Via::Mcp(_) => false,
        };
        Ok(crate::hosts::answer(cx.app, owner))
    }
}

impl Handle for HostsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<HostList, OpError> {
        // Sandboxes' states are cheap to ask for (and asking doesn't wake
        // them), so the list a client gets is current.
        let _ = tokio::time::timeout(Duration::from_secs(3), cx.app.hosts.ask_providers()).await;
        Ok(cx.app.hosts.list())
    }
}

impl Handle for HostAdd {
    async fn handle(cx: &Cx<'_>, _: (), req: AddHost) -> Result<Host, OpError> {
        let h = cx.app.hosts.add(req).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))?;
        let hosts = cx.app.hosts.clone();
        tokio::spawn(async move { hosts.probe().await });
        Ok(h)
    }
}

impl Handle for HostRemove {
    async fn handle(cx: &Cx<'_>, name: String, _: Empty) -> Result<Empty, OpError> {
        // Its tunnel goes with it.
        cx.app.dial_outs.drop_host(&name);
        if cx.app.hosts.remove(&name) {
            Ok(Empty {})
        } else {
            Err(OpError::Status(StatusCode::NOT_FOUND, format!("no host {name}")))
        }
    }
}

impl Handle for HostTokenMint {
    async fn handle(cx: &Cx<'_>, name: String, _: Empty) -> Result<HostToken, OpError> {
        let t = cx.app.hosts.mint_token(&name).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))?;
        // A new token replaces the old one: so does the connection.
        cx.app.dial_outs.drop_host(&name);
        Ok(t)
    }
}

impl Handle for HostTokenRevoke {
    async fn handle(cx: &Cx<'_>, name: String, _: Empty) -> Result<Empty, OpError> {
        let had = cx.app.hosts.revoke_token(&name);
        let connected = cx.app.dial_outs.drop_host(&name);
        if had || connected {
            Ok(Empty {})
        } else {
            Err(OpError::Status(StatusCode::NOT_FOUND, format!("{name} has no token")))
        }
    }
}
