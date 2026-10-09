//! The files routes (#578): a directory's listing, one entry, the
//! directories used lately, and `cd` typed into a shell. HTTP only, on
//! `fs.rs`'s router. Listing, stat and recent are the owner's; `cd` is an
//! editor's on the pane. `read` (bytes with headers) and `watch` (a stream)
//! stay written out.

use arugula_proto::{
    PaneId,
    api::Empty,
    fs::{CdRequest, FsEntry, FsList, FsQuery},
    op::ops::{FsListGet, FsRecentGet, FsStatGet, PaneCd},
};

use super::{Cx, Handle, OpError};
use crate::fs::FsError;

/// `FsError`'s answer as it was: its status and `{"error": …}`.
impl From<FsError> for OpError {
    fn from(e: FsError) -> Self {
        OpError::Status(e.status(), e.to_string())
    }
}

impl Handle for FsListGet {
    async fn handle(cx: &Cx<'_>, _: (), q: FsQuery) -> Result<FsList, OpError> {
        Ok(crate::fs::list(cx.app, &q).await?)
    }
}

impl Handle for FsStatGet {
    async fn handle(cx: &Cx<'_>, _: (), q: FsQuery) -> Result<FsEntry, OpError> {
        Ok(crate::fs::stat(cx.app, &q).await?)
    }
}

impl Handle for FsRecentGet {
    async fn handle(cx: &Cx<'_>, _: (), q: FsQuery) -> Result<Vec<String>, OpError> {
        Ok(crate::fs::recent(cx.app, &q).await?)
    }
}

impl Handle for PaneCd {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: CdRequest) -> Result<Empty, OpError> {
        crate::fs::cd(cx.app, id, req).await?;
        Ok(Empty {})
    }
}
