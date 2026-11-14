//! A session's project, inferred (Arugula projects RFC): when a pane opens
//! on this host in a clone whose GitHub repo is linked to a project, and
//! its session has none yet, the session goes in that project, and keeps
//! the repo it came from (`Session::project_repo`). A later pane in another
//! repo never moves it. A pane that opens before its shell says where it
//! is goes by the first directory it reports. Without an answer (no link,
//! control unreachable) it waits for the session's next pane. Set by hand
//! from the session's menu, or taken away there (`Intent::SetSessionProject`):
//! then nothing infers one again, and control isn't asked.
//!
//! An inferred project follows its repo (Arugula #992): when the repo is
//! linked to another project, the sessions inferred from it take that one.
//! The daemon asks control each repo's project again when its relay socket
//! comes up, so a machine that was away catches up, and for one repo when
//! control says it moved (`projects.repo_moved`, `forges::op_from_control`).
//! Only the label changes: no pane, tab or agent restarts. A repo linked
//! to no project now, or no answer, leaves the label as it was.

use std::sync::Arc;

use arugula_control_wire::forge::OpAnswer;
use arugula_proto::EventKind;
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;

use crate::{
    control::Control,
    mux::{Api, MuxHandle},
};

/// Infer each new pane's session's project, while it has none, and follow
/// the inferred ones' repos each time control's relay comes up.
pub fn start(mux: MuxHandle, control: Arc<Control>) {
    let (m, c) = (mux.clone(), control.clone());
    crate::task::spawn(async move {
        let mut connects = c.on_connect();
        // A connect before this task subscribed counts too.
        connects.mark_changed();
        while connects.changed().await.is_ok() {
            if *connects.borrow_and_update() > 0 {
                follow(&m, &c, None).await;
            }
        }
    });
    crate::task::spawn(async move {
        let mut events = mux.events();
        // Opened before its shell said where it is: its first `Cwd` says.
        let mut unplaced = std::collections::HashSet::new();
        loop {
            let (pane, cwd) = match events.recv().await {
                Ok(e) => match (e.pane, e.kind) {
                    (Some(p), EventKind::Opened) => (p, None),
                    (Some(p), EventKind::Cwd { path }) if unplaced.remove(&p) => (p, Some(path)),
                    (Some(p), EventKind::Closed) => {
                        unplaced.remove(&p);
                        continue;
                    }
                    _ => continue,
                },
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let Some(panes) = mux.api(Api::Panes).await else { return };
            let Some(p) = panes.into_iter().find(|p| p.info.id == pane) else { continue };
            // Another machine's clone isn't read from here.
            if p.info.session_project.is_some() || p.info.host.is_some() {
                continue;
            }
            let Some(cwd) = cwd.or(p.info.cwd) else {
                unplaced.insert(pane);
                continue;
            };
            let session = p.session;
            // Set or taken away by hand: control isn't asked.
            if mux.api(|r| Api::Inferable(session, r)).await != Some(true) {
                continue;
            }
            let Some((repo, project)) = crate::mcp::issues::linked_project(&control, &cwd).await else { continue };
            if mux.api(|r| Api::InferProject(session, project.clone(), repo, r)).await == Some(true) {
                debug!(session, project = project.id, pane, "a session's project, from its pane's linked repo");
            }
        }
    });
}

/// `projects.repo_moved` (Arugula #992): control says `repo` is linked to
/// another project now. Answered `done` once its sessions follow.
pub async fn repo_moved(mux: &MuxHandle, control: &Control, repo: &str) -> OpAnswer {
    follow(mux, control, Some(repo)).await;
    OpAnswer::Done { url: None, routed: None }
}

/// Ask control the project of each repo sessions' projects were inferred
/// from (or only `only`), and move those sessions to it.
async fn follow(mux: &MuxHandle, control: &Control, only: Option<&str>) {
    let Some(repos) = mux.api(Api::ProjectRepos).await else { return };
    for repo in repos.into_iter().filter(|r| only.is_none_or(|o| o == r)) {
        let Some(project) = crate::mcp::issues::repo_project(control, &repo).await else { continue };
        mux.api(|r| Api::FollowRepo(repo, project, r)).await;
    }
}
