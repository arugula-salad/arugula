//! `synced`: the history other hosts pushed to this home daemon (`arugula
//! synced`), listed, forgotten and re-sealed under a new key. All the
//! owner's. Their pushes (`/api/sync/…`, a host token's) stay hand-written
//! in `sync.rs`.

use arugula_proto::{
    api::{Empty, SyncedHost, SyncedRotated},
    op::ops::{SyncedForget, SyncedList, SyncedRotate},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};

impl Handle for SyncedList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<SyncedHost>, OpError> {
        Ok(cx.app.synced.hosts())
    }
}

impl Handle for SyncedRotate {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<SyncedRotated, OpError> {
        let synced = cx.app.synced.clone();
        match tokio::task::spawn_blocking(move || synced.rotate()).await {
            Ok(Ok(key)) => Ok(SyncedRotated { key }),
            Ok(Err(e)) => Err(OpError::Failed(e.to_string())),
            Err(e) => Err(OpError::Failed(e.to_string())),
        }
    }
}

impl Handle for SyncedForget {
    async fn handle(cx: &Cx<'_>, host: String, _: Empty) -> Result<Empty, OpError> {
        if cx.app.synced.remove_host(&host) {
            Ok(Empty {})
        } else {
            Err(OpError::Status(StatusCode::NOT_FOUND, format!("no synced history from {host}")))
        }
    }
}
