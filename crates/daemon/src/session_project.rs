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
//! linked to another project, the sessions inferred from it take that one,
//! and inferred work in the old project comes along without its card
//! (`Mux::follow_repo`). The daemon asks control each repo's project again
//! when its relay socket comes up, so a machine that was away catches up,
//! and for one repo when control says it moved (`projects.repo_moved`,
//! `forges::op_from_control`). Only the label changes: no pane, tab or
//! agent restarts. A repo linked to no project now, or no answer, leaves
//! the label as it was.
//!
//! Its card too, the same way: a terminal on a `feat/N-…` branch puts its
//! session on `#N` when a board has a row for it
//! (`board_attach::infer_card`), and never over one set by hand.
//!
//! Every session has a project (Arugula #1163). One made from a page in a
//! project starts there, set by hand (`Intent::NewSession`'s `project`).
//! One that nothing places (no pane in a linked clone, none set by hand)
//! goes in the owner's default project, as control answers it
//! (`mcp::issues::owners_default`): their personal one, or the team's on
//! a team's machine. That counts as inferred, not set by hand, so a later
//! pane in a linked clone, or a branch's card, still moves it
//! (`Mux::default_project`). Without an answer it waits: each time
//! control's relay comes up, every session still without a project is
//! placed as a new pane's would be, so those from before this one, and
//! those made while control was away, get theirs then. One taken out of
//! its project by hand stays out.

use std::sync::Arc;

use arugula_control_wire::forge::OpAnswer;
use arugula_core::ProjectRef;
use arugula_proto::{EventKind, SessionId};
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;

use crate::{
    control::Control,
    mux::{Api, MuxHandle},
    server::App,
};

/// Infer each new pane's session's project, while it has none (or only
/// the default), and its card, else put it in the owner's default; and
/// each time control's relay comes up, follow the inferred projects'
/// repos and place the sessions still without one.
pub fn start(app: Arc<App>) {
    let a = app.clone();
    crate::task::spawn(async move {
        let mut connects = a.control.on_connect();
        // A connect before this task subscribed counts too.
        connects.mark_changed();
        while connects.changed().await.is_ok() {
            if *connects.borrow_and_update() > 0 {
                follow(&a.mux, &a.control, None).await;
                place_unplaced(&a.mux, &a.control).await;
            }
        }
    });
    crate::task::spawn(async move {
        let (mux, control) = (&app.mux, &app.control);
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
            let session = p.session;
            // Another machine's clone isn't read from here.
            let local = p.info.host.is_none();
            let cwd = cwd.or(p.info.cwd).filter(|_| local);
            if local && cwd.is_none() {
                unplaced.insert(pane);
            }
            let mut project = p.info.session_project.map(|p| p.id);
            // Set or taken away by hand: control isn't asked.
            if mux.api(|r| Api::Inferable(session, r)).await == Some(true) {
                if let Some(cwd) = &cwd
                    && let Some(linked) = infer(mux, control, session, cwd).await
                {
                    debug!(session, project = linked, pane, "a session's project, from its pane's linked repo");
                    project = Some(linked);
                } else if project.is_none() {
                    project = place_default(mux, control, session, None).await;
                }
            }
            let Some(_) = cwd else { continue };
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

/// `session`'s project from the linked repo of its pane's clone at `cwd`,
/// while it may take one: its id, when it was set.
async fn infer(mux: &MuxHandle, control: &Control, session: SessionId, cwd: &str) -> Option<String> {
    let (repo, linked) = crate::mcp::issues::linked_project(control, cwd).await?;
    let id = linked.id.clone();
    (mux.api(|r| Api::InferProject(session, linked, repo, r)).await == Some(true)).then_some(id)
}

/// Put `session` in the owner's default project (`known`, else as control
/// answers it) while nothing placed it: its id, when it was set.
async fn place_default(
    mux: &MuxHandle,
    control: &Control,
    session: SessionId,
    known: Option<ProjectRef>,
) -> Option<String> {
    let default = match known {
        Some(d) => d,
        None => crate::mcp::issues::owners_default(control).await?,
    };
    let id = default.id.clone();
    if mux.api(|r| Api::DefaultProject(session, default, r)).await != Some(true) {
        return None;
    }
    debug!(session, project = id, "a session nothing placed, in the owner's default project");
    Some(id)
}

/// Every session still without a project, and not taken out of one by
/// hand, placed as a new pane's would be: by its panes here in a linked
/// clone, else in the owner's default project. Control is asked the
/// default once.
async fn place_unplaced(mux: &MuxHandle, control: &Control) {
    let Some(sessions) = mux.api(Api::Sessions).await else { return };
    let unplaced: Vec<SessionId> = sessions.iter().filter(|s| s.unplaced()).map(|s| s.id).collect();
    if unplaced.is_empty() {
        return;
    }
    let Some(panes) = mux.api(Api::Panes).await else { return };
    let mut default = None;
    for session in unplaced {
        let mut cwds: Vec<&str> = panes
            .iter()
            .filter(|p| p.session == session && p.info.host.is_none())
            .filter_map(|p| p.info.cwd.as_deref())
            .collect();
        cwds.dedup();
        let mut placed = false;
        for cwd in cwds {
            if infer(mux, control, session, cwd).await.is_some() {
                placed = true;
                break;
            }
        }
        if placed {
            continue;
        }
        if default.is_none() {
            default = crate::mcp::issues::owners_default(control).await;
            // No answer: the next connect asks again.
            if default.is_none() {
                return;
            }
        }
        place_default(mux, control, session, default.clone()).await;
    }
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
