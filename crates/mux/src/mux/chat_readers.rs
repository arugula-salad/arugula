//! Who may read each pane's and session's thread in control's chat (M124,
//! S41 decision 2): this machine decides, as it decides who reads the pane,
//! and the daemon tells control. Chat itself lives in control (M125).

use super::{ChatReaders, ChatTarget, Daemon};
use crate::acl::Principal;
use arugula_core::Role;

impl Daemon {
    /// Who may read the thread on each pane and session here now, of the
    /// people who can reach this machine. The owner reads everything and
    /// isn't listed.
    pub(super) fn chat_readers(&self) -> ChatReaders {
        let people: Vec<_> = self.reachers().into_iter().filter(|p| !p.is_owner()).collect();
        let mut targets: Vec<ChatTarget> =
            self.panes.keys().chain(self.blocks.keys()).map(|p| ChatTarget::Pane(*p)).collect();
        targets.extend(self.mux.sessions.iter().map(|s| ChatTarget::Session(s.id)));
        targets.sort();
        targets.dedup();
        targets
            .into_iter()
            .map(|t| {
                let readers = people
                    .iter()
                    .filter_map(|p| self.chat_role(p, t).map(|(role, from)| (p.id().to_owned(), from, role < Role::Editor)))
                    .collect();
                (t, readers)
            })
            .collect()
    }

    /// `who`'s role in a thread here, and the time its messages start for
    /// them: a pane's thread is read by whoever may read the pane (a
    /// private pane's only by its owner), a session's by whoever has a role
    /// in it. A "from now" share reads from when it was made.
    fn chat_role(&self, who: &Principal, target: ChatTarget) -> Option<(Role, u64)> {
        let session = match target {
            ChatTarget::Pane(p) => self.session_of(p).filter(|_| self.readable(who, p))?,
            ChatTarget::Session(s) => s,
        };
        let role = self.config.acl.role(who, session)?;
        Some((role, self.config.acl.share_at(who, session).unwrap_or(0)))
    }

    /// Everyone who may reach this machine: the owner, the team's other
    /// owners (#386), everyone shared with, team members (connected or
    /// not), and whoever is connected.
    fn reachers(&self) -> Vec<Principal> {
        let mut out = vec![Principal::Owner];
        out.extend(self.config.people.co_owners());
        for g in self.config.acl.list() {
            out.push(Principal::User { id: g.principal.clone(), name: g.name.clone(), pic: None });
        }
        out.extend(self.config.people.team_people());
        for c in self.clients.values() {
            out.push(c.principal.clone());
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| seen.insert(p.id().to_owned()));
        out
    }
}
