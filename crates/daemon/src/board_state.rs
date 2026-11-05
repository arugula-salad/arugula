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
//!   unknown, so a machine that is
//!   gone stops holding its rows. A block this daemon knew that is no longer
//!   among its panes (its close was missed) is sent as closed.
//! - **Debounce:** the mux's events are read on a task of their own, and
//!   only the newest word per pane is kept, with when it was seen; every
//!   [`EVERY`] what's kept is sent, one request at a time, with its age, so
//!   a chatty agent costs a request every couple of seconds at most and the
//!   mux never waits on control.
//! - **No grant, no retry loop:** control answers 403 without a
//!   `boards.write` grant (404 with the flag off there), and then nothing is
//!   sent for [`NO_GRANT`]; a failure to reach control waits [`TROUBLE`].
//!   What is kept stays kept through either wait, and events go on being
//!   kept, so a block that closes during an outage is still told (its age
//!   says when). A report control refuses for itself (a 400) is dropped
//!   and the rest go on.
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
        diffstat: w.diffstat.as_deref().and_then(|d| d.lines().last()).and_then(diffstat),
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

/// The newest word for each pane not yet sent, with when it was seen.
#[derive(Default)]
struct Queue(HashMap<PaneId, (BlockWord, Instant)>);

impl Queue {
    /// A word seen `at`: it replaces the pane's older one.
    fn note(&mut self, id: PaneId, word: BlockWord, at: Instant) {
        self.0.insert(id, (word, at));
    }

    /// A pane's state as it is now, sent again: a close still waiting to be
    /// sent stays (the pane is gone, so nothing newer can come).
    fn again(&mut self, id: PaneId, word: BlockWord, at: Instant) {
        if !matches!(self.0.get(&id), Some((BlockWord::Closed, _))) {
            self.note(id, word, at);
        }
    }

    /// What wasn't sent, back in the queue: a word noted since is newer and
    /// wins.
    fn put_back(&mut self, rest: impl IntoIterator<Item = (PaneId, (BlockWord, Instant))>) {
        for (id, w) in rest {
            self.0.entry(id).or_insert(w);
        }
    }

    /// Everything, oldest pane id first.
    fn take(&mut self) -> Vec<(PaneId, (BlockWord, Instant))> {
        let mut batch: Vec<_> = self.0.drain().collect();
        batch.sort_by_key(|(id, _)| *id);
        batch
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
    // When every live agent block is next sent again; joining sets it to now.
    let mut again_at: Option<Instant> = None;
    let mut joined = false;
    // Control refused a report with its age: it is from before Arugula #745,
    // so reports go without one.
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
                if on && !joined {
                    again_at = Some(Instant::now());
                }
                joined = on;
                if !on {
                    pending.0.clear();
                    continue;
                }
                if quiet_until.is_some_and(|t| Instant::now() < t) {
                    continue;
                }
                quiet_until = None;
                if again_at.is_none_or(|t| Instant::now() >= t) {
                    again_at = Some(Instant::now() + Duration::from_millis(BLOCK_STATE_EVERY_MS));
                    send_again(&app, &mut pending, &mut agents, &mut others).await;
                }
                let mut batch = pending.take().into_iter();
                while let Some((id, (word, seen))) = batch.next() {
                    let repo = repos.entry(id).or_default();
                    if word == BlockWord::Idle
                        && let Some(w) = crate::ops::prompt::turn_result(&app, id).await.and_then(|t| t.work)
                    {
                        *repo = work_of(&w);
                    }
                    let state = body(word, repo, title_of(&app, id).await);
                    if word == BlockWord::Closed {
                        agents.remove(&id);
                        repos.remove(&id);
                    }
                    let age = u64::try_from(seen.elapsed().as_millis()).unwrap_or(u64::MAX);
                    match send(&app, id, &state, age, &mut plain).await {
                        Ok(()) => debug!(pane = id, "told control a block's state"),
                        Err(Wait::Dropped(why)) => warn!(pane = id, "control refused a block's state: {why}"),
                        Err(Wait::For(d, why)) => {
                            warn!(pane = id, "not telling control about a block for a while: {why}");
                            quiet_until = Some(Instant::now() + d);
                            // Kept for after the wait: this pane's word and
                            // the rest of the batch's, with when each was
                            // seen. After a refusal the grant may be new by
                            // then, so every block is sent again.
                            pending.put_back(std::iter::once((id, (word, seen))).chain(batch));
                            if d == NO_GRANT {
                                again_at = None;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Queue every live agent block's state as it is now, and a close for each
/// block known here that is no longer among the panes.
async fn send_again(app: &App, pending: &mut Queue, agents: &mut HashSet<PaneId>, others: &mut HashSet<PaneId>) {
    let Some(panes) = app.mux.api(Api::Panes).await else { return };
    let now = Instant::now();
    let mut live = HashSet::new();
    for p in &panes {
        if p.info.kind != BlockType::Agent {
            continue;
        }
        live.insert(p.info.id);
        agents.insert(p.info.id);
        others.remove(&p.info.id);
        pending.again(p.info.id, word_now(p.info.attention), now);
    }
    for gone in agents.iter().filter(|id| !live.contains(id)) {
        pending.again(*gone, BlockWord::Closed, now);
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
enum Wait {
    /// Nothing more is sent for a while.
    For(Duration, String),
    /// Control refused this report itself: the rest go on.
    Dropped(String),
}

/// What a refusal means for what's kept.
fn wait_of(e: CloudRefused) -> Wait {
    match e {
        // No grant (403), the flag off there (404), a control from before
        // this (405): ask again after a while, not on every event.
        CloudRefused::Said { status: 403..=405, said, .. } => Wait::For(NO_GRANT, said),
        CloudRefused::OldControl | CloudRefused::Off | CloudRefused::NotJoined => {
            Wait::For(NO_GRANT, "control doesn't take it".into())
        }
        // Too many, or slow: try again.
        CloudRefused::Said { status: 408 | 429, said, .. } => Wait::For(TROUBLE, said),
        // This report: sending it again would be refused again.
        CloudRefused::Said { status: 400..=499, said, .. } => Wait::Dropped(said),
        e => Wait::For(TROUBLE, e.to_string()),
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
            call(put(None)).await.map_err(|_| Wait::Dropped(why))?;
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
        q.put_back(batch);
        assert_eq!(q.0[&1], (BlockWord::Idle, later), "the newer word");
        assert_eq!(q.0[&2], (BlockWord::Closed, start), "kept, with when it was seen");
        q.again(2, BlockWord::Working, later);
        q.again(1, BlockWord::NeedsInput, later);
        assert_eq!(q.0[&2].0, BlockWord::Closed, "a close stays");
        assert_eq!(q.0[&1].0, BlockWord::NeedsInput);
    }

    #[test]
    fn a_failure_to_reach_control_waits_and_a_bad_report_is_dropped() {
        let said = |status| CloudRefused::Said { status, said: String::new(), reason: None };
        assert!(matches!(wait_of(said(403)), Wait::For(d, _) if d == NO_GRANT));
        assert!(matches!(wait_of(said(503)), Wait::For(d, _) if d == TROUBLE));
        assert!(matches!(wait_of(said(429)), Wait::For(d, _) if d == TROUBLE));
        assert!(matches!(wait_of(CloudRefused::Unreachable("x".into())), Wait::For(d, _) if d == TROUBLE));
        assert!(matches!(wait_of(said(400)), Wait::Dropped(_)));
        assert!(matches!(wait_of(said(422)), Wait::Dropped(_)));
    }
}
