//! Telling control what this machine's agent blocks are doing, for the
//! board rows that point at them (boards, step 4; aru-116's
//! second ruling).
//!
//! What is sent is [`BlockState`] and nothing else: the state word, the
//! name its starter gave a block that was started on an issue (#372), the
//! branch, the short HEAD, whether it is pushed, the number of uncommitted
//! files and the diffstat as three numbers. Its type has no field for
//! message text, file names or a transcript, and this module reads none
//! (a turn's message is dropped where [`work_of`] takes the work out of it).
//!
//! - **When:** an agent block's `Attention` event says needs input (or
//!   working, or idle), `Prompted` says working, `Closed` says closed. At
//!   idle the state also carries the turn's work summary (AR-1: the same
//!   `turn.work` that `prompt_agent` and `wait` return); the other states
//!   repeat the last work seen, so a row keeps its branch while it works.
//!   Only agent blocks, and only when this machine has joined control.
//! - **Again:** every live agent block's state is sent again on start, on
//!   joining control, when a refusal's wait ends (a grant may be new), and
//!   every [`BLOCK_STATE_EVERY_MS`] (Arugula #745). Control counts a report
//!   older than [`arugula_control_wire::boards::BLOCK_STATE_STALE_MS`] as
//!   unknown, so a machine that is gone stops holding its rows. A block this daemon knew that is no longer
//!   among its panes (its close was missed) is sent as closed.
//! - **Debounce:** the mux's events are read on a task of their own, and
//!   only the newest word per pane is kept, with when it was seen; every
//!   [`EVERY`] what's kept is sent, the longest waiting first, one request
//!   at a time, with its age, so a chatty agent costs a request every
//!   couple of seconds at most and the mux never waits on control.
//! - **No grant, no retry loop:** control answers 403 without a
//!   `boards.write` grant (404 with the flag off there, 401 or 410 for a
//!   machine it doesn't take), and then nothing is sent for [`NO_GRANT`];
//!   a failure to reach control, a 429 or a timeout waits [`TROUBLE`]. What
//!   is kept stays kept through either wait, and events go on being kept,
//!   so a block that closes during an outage is still told (its age says
//!   when). A 5xx fails that report alone: the rest go on, and it is tried
//!   again after [`TROUBLE`], up to [`TRIES`] times ([`CLOSE_TRIES`] for a
//!   close). A report control refuses for itself (a 400, 413 or 422) is
//!   dropped.
//!
//! Rows are not added here: a person or a lead adds them (`board_write`
//! with an agent block's pointer), and this keeps their state current.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_wire::boards::{
    AGENT_BLOCK_STATE, BLOCK_STATE_EVERY_MS, BlockState, BlockWord, Diffstat, PutBlockState,
};
use arugula_proto::{Attention, BlockType, EventKind, PaneId, api::Work};
use reqwest::Method;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, warn};

use crate::{control::CloudRefused, mux::Api, server::App};

/// How often what has changed is sent.
const EVERY: Duration = Duration::from_secs(2);
/// How long nothing is sent after control refuses for want of a grant (or
/// the flag, or a route).
const NO_GRANT: Duration = Duration::from_secs(600);
/// How long after control couldn't be reached or failed.
const TROUBLE: Duration = Duration::from_secs(30);
/// The longest one request may take.
const REQUEST: Duration = Duration::from_secs(20);

/// What the daemon last knew of a pane's repo.
#[derive(Clone, Default)]
struct Repo {
    branch: Option<String>,
    head: Option<String>,
    pushed: Option<bool>,
    uncommitted: Option<u32>,
    diffstat: Option<Diffstat>,
}

/// The facts of a turn's work that a board may hold.
fn work_of(w: &Work) -> Repo {
    Repo {
        branch: Some(w.branch.clone()),
        head: Some(w.head.clone()).filter(|h| !h.is_empty()),
        pushed: Some(w.pushed),
        uncommitted: Some(w.uncommitted_total),
        // The turn's own diff: a turn that left none says so with zeros,
        // which control reads as none, so the last turn's stat goes (a
        // report that leaves a field out keeps what was said).
        diffstat: Some(w.diffstat.as_deref().and_then(|d| d.lines().last()).and_then(diffstat).unwrap_or(Diffstat {
            files: 0,
            added: 0,
            removed: 0,
        })),
    }
}

/// A `git diff --stat` summary line (` 3 files changed, 40 insertions(+),
/// 2 deletions(-)`) as numbers; none for a line that isn't one.
fn diffstat(line: &str) -> Option<Diffstat> {
    let count = |word: &str| -> Option<u32> {
        line.split(", ")
            .find(|p| p.contains(word))
            .and_then(|p| p.split_whitespace().next())
            .and_then(|n| n.parse().ok())
    };
    let files = count("changed")?;
    Some(Diffstat { files, added: count("insertion").unwrap_or(0), removed: count("deletion").unwrap_or(0) })
}

fn body(word: BlockWord, repo: &Repo, title: Option<String>) -> BlockState {
    BlockState {
        state: word,
        title,
        branch: repo.branch.clone(),
        head: repo.head.clone(),
        pushed: repo.pushed,
        uncommitted: repo.uncommitted,
        diffstat: repo.diffstat,
        since: None,
    }
}

/// What an event says of an agent block's state.
fn word_of(kind: &EventKind) -> Option<BlockWord> {
    match kind {
        EventKind::Attention { state: Attention::NeedsInput, .. } => Some(BlockWord::NeedsInput),
        EventKind::Attention { state: Attention::Working, .. } | EventKind::Prompted => Some(BlockWord::Working),
        EventKind::Attention { state: Attention::Idle | Attention::Done, .. } => Some(BlockWord::Idle),
        EventKind::Closed => Some(BlockWord::Closed),
        _ => None,
    }
}

/// How many times a word that isn't a close is tried before it is dropped.
const TRIES: u32 = 5;
/// How many 5xx answers a close is given before it is dropped: about an
/// hour of them, with [`TROUBLE`] between, so a close control keeps
/// failing doesn't slow every other report until the daemon restarts.
/// Control out of reach counts none (see [`Queue::after`]). Control counts
/// the block as unknown once its last report is stale.
const CLOSE_TRIES: u32 = 120;

/// A word not yet sent: when it was seen, and how many times it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kept {
    word: BlockWord,
    seen: Instant,
    tries: u32,
}

/// The newest word for each pane not yet sent.
#[derive(Default)]
struct Queue(HashMap<PaneId, Kept>);

impl Queue {
    /// A word seen `at`: it replaces the pane's older one.
    fn note(&mut self, id: PaneId, word: BlockWord, at: Instant) {
        self.0.insert(id, Kept { word, seen: at, tries: 0 });
    }

    /// A pane's state as it is now, sent again: a close still waiting to be
    /// sent stays (the pane is gone, so nothing newer can come).
    fn again(&mut self, id: PaneId, word: BlockWord, at: Instant) {
        if !matches!(self.0.get(&id), Some(k) if k.word == BlockWord::Closed) {
            self.note(id, word, at);
        }
    }

    /// A word that wasn't sent, back in the queue: a word noted since is
    /// newer and wins. `failed`: control answered this report with a 5xx,
    /// so it counts a try, and a word goes after [`TRIES`] ([`CLOSE_TRIES`]
    /// for a close). False when it went.
    fn put_back(&mut self, id: PaneId, mut k: Kept, failed: bool) -> bool {
        if failed {
            k.tries += 1;
            let most = if k.word == BlockWord::Closed { CLOSE_TRIES } else { TRIES };
            if k.tries >= most {
                return false;
            }
        }
        self.0.entry(id).or_insert(k);
        true
    }

    /// A word that `wait` kept from being sent, back in the queue. Only a
    /// 5xx for this report ([`Wait::Failed`]) counts a try: control out of
    /// reach, slow or busy says nothing of the report, so a close made on a
    /// flight waits as long as the flight. False when it went.
    fn after(&mut self, id: PaneId, k: Kept, wait: &Wait) -> bool {
        self.put_back(id, k, matches!(wait, Wait::Failed(_)))
    }

    /// Everything, the longest waiting first, so one pane that fails can't
    /// keep the rest back.
    fn take(&mut self) -> Vec<(PaneId, Kept)> {
        let mut batch: Vec<_> = self.0.drain().collect();
        batch.sort_by_key(|(id, k)| (k.seen, *id));
        batch
    }
}

/// When every live agent block is sent again: on joining control (so on
/// start too), when a refusal's wait ends, and every
/// [`BLOCK_STATE_EVERY_MS`].
#[derive(Default)]
struct Again {
    at: Option<Instant>,
    joined: bool,
}

impl Again {
    /// What the tick saw of joining: joining makes it due now.
    fn joined(&mut self, on: bool, now: Instant) {
        if on && !self.joined {
            self.at = Some(now);
        }
        self.joined = on;
    }

    /// Whether it is due now; if so, the next is in
    /// [`BLOCK_STATE_EVERY_MS`].
    fn due(&mut self, now: Instant) -> bool {
        if self.at.is_some_and(|t| now < t) {
            return false;
        }
        self.at = Some(now + Duration::from_millis(BLOCK_STATE_EVERY_MS));
        true
    }

    /// Control refused for want of a grant: once the wait ends a grant may
    /// be new, so it is due then.
    fn refused(&mut self) {
        self.at = None;
    }
}

/// The word a pane's attention says now, for a block sent again.
fn word_now(a: Attention) -> BlockWord {
    match a {
        Attention::NeedsInput => BlockWord::NeedsInput,
        Attention::Working => BlockWord::Working,
        Attention::Idle | Attention::Done => BlockWord::Idle,
    }
}

/// What sending every block again says: each live agent block's state now,
/// and a close for each block `known` here that is no longer among them
/// (its close was missed).
fn again_words(live: &[(PaneId, Attention)], known: &HashSet<PaneId>) -> Vec<(PaneId, BlockWord)> {
    let ids: HashSet<PaneId> = live.iter().map(|(id, _)| *id).collect();
    let mut words: Vec<_> = live.iter().map(|(id, a)| (*id, word_now(*a))).collect();
    words.extend(known.iter().filter(|id| !ids.contains(id)).map(|id| (*id, BlockWord::Closed)));
    words
}

/// Follow the mux's events and keep control told, for as long as the
/// daemon runs.
pub async fn keep_reporting(app: Arc<App>) {
    let mut events = app.mux.events();
    let mut pending = Queue::default();
    let mut agents: HashSet<PaneId> = HashSet::new();
    // Panes known not to be agent blocks (a pane's kind never changes), so
    // a terminal's events don't ask the mux for its panes each time.
    let mut others: HashSet<PaneId> = HashSet::new();
    let mut repos: HashMap<PaneId, Repo> = HashMap::new();
    let mut quiet_until: Option<Instant> = None;
    let mut again = Again::default();
    // Control refused a report with its age: it is from before Arugula #745,
    // so reports go without one until every block is sent again, when it is
    // tried once more (control may have been upgraded).
    let mut plain = false;
    let mut tick = tokio::time::interval(EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            ev = events.recv() => {
                let ev = match ev {
                    Ok(ev) => ev,
                    // Missed some: the next event of each pane says it anew,
                    // and a close that was missed is found when every block
                    // is sent again.
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                };
                let (Some(id), Some(word)) = (ev.pane, word_of(&ev.kind)) else { continue };
                // Not joined: nothing is kept, and joining sends every block.
                if !enabled(&app) {
                    continue;
                }
                if !agents.contains(&id) {
                    if word == BlockWord::Closed {
                        others.remove(&id);
                        continue;
                    }
                    if others.contains(&id) {
                        continue;
                    }
                    if !is_agent(&app, id).await {
                        others.insert(id);
                        continue;
                    }
                    agents.insert(id);
                }
                pending.note(id, word, Instant::now());
            }
            _ = tick.tick() => {
                let on = enabled(&app);
                again.joined(on, Instant::now());
                if !on {
                    pending.0.clear();
                    continue;
                }
                if quiet_until.is_some_and(|t| Instant::now() < t) {
                    continue;
                }
                quiet_until = None;
                if again.due(Instant::now()) {
                    plain = false;
                    send_again(&app, &mut pending, &mut agents, &mut others).await;
                }
                let mut failed = false;
                let mut batch = pending.take().into_iter();
                while let Some((id, kept)) = batch.next() {
                    let repo = repos.entry(id).or_default();
                    if kept.word == BlockWord::Idle
                        && let Some(w) = crate::ops::prompt::turn_result(&app, id).await.and_then(|t| t.work)
                    {
                        *repo = work_of(&w);
                    }
                    let state = body(kept.word, repo, title_of(&app, id).await);
                    if kept.word == BlockWord::Closed {
                        agents.remove(&id);
                        repos.remove(&id);
                    }
                    let age = u64::try_from(kept.seen.elapsed().as_millis()).unwrap_or(u64::MAX);
                    match send(&app, id, &state, age, &mut plain).await {
                        Ok(()) => debug!(pane = id, "told control a block's state"),
                        Err(Wait::Dropped(why)) => warn!(pane = id, "control refused a block's state: {why}"),
                        // This report failed: the rest go on, and it is
                        // tried again after a wait.
                        Err(ref wait @ Wait::Failed(ref why)) => {
                            failed = true;
                            if !pending.after(id, kept, wait) {
                                warn!(pane = id, "gave up telling control a block's state: {why}");
                            }
                        }
                        Err(ref wait @ Wait::For(d, ref why)) => {
                            warn!(pane = id, "not telling control about a block for a while: {why}");
                            quiet_until = Some(Instant::now() + d);
                            // Kept for after the wait, with when each was
                            // seen, no try counted. After a refusal the grant
                            // may be new by then, so every block is sent
                            // again.
                            pending.after(id, kept, wait);
                            for (id, k) in batch.by_ref() {
                                pending.put_back(id, k, false);
                            }
                            if d == NO_GRANT {
                                again.refused();
                            }
                            break;
                        }
                    }
                }
                if failed && quiet_until.is_none() {
                    quiet_until = Some(Instant::now() + TROUBLE);
                }
            }
        }
    }
}

/// Queue every live agent block's state as it is now, and a close for each
/// block known here that is no longer among the panes.
async fn send_again(app: &App, pending: &mut Queue, agents: &mut HashSet<PaneId>, others: &mut HashSet<PaneId>) {
    let Some(panes) = app.mux.api(Api::Panes).await else { return };
    let live: Vec<(PaneId, Attention)> =
        panes.iter().filter(|p| p.info.kind == BlockType::Agent).map(|p| (p.info.id, p.info.attention)).collect();
    let now = Instant::now();
    for (id, word) in again_words(&live, agents) {
        pending.again(id, word, now);
    }
    for (id, _) in &live {
        agents.insert(*id);
        others.remove(id);
    }
}

/// Whether this machine reports now: it has joined control.
fn enabled(app: &App) -> bool {
    app.control.enrolled().is_some()
}

async fn is_agent(app: &App, id: PaneId) -> bool {
    app.mux.api(Api::Panes).await.unwrap_or_default().iter().any(|p| p.info.id == id && p.info.kind == BlockType::Agent)
}

/// The name a block started on an issue was given (`start_agent`'s `title`,
/// or a rename): its chip on a board. Never the first words of a prompt, and
/// none for a block that isn't on an issue.
async fn title_of(app: &App, id: PaneId) -> Option<String> {
    let config = app.mux.api(|r| Api::Block(id, r)).await.flatten()?.config();
    config.get("work").filter(|w| !w.is_null())?;
    let title = config["title"].as_str()?.trim();
    (!title.is_empty() && title.chars().count() <= 200 && !title.chars().any(char::is_control))
        .then(|| title.to_owned())
}

/// Why a report wasn't sent.
#[derive(Debug)]
enum Wait {
    /// Nothing more is sent for a while: control can't be reached, is busy,
    /// or refuses this machine.
    For(Duration, String),
    /// This report failed (a 5xx): the rest go on, and it is tried again
    /// after [`TROUBLE`], up to [`TRIES`] times ([`CLOSE_TRIES`] for a close).
    Failed(String),
    /// Control refused this report itself: sending it again would be
    /// refused again.
    Dropped(String),
}

/// What a refusal means for what's kept.
fn wait_of(e: CloudRefused) -> Wait {
    match e {
        // Not signed in as far as control can tell (401), no grant (403),
        // the flag off there (404), a control from before this (405), the
        // machine gone from it (410): ask again after a while, not on every
        // event.
        CloudRefused::Said { status: 401 | 403..=405 | 410, said, .. } => Wait::For(NO_GRANT, said),
        CloudRefused::OldControl | CloudRefused::Off | CloudRefused::NotJoined => {
            Wait::For(NO_GRANT, "control doesn't take it".into())
        }
        // Too many, or slow: everything waits.
        CloudRefused::Said { status: 408 | 429, said, .. } => Wait::For(TROUBLE, said),
        // This report's body: malformed, too big, or a field control
        // doesn't take.
        CloudRefused::Said { status: 400 | 413 | 422, said, .. } => Wait::Dropped(said),
        CloudRefused::Said { said, .. } => Wait::Failed(said),
        e @ (CloudRefused::Unreachable(_) | CloudRefused::Maintenance) => Wait::For(TROUBLE, e.to_string()),
    }
}

async fn send(app: &App, id: PaneId, state: &BlockState, age: u64, plain: &mut bool) -> Result<(), Wait> {
    let put = |age_ms| PutBlockState { pane: id.to_string(), block: state.clone().into(), age_ms };
    let call = |put: PutBlockState| async move {
        let body = serde_json::to_value(&put).map_err(|e| Wait::Dropped(e.to_string()))?;
        let call = app.control.cloud_now(Method::PUT, AGENT_BLOCK_STATE, Some(&body));
        match tokio::time::timeout(REQUEST, call).await {
            Err(_) => Err(Wait::For(TROUBLE, "control didn't answer".into())),
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(wait_of(e)),
        }
    };
    if *plain {
        return call(put(None)).await;
    }
    match call(put(Some(age))).await {
        // A control from before Arugula #745 refuses a field it doesn't
        // know: if the same report without the age goes through, it's that.
        Err(Wait::Dropped(why)) => {
            // Refused without the age too: this report. Failed or timed out:
            // kept, as any other.
            call(put(None)).await.map_err(|e| match e {
                Wait::Dropped(_) => Wait::Dropped(why),
                e => e,
            })?;
            *plain = true;
            debug!("control takes a block's state without its age");
            Ok(())
        }
        sent => sent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stat_summary_becomes_numbers_and_nothing_else() {
        assert_eq!(
            diffstat(" 3 files changed, 40 insertions(+), 2 deletions(-)"),
            Some(Diffstat { files: 3, added: 40, removed: 2 })
        );
        assert_eq!(diffstat(" 1 file changed, 1 deletion(-)"), Some(Diffstat { files: 1, added: 0, removed: 1 }));
        assert_eq!(diffstat(" src/secret.rs | 4 ++--"), None);
    }

    /// A turn's work sets the diffstat, to zeros when the turn left no diff,
    /// so the last turn's stat doesn't stay on the board.
    #[test]
    fn a_turn_with_no_diff_says_so() {
        let work = |diffstat: Option<&str>| Work {
            worktree: None,
            branch: "feat/x".into(),
            head: "abc1234".into(),
            pushed: false,
            uncommitted: vec![],
            uncommitted_total: 0,
            commits: None,
            diffstat: diffstat.map(str::to_owned),
        };
        let stat = work_of(&work(Some(" a.rs | 2 +-\n 1 file changed, 1 insertion(+), 1 deletion(-)"))).diffstat;
        assert_eq!(stat, Some(Diffstat { files: 1, added: 1, removed: 1 }));
        assert_eq!(work_of(&work(None)).diffstat, Some(Diffstat { files: 0, added: 0, removed: 0 }));
    }

    /// A close made while control is out of reach outlasts any number of
    /// unreachable sends, timeouts and 429s: only a 5xx counts a try.
    #[test]
    fn a_close_survives_a_long_outage() {
        let close = Kept { word: BlockWord::Closed, seen: Instant::now(), tries: 0 };
        let mut q = Queue::default();
        let mut k = close;
        for n in 0..10 * CLOSE_TRIES {
            let wait = match n % 3 {
                0 => Wait::For(TROUBLE, "can't reach control".into()),
                1 => Wait::For(TROUBLE, "control didn't answer".into()),
                _ => Wait::For(NO_GRANT, "403".into()),
            };
            assert!(q.after(1, k, &wait), "kept after {n} waits");
            k = q.take()[0].1;
        }
        assert_eq!(k.tries, 0, "no try counted");
        let failed = Wait::Failed("502".into());
        for _ in 1..CLOSE_TRIES {
            assert!(q.after(1, k, &failed));
            k = q.take()[0].1;
        }
        assert!(!q.after(1, k, &failed), "5xx after 5xx: given up at {CLOSE_TRIES}");
    }

    /// What wasn't sent goes back with when it was seen, a newer word wins,
    /// and a close waiting to be sent isn't undone by sending every block
    /// again (Arugula #745).
    #[test]
    fn what_wasnt_sent_is_kept_with_when_it_was_seen() {
        let start = Instant::now();
        let later = start + Duration::from_secs(5);
        let mut q = Queue::default();
        q.note(1, BlockWord::Working, start);
        q.note(2, BlockWord::Closed, start);
        let batch = q.take();
        assert!(q.0.is_empty());
        // A send failed: while it waited, pane 1 went idle.
        q.note(1, BlockWord::Idle, later);
        for (id, k) in batch {
            assert!(q.put_back(id, k, true));
        }
        assert_eq!((q.0[&1].word, q.0[&1].seen), (BlockWord::Idle, later), "the newer word");
        assert_eq!((q.0[&2].word, q.0[&2].seen, q.0[&2].tries), (BlockWord::Closed, start, 1), "kept, with when");
        q.again(2, BlockWord::Working, later);
        q.again(1, BlockWord::NeedsInput, later);
        assert_eq!(q.0[&2].word, BlockWord::Closed, "a close stays");
        assert_eq!(q.0[&1].word, BlockWord::NeedsInput);
    }

    /// The longest waiting goes first, whatever its pane's number, so a pane
    /// that keeps failing can't hold back the others; a word that isn't a
    /// close is given up after its tries, and a close after many more.
    #[test]
    fn the_oldest_goes_first_and_a_close_is_tried_longest() {
        let start = Instant::now();
        let mut q = Queue::default();
        q.note(1, BlockWord::Working, start + Duration::from_secs(3));
        q.note(9, BlockWord::Idle, start);
        let order: Vec<_> = q.take().into_iter().map(|(id, _)| id).collect();
        assert_eq!(order, [9, 1]);

        let working = Kept { word: BlockWord::Working, seen: start, tries: 0 };
        let closed = Kept { word: BlockWord::Closed, ..working };
        let (mut w, mut c) = (working, closed);
        for n in 1..TRIES {
            assert!(q.put_back(3, w, true), "try {n}");
            w = q.take()[0].1;
            assert!(q.put_back(4, c, true));
            c = q.take()[0].1;
        }
        assert!(!q.put_back(3, w, true), "given up after {TRIES}");
        assert!(q.put_back(4, c, true), "a close is kept longer");
        let mut c = q.take()[0].1;
        while c.tries + 1 < CLOSE_TRIES {
            assert!(q.put_back(4, c, true));
            c = q.take()[0].1;
        }
        assert!(!q.put_back(4, c, true), "but not for good: given up after {CLOSE_TRIES}");
        assert!(q.put_back(5, working, false), "not tried: no try counted");
    }

    /// Every block is sent again on joining (so on start), every
    /// `BLOCK_STATE_EVERY_MS`, and when a refusal's wait ends; not while
    /// out of control.
    #[test]
    fn every_block_is_sent_again_on_joining_on_a_refusal_and_every_five_minutes() {
        let t = Instant::now();
        let every = Duration::from_millis(BLOCK_STATE_EVERY_MS);
        let mut a = Again::default();
        a.joined(true, t);
        assert!(a.due(t), "on start, joined");
        assert!(!a.due(t + Duration::from_secs(2)));
        assert!(!a.due(t + every - Duration::from_secs(1)));
        assert!(a.due(t + every), "five minutes on");
        let t2 = t + every + Duration::from_secs(10);
        a.refused();
        assert!(a.due(t2), "after a refusal's wait");
        a.joined(false, t2);
        a.joined(true, t2 + Duration::from_secs(4));
        assert!(a.due(t2 + Duration::from_secs(4)), "joining again");
    }

    /// Sending every block again says each live one's state now, and a
    /// close for a block known here that is gone.
    #[test]
    fn a_gone_block_is_sent_as_closed() {
        let known: HashSet<PaneId> = [1, 2].into();
        let mut words = again_words(&[(1, Attention::Done), (3, Attention::NeedsInput)], &known);
        words.sort_by_key(|(id, _)| *id);
        assert_eq!(words, [(1, BlockWord::Idle), (2, BlockWord::Closed), (3, BlockWord::NeedsInput)]);
    }

    #[test]
    fn a_failure_to_reach_control_waits_and_a_bad_report_is_dropped() {
        let said = |status| CloudRefused::Said { status, said: String::new(), reason: None };
        for status in [401, 403, 404, 405, 410] {
            assert!(matches!(wait_of(said(status)), Wait::For(d, _) if d == NO_GRANT), "{status}");
        }
        for status in [408, 429] {
            assert!(matches!(wait_of(said(status)), Wait::For(d, _) if d == TROUBLE), "{status}");
        }
        assert!(matches!(wait_of(CloudRefused::Unreachable("x".into())), Wait::For(d, _) if d == TROUBLE));
        for status in [500, 502, 503, 409] {
            assert!(matches!(wait_of(said(status)), Wait::Failed(_)), "{status}");
        }
        for status in [400, 413, 422] {
            assert!(matches!(wait_of(said(status)), Wait::Dropped(_)), "{status}");
        }
    }
}
