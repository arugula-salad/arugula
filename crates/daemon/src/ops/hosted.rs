//! `hosted_setup.get`: a hosted machine's setup and where its repos stand
//! (M183), for `arugula cloud rebuild` to warn with. HTTP and the CLI only.

use arugula_proto::{
    api::{Empty, HostedSetup},
    op::ops::HostedSetupGet,
};

use super::{Cx, Handle, OpError};

/// What this box's setup did, and each repo's changed files and commits on
/// no remote branch. Empty on a machine that isn't hosted.
impl Handle for HostedSetupGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<HostedSetup, OpError> {
        Ok(crate::hosted::setup::status(cx.app.control.state_dir()).await)
    }
}
