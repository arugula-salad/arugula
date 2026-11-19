//! `sharing` (native #205): the machine's sharing settings, Guest SSH access
//! and agent mentions and what they may learn of its sessions, in the state dir's `sharing.json`. HTTP only, as `flags` and
//! `telemetry` are; the owner's, and turning one on is theirs in person: a
//! way in is not an agent's to open.

use arugula_proto::{
    api::Empty,
    op::ops::{RemoteSessionsList, SharingGet, SharingSet},
    sharing::{self, REMOTE_SESSIONS_SHOWN, RemoteSessions, Sharing, SharingSetRequest},
};

use super::{Cx, Handle, OpError};

/// The settings as they are now. A build without Labs has no guest ssh:
/// the setting is kept, and nothing follows.
fn now(cx: &Cx<'_>) -> Sharing {
    let dir = cx.app.control.state_dir();
    Sharing {
        guest_ssh: sharing::guest_ssh(dir),
        agent_mentions: sharing::agent_mentions(dir),
        agent_sessions: sharing::agent_sessions(dir),
        agent_feedback: sharing::agent_feedback(dir),
        built: crate::lab_flags::BUILT,
    }
}

impl Handle for SharingGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Sharing, OpError> {
        Ok(now(cx))
    }
}

/// Who reached the machine through control (Arugula #1094), from the log
/// it keeps itself (aru-141), and whether they may type now.
impl Handle for RemoteSessionsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<RemoteSessions, OpError> {
        let dir = cx.app.control.state_dir();
        Ok(RemoteSessions {
            remote_input: sharing::remote_input(dir),
            sessions: crate::remote_log::sessions(dir, true, REMOTE_SESSIONS_SHOWN),
        })
    }
}

impl Handle for SharingSet {
    async fn handle(cx: &Cx<'_>, _: (), req: SharingSetRequest) -> Result<Sharing, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden("sharing settings are the owner's to change, not an agent's".into()));
        }
        if let Some(on) = req.guest_ssh {
            sharing::set_guest_ssh(cx.app.control.state_dir(), on)
                .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", sharing::FILE)))?;
            tracing::info!(on, "guest ssh access set");
        }
        if let Some(on) = req.agent_mentions {
            sharing::set_agent_mentions(cx.app.control.state_dir(), on)
                .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", sharing::FILE)))?;
            tracing::info!(on, "agent mentions set");
        }
        if let Some(what) = req.agent_sessions {
            sharing::set_agent_sessions(cx.app.control.state_dir(), what)
                .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", sharing::FILE)))?;
            tracing::info!(what = what.word(), "agent sessions set");
        }
        if let Some(what) = req.agent_sessions {
            sharing::set_agent_sessions(cx.app.control.state_dir(), what)
                .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", sharing::FILE)))?;
            tracing::info!(what = what.word(), "agent sessions set");
        }
        if let Some(on) = req.agent_feedback {
            sharing::set_agent_feedback(cx.app.control.state_dir(), on)
                .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", sharing::FILE)))?;
            tracing::info!(on, "agent feedback set");
        }
        Ok(now(cx))
    }
}
