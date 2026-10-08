//! `shell-env`: the shell environment blocks that run your tools get, and
//! resolving it again (#74). HTTP and the CLI only; no MCP tool.

use arugula_proto::{
    api::{Empty, ShellEnv},
    op::ops::{ShellEnvGet, ShellEnvRefresh},
};

use super::{Cx, Handle, OpError};

/// The user's shell environment blocks that run the user's tools get here,
/// waiting for it if it's still being resolved. Its `PATH` and the names of
/// the rest.
impl Handle for ShellEnvGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<ShellEnv, OpError> {
        let s = &cx.app.mux.shell_env;
        let r = s.local().await;
        Ok(ShellEnv {
            shell: s.shell().to_owned(),
            ok: r.error.is_none(),
            ms: r.took.as_millis() as u64,
            path: r.get("PATH").map(str::to_owned),
            vars: r.vars.iter().map(|(k, _)| k.clone()).collect(),
            error: r.error.clone(),
        })
    }
}

/// Here, and on each machine when next needed, after changing an rc file.
impl Handle for ShellEnvRefresh {
    async fn handle(cx: &Cx<'_>, path: (), req: Empty) -> Result<ShellEnv, OpError> {
        cx.app.mux.shell_env.refresh();
        ShellEnvGet::handle(cx, path, req).await
    }
}
