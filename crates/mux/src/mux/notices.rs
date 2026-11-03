//! Attention that belongs to no pane (#551): a notice the machine raises for
//! its owner, kept beside the panes' attention. One per key; raised again, it
//! counts up. The owner's `State` and `Delta`s carry them, and the event
//! stream says when one comes and goes.

use super::Daemon;
use arugula_proto::{EventKind, Notice};

impl Daemon {
    /// Raise `n`, or count it into the notice the same key holds (the
    /// latest headline, the first `since_ms`).
    pub(super) fn raise_notice(&mut self, n: Notice) {
        let notice = match self.pane_less.remove(&n.key) {
            Some(old) => Notice { since_ms: old.since_ms, count: old.count.saturating_add(n.count.max(1)), ..n },
            None => Notice { count: n.count.max(1), ..n },
        };
        self.pane_less.insert(notice.key.clone(), notice.clone());
        self.emit(None, EventKind::Notice { notice });
        self.broadcast();
    }

    /// Take the notice at `key` away: whether one was there.
    pub(super) fn clear_notice(&mut self, key: &str) -> bool {
        let there = self.pane_less.remove(key).is_some();
        if there {
            self.emit(None, EventKind::NoticeGone { key: key.to_owned() });
            self.broadcast();
        }
        there
    }

    /// The notices, oldest first.
    pub(super) fn notices_list(&self) -> Vec<Notice> {
        let mut out: Vec<Notice> = self.pane_less.values().cloned().collect();
        out.sort_by(|a, b| (a.since_ms, &a.key).cmp(&(b.since_ms, &b.key)));
        out
    }
}
