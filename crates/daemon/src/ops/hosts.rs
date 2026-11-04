//! The host list's routes (#578): who this daemon is, the other daemons it
//! lists, adding and removing them. HTTP only, on `hosts.rs`'s router.
//! `GET /api/host` is anyone's (the owner also hears how the machine stands
//! with control); the rest are the owner's.

use arugula_proto::{
    api::Empty,
    hosts::{AddHost, Host, HostAnswer, HostList},
    op::ops::{HostAdd, HostGet, HostRemove, HostsList},
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
        Ok(cx.app.hosts.list())
    }
}

impl Handle for HostAdd {
    async fn handle(cx: &Cx<'_>, _: (), req: AddHost) -> Result<Host, OpError> {
        let h = cx.app.hosts.add(req).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))?;
        let hosts = cx.app.hosts.clone();
        crate::task::spawn(async move { hosts.probe().await });
        Ok(h)
    }
}

impl Handle for HostRemove {
    async fn handle(cx: &Cx<'_>, name: String, _: Empty) -> Result<Empty, OpError> {
        if cx.app.hosts.remove(&name) {
            Ok(Empty {})
        } else {
            Err(OpError::Status(StatusCode::NOT_FOUND, format!("no host {name}")))
        }
    }
}
