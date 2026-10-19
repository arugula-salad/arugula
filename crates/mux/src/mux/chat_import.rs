//! Bringing this machine's threads into control's chat (M124, S41 q6): what
//! the store holds, who reads each pane's and session's thread now, and
//! retiring the store once control has it. The daemon does the talking.

use super::{ChatExport, ChatReaders, Daemon, SAVE_DEBOUNCE};
use arugula_proto::ThreadTarget;
use tokio::time::Instant;

impl Daemon {
    /// Every pane's and session's thread with messages, and every
    /// conversation with its timeline (none once imported).
    pub(super) fn chat_export(&self) -> ChatExport {
        if self.threads.imported() {
            return ChatExport { imported: true, ..Default::default() };
        }
        let mut threads: Vec<_> = self
            .threads
            .targets()
            .filter(|t| !matches!(t, ThreadTarget::Conv(_)))
            .map(|t| (t, self.threads.get(t).to_vec()))
            .filter(|(_, m)| !m.is_empty())
            .collect();
        threads.sort_by_key(|(t, _)| *t);
        let mut convs: Vec<_> =
            self.threads.convs().map(|c| (c.clone(), self.threads.get(ThreadTarget::Conv(c.id)).to_vec())).collect();
        convs.sort_by_key(|(c, _)| (c.created, c.id));
        ChatExport { threads, convs, imported: false }
    }

    /// Who may read the thread on each pane and session here now, as
    /// `thread_role` decides it, of the people who can reach this machine.
    /// The owner reads everything and isn't listed.
    pub(super) fn chat_readers(&self) -> ChatReaders {
        let people: Vec<_> = self.mentionable().into_iter().filter(|p| !p.is_owner()).collect();
        let mut targets: Vec<ThreadTarget> =
            self.panes.keys().chain(self.blocks.keys()).map(|p| ThreadTarget::Pane(*p)).collect();
        targets.extend(self.mux.sessions.iter().map(|s| ThreadTarget::Session(s.id)));
        targets.sort();
        targets.dedup();
        targets
            .into_iter()
            .map(|t| {
                let readers = people
                    .iter()
                    .filter_map(|p| {
                        self.thread_role(p, t)
                            .map(|(role, from)| (p.id().to_owned(), from, role < arugula_core::Role::Editor))
                    })
                    .collect();
                (t, readers)
            })
            .collect()
    }

    /// Control has the threads it was given (`given`: each one's last
    /// message): `threads/` becomes `threads.imported/` and an empty store
    /// takes its place. Not if anything was posted since the export (or a
    /// conversation made): the daemon exports again, and control takes
    /// only what it hasn't got.
    pub(super) fn chat_handoff(&mut self, given: &[(ThreadTarget, u64)]) -> Result<(), String> {
        if self.threads.imported() {
            return Ok(());
        }
        if self.chat_lasts() != Self::sorted(given) {
            return Err("posted to since: export again".into());
        }
        self.threads.retire().map_err(|e| format!("can't move the threads aside: {e}"))?;
        // Conversations let their members in (#856): no more.
        self.config.acl.set_reach(self.threads.reach());
        self.save_due.get_or_insert_with(|| Instant::now() + SAVE_DEBOUNCE);
        self.soon();
        Ok(())
    }

    /// Each thread's last message number, and each conversation's (0 for
    /// one with no messages), in order.
    fn chat_lasts(&self) -> Vec<(ThreadTarget, u64)> {
        let last = |t| self.threads.get(t).last().map_or(0, |m| m.id);
        let threads = self.threads.targets().filter(|t| !matches!(t, ThreadTarget::Conv(_)));
        let convs = self.threads.convs().map(|c| ThreadTarget::Conv(c.id));
        Self::sorted(&threads.chain(convs).map(|t| (t, last(t))).collect::<Vec<_>>())
    }

    fn sorted(l: &[(ThreadTarget, u64)]) -> Vec<(ThreadTarget, u64)> {
        let mut l = l.to_vec();
        l.sort();
        l.dedup();
        l
    }
}
