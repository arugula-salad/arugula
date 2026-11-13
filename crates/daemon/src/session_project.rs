//! A session's project, inferred once (Arugula projects RFC): when a pane
//! opens on this host in a clone whose GitHub repo is linked to a project,
//! and its session has none yet, the session goes in that project. Once it
//! has one it stays put: a later pane in another repo, or the repo linked
//! elsewhere, never moves it (the mux sets it only while it has none,
//! `Mux::infer_project`). Without an answer (no link, control unreachable)
//! it waits for the session's next pane. Set by hand from the session's
//! menu, or taken away there (`Intent::SetSessionProject`): then nothing
//! infers one again, and control isn't asked.
//!
//! Its card too, the same way: a terminal on a `feat/N-…` branch puts its
//! session on `#N` when a board has a row for it
//! (`board_attach::infer_card`), and never over one set by hand.

use std::sync::Arc;

use arugula_proto::EventKind;
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;

use crate::{mux::Api, server::App};

/// Infer each new pane's session's project, while it has none, and its
/// card.
pub fn start(app: Arc<App>) {
    crate::task::spawn(async move {
        let (mux, control) = (&app.mux, &app.control);
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
            if p.info.host.is_some() {
                continue;
            }
            let session = p.session;
            let mut project = p.info.session_project.map(|p| p.id);
            // Set or taken away by hand: control isn't asked.
            if project.is_none()
                && let Some(cwd) = p.info.cwd
                && mux.api(|r| Api::Inferable(session, r)).await == Some(true)
                && let Some(linked) = crate::mcp::issues::linked_project(control, &cwd).await
                && mux.api(|r| Api::InferProject(session, linked.clone(), r)).await == Some(true)
            {
                debug!(session, project = linked.id, pane, "a session's project, from its pane's linked repo");
                project = Some(linked.id);
            }
            // A terminal's branch names its session's card; an agent's is
            // its own (`start_agent`).
            if p.info.kind == arugula_proto::BlockType::Terminal
                && let Some(branch) = p.info.branch
            {
                crate::board_attach::infer_card(&app, session, project, &branch, pane).await;
            }
        }
    });
}
