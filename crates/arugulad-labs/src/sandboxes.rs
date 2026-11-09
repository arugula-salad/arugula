//! The home daemon's provider's sandboxes (`arugula sandboxes`, the
//! *Sandboxes…* dialog): listing them, for a shell on one with no daemon
//! there. That shell is a pane here whose terminal is a plain exec on the
//! sandbox (`RunRequest::sandbox`, a borrowed machine in `mux/machines.rs`).
//! The output is logged here, but while the pane isn't attached (a daemon
//! restart) the sandbox keeps only what its provider replays, about 6.5 KB
//! on Fly: such shells are disposable.

use std::sync::Arc;

use arugula_proto::{
    api::Empty,
    hosts::{ProviderInfo, SandboxInfo, SandboxList},
    op::ops::SandboxesList,
};
use axum::{Router, http::StatusCode};

use super::Labs;
use crate::{
    ops::{Cx, Handle, OpError},
    server::App,
};

/// Our own machines' sandboxes (VM panes and tabs, any daemon's): not for
/// opening shells on (`sprite_prefix`, and illogical's from before the
/// rename, #509).
const EPHEMERAL: [&str; 2] = ["arugula-eph-", "illogical-eph-"];

fn ephemeral(sandbox: &str) -> bool {
    EPHEMERAL.iter().any(|p| sandbox.starts_with(p))
}

/// The sandboxes route, as an operation (below). The owner's.
pub fn routes() -> Router<Arc<App>> {
    use crate::ops::OpRoutes;
    Router::new().op_of::<SandboxesList, Labs>()
}

pub async fn list(app: &App) -> Result<SandboxList, (StatusCode, String)> {
    let Some(p) = app.mux.provider.clone() else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "no sandbox provider is set up here".into()));
    };
    let all = match p.list("").await {
        Ok(l) => l,
        Err(e) => return Err((StatusCode::BAD_GATEWAY, format!("{}: {e}", p.name()))),
    };
    let sandboxes = all
        .into_iter()
        .filter(|s| !ephemeral(&s.name))
        .map(|s| SandboxInfo { name: s.name, status: s.status })
        .collect();
    let provider = ProviderInfo { name: p.name().to_owned(), exec_replay: p.caps().exec_replay };
    Ok(SandboxList { provider, sandboxes })
}

impl Handle<Labs> for SandboxesList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<SandboxList, OpError> {
        list(cx.app).await.map_err(|(code, why)| OpError::Status(code, why))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_machines_are_not_sandboxes_to_open_shells_on() {
        assert!(ephemeral("arugula-eph-7-3") && ephemeral("illogical-eph-1-1"));
        assert!(!ephemeral("box") && !ephemeral("eph-arugula"));
    }
}
