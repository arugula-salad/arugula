//! Follow-ups for Claude Code in a terminal that wait for its inbox hook,
//! and which turn runs a mention (#540).
//!
//! A follow-up may carry the mention it is ([`MentionTag`]). The turn that
//! takes it starts when it is handed to the hook (at once if the hook waits,
//! else when the hook next waits) and ends at the pane's next `Stop` hook;
//! [`Turns`] hears both. A follow-up without a tag (a person's) is told of
//! to no one.

use std::collections::{HashMap, VecDeque};

use arugula_proto::{Driver, PaneId};

use super::{MentionTag, Turn, Turns};

/// A follow-up that waits: its text, whose it is, and its mention.
type Waiting = (String, Driver, Option<MentionTag>);

#[derive(Default)]
pub(super) struct Followups {
    queued: HashMap<PaneId, VecDeque<Waiting>>,
    /// The mention whose turn is running in each pane.
    running: HashMap<PaneId, MentionTag>,
}

impl Followups {
    /// Whether `pane` already has `max` waiting.
    pub(super) fn full(&self, pane: PaneId, max: usize) -> bool {
        self.queued.get(&pane).is_some_and(|q| q.len() >= max)
    }

    /// Keep one for the hook's next wait.
    pub(super) fn push(&mut self, pane: PaneId, text: String, by: Driver, tag: Option<MentionTag>) {
        self.queued.entry(pane).or_default().push_back((text, by, tag));
    }

    /// The hook waits: the oldest waiting follow-up goes to it now, and its
    /// turn starts.
    pub(super) fn take(&mut self, pane: PaneId, turns: &dyn Turns) -> Option<(String, Driver)> {
        let (text, by, tag) = self.queued.get_mut(&pane)?.pop_front()?;
        self.started(pane, tag, turns);
        Some((text, by))
    }

    /// A follow-up went straight to a hook that was waiting: its turn starts.
    pub(super) fn started(&mut self, pane: PaneId, tag: Option<MentionTag>, turns: &dyn Turns) {
        if let Some(tag) = tag {
            turns.turn(pane, &tag, Turn::Started);
            self.running.insert(pane, tag);
        }
    }

    /// The pane's `Stop` hook spoke: the turn that was running has ended.
    pub(super) fn stopped(&mut self, pane: PaneId, turns: &dyn Turns) {
        if let Some(tag) = self.running.remove(&pane) {
            turns.turn(pane, &tag, Turn::Ended);
        }
    }

    /// The pane closed: nothing more is said of its turn.
    pub(super) fn gone(&mut self, pane: PaneId) {
        self.queued.remove(&pane);
        self.running.remove(&pane);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    struct Heard(Mutex<Vec<(PaneId, i64, Turn)>>);

    impl Turns for Heard {
        fn turn(&self, pane: PaneId, tag: &MentionTag, turn: Turn) {
            self.0.lock().unwrap().push((pane, tag.seq, turn));
        }
    }

    impl Heard {
        fn all(&self) -> Vec<(PaneId, i64, Turn)> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    fn by() -> Driver {
        Driver { who: "agent-mention".into(), name: "x".into() }
    }

    fn tag(seq: i64) -> Option<MentionTag> {
        Some(MentionTag { room: "ch_x".into(), seq })
    }

    /// Queued, then taken when the hook waits: working then, read at the
    /// next Stop.
    #[test]
    fn a_queued_mention_is_working_when_the_hook_takes_it_and_read_at_the_next_stop() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        f.push(4, "one".into(), by(), tag(7));
        assert!(heard.all().is_empty(), "queued is not working");
        // The turn that was running ended; nothing of ours.
        f.stopped(4, &heard);
        assert!(heard.all().is_empty());
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("one"));
        assert_eq!(heard.all(), [(4, 7, Turn::Started)]);
        f.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 7, Turn::Ended)]);
        f.stopped(4, &heard);
        assert!(heard.all().is_empty(), "said once");
    }

    /// The hook was waiting: the turn starts at once.
    #[test]
    fn a_mention_handed_to_a_waiting_hook_is_working_at_once() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        f.started(4, tag(8), &heard);
        assert_eq!(heard.all(), [(4, 8, Turn::Started)]);
        f.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 8, Turn::Ended)]);
    }

    /// A person's follow-up is told of to no one, and a closed pane says no more.
    #[test]
    fn an_untagged_follow_up_says_nothing_and_a_closed_pane_goes_quiet() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        f.push(4, "hi".into(), by(), None);
        assert!(f.take(4, &heard).is_some());
        f.stopped(4, &heard);
        f.started(4, None, &heard);
        assert!(heard.all().is_empty());
        f.started(5, tag(9), &heard);
        heard.all();
        f.gone(5);
        f.stopped(5, &heard);
        assert!(heard.all().is_empty());
    }

    #[test]
    fn the_queue_is_bounded_per_pane_and_goes_oldest_first() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        for i in 0..3 {
            f.push(4, format!("m{i}"), by(), None);
        }
        assert!(f.full(4, 3) && !f.full(4, 4) && !f.full(5, 1));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("m0"));
    }
}
