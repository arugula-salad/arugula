//! `sandboxes`: the home daemon's provider sandboxes, and making a daemon
//! resident in one (`arugula sandboxes`). Labs, and the owner's; the work is
//! `labs/resident.rs`, whose twin in a build without Labs has no route to
//! answer from.

use arugula_proto::{
    api::Empty,
    hosts::{Demoted, Host, PromoteRequest, SandboxList},
    op::ops::{SandboxDemote, SandboxPromote, SandboxesList},
};

use super::{Cx, Handle, OpError};

fn failed((code, why): crate::labs::SandboxFailed) -> OpError {
    OpError::Status(code, why)
}

impl Handle for SandboxesList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<SandboxList, OpError> {
        crate::labs::sandboxes(cx.app).await.map_err(failed)
    }
}

impl Handle for SandboxPromote {
    async fn handle(cx: &Cx<'_>, sandbox: String, req: PromoteRequest) -> Result<Host, OpError> {
        crate::labs::sandbox_promote(cx.app, &sandbox, req).await.map_err(failed)
    }
}

impl Handle for SandboxDemote {
    async fn handle(cx: &Cx<'_>, sandbox: String, _: Empty) -> Result<Demoted, OpError> {
        crate::labs::sandbox_demote(cx.app, &sandbox).await.map_err(failed)
    }
}
