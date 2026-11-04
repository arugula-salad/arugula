//! Who may read each pane and session here (M124, S41 decision 2): this
//! machine decides, as it decides who reads the pane, and the binary tells
//! whoever asks (control, for what it keeps about them).

use std::collections::HashSet;

use super::{Daemon, Readable, Readers};
use crate::acl::Principal;
use arugula_core::Role;

impl Daemon {
    /// Who may read each pane and session here now, of the people who can
    /// reach this machine. The owner reads everything and isn't listed.
    ///
    /// A team's other owners (#386), and a hosted workspace's editors, get
    /// in as the owner, so they read what the owner reads: every pane, a
    /// private one included (native #684, #705). Each is listed by their
    /// own account, as the owner's role, from the start.
    pub(super) fn readers(&self) -> Readers {
        let as_owner: HashSet<String> = self.config.people.as_owner().into_iter().map(|p| p.id().to_owned()).collect();
        let people: Vec<(String, Principal)> = self
            .reachers()
            .into_iter()
            .filter(|p| !p.is_owner())
            .map(|p| {
                let id = p.id().to_owned();
                // As the machine admits them: a co-owner, or a hosted
                // workspace's editor, as the owner.
                let as_who = if as_owner.contains(&id) { Principal::Owner } else { p };
                (id, as_who)
            })
            .collect();
        let mut targets: Vec<Readable> =
            self.panes.keys().chain(self.blocks.keys()).map(|p| Readable::Pane(*p)).collect();
        targets.extend(self.mux.sessions.iter().map(|s| Readable::Session(s.id)));
        targets.sort();
        targets.dedup();
        targets
            .into_iter()
            .map(|t| {
                let readers = people
                    .iter()
                    .filter_map(|(id, who)| {
                        self.role_on(who, t).map(|(role, from)| (id.clone(), from, role < Role::Editor))
                    })
                    .collect();
                (t, readers)
            })
            .collect()
    }

    /// `who`'s role on a pane or session here, and the time what they may
    /// read of it starts: a pane is read by whoever may read it
    /// ([`Self::readable`]: a private pane only by its owner, and by the
    /// team's other owners, who are the owner here), a session by whoever
    /// has a role in it. A "from now" share reads from when it was made.
    fn role_on(&self, who: &Principal, target: Readable) -> Option<(Role, u64)> {
        let session = match target {
            Readable::Pane(p) => self.session_of(p).filter(|_| self.readable(who, p))?,
            Readable::Session(s) => s,
        };
        let role = self.config.acl.role(who, session)?;
        Some((role, self.config.acl.share_at(who, session).unwrap_or(0)))
    }

    /// Everyone who may reach this machine: the owner, whoever else gets
    /// in as the owner (the team's other owners, #386; a hosted
    /// workspace's editors, native #705), everyone shared with, team
    /// members (connected or not), and whoever is connected.
    fn reachers(&self) -> Vec<Principal> {
        let mut out = vec![Principal::Owner];
        out.extend(self.config.people.as_owner());
        for g in self.config.acl.list() {
            out.push(Principal::User { id: g.principal.clone(), name: g.name.clone(), pic: None });
        }
        out.extend(self.config.people.team_people());
        for c in self.clients.values() {
            out.push(c.principal.clone());
        }
        let mut seen = HashSet::new();
        out.retain(|p| seen.insert(p.id().to_owned()));
        out
    }
}
