//! What the mux calls out to and must not name: notifications, the people
//! behind accounts, an agent's conversations and what a closed pane leaves
//! behind, each a trait in `Config`, and the cards that make a terminal wait
//! (`PaneCards`), which `start` takes. The binary hands the mux its
//! implementations.

use crate::acl::Principal;
use arugula_proto::{DiffInfo, Driver, PaneId, Reason, ask::Answered};
use serde_json::Value;
use std::{any::Any, collections::HashMap, fmt::Debug, path::PathBuf, sync::Arc};
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

/// Something outside the mux that makes a terminal wait on you (#699): a
/// card of its own, shown in the pane and ordered with its other reasons.
/// It is held by the mux and runs on its task, so what it does comes in the
/// same order as everything else the task does. The binary's own type is
/// reached through [`as_any`](Self::as_any), by [`Api::Cards`].
///
/// [`Api::Cards`]: super::Api::Cards
pub trait PaneCards: Send + Debug {
    /// Whether this terminal's process is linked to it (`PaneInfo.claude_ide`).
    fn linked(&self, pane: PaneId) -> bool;
    /// Each tick of the mux task.
    fn tick(&mut self, cx: &mut dyn CardsCx);
    /// The binary's own type, for its calls.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A call on the cards, with what they may do to the mux (`Api::Cards`).
pub type CardsCall = Box<dyn FnOnce(&mut dyn PaneCards, &mut dyn CardsCx) + Send>;

/// What the owner of a card may do to the mux, on the mux task.
pub trait CardsCx {
    /// The terminal whose process is, or is above, `pid`.
    fn pane_of_pid(&self, pid: u32) -> Option<PaneId>;
    /// A terminal's working directory.
    fn pane_cwd(&self, pane: PaneId) -> Option<String>;
    /// This pane's card is now `card`, or gone: its reason and what it shows
    /// (`PaneInfo.diff`). Its attention follows.
    fn card(&mut self, pane: PaneId, card: Option<(Reason, DiffInfo)>);
    /// The terminal answered the card first.
    fn answered(&mut self, pane: PaneId, answered: Answered);
    /// Someone answered the card: every client's card closes saying who, and
    /// the pane's history and the audit log say so too.
    fn record_answer(&mut self, pane: PaneId, by: &Driver, id: &str, how: &str, headline: &str);
    /// The pane's details changed.
    fn touch(&mut self, pane: PaneId);
    /// The mux task's inbox, to call it again later.
    fn sender(&self) -> mpsc::UnboundedSender<Cmd>;
}

/// A pane closed for good.
pub trait PaneGone: Send + Sync + Debug {
    /// Remove its uploads.
    fn forget(&self, pane: PaneId);
    /// Remove its uploads on the machine it ran on, if that machine stays.
    fn forget_on(&self, provider: Arc<dyn crate::provider::Provider>, sprite: String, pane: PaneId);
}
