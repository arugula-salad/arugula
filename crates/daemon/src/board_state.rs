//! Telling control what this machine's agent blocks are doing, for the
//! board rows that point at them (live status board, step 4; aru-116's
//! second ruling).
//!
//! What is sent is [`BlockState`] and nothing else: the state word, the
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
//!   Only agent blocks, only with the `boards` flag on here, and only when
//!   this machine has joined control.
//! - **Debounce:** the mux's events are read on a task of their own, and
//!   only the newest word per pane is kept; every [`EVERY`] what's kept is
//!   sent, one request at a time, so a chatty agent costs a request every
//!   couple of seconds at most and the mux never waits on control.
//! - **No grant, no retry loop:** control answers 403 without a
//!   `boards.write` grant (404 with the flag off there), and then nothing is
//!   sent for [`NO_GRANT`]; a failure to reach control waits [`TROUBLE`].
//!   What was kept for a pane is sent again after a transient failure and
//!   dropped after a refusal (the next event says it anew).
//!
//! Rows are not added here: a person or a lead adds them (`board_write`
//! with an agent block's pointer), and this keeps their state current.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_wire::boards::{AGENT_BLOCK_STATE, BlockState, BlockWord, Diffstat, PutBlockState};
use arugula_proto::{Attention, BlockType, EventKind, PaneId, api::Work, flags};
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

fn body(word: BlockWord, repo: &Repo) -> BlockState {
    BlockState {
        state: word,
        branch: repo.branch.clone(),
        head: repo.head.clone(),
        pushed: repo.pushed,
        uncommitted: repo.uncommitted,
        diffstat: repo.diffstat,
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

/// Follow the mux's events and keep control told, for as long as the
/// daemon runs.
pub async fn keep_reporting(app: Arc<App>) {
    let mut events = app.mux.events();
    // The newest word for each pane not yet sent.
    let mut pending: HashMap<PaneId, BlockWord> = HashMap::new();
    let mut agents: HashSet<PaneId> = HashSet::new();
    // Panes known not to be agent blocks (a pane's kind never changes), so
    // a terminal's events don't ask the mux for its panes each time.
    let mut others: HashSet<PaneId> = HashSet::new();
    let mut repos: HashMap<PaneId, Repo> = HashMap::new();
    let mut quiet_until: Option<Instant> = None;
    let mut tick = tokio::time::interval(EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            ev = events.recv() => {
                let ev = match ev {
                    Ok(ev) => ev,
                    // Missed some: the next event of each pane says it anew.
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                };
                let (Some(id), Some(word)) = (ev.pane, word_of(&ev.kind)) else { continue };
                if !enabled(&app) || quiet_until.is_some_and(|t| Instant::now() < t) {
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
                pending.insert(id, word);
            }
            _ = tick.tick(), if !pending.is_empty() => {
                if quiet_until.is_some_and(|t| Instant::now() < t) || !enabled(&app) {
                    pending.clear();
                    continue;
                }
                quiet_until = None;
                let mut batch: Vec<_> = pending.drain().collect();
                batch.sort_by_key(|(id, _)| *id);
                let mut batch = batch.into_iter();
                while let Some((id, word)) = batch.next() {
                    let repo = repos.entry(id).or_default();
                    if word == BlockWord::Idle
                        && let Some(w) = crate::ops::prompt::turn_result(&app, id).await.and_then(|t| t.work)
                    {
                        *repo = work_of(&w);
                    }
                    let state = body(word, repo);
                    if word == BlockWord::Closed {
                        agents.remove(&id);
                        repos.remove(&id);
                    }
                    match send(&app, id, &state).await {
                        Ok(()) => debug!(pane = id, "told control a block's state"),
                        Err(Wait::For(d, why)) => {
                            warn!(pane = id, "not telling control about a block for a while: {why}");
                            quiet_until = Some(Instant::now() + d);
                            // A transient failure keeps this pane's word and
                            // the rest of the batch's, for after the wait.
                            if d == TROUBLE {
                                pending.insert(id, word);
                                pending.extend(batch);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Whether this machine reports now: the `boards` flag is on here, and it
/// has joined control.
fn enabled(app: &App) -> bool {
    crate::labs::flags(app.control.state_dir()).has(flags::BOARDS) && app.control.enrolled().is_some()
}

async fn is_agent(app: &App, id: PaneId) -> bool {
    app.mux.api(Api::Panes).await.unwrap_or_default().iter().any(|p| p.info.id == id && p.info.kind == BlockType::Agent)
}

/// Why sending stopped, and for how long.
enum Wait {
    For(Duration, String),
}

async fn send(app: &App, id: PaneId, state: &BlockState) -> Result<(), Wait> {
    let put = PutBlockState { pane: id.to_string(), block: state.clone() };
    let body = serde_json::to_value(&put).map_err(|e| Wait::For(NO_GRANT, e.to_string()))?;
    let call = app.control.cloud_now(Method::PUT, AGENT_BLOCK_STATE, Some(&body));
    match tokio::time::timeout(REQUEST, call).await {
        Err(_) => Err(Wait::For(TROUBLE, "control didn't answer".into())),
        Ok(Ok(_)) => Ok(()),
        // No grant (403), the flag off there (404), a control from before
        // this (405): ask again after a while, not on every event.
        Ok(Err(CloudRefused::Said { status: 403..=405, said, .. })) => Err(Wait::For(NO_GRANT, said)),
        Ok(Err(CloudRefused::OldControl | CloudRefused::Off | CloudRefused::NotJoined)) => {
            Err(Wait::For(NO_GRANT, "control doesn't take it".into()))
        }
        Ok(Err(e)) => Err(Wait::For(TROUBLE, e.to_string())),
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
}
