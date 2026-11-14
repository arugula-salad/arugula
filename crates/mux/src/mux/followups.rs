//! Follow-ups for Claude Code in a terminal that wait for its inbox hook,
//! and which turn runs a tagged one (#540).
//!
//! A follow-up may carry a [`Tag`] the binary gave it (#675): the mux
//! never reads it, and tells the binary ([`Followups`]) where the turn that
//! takes it is. The turn starts when it is handed to the hook (at once if
//! the hook waits, else when the hook next waits, or at the next tool call:
//! #644) and ends at the pane's next `Stop` hook. A follow-up without a tag
//! (a person's) is told of to no one.
//!
//! The queue is kept on disk (#630, [`Saved`]), so a daemon restart or an
//! update loses none. A hook that acks ([`Hand::acks`]) gets each follow-up
//! with an id, and it stays [`Handed`] until the hook says it printed it:
//! one that is never acked is offered again ([`ACK_WITHIN_MS`]). Each one
//! keeps the Claude Code session the pane had when it came; handed to
//! another, it says so ([`EARLIER`]).

use std::collections::{HashMap, VecDeque};

use arugula_proto::{Driver, PaneId};
use serde::{Deserialize, Serialize};

use super::{Followups, Tag, Turn};

/// How long a follow-up handed to a hook that acks may go unacked before
/// it is offered again: the hook printed nothing, or died first.
pub(super) const ACK_WITHIN_MS: u64 = 30_000;

/// The first line of a follow-up handed to another Claude Code session than
/// the one the pane had when it was sent (#630).
pub(super) const EARLIER: &str = "(This follow-up was sent to an earlier Claude Code session in this pane, \
before /clear or a new session: it may not be about your current work.)";

/// A follow-up that waits: its text, whose it is, its tag, and the Claude
/// Code session it was for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Entry {
    pub text: String,
    pub by: Driver,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<Tag>,
    /// The pane's Claude Code session when it came, if it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// Follow-ups handed to a hook as one (`id`), waiting for its ack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Handed {
    pub id: u64,
    pub at: u64,
    pub entries: Vec<Entry>,
}

/// Who a follow-up is being handed to: the session the hook speaks for,
/// whether it acks, and when.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Hand<'a> {
    pub session: Option<&'a str>,
    pub acks: bool,
    pub now: u64,
}

/// What a hook is given: the text, whose it is, and the id it acks, if it
/// acks.
pub(super) type Given = (String, Driver, Option<u64>);

/// The queue as `followups.json` keeps it (#630).
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Saved {
    #[serde(default)]
    pub next: u64,
    #[serde(default)]
    pub queued: Vec<(PaneId, Vec<Entry>)>,
    #[serde(default)]
    pub handed: Vec<(PaneId, Vec<Handed>)>,
    #[serde(default)]
    pub running: Vec<(PaneId, Vec<Tag>)>,
    /// The panes whose hooks have spoken (`PaneInfo.unhooked`), so a restart
    /// doesn't make every pane look unhooked until its next hook.
    #[serde(default)]
    pub hooked: Vec<PaneId>,
}

#[derive(Default)]
pub(super) struct Queue {
    queued: HashMap<PaneId, VecDeque<Entry>>,
    /// The tags whose turn is running in each pane: several when the
    /// follow-up was several joined as one (#580).
    running: HashMap<PaneId, Vec<Tag>>,
    /// Handed to a hook that acks, and not acked yet.
    handed: HashMap<PaneId, Vec<Handed>>,
    next: u64,
    /// Changed since it was last written.
    dirty: bool,
}

impl Queue {
    /// Whether `pane` already has `max` waiting.
    pub(super) fn full(&self, pane: PaneId, max: usize) -> bool {
        self.queued.get(&pane).is_some_and(|q| q.len() >= max)
    }

    /// Keep one for the hook's next wait.
    #[cfg(test)]
    pub(super) fn push(&mut self, pane: PaneId, text: String, by: Driver, tag: Option<Tag>) {
        self.push_for(pane, Entry { text, by, tag, session: None });
    }

    /// [`push`](Self::push), with the session it was for.
    pub(super) fn push_for(&mut self, pane: PaneId, entry: Entry) {
        self.queued.entry(pane).or_default().push_back(entry);
        self.dirty = true;
    }

    /// The card to raise for what is held on `pane` (#579): the oldest
    /// tagged follow-up's card, and how many are held. A person's follow-up
    /// (no tag) is never held.
    pub(super) fn held(&self, pane: PaneId) -> Option<(String, usize)> {
        let q = self.queued.get(&pane)?;
        let mut held = q.iter().filter_map(|e| e.tag.as_ref());
        let first = held.next()?;
        Some((first.card.clone(), 1 + held.count()))
    }

    /// The owner tapped deliver (#579): the oldest held follow-up comes out
    /// of the queue to be typed, and its turn starts.
    pub(super) fn deliver_held(&mut self, pane: PaneId, told: &dyn Followups) -> Option<String> {
        let q = self.queued.get_mut(&pane)?;
        let at = q.iter().position(|e| e.tag.is_some())?;
        let e = q.remove(at)?;
        self.dirty = true;
        self.started(pane, e.tag, told);
        Some(e.text)
    }

    /// The owner dismissed the card (#579): the held follow-ups are dropped.
    /// How many.
    pub(super) fn drop_held(&mut self, pane: PaneId) -> usize {
        let Some(q) = self.queued.get_mut(&pane) else { return 0 };
        let before = q.len();
        q.retain(|e| e.tag.is_none());
        self.dirty |= before != q.len();
        before - q.len()
    }

    /// The hook waits: the oldest waiting follow-up goes to it now, and its
    /// turn starts. When it is tagged, the others queued that the binary says
    /// go with it come as one follow-up, oldest first (#580); the rest wait
    /// for the next take, and so does a person's.
    #[cfg(test)]
    pub(super) fn take(&mut self, pane: PaneId, told: &dyn Followups) -> Option<(String, Driver)> {
        self.take_for(pane, Hand::default(), told).map(|(t, b, _)| (t, b))
    }

    /// [`take`](Self::take), handed as `hand` says (#630): one unacked for
    /// too long goes first again.
    pub(super) fn take_for(&mut self, pane: PaneId, hand: Hand<'_>, told: &dyn Followups) -> Option<Given> {
        self.unacked(pane, hand.now);
        let q = self.queued.get_mut(&pane)?;
        let first = q.pop_front()?;
        self.dirty = true;
        if first.tag.is_none() {
            return Some(self.hand(pane, hand, vec![first], None));
        }
        let (text, entries) = Self::join(q, first, told);
        let tags = entries.iter().filter_map(|e| e.tag.clone()).collect();
        self.started_all(pane, tags, told);
        Some(self.hand(pane, hand, entries, Some(text)))
    }

    /// A tool call just ended in a running turn (#644): the oldest waiting
    /// tagged follow-up goes in now, with the others that go with it. A
    /// person's follow-up stays for the inbox hook, which starts a turn of
    /// its own, and so do the tagged ones that don't go with it, for the
    /// next tool call.
    #[cfg(test)]
    pub(super) fn take_tagged(&mut self, pane: PaneId, told: &dyn Followups) -> Option<(String, Driver)> {
        self.take_tagged_for(pane, Hand::default(), told).map(|(t, b, _)| (t, b))
    }

    /// [`take_tagged`](Self::take_tagged), handed as `hand` says (#630).
    pub(super) fn take_tagged_for(&mut self, pane: PaneId, hand: Hand<'_>, told: &dyn Followups) -> Option<Given> {
        self.unacked(pane, hand.now);
        let q = self.queued.get_mut(&pane)?;
        let at = q.iter().position(|e| e.tag.is_some())?;
        let first = q.remove(at)?;
        self.dirty = true;
        let (text, entries) = Self::join(q, first, told);
        let tags = entries.iter().filter_map(|e| e.tag.clone()).collect();
        self.started_all(pane, tags, told);
        Some(self.hand(pane, hand, entries, Some(text)))
    }

    /// A follow-up goes straight to a hook that was waiting: it waits for
    /// the hook's ack if the hook acks. Its turn starts once it's sent
    /// ([`started`](Self::started)).
    pub(super) fn hand_now(&mut self, pane: PaneId, entry: Entry, hand: Hand<'_>) -> Given {
        self.hand(pane, hand, vec![entry], None)
    }

    /// The text for `entries` (`text` when several were joined), said to be
    /// for an earlier session when it was; kept until acked when the hook
    /// acks.
    fn hand(&mut self, pane: PaneId, hand: Hand<'_>, entries: Vec<Entry>, text: Option<String>) -> Given {
        let text = text.unwrap_or_else(|| entries[0].text.clone());
        let by = entries[0].by.clone();
        let earlier = match hand.session {
            Some(now) => entries.iter().any(|e| e.session.as_deref().is_some_and(|s| s != now)),
            None => false,
        };
        let text = if earlier { format!("{EARLIER}\n{text}") } else { text };
        if !hand.acks {
            return (text, by, None);
        }
        self.next += 1;
        let id = self.next;
        self.handed.entry(pane).or_default().push(Handed { id, at: hand.now, entries });
        self.dirty = true;
        (text, by, Some(id))
    }

    /// The hook printed follow-up `id`: it's done. Whether it was waiting.
    pub(super) fn ack(&mut self, pane: PaneId, id: u64) -> bool {
        let Some(h) = self.handed.get_mut(&pane) else { return false };
        let before = h.len();
        h.retain(|x| x.id != id);
        let acked = h.len() != before;
        if h.is_empty() {
            self.handed.remove(&pane);
        }
        self.dirty |= acked;
        acked
    }

    /// What was handed to `pane`'s hook and not acked within
    /// [`ACK_WITHIN_MS`] goes back to the front of its queue, in order.
    fn unacked(&mut self, pane: PaneId, now: u64) {
        let Some(h) = self.handed.get_mut(&pane) else { return };
        let (stale, kept): (Vec<Handed>, Vec<Handed>) =
            std::mem::take(h).into_iter().partition(|x| now.saturating_sub(x.at) >= ACK_WITHIN_MS);
        *h = kept;
        if h.is_empty() {
            self.handed.remove(&pane);
        }
        if stale.is_empty() {
            return;
        }
        let q = self.queued.entry(pane).or_default();
        for e in stale.into_iter().flat_map(|x| x.entries).rev() {
            q.push_front(e);
        }
        self.dirty = true;
    }

    /// The follow-up `first`, and the others waiting in `q` that the binary
    /// says go with it, in order; they leave `q`. The text for all of them,
    /// and every one taken.
    fn join(q: &mut VecDeque<Entry>, first: Entry, told: &dyn Followups) -> (String, Vec<Entry>) {
        let mut text = first.text.clone();
        let mut tags = vec![first.tag.clone().expect("a tagged follow-up")];
        let mut taken = vec![first];
        let mut rest = VecDeque::new();
        for e in std::mem::take(q) {
            match e.tag.as_ref().and_then(|tag| told.joined(&tags, tag)) {
                Some(joined) => {
                    text = joined;
                    tags.push(e.tag.clone().expect("joined is tagged"));
                    taken.push(e);
                }
                None => rest.push_back(e),
            }
        }
        *q = rest;
        (text, taken)
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
        // and its `Stop` ends all of them. One offered again isn't twice.
        let running = self.running.entry(pane).or_default();
        for tag in tags {
            if !running.contains(&tag) {
                running.push(tag);
            }
        }
        self.dirty = true;
    }

    /// The pane's `Stop` hook spoke: the turn that was running has ended.
    pub(super) fn stopped(&mut self, pane: PaneId, told: &dyn Followups) {
        for tag in self.running.remove(&pane).unwrap_or_default() {
            self.dirty = true;
            told.turn(pane, &tag, Turn::Ended);
        }
    }

    /// The pane closed: nothing more is said of its turn.
    pub(super) fn gone(&mut self, pane: PaneId) {
        let had = self.queued.remove(&pane).is_some() | self.running.remove(&pane).is_some();
        self.dirty |= had | self.handed.remove(&pane).is_some();
    }

    /// Something kept with it changed (the panes whose hooks spoke).
    pub(super) fn changed(&mut self) {
        self.dirty = true;
    }

    /// Whether it changed since [`saved`](Self::saved) last said; clears it.
    pub(super) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// As `followups.json` keeps it, with the panes whose hooks spoke.
    pub(super) fn saved(&self, hooked: &std::collections::HashSet<PaneId>) -> Saved {
        let sorted = |mut v: Vec<PaneId>| {
            v.sort_unstable();
            v
        };
        Saved {
            next: self.next,
            queued: sorted(self.queued.keys().copied().collect())
                .into_iter()
                .filter_map(|p| Some((p, self.queued.get(&p)?.iter().cloned().collect::<Vec<_>>())))
                .filter(|(_, q)| !q.is_empty())
                .collect(),
            handed: sorted(self.handed.keys().copied().collect())
                .into_iter()
                .filter_map(|p| Some((p, self.handed.get(&p)?.clone())))
                .collect(),
            running: sorted(self.running.keys().copied().collect())
                .into_iter()
                .filter_map(|p| Some((p, self.running.get(&p)?.clone())))
                .collect(),
            hooked: sorted(hooked.iter().copied().collect()),
        }
    }

    /// The queue `followups.json` kept, for the panes in `alive`; what was
    /// handed and never acked waits again, first. The tags of panes that are
    /// gone come back with it, each to be told it was dropped.
    pub(super) fn restore(saved: Saved, alive: &dyn Fn(PaneId) -> bool) -> (Self, Vec<(PaneId, Tag)>) {
        let mut q = Queue { next: saved.next, ..Default::default() };
        let mut gone: Vec<(PaneId, Tag)> = vec![];
        let mut lost = |pane: PaneId, tags: &mut dyn Iterator<Item = Tag>| gone.extend(tags.map(|t| (pane, t)));
        for (pane, handed) in saved.handed {
            let entries = handed.into_iter().flat_map(|h| h.entries);
            if alive(pane) {
                q.queued.entry(pane).or_default().extend(entries);
            } else {
                lost(pane, &mut entries.filter_map(|e| e.tag));
            }
        }
        for (pane, entries) in saved.queued {
            if alive(pane) {
                q.queued.entry(pane).or_default().extend(entries);
            } else {
                lost(pane, &mut entries.into_iter().filter_map(|e| e.tag));
            }
        }
        for (pane, tags) in saved.running {
            if alive(pane) {
                q.running.insert(pane, tags);
            } else {
                lost(pane, &mut tags.into_iter());
            }
        }
        (q, gone)
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

    fn hand(session: Option<&str>, now: u64) -> Hand<'_> {
        Hand { session, acks: true, now }
    }

    /// A hook that acks (#630): what it's given stays until it acks; one
    /// never acked comes again, first, after [`ACK_WITHIN_MS`], and its turn
    /// isn't said twice; an acked one never comes again.
    #[test]
    fn a_follow_up_stays_until_acked_and_comes_again_if_it_isnt() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        f.push(4, "one".into(), by(), tag(7));
        f.push(4, "two".into(), by(), None);
        let (text, _, id) = f.take_for(4, hand(None, 1_000), &heard).unwrap();
        assert_eq!((text.as_str(), id.is_some()), ("one", true));
        assert_eq!(heard.all(), [(4, 7, Turn::Started)]);
        // Not acked, and not stale yet: the next is "two".
        let (text, _, two) = f.take_for(4, hand(None, 2_000), &heard).unwrap();
        assert_eq!(text, "two");
        assert!(f.ack(4, two.unwrap()));
        // Stale: "one" comes again, with a new id.
        let (text, _, again) = f.take_for(4, hand(None, 1_000 + ACK_WITHIN_MS), &heard).unwrap();
        assert_eq!(text, "one");
        assert_ne!(again, id);
        assert!(!f.ack(4, id.unwrap()), "the first id is gone");
        assert!(f.ack(4, again.unwrap()));
        assert!(f.take_for(4, hand(None, 10 * ACK_WITHIN_MS), &heard).is_none(), "acked: never again");
        heard.all();
        f.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 7, Turn::Ended)], "ended once");
        // A hook that doesn't ack is given no id, and nothing is kept.
        f.push(4, "three".into(), by(), None);
        let (_, _, none) = f.take_for(4, Hand { acks: false, ..hand(None, 0) }, &heard).unwrap();
        assert_eq!(none, None);
        assert!(f.take_for(4, hand(None, 10 * ACK_WITHIN_MS), &heard).is_none());
    }

    /// Sent to one Claude Code session, taken by another (#630): it says so
    /// first. The same session, or one unknown, gets it as it was.
    #[test]
    fn a_follow_up_for_an_earlier_session_says_so() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        let entry = |text: &str, session: Option<&str>| Entry {
            text: text.into(),
            by: by(),
            tag: None,
            session: session.map(str::to_owned),
        };
        f.push_for(4, entry("old", Some("s1")));
        f.push_for(4, entry("same", Some("s2")));
        f.push_for(4, entry("unknown", None));
        let (text, _, _) = f.take_for(4, hand(Some("s2"), 0), &heard).unwrap();
        assert_eq!(text, format!("{EARLIER}\nold"));
        assert_eq!(f.take_for(4, hand(Some("s2"), 0), &heard).unwrap().0, "same");
        assert_eq!(f.take_for(4, hand(Some("s2"), 0), &heard).unwrap().0, "unknown");
        f.push_for(4, entry("no hook session", Some("s1")));
        assert_eq!(f.take_for(4, hand(None, 0), &heard).unwrap().0, "no hook session");
    }

    /// The queue survives a restart (#630): what waited, what was handed
    /// and not acked (first), the running turn, and the hooked panes come
    /// back for panes that are still there; a gone pane's tags come back to
    /// be told they were dropped.
    #[test]
    fn the_queue_is_kept_across_a_restart() {
        let (mut f, heard) = (Queue::default(), Heard::default());
        f.push(4, "handed".into(), by(), tag(1));
        f.push(4, "waiting".into(), by(), None);
        f.push(5, "gone".into(), by(), tag(2));
        let (_, _, id) = f.take_for(4, hand(None, 0), &heard).unwrap();
        assert!(id.is_some());
        assert!(f.take_dirty() && !f.take_dirty());
        let hooked: std::collections::HashSet<PaneId> = [4, 5].into();
        let saved = serde_json::to_vec(&f.saved(&hooked)).unwrap();
        let saved: Saved = serde_json::from_slice(&saved).unwrap();
        assert_eq!(saved.hooked, [4, 5]);
        let (mut back, gone) = Queue::restore(saved, &|p| p == 4);
        assert_eq!(gone.iter().map(|(p, t)| (*p, number(t))).collect::<Vec<_>>(), [(5, 2)]);
        heard.all();
        let (text, _, again) = back.take_for(4, hand(None, 0), &heard).unwrap();
        assert_eq!(text, "handed");
        assert!(again.unwrap() > id.unwrap(), "ids go on from where they were");
        assert_eq!(back.take_for(4, hand(None, 0), &heard).unwrap().0, "waiting");
        heard.all();
        back.stopped(4, &heard);
        assert_eq!(heard.all(), [(4, 1, Turn::Ended)], "the running turn ends once after the restart");
    }
}
