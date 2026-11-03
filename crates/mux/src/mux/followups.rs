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

use super::{MentionTag, Turn, Turns, absorb};

/// A follow-up that waits: its text, whose it is, and its mention.
type Waiting = (String, Driver, Option<MentionTag>);

#[derive(Default)]
pub(super) struct Followups {
    queued: HashMap<PaneId, VecDeque<Waiting>>,
    /// The mentions whose turn is running in each pane: several when the
    /// follow-up merged a thread's (#580).
    running: HashMap<PaneId, Vec<MentionTag>>,
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

    /// The card to raise for what is held on `pane` (#579): the oldest
    /// mention's headline, and how many are held. A person's follow-up
    /// (no mention) is never held.
    pub(super) fn held(&self, pane: PaneId) -> Option<(String, usize)> {
        let q = self.queued.get(&pane)?;
        let mut held = q.iter().filter_map(|(_, _, tag)| tag.as_ref());
        let first = held.next()?;
        Some((first.headline.clone(), 1 + held.count()))
    }

    /// The owner tapped deliver (#579): the oldest held mention comes out of
    /// the queue to be typed, and its turn starts.
    pub(super) fn deliver_held(&mut self, pane: PaneId, turns: &dyn Turns) -> Option<String> {
        let q = self.queued.get_mut(&pane)?;
        let at = q.iter().position(|(_, _, tag)| tag.is_some())?;
        let (text, _, tag) = q.remove(at)?;
        self.started(pane, tag, turns);
        Some(text)
    }

    /// The owner dismissed the card (#579): the held mentions are dropped.
    /// How many.
    pub(super) fn drop_held(&mut self, pane: PaneId) -> usize {
        let Some(q) = self.queued.get_mut(&pane) else { return 0 };
        let before = q.len();
        q.retain(|(_, _, tag)| tag.is_none());
        before - q.len()
    }

    /// The hook waits: the oldest waiting follow-up goes to it now, and its
    /// turn starts. When it is a mention, the others queued from its thread
    /// come with it as one follow-up, oldest first (#580); the other
    /// threads' wait for the next take, and so does a person's.
    pub(super) fn take(&mut self, pane: PaneId, turns: &dyn Turns) -> Option<(String, Driver)> {
        let q = self.queued.get_mut(&pane)?;
        let (mut text, by, tag) = q.pop_front()?;
        let Some(first) = tag else { return Some((text, by)) };
        let mut tags = vec![first];
        let mut rest = VecDeque::new();
        for (t, b, tag) in std::mem::take(q) {
            match tag {
                Some(tag) if absorb(&mut text, &mut tags, &tag) => {}
                tag => rest.push_back((t, b, tag)),
            }
        }
        *q = rest;
        self.started_all(pane, tags, turns);
        Some((text, by))
    }

    /// A follow-up went straight to a hook that was waiting: its turn starts.
    pub(super) fn started(&mut self, pane: PaneId, tag: Option<MentionTag>, turns: &dyn Turns) {
        self.started_all(pane, tag.into_iter().collect(), turns);
    }

    fn started_all(&mut self, pane: PaneId, tags: Vec<MentionTag>, turns: &dyn Turns) {
        if tags.is_empty() {
            return;
        }
        for tag in &tags {
            turns.turn(pane, tag, Turn::Started);
        }
        self.running.insert(pane, tags);
    }

    /// The pane's `Stop` hook spoke: the turn that was running has ended.
    pub(super) fn stopped(&mut self, pane: PaneId, turns: &dyn Turns) {
        for tag in self.running.remove(&pane).unwrap_or_default() {
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

    use super::{super::MentionThread, *};

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
        Some(MentionTag { room: "ch_x".into(), seq, headline: format!("held {seq}"), thread: None })
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

    /// Held (#579): only a mention is held, never a person's follow-up;
    /// deliver takes the oldest mention out and starts its turn; dismiss
    /// drops the mentions and leaves a person's follow-up.
    #[test]
    fn a_held_mention_is_delivered_once_or_dropped_and_a_persons_follow_up_stays() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        f.push(4, "hi".into(), by(), None);
        assert_eq!(f.held(4), None);
        f.push(4, "one".into(), by(), tag(7));
        f.push(4, "two".into(), by(), tag(8));
        assert_eq!(f.held(4), Some(("held 7".into(), 2)));
        assert!(heard.all().is_empty(), "held is not working");
        assert_eq!(f.deliver_held(4, &heard).as_deref(), Some("one"));
        assert_eq!(heard.all(), [(4, 7, Turn::Started)]);
        assert_eq!(f.held(4), Some(("held 8".into(), 1)));
        assert_eq!(f.drop_held(4), 1);
        assert!(heard.all().is_empty(), "a dropped mention is told of to no one");
        assert_eq!((f.held(4), f.deliver_held(4, &heard)), (None, None));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("hi"));
    }

    fn reply(room: &str, seq: i64, root: i64) -> Option<MentionTag> {
        let thread = MentionThread {
            root,
            head: format!("Replies in {room} to \"root {root}\":"),
            line: format!("ada: reply {seq}"),
            tail: "(tail)".into(),
        };
        Some(MentionTag { room: room.into(), seq, headline: String::new(), thread: Some(thread) })
    }

    /// Three replies in two threads (#580): the first take holds both of its
    /// thread's, oldest first; the other thread waits for the next take. Every
    /// tag is working at its take and read at the Stop after it.
    #[test]
    fn replies_of_one_thread_are_one_take_and_each_is_working_then_read() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        f.push(4, "r1".into(), by(), reply("ch_x", 11, 10));
        f.push(4, "s1".into(), by(), reply("ch_x", 21, 20));
        f.push(4, "hi".into(), by(), None);
        f.push(4, "r2".into(), by(), reply("ch_x", 12, 10));
        // Same root number in another room is another thread.
        f.push(4, "o1".into(), by(), reply("ch_y", 13, 10));
        let (text, _) = f.take(4, &heard).unwrap();
        assert_eq!(text, "Replies in ch_x to \"root 10\":\nada: reply 11\nada: reply 12\n\n(tail)");
        assert_eq!(heard.all(), [(4, 11, Turn::Started), (4, 12, Turn::Started)]);
        f.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 11, Turn::Ended), (4, 12, Turn::Ended)]);
        // The other thread, then the person's follow-up, then the other room.
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("s1"));
        heard.all();
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("hi"));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("o1"));
        assert!(f.take(4, &heard).is_none());
    }

    /// A single reply keeps its own words (#580).
    #[test]
    fn a_lone_reply_keeps_its_own_framing() {
        let (mut f, heard) = (Followups::default(), Heard::default());
        f.push(4, "framed".into(), by(), reply("ch_x", 11, 10));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("framed"));
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
