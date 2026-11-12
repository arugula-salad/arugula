//! A session's project, inferred once (Arugula projects RFC): when a pane
//! opens on this host in a clone whose GitHub repo is linked to a project,
//! and its session has none yet, the session goes in that project. Once it
//! has one it stays put: a later pane in another repo, or the repo linked
//! elsewhere, never moves it (the mux sets it only while it has none,
//! `Mux::infer_project`). Without an answer (no link, control unreachable)
//! it waits for the session's next pane. Set by hand from the session's
//! menu: `Intent::SetSessionProject`.

use std::sync::Arc;

use arugula_proto::EventKind;
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;

use crate::{
    control::Control,
    mux::{Api, MuxHandle},
};

/// Infer each new pane's session's project, while it has none.
pub fn start(mux: MuxHandle, control: Arc<Control>) {
    crate::task::spawn(async move {
        let mut events = mux.events();
        loop {
            let pane = match events.recv().await {
                Ok(e) if matches!(e.kind, EventKind::Opened) => e.pane,
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(pane) = pane else { continue };
            let Some(panes) = mux.api(Api::Panes).await else { return };
            let Some(p) = panes.into_iter().find(|p| p.info.id == pane) else { continue };
            // Another machine's clone isn't read from here.
            if p.info.session_project.is_some() || p.info.host.is_some() {
                continue;
            }
            let Some(cwd) = p.info.cwd else { continue };
            let Some(project) = crate::mcp::issues::linked_project(&control, &cwd).await else { continue };
            let session = p.session;
            if mux.api(|r| Api::InferProject(session, project.clone(), r)).await == Some(true) {
                debug!(session, project = project.id, pane, "a session's project, from its pane's linked repo");
            }
        }
    });
}
