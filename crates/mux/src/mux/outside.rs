//! What the mux calls out to and must not name: notifications, the people
//! behind accounts, an agent's conversations, Claude Code's IDE and what a
//! closed pane leaves behind. Each is a trait in `Config`, and the binary
//! hands the mux its implementations.

use crate::acl::Principal;
use arugula_proto::PaneId;
use serde_json::Value;
use std::{collections::HashMap, fmt::Debug, path::PathBuf, sync::Arc};
use tokio::sync::mpsc;

use super::Cmd;

/// Tell people a pane wants them, on their phones.
pub trait Notify: Send + Sync + Debug {
    /// Web Push first, to the subscriptions `to` picks (`web_extra` in the
    /// payload), then Arugula control (M21), to the same people (`extra`
    /// in the payload).
    fn notify(
        &self,
        pane: PaneId,
        title: &str,
        body: &str,
        web_extra: Option<Value>,
        extra: Option<Value>,
        to: &dyn Fn(&Principal) -> bool,
    );
}

/// Who else is on this daemon's team, through Arugula control.
pub trait People: Send + Sync + Debug {
    /// Whether this daemon is a team's (M19).
    fn is_team(&self) -> bool;
    /// Whether this daemon's own team has `account` as an owner.
    fn owns_here(&self, account: &str) -> bool;
    /// The team's other owners (#386), by account.
    fn co_owners(&self) -> Vec<Principal>;
    /// People who have a team role here, connected or not.
    fn team_people(&self) -> Vec<Principal>;
    /// A hosted sandbox's last session closed (M20).
    fn sandbox_done(self: Arc<Self>);
}

/// The Claude Code conversations on this machine (#146).
pub trait AgentSessions: Send + Sync + Debug {
    /// The transcript of a conversation, by id or a unique prefix.
    fn transcript(&self, id: &str) -> Option<PathBuf>;
    /// The conversation each of our panes' Claude Code holds now, by pane,
    /// with its folder. `panes`: the processes of our panes, by pid.
    fn live(&self, panes: HashMap<u32, PaneId>) -> HashMap<PaneId, (String, Option<String>)>;
}

/// Claude Code's IDE (M28): the link to the connections.
pub trait IdeLink: Send + Sync + Debug {
    /// The port Claude Code connects to (`CLAUDE_CODE_SSE_PORT`).
    fn port(&self) -> u16;
    /// Answer a call that waits.
    fn reply(&self, conn: u64, id: &Value, result: Value);
    /// Claude Code closed these diffs.
    fn closed(&self, conn: u64, ids: &[Value]);
    /// Pass a diff to the IDE that gets them, if it isn't us, and answer
    /// `true`. `false`: keep it here. If passing it on fails, `back` gets
    /// the call again as `openDiff/here`.
    fn forward_diff(self: Arc<Self>, conn: u64, id: &Value, args: &Value, back: mpsc::UnboundedSender<Cmd>) -> bool;
}

/// A pane closed for good.
pub trait PaneGone: Send + Sync + Debug {
    /// Remove its uploads.
    fn forget(&self, pane: PaneId);
    /// Remove its uploads on the machine it ran on, if that machine stays.
    fn forget_on(&self, provider: Arc<dyn crate::provider::Provider>, sprite: String, pane: PaneId);
}
