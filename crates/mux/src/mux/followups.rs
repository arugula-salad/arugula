//! Follow-ups for Claude Code in a terminal that wait for its inbox hook,
//! and which turn runs a tagged one (#540).
//!
//! A follow-up may carry a [`Tag`] the binary gave it (#675): the mux
//! never reads it, and tells the binary ([`Followups`]) where the turn that
//! takes it is. The turn starts when it is handed to the hook (at once if
//! the hook waits, else when the hook next waits, or at the next tool call:
//! #644) and ends at the pane's next `Stop` hook. A follow-up without a tag
//! (a person's) is told of to no one.

use std::collections::{HashMap, VecDeque};

use arugula_proto::{Driver, PaneId};

use super::{Followups, Tag, Turn};

/// A follow-up that waits: its text, whose it is, and its tag.
type Waiting = (String, Driver, Option<Tag>);

#[derive(Default)]
pub(super) struct Queue {
    queued: HashMap<PaneId, VecDeque<Waiting>>,
    /// The tags whose turn is running in each pane: several when the
    /// follow-up was several joined as one (#580).
    running: HashMap<PaneId, Vec<Tag>>,
}

impl Queue {
    /// Whether `pane` already has `max` waiting.
    pub(super) fn full(&self, pane: PaneId, max: usize) -> bool {
        self.queued.get(&pane).is_some_and(|q| q.len() >= max)
    }

    /// Keep one for the hook's next wait.
    pub(super) fn push(&mut self, pane: PaneId, text: String, by: Driver, tag: Option<Tag>) {
        self.queued.entry(pane).or_default().push_back((text, by, tag));
    }

    /// The card to raise for what is held on `pane` (#579): the oldest
    /// tagged follow-up's card, and how many are held. A person's follow-up
    /// (no tag) is never held.
    pub(super) fn held(&self, pane: PaneId) -> Option<(String, usize)> {
        let q = self.queued.get(&pane)?;
        let mut held = q.iter().filter_map(|(_, _, tag)| tag.as_ref());
        let first = held.next()?;
        Some((first.card.clone(), 1 + held.count()))
    }

    /// The owner tapped deliver (#579): the oldest held follow-up comes out
    /// of the queue to be typed, and its turn starts.
    pub(super) fn deliver_held(&mut self, pane: PaneId, told: &dyn Followups) -> Option<String> {
        let q = self.queued.get_mut(&pane)?;
        let at = q.iter().position(|(_, _, tag)| tag.is_some())?;
        let (text, _, tag) = q.remove(at)?;
        self.started(pane, tag, told);
        Some(text)
    }

    /// The owner dismissed the card (#579): the held follow-ups are dropped.
    /// How many.
    pub(super) fn drop_held(&mut self, pane: PaneId) -> usize {
        let Some(q) = self.queued.get_mut(&pane) else { return 0 };
        let before = q.len();
        q.retain(|(_, _, tag)| tag.is_none());
        before - q.len()
    }

    /// The hook waits: the oldest waiting follow-up goes to it now, and its
    /// turn starts. When it is tagged, the others queued that the binary says
    /// go with it come as one follow-up, oldest first (#580); the rest wait
    /// for the next take, and so does a person's.
    pub(super) fn take(&mut self, pane: PaneId, told: &dyn Followups) -> Option<(String, Driver)> {
        let q = self.queued.get_mut(&pane)?;
        let (text, by, tag) = q.pop_front()?;
        let Some(first) = tag else { return Some((text, by)) };
        let (text, tags) = Self::join(q, text, first, told);
        self.started_all(pane, tags, told);
        Some((text, by))
    }

    /// A tool call just ended in a running turn (#644): the oldest waiting
    /// tagged follow-up goes in now, with the others that go with it. A
    /// person's follow-up stays for the inbox hook, which starts a turn of
    /// its own, and so do the tagged ones that don't go with it, for the
    /// next tool call.
    pub(super) fn take_tagged(&mut self, pane: PaneId, told: &dyn Followups) -> Option<(String, Driver)> {
        let q = self.queued.get_mut(&pane)?;
        let at = q.iter().position(|(_, _, tag)| tag.is_some())?;
        let (text, by, tag) = q.remove(at)?;
        let (text, tags) = Self::join(q, text, tag?, told);
        self.started_all(pane, tags, told);
        Some((text, by))
    }

    /// The follow-up tagged `first`, with `text`, takes the others waiting
    /// in `q` that the binary says go with it, in order; they leave `q`.
    /// The text for all of them, and every tag taken.
    fn join(q: &mut VecDeque<Waiting>, mut text: String, first: Tag, told: &dyn Followups) -> (String, Vec<Tag>) {
        let mut tags = vec![first];
        let mut rest = VecDeque::new();
        for (t, b, tag) in std::mem::take(q) {
            match tag {
                Some(tag) => match told.joined(&tags, &tag) {
                    Some(joined) => {
                        text = joined;
                        tags.push(tag);
                    }
                    None => rest.push_back((t, b, Some(tag))),
                },
                None => rest.push_back((t, b, None)),
            }
        }
        *q = rest;
        (text, tags)
    }

    /// A follow-up went straight to a hook that was waiting: its turn starts.
    pub(super) fn started(&mut self, pane: PaneId, tag: Option<Tag>, told: &dyn Followups) {
        self.started_all(pane, tag.into_iter().collect(), told);
    }

    fn started_all(&mut self, pane: PaneId, tags: Vec<Tag>, told: &dyn Followups) {
        if tags.is_empty() {
            return;
        }
        for tag in &tags {
            told.turn(pane, tag, Turn::Started);
        }
        // Added to, not replaced: a turn takes several follow-ups (#644),
        // and its `Stop` ends all of them.
        self.running.entry(pane).or_default().extend(tags);
    }

    /// The pane's `Stop` hook spoke: the turn that was running has ended.
    pub(super) fn stopped(&mut self, pane: PaneId, told: &dyn Followups) {
        for tag in self.running.remove(&pane).unwrap_or_default() {
            told.turn(pane, &tag, Turn::Ended);
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

    /// A binary that hears each turn by the number in its tag, and joins
    /// tags of one group: `data` is `group/number` for those, a number
    /// alone for one that joins nothing.
    #[derive(Debug, Default)]
    struct Heard(Mutex<Vec<(PaneId, i64, Turn)>>);

    fn number(tag: &Tag) -> i64 {
        tag.data.rsplit('/').next().unwrap().parse().unwrap()
    }

    fn group(tag: &Tag) -> Option<&str> {
        tag.data.split_once('/').map(|(g, _)| g)
    }

    impl Followups for Heard {
        fn turn(&self, pane: PaneId, tag: &Tag, turn: Turn) {
            self.0.lock().unwrap().push((pane, number(tag), turn));
        }

        fn joined(&self, tags: &[Tag], next: &Tag) -> Option<String> {
            let g = group(next)?;
            if group(&tags[0]) != Some(g) {
                return None;
            }
            let lines = tags.iter().chain([next]).map(|t| format!("reply {}", number(t))).collect::<Vec<_>>();
            Some(format!("In {g}:\n{}", lines.join("\n")))
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

    fn tag(n: i64) -> Option<Tag> {
        Some(Tag { data: n.to_string(), card: format!("held {n}") })
    }

    fn grouped(g: &str, n: i64) -> Option<Tag> {
        Some(Tag { data: format!("{g}/{n}"), card: String::new() })
    }

    /// Queued, then taken when the hook waits: working then, read at the
    /// next Stop.
    #[test]
    fn a_queued_tagged_follow_up_is_working_when_the_hook_takes_it_and_read_at_the_next_stop() {
        let (mut f, heard) = (Queue::default(), Heard::default());
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
    fn a_tagged_follow_up_handed_to_a_waiting_hook_is_working_at_once() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        f.started(4, tag(8), &heard);
        assert_eq!(heard.all(), [(4, 8, Turn::Started)]);
        f.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 8, Turn::Ended)]);
    }

    /// A person's follow-up is told of to no one, and a closed pane says no more.
    #[test]
    fn an_untagged_follow_up_says_nothing_and_a_closed_pane_goes_quiet() {
        let (mut f, heard) = (Queue::default(), Heard::default());
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

    /// A tagged follow-up taken at a tool call is working at once, and read
    /// at the turn's `Stop`, along with any taken earlier in that turn
    /// (#644). A person's follow-up isn't taken: it waits for the inbox hook.
    #[test]
    fn a_tool_call_takes_a_tagged_follow_up_and_leaves_a_persons() {
        let (mut f, turns) = (Queue::default(), Heard::default());
        assert!(f.take_tagged(1, &turns).is_none(), "nothing waits");
        f.push(1, "from a person".into(), by(), None);
        assert!(f.take_tagged(1, &turns).is_none(), "a person's isn't for a tool call");
        f.push(1, "one".into(), by(), tag(5));
        f.push(1, "two".into(), by(), tag(6));
        assert_eq!(f.take_tagged(1, &turns).unwrap().0, "one");
        assert_eq!(turns.all(), [(1, 5, Turn::Started)]);
        assert_eq!(f.take_tagged(1, &turns).unwrap().0, "two");
        assert_eq!(turns.all(), [(1, 6, Turn::Started)]);
        assert!(f.take_tagged(1, &turns).is_none());
        f.stopped(1, &turns);
        assert_eq!(turns.all(), [(1, 5, Turn::Ended), (1, 6, Turn::Ended)]);
        assert_eq!(f.take(1, &turns).unwrap().0, "from a person");
    }

    /// Held (#579): only a tagged follow-up is held, never a person's;
    /// deliver takes the oldest tagged one out and starts its turn; dismiss
    /// drops the tagged ones and leaves a person's.
    #[test]
    fn a_held_follow_up_is_delivered_once_or_dropped_and_a_persons_stays() {
        let (mut f, heard) = (Queue::default(), Heard::default());
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
        assert!(heard.all().is_empty(), "a dropped follow-up is told of to no one");
        assert_eq!((f.held(4), f.deliver_held(4, &heard)), (None, None));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("hi"));
    }

    /// Three follow-ups in two groups (#580): the first take holds both of
    /// its group's, oldest first, as the binary joins them; the other group
    /// waits for the next take. Every tag is working at its take and read at
    /// the Stop after it.
    #[test]
    fn follow_ups_the_binary_joins_are_one_take_and_each_is_working_then_read() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        f.push(4, "r1".into(), by(), grouped("x", 11));
        f.push(4, "s1".into(), by(), grouped("y", 21));
        f.push(4, "hi".into(), by(), None);
        f.push(4, "r2".into(), by(), grouped("x", 12));
        f.push(4, "o1".into(), by(), grouped("z", 13));
        let (text, _) = f.take(4, &heard).unwrap();
        assert_eq!(text, "In x:\nreply 11\nreply 12");
        assert_eq!(heard.all(), [(4, 11, Turn::Started), (4, 12, Turn::Started)]);
        f.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 11, Turn::Ended), (4, 12, Turn::Ended)]);
        // The other group, then the person's follow-up, then the third.
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("s1"));
        heard.all();
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("hi"));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("o1"));
        assert!(f.take(4, &heard).is_none());
    }

    /// A lone one keeps its own words (#580): the binary isn't asked for
    /// a text until a second one goes with it.
    #[test]
    fn a_lone_follow_up_keeps_its_own_framing() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        f.push(4, "framed".into(), by(), grouped("x", 11));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("framed"));
    }

    #[test]
    fn the_queue_is_bounded_per_pane_and_goes_oldest_first() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        for i in 0..3 {
            f.push(4, format!("m{i}"), by(), None);
        }
        assert!(f.full(4, 3) && !f.full(4, 4) && !f.full(5, 1));
        assert_eq!(f.take(4, &heard).map(|(t, _)| t).as_deref(), Some("m0"));
    }
}
