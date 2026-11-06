//! Who may read each pane and session here (M124, S41 decision 2): this
//! machine decides, as it decides who reads the pane, and the binary tells
//! whoever asks (control, for what it keeps about them).

use std::collections::HashSet;

use super::{Daemon, Readable, Readers};
use crate::{acl::Principal, store::ReadersAtClose};
use arugula_core::Role;
use arugula_proto::SessionId;

impl Daemon {
    /// Who may read each pane and session here now, of the people who can
    /// reach this machine. The owner reads everything and isn't listed.
    ///
    /// A team's other owners (#386), and a hosted workspace's editors, get
    /// in as the owner, so they read what the owner reads: every pane, a
    /// private one included (native #684, #705). Each is listed by their
    /// own account, as the owner's role, from the start.
    ///
    /// A pane or session that closed is listed too, while it's remembered
    /// (`store::Closed`), and only ever narrows (Jake, 2026-11-05): it's
    /// read by those who read it when it closed and still may now, as if
    /// it were still there, each from no earlier than they did then. A
    /// share of its session taken away later, or a member leaving the
    /// team, takes its thread; a share given later, or a member joining,
    /// doesn't give it. Once its session has closed too, the session's
    /// shares are gone with it.
    pub(super) fn readers(&self) -> Readers {
        let people = self.people();
        let mut targets: Vec<Readable> =
            self.panes.keys().chain(self.blocks.keys()).map(|p| Readable::Pane(*p)).collect();
        targets.extend(self.mux.sessions.iter().map(|s| Readable::Session(s.id)));
        targets.extend(self.closed.panes.keys().map(|p| Readable::Pane(*p)));
        targets.extend(self.closed.sessions.keys().map(|s| Readable::Session(*s)));
        targets.sort();
        targets.dedup();
        targets
            .into_iter()
            .map(|t| {
                let readers = people
                    .iter()
                    .filter_map(|(id, who)| {
                        self.role_on(id, who, t).map(|(role, from)| (id.clone(), from, role < Role::Editor))
                    })
                    .collect();
                (t, readers)
            })
            .collect()
    }

    /// Who reads a pane of `session` (`private` or not), or the session,
    /// now: by principal id, from when. What a pane or session that's
    /// closing keeps (`store::Closed`), and whether the team's part of it
    /// waits for the roster.
    pub(super) fn readers_at_close(&self, session: SessionId, private: bool) -> (ReadersAtClose, bool) {
        let readers = self
            .people()
            .into_iter()
            .filter_map(|(id, who)| Some((id, self.role_in(&who, session, private)?.1)))
            .collect();
        (readers, !self.config.people.roster_known())
    }

    /// Whenever who may read what changes (and before the lists are
    /// said), make the closed panes' and sessions' readers stick: whoever
    /// no longer reads one is taken off it for good, so a share given them
    /// again later doesn't bring it back, whether chat is on or not. Not
    /// those kept out only by their team's lock, which is lifted: they're
    /// refused while it lasts (`readers`) and read again after. Only once
    /// the team's roster is known, which is also when one that closed
    /// before it was gets the team's part of who read it: its members and
    /// owners (its shares were known when it closed).
    pub(super) fn settle_closed(&mut self) {
        let known = self.config.people.roster_known();
        if !known || self.closed.is_empty() {
            return;
        }
        let people: std::collections::HashMap<String, Principal> = self.people().into_iter().collect();
        let locked: HashSet<String> = self.config.people.locked_out().into_iter().collect();
        let team: Vec<(&String, &Principal)> = {
            let ids: HashSet<String> = self
                .config
                .people
                .team_people()
                .into_iter()
                .chain(self.config.people.as_owner())
                .map(|p| p.id().to_owned())
                .collect();
            people.iter().filter(|(id, _)| ids.contains(*id)).collect()
        };
        let mut closed = std::mem::take(&mut self.closed);
        let mut changed = false;
        let mut settle = |readers: &mut ReadersAtClose, pending: &mut bool, session: SessionId, private: bool| {
            if std::mem::take(pending) {
                changed = true;
                for (id, who) in &team {
                    if let Some((_, from)) = self.role_in(who, session, private) {
                        readers.entry((*id).clone()).or_insert(from);
                    }
                }
            }
            let before = readers.len();
            readers.retain(|id, _| {
                locked.contains(id) || people.get(id).is_some_and(|who| self.role_in(who, session, private).is_some())
            });
            changed |= readers.len() != before;
        };
        for c in closed.panes.values_mut() {
            settle(&mut c.readers, &mut c.pending, c.session, c.private);
        }
        for (s, c) in closed.sessions.iter_mut() {
            settle(&mut c.readers, &mut c.pending, *s, false);
        }
        closed.dirty |= changed;
        self.closed = closed;
        self.save_closed();
    }

    /// Everyone but the owner who may reach this machine, by principal id,
    /// as the machine admits them: a co-owner, or a hosted workspace's
    /// editor, as the owner.
    fn people(&self) -> Vec<(String, Principal)> {
        let as_owner: HashSet<String> = self.config.people.as_owner().into_iter().map(|p| p.id().to_owned()).collect();
        self.reachers()
            .into_iter()
            .filter(|p| !p.is_owner())
            .map(|p| {
                let id = p.id().to_owned();
                let as_who = if as_owner.contains(&id) { Principal::Owner } else { p };
                (id, as_who)
            })
            .collect()
    }

    /// `who`'s (`id`'s) role on a pane or session here, and the time what
    /// they may read of it starts: a pane is read by whoever may read it
    /// ([`Self::readable`]: a private pane only by its owner, and by the
    /// team's other owners, who are the owner here), a session by whoever
    /// has a role in it. A "from now" share reads from when it was made.
    /// A closed one only by whom it kept, as it was when it closed.
    fn role_on(&self, id: &str, who: &Principal, target: Readable) -> Option<(Role, u64)> {
        let open = |p| self.panes.contains_key(&p) || self.blocks.contains_key(&p);
        let (session, private, kept) = match target {
            Readable::Pane(p) if !open(p) => {
                let c = self.closed.panes.get(&p)?;
                (c.session, c.private, Some(&c.readers))
            }
            Readable::Pane(p) => (self.session_of(p).filter(|_| self.readable(who, p))?, false, None),
            Readable::Session(s) if self.mux.session(s).is_err() => {
                (s, false, Some(&self.closed.sessions.get(&s)?.readers))
            }
            Readable::Session(s) => (s, false, None),
        };
        let (role, from) = self.role_in(who, session, private)?;
        match kept {
            None => Some((role, from)),
            Some(kept) => kept.get(id).map(|then| (role, from.max(*then))),
        }
    }

    /// `who`'s role in `session`, through a pane of it that's `private` or
    /// not, and the time what they may read starts.
    fn role_in(&self, who: &Principal, session: SessionId, private: bool) -> Option<(Role, u64)> {
        if private && !who.is_owner() {
            return None;
        }
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
