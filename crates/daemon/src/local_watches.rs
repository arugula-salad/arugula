//! Local watches (https://app.arugula.io/issues/iss_ad072f84ac1c0fdffb66fbdb982902e4):
//! `watch` on the event catalog's daemon types (`agent.idle`,
//! `agent.needs_input`, `pane.command.ended`, `pane.exited`,
//! `pane.output.matched`), held by the daemon itself, so they work with no
//! account. There is no daemon-side event log: a watch is matched against
//! the same pane and block states `wait` reads (`api::attention_of`, a
//! pane's `status`), checked when the mux says something happened to its
//! pane.
//!
//! `pane.output.matched` is matched against the pane's output instead, not
//! its state: a watch keeps the stream offset it has read to, which starts
//! at the pane's end when it's made, so what was on screen or in scrollback
//! never fires it, and [`scan`] reads the lines past it, ANSI stripped as
//! `wait`'s `until: match` strips them. The offset is kept in [`FILE`] now
//! and then; after a restart the lines since are read again, which can't
//! match, since a watch that matched is gone.
//!
//! The rules are a cloud watch's: it belongs to the Claude Code session that
//! made it, fires once, expires (24 hours unless told, at most 7 days) with
//! one last message, says so at create when it has already happened, and a
//! session has at most [`PER_SESSION`] live. Its message goes to the
//! session's follow-up queue the way a cloud watch's does
//! (`agent_mentions::deliver`, with its [`WatchTag`] and no room), so the
//! inbox hook, the told-once record and a dropped message's resend are the
//! same. The watches and their messages until told are kept in [`FILE`] in
//! the state dir, so they survive a restart.
//!
//! `wait` with an event of these types is a short-lived watch
//! ([`Ask::wait`], https://app.arugula.io/issues/iss_1545620a176cab44b21084c27493241e):
//! its message goes to the call that waits ([`Store::fired`]), never to a
//! session's queue, and it's gone when the wait ends ([`Waiting`]). It
//! isn't kept in [`FILE`]: a restart ends the call too.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use arugula_control_wire::forge::{AgentMention, OpAnswer, RoutedStep};
use arugula_control_wire::watch::{
    EXPIRES_DEFAULT_MS, EXPIRES_MAX_MS, PER_SESSION, WatchMade, WatchRow, WatchState, WatchTag,
};
use arugula_proto::{Attention, EventKind, PaneId};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

use arugula_mux::store::PaneLog;

use crate::{mux::Api, server::App};

/// The file in the state dir that keeps them.
const FILE: &str = "local-watches.json";

/// How often a message no session took yet is tried again.
const RETRY: Duration = Duration::from_secs(60);

/// How often an agent's watches are looked at again with nothing said of
/// its pane: an agent that carries on by itself (#606) goes idle when its
/// background work ends, which the mux has no event for.
const SWEEP: Duration = Duration::from_secs(30);

/// How often the output of a pane with an output watch is read: the mux
/// says nothing when a pane prints.
const OUTPUT_POLL: Duration = Duration::from_secs(2);

/// How often a watch's offset in a pane's output is kept, while it moves.
const CHECKPOINT_MS: i64 = 30_000;

/// The longest a line without its end is held back to be read whole, in
/// bytes: a longer one (a progress bar that only returns its carriage) is
/// read as it stands.
const TAIL_MAX: usize = 64 * 1024;

/// The longest a title is quoted in a message, in characters.
const TITLE_MAX: usize = 80;

/// The longest a matching line is quoted in a message, in characters.
const LINE_MAX: usize = 200;

/// The daemon's types in the event catalog, as a watch waits for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum On {
    /// `agent.idle`: its agent finished its turn.
    Idle,
    /// `agent.needs_input`: its agent asks a person something.
    NeedsInput,
    /// `pane.command.ended`.
    CommandEnded,
    /// `pane.exited`.
    Exited,
    /// `pane.output.matched`: a new line of its output matches a pattern.
    Output,
}

impl On {
    /// The type of the catalog's `name`, if it's one of the daemon's.
    pub fn of(name: &str) -> Option<On> {
        Some(match name {
            "agent.idle" => On::Idle,
            "agent.needs_input" => On::NeedsInput,
            "pane.command.ended" => On::CommandEnded,
            "pane.exited" => On::Exited,
            "pane.output.matched" => On::Output,
            _ => return None,
        })
    }

    /// Its name in the catalog.
    pub fn name(self) -> &'static str {
        match self {
            On::Idle => "agent.idle",
            On::NeedsInput => "agent.needs_input",
            On::CommandEnded => "pane.command.ended",
            On::Exited => "pane.exited",
            On::Output => "pane.output.matched",
        }
    }

    /// Whether it's matched on an agent's attention.
    fn attention(self) -> bool {
        matches!(self, On::Idle | On::NeedsInput)
    }
}

/// What a watch asks for, checked by the tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// The Claude Code session it wakes.
    pub session: String,
    pub session_name: Option<String>,
    pub on: On,
    pub pane: PaneId,
    /// Only an exit with this code (`pane.command.ended`, `pane.exited`).
    pub exit: Option<i32>,
    /// A regex (`pane.output.matched`): a line of new output it matches.
    pub pattern: Option<String>,
    pub note: Option<String>,
    pub expires_in_ms: Option<u64>,
    /// A `wait`'s: its message goes to the call, not the session's queue.
    pub wait: bool,
}

/// What was seen of a pane: as it is now, or as an event says. A field that
/// is none wasn't seen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    pub title: Option<String>,
    /// Its attention as `wait` reads it: an agent that carries on is
    /// working.
    pub attention: Option<Attention>,
    /// A command in it ended, with its exit code.
    pub ended: Option<Option<i32>>,
    /// Its process exited, with its code.
    pub exited: Option<Option<i32>>,
    /// It closed, or there's no such pane.
    pub gone: bool,
    /// Where its output stands: the offset a watch on it reads from.
    pub out_at: Option<u64>,
}

/// One watch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Watch {
    id: String,
    session: String,
    #[serde(default)]
    session_name: Option<String>,
    on: On,
    pane: PaneId,
    #[serde(default)]
    exit: Option<i32>,
    /// The regex of a `pane.output.matched`.
    #[serde(default)]
    pattern: Option<String>,
    /// How far into the pane's output it has read.
    #[serde(default)]
    at: Option<u64>,
    #[serde(default)]
    note: Option<String>,
    /// The pane's title, as last seen.
    #[serde(default)]
    title: Option<String>,
    created_ms: i64,
    expires_ms: i64,
    /// Its one message, once it fired or ended: none while it's live.
    #[serde(default)]
    told: Option<Told>,
    /// A `wait`'s ([`Ask::wait`]): not saved, and not sent.
    #[serde(skip)]
    wait: bool,
}

/// A watch's one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Told {
    /// Fired, expired, or ended (its pane closed).
    state: WatchState,
    text: String,
    at_ms: i64,
    /// Its session took it. Until then it's tried again; a dropped one is
    /// tried again too ([`Store::untold`]).
    #[serde(default)]
    delivered: bool,
}

impl Watch {
    fn live(&self) -> bool {
        self.told.is_none()
    }

    /// The pane as a message names it: `pane %4 ("Worker")`.
    fn pane_words(&self) -> String {
        match &self.title {
            Some(t) => format!("pane %{} ({t:?})", self.pane),
            None => format!("pane %{}", self.pane),
        }
    }

    /// What it waits for, as `list` says it.
    fn target(&self) -> String {
        let exit = self.exit.map(|e| format!(" with exit {e}")).unwrap_or_default();
        let exit = match &self.pattern {
            Some(p) => format!(" matching {p:?}"),
            None => exit,
        };
        format!("{} on pane %{}{exit} (on this machine)", self.on.name(), self.pane)
    }

    /// What happened, if `seen` is what it waits for.
    fn met(&self, seen: &Seen) -> Option<String> {
        let code = |c: Option<i32>| c.map_or("no exit code".to_owned(), |c| format!("exit code {c}"));
        let exit_ok = |c: Option<i32>| self.exit.is_none() || self.exit == c;
        match self.on {
            On::Idle => matches!(seen.attention, Some(Attention::Idle | Attention::Done))
                .then(|| "its agent finished its turn and waits for a prompt".into()),
            On::NeedsInput => (seen.attention == Some(Attention::NeedsInput))
                .then(|| "its agent asks a person something: a permission or a question".into()),
            On::CommandEnded => {
                seen.ended.filter(|c| exit_ok(*c)).map(|c| format!("a command in its shell finished, {}", code(c)))
            }
            On::Exited => seen.exited.filter(|c| exit_ok(*c)).map(|c| format!("its process exited, {}", code(c))),
            // Its output is read as it comes ([`Store::scanned`]), never as it stands.
            On::Output => None,
        }
    }

    /// Its message: what happened, then the agent's note apart.
    fn message(&self, head: String) -> String {
        match &self.note {
            Some(n) => format!("{head}\nYour note: {n}"),
            None => head,
        }
    }

    fn tell(&mut self, state: WatchState, head: String, now: i64) {
        let text = self.message(head);
        self.told = Some(Told { state, text, at_ms: now, delivered: false });
    }
}

/// A title as a message quotes it: its first line, trimmed, at most
/// [`TITLE_MAX`] characters.
fn title(t: Option<&str>) -> Option<String> {
    let t = t?.lines().next()?.trim();
    (!t.is_empty()).then(|| t.chars().take(TITLE_MAX).collect())
}

/// When, as people read it: UTC to the minute.
fn when(ms: i64) -> String {
    let t = arugula_e2e::deploy::chant::timestamp(u64::try_from(ms).unwrap_or(0));
    format!("{}Z", t.get(..16).unwrap_or(&t))
}

fn new_id() -> String {
    let b: [u8; 8] = crate::push::random();
    format!("wch_{}", b.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn now() -> i64 {
    i64::try_from(arugula_e2e::now_ms()).unwrap_or(i64::MAX)
}

/// The machine's local watches: in memory, and in [`FILE`] when opened on
/// a state dir.
#[derive(Default)]
pub struct Store {
    file: Option<PathBuf>,
    watches: Mutex<Vec<Watch>>,
    /// A watch was made, or a message is to be sent again: [`keep`] looks.
    changed: tokio::sync::Notify,
    /// A watch fired or ended: a `wait` on one looks ([`Store::fired`]).
    told: tokio::sync::Notify,
    /// When an output watch's offset was last kept, in ms.
    checkpoint: AtomicI64,
}

impl Store {
    /// What `dir` kept, or none: a missing or unreadable file is no
    /// watches, never a failed start.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(FILE);
        let watches = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                warn!(error = %e, "ignoring the saved local watches");
                vec![]
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                warn!(error = %e, "can't read the saved local watches");
                vec![]
            }
        };
        Store { file: Some(path), watches: Mutex::new(watches), ..Store::default() }
    }

    fn save(&self, w: &[Watch]) {
        let Some(path) = &self.file else { return };
        let kept: Vec<&Watch> = w.iter().filter(|w| !w.wait).collect();
        let bytes = serde_json::to_vec_pretty(&kept).unwrap_or_default();
        if let Err(e) = arugula_mux::store::write_atomic(path, &bytes) {
            warn!(error = %e, "can't save the local watches");
        }
    }

    /// Change them, and keep what changed.
    fn with<T>(&self, f: impl FnOnce(&mut Vec<Watch>) -> (T, bool)) -> T {
        let mut w = self.watches.lock().unwrap();
        let (out, changed) = f(&mut w);
        if changed {
            self.save(&w);
        }
        out
    }

    /// Make a watch, its pane `seen` as it is now; or say it has already
    /// happened, and make none.
    pub fn make(&self, ask: Ask, seen: &Seen, now: i64) -> Result<WatchMade, String> {
        if seen.gone {
            return Err(format!("pane {}: no such pane or block on this machine", ask.pane));
        }
        if ask.on == On::Output {
            let p = ask.pattern.as_deref().ok_or("pane.output.matched needs match pattern: a regex")?;
            Regex::new(p).map_err(|e| format!("match pattern: {e}"))?;
        }
        let expires_in = ask.expires_in_ms.unwrap_or(EXPIRES_DEFAULT_MS).min(EXPIRES_MAX_MS);
        let w = Watch {
            id: new_id(),
            session: ask.session,
            session_name: ask.session_name,
            on: ask.on,
            pane: ask.pane,
            exit: ask.exit,
            pattern: ask.pattern,
            at: seen.out_at,
            note: ask.note,
            title: title(seen.title.as_deref()),
            created_ms: now,
            expires_ms: now.saturating_add(i64::try_from(expires_in).unwrap_or(i64::MAX)),
            told: None,
            wait: ask.wait,
        };
        if let Some(what) = w.met(seen) {
            return Ok(WatchMade {
                id: None,
                state: WatchState::AlreadyMet,
                expires_ms: None,
                met: Some(format!("{}: {what}", w.pane_words())),
            });
        }
        self.with(|all| {
            // A wait's is over with its call: it counts for none.
            let live = all.iter().filter(|x| x.live() && !x.wait && x.session == w.session).count();
            if !w.wait && live >= PER_SESSION {
                let err = format!("this session has {live} live watches, the most it may: cancel one first");
                return (Err(err), false);
            }
            let made = WatchMade {
                id: Some(w.id.clone()),
                state: WatchState::Live,
                expires_ms: Some(w.expires_ms),
                met: None,
            };
            all.push(w);
            (Ok(made), true)
        })
        .inspect(|_| self.changed.notify_one())
    }

    /// What was seen of `pane`: the live watches on it that it meets fire,
    /// and those on a pane that's gone end. Whether any did.
    pub fn see(&self, pane: PaneId, seen: &Seen, now: i64) -> bool {
        let any = self.with(|all| {
            let mut any = false;
            for w in all.iter_mut().filter(|w| w.live() && w.pane == pane) {
                if let Some(t) = title(seen.title.as_deref()) {
                    w.title = Some(t);
                }
                if let Some(what) = w.met(seen) {
                    let head = format!("{}: {}: {what}.", w.on.name(), w.pane_words());
                    w.tell(WatchState::Fired, head, now);
                    any = true;
                } else if seen.gone {
                    let head =
                        format!("{}: {}: it closed, so this watch ended without firing.", w.on.name(), w.pane_words());
                    w.tell(WatchState::Ended, head, now);
                    any = true;
                }
            }
            (any, any)
        });
        if any {
            self.told.notify_waiters();
        }
        any
    }

    /// End the live watches whose time ran out, each with its one message,
    /// and forget the told ones past a watch's longest life. Whether any
    /// changed.
    pub fn expire(&self, now: i64) -> bool {
        self.with(|all| {
            let mut any = false;
            for w in all.iter_mut().filter(|w| w.live() && w.expires_ms <= now) {
                let head = format!(
                    "{}: {}: this watch ended without firing; its time ran out at {}.",
                    w.on.name(),
                    w.pane_words(),
                    when(w.expires_ms)
                );
                w.tell(WatchState::Expired, head, now);
                any = true;
            }
            if any {
                self.told.notify_waiters();
            }
            let before = all.len();
            let max = i64::try_from(EXPIRES_MAX_MS).unwrap_or(i64::MAX);
            all.retain(|w| w.told.as_ref().is_none_or(|t| now.saturating_sub(t.at_ms) < max));
            any |= all.len() != before;
            (any, any)
        })
    }

    /// The output watches that are live: the watch, its pane, how far it
    /// has read and its pattern.
    fn outputs(&self) -> Vec<(String, PaneId, u64, String)> {
        let w = self.watches.lock().unwrap();
        w.iter()
            .filter(|w| w.live() && w.on == On::Output)
            .filter_map(|w| Some((w.id.clone(), w.pane, w.at?, w.pattern.clone()?)))
            .collect()
    }

    /// Output watch `id` read its pane's output as far as `at`, and found
    /// `hit`, a line, or not. A watch that found one fires; a pane's
    /// `title` as it is now. Whether it fired.
    fn scanned(&self, id: &str, at: u64, hit: Option<&str>, title_now: Option<&str>, now: i64) -> bool {
        let fired = self.with(|all| {
            let Some(w) = all.iter_mut().find(|w| w.id == id && w.live()) else { return (false, false) };
            if let Some(t) = title(title_now) {
                w.title = Some(t);
            }
            let moved = w.at.is_none_or(|a| at > a);
            w.at = Some(at);
            let Some(line) = hit else {
                // Only now and then kept: read again, a line can't match
                // that didn't.
                let due = moved && now.saturating_sub(self.checkpoint.load(Ordering::Relaxed)) >= CHECKPOINT_MS;
                if due {
                    self.checkpoint.store(now, Ordering::Relaxed);
                }
                return (false, due);
            };
            let mut line: String = line.trim().chars().take(LINE_MAX + 1).collect();
            if line.chars().count() > LINE_MAX {
                line = line.chars().take(LINE_MAX).collect::<String>() + "…";
            }
            let head = format!("{}: {}: {line}", w.on.name(), w.pane_words());
            w.tell(WatchState::Fired, head, now);
            (true, true)
        });
        if fired {
            self.told.notify_waiters();
        }
        fired
    }

    /// When the next live watch expires.
    fn next_expiry(&self) -> Option<i64> {
        self.watches.lock().unwrap().iter().filter(|w| w.live()).map(|w| w.expires_ms).min()
    }

    /// The panes live watches wait on, and for what.
    fn live(&self) -> Vec<(PaneId, On)> {
        let mut out: Vec<(PaneId, On)> =
            self.watches.lock().unwrap().iter().filter(|w| w.live()).map(|w| (w.pane, w.on)).collect();
        out.sort_by_key(|(p, on)| (*p, on.name()));
        out.dedup();
        out
    }

    /// The messages no session took yet: the watch, its session and its
    /// text.
    fn untaken(&self) -> Vec<(String, String, String)> {
        let w = self.watches.lock().unwrap();
        w.iter()
            .filter(|w| !w.wait)
            .filter_map(|w| {
                let t = w.told.as_ref().filter(|t| !t.delivered)?;
                Some((w.id.clone(), w.session.clone(), t.text.clone()))
            })
            .collect()
    }

    /// Watch `id`'s message reached its session.
    fn delivered(&self, id: &str) {
        self.with(|all| {
            let t = all.iter_mut().find(|w| w.id == id).and_then(|w| w.told.as_mut()).filter(|t| !t.delivered);
            let changed = t.map(|t| t.delivered = true).is_some();
            ((), changed)
        });
    }

    /// Watch `id`'s message was dropped before its session took it: it's
    /// sent again.
    pub fn untold(&self, id: &str) {
        let changed = self.with(|all| {
            let t = all.iter_mut().find(|w| w.id == id).and_then(|w| w.told.as_mut()).filter(|t| t.delivered);
            let changed = t.map(|t| t.delivered = false).is_some();
            (changed, changed)
        });
        if changed {
            self.changed.notify_one();
        }
    }

    /// The live watches, `session`'s or every session's, the newest first.
    pub fn list(&self, session: Option<&str>) -> Vec<WatchRow> {
        let w = self.watches.lock().unwrap();
        let mut rows: Vec<WatchRow> = w
            .iter()
            .filter(|w| w.live() && session.is_none_or(|s| w.session == s))
            .map(|w| WatchRow {
                id: w.id.clone(),
                session: w.session.clone(),
                session_name: w.session_name.clone(),
                targets: vec![w.target()],
                state: WatchState::Live,
                created_ms: w.created_ms,
                expires_ms: w.expires_ms,
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.created_ms));
        rows
    }

    /// Whether `id` is one of the local watches, live or not.
    pub fn has(&self, id: &str) -> bool {
        self.watches.lock().unwrap().iter().any(|w| w.id == id)
    }

    /// Cancel `session`'s live watch `id`, or (`id` none) all of them: no
    /// message follows. The ids cancelled.
    pub fn cancel(&self, session: &str, id: Option<&str>) -> Vec<String> {
        let gone = self.with(|all| {
            let mut gone = vec![];
            all.retain(|w| {
                let it = w.live() && w.session == session && id.is_none_or(|i| w.id == i);
                if it {
                    gone.push(w.id.clone());
                }
                !it
            });
            let changed = !gone.is_empty();
            (gone, changed)
        });
        if !gone.is_empty() {
            self.told.notify_waiters();
        }
        gone
    }

    /// A wait's watch `id`'s message, once it fires or ends, and the watch
    /// with it; none if it's gone without one (cancelled).
    pub async fn fired(&self, id: &str) -> Option<String> {
        loop {
            let told = self.told.notified();
            tokio::pin!(told);
            told.as_mut().enable();
            {
                let mut all = self.watches.lock().unwrap();
                let at = all.iter().position(|w| w.id == id)?;
                if all[at].told.is_some() {
                    return all.remove(at).told.map(|t| t.text);
                }
            }
            told.await;
        }
    }

    /// Forget a wait's watch `id`, live or not: its call is over.
    fn forget_wait(&self, id: &str) {
        let mut all = self.watches.lock().unwrap();
        all.retain(|w| !(w.wait && w.id == id));
    }
}

/// A wait's watch, while its call waits: dropped (the wait answered, timed
/// out, or its client went away), the watch is gone.
pub struct Waiting<'a> {
    store: &'a Store,
    pub id: String,
}

impl<'a> Waiting<'a> {
    pub fn new(store: &'a Store, id: String) -> Self {
        Waiting { store, id }
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.store.forget_wait(&self.id);
    }
}

/// Pane `pane` as it is now, for watches `on` it: its title, and its
/// attention or its last command and exit. Gone when it's closed.
pub async fn seen_now(app: &App, pane: PaneId, on: On) -> Seen {
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    let Some(p) = panes.into_iter().find(|p| p.info.id == pane) else {
        return Seen { gone: true, ..Seen::default() };
    };
    let mut seen = Seen { title: p.info.title.clone(), ..Seen::default() };
    if on.attention() {
        match crate::api::attention_of(app, pane).await {
            Ok((a, _)) => seen.attention = Some(a),
            Err(_) => seen.gone = true,
        }
        return seen;
    }
    // A block has no process of its own: nothing ends or exits in it.
    let Some(h) = app.mux.api(|r| Api::Pane(pane, r)).await.flatten() else { return seen };
    let s = h.status();
    match on {
        On::Exited => seen.exited = s.exited,
        On::Output => seen.out_at = Some(s.end),
        // A command that ended since the last input, as `wait` takes it.
        _ => {
            if s.current.is_none()
                && let Some(l) = s.last.filter(|l| l.start >= s.input_at)
            {
                seen.ended = Some(l.exit);
            }
        }
    }
    seen
}

/// Whether `pane` is a terminal or a block a watch `on` it could fire for:
/// an agent's types are for a block or a terminal with an agent's attention,
/// a command's and an exit's for a terminal.
pub async fn fits(app: &App, pane: PaneId, on: On) -> Result<(), String> {
    if on.attention() {
        return Ok(());
    }
    match app.mux.api(|r| Api::Pane(pane, r)).await.flatten() {
        Some(_) => Ok(()),
        None if app.mux.api(|r| Api::Block(pane, r)).await.flatten().is_some() => {
            Err(format!("%{pane} is a block, with no process of its own: {} is for a terminal", on.name()))
        }
        None => Err(format!("pane {pane}: no such pane or block on this machine")),
    }
}

/// Make a watch from a session in one of this machine's panes, against
/// the pane as it is now.
pub async fn make(app: &App, ask: Ask) -> Result<WatchMade, String> {
    fits(app, ask.pane, ask.on).await?;
    let seen = seen_now(app, ask.pane, ask.on).await;
    app.local_watches.make(ask, &seen, now())
}

/// What an event of the mux says of `pane`, for watches `ons` on it.
async fn seen_by(app: &App, pane: PaneId, kind: &EventKind, ons: &[On]) -> Option<Seen> {
    match kind {
        EventKind::Closed => Some(Seen { gone: true, ..Seen::default() }),
        EventKind::Exit { code, .. } => Some(Seen { exited: Some(*code), ..now_titled(app, pane).await }),
        EventKind::CommandEnd { exit, .. } => Some(Seen { ended: Some(*exit), ..now_titled(app, pane).await }),
        EventKind::Attention { .. } | EventKind::Prompted if ons.iter().any(|o| o.attention()) => {
            Some(seen_now(app, pane, On::Idle).await)
        }
        _ => None,
    }
}

/// The pane's title now, and nothing else seen.
async fn now_titled(app: &App, pane: PaneId) -> Seen {
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    Seen { title: panes.into_iter().find(|p| p.info.id == pane).and_then(|p| p.info.title), ..Seen::default() }
}

/// Look at every live watch's pane as it is now.
async fn check_all(app: &App, only_attention: bool) {
    for (pane, on) in app.local_watches.live() {
        if only_attention && !on.attention() {
            continue;
        }
        let seen = seen_now(app, pane, on).await;
        app.local_watches.see(pane, &seen, now());
    }
}

/// Read the lines of the pane's output in `dir` from `from` on: how far it
/// read, and the first line `re` matches, if any. A line without its end
/// yet is matched as it stands but read again, so one printed in pieces is
/// seen whole. None when there's nothing new.
fn scan(dir: PathBuf, from: u64, re: &Regex) -> Option<(u64, Option<String>)> {
    let log = PaneLog::open(dir).ok()?;
    // The log can be behind the pane's end: what's past it is read later.
    if from >= log.end() {
        return None;
    }
    let (start, bytes) = log.read_from(from).ok()?;
    let hit = crate::osc::strip(&bytes).lines().find(|l| re.is_match(l)).map(str::to_owned);
    let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let whole = if bytes.len() - whole > TAIL_MAX || hit.is_some() { bytes.len() } else { whole };
    Some((start + whole as u64, hit))
}

/// Read the new output of every pane an output watch is on.
async fn scan_outputs(app: &App) {
    for (id, pane, at, pattern) in app.local_watches.outputs() {
        let Ok(re) = Regex::new(&pattern) else { continue };
        let dir = app.mux.store.pane_dir(pane);
        let Some((to, hit)) = crate::task::spawn_blocking(move || scan(dir, at, &re)).await.ok().flatten() else {
            continue;
        };
        let titled = if hit.is_some() { now_titled(app, pane).await.title } else { None };
        app.local_watches.scanned(&id, to, hit.as_deref(), titled.as_deref(), now());
    }
}

/// Send the messages no session took yet, each to its session's queue as
/// a cloud watch's goes.
async fn deliver_all(app: &App) {
    for (id, session, text) in app.local_watches.untaken() {
        let m = AgentMention {
            room: String::new(),
            room_name: "watch".into(),
            author: "Arugula".into(),
            text,
            watch: Some(WatchTag { id: id.clone(), session }),
            ..AgentMention::default()
        };
        let answer = crate::agent_mentions::deliver(app, &m).await;
        if let OpAnswer::Done { routed: Some(r), .. } = answer
            && r.step != RoutedStep::Away
        {
            app.local_watches.delivered(&id);
        }
    }
}

/// Keeps the local watches: matches them as the mux says their panes
/// change, ends them as they expire, and sends their messages until each
/// session takes its own.
pub async fn keep(app: Arc<App>) {
    let mut events = app.mux.events();
    // What happened while the daemon was down: the panes as they are now.
    check_all(&app, false).await;
    loop {
        let at = now();
        app.local_watches.expire(at);
        scan_outputs(&app).await;
        deliver_all(&app).await;
        let live = app.local_watches.live();
        let mut wait = Duration::from_secs(3600);
        if !app.local_watches.untaken().is_empty() {
            wait = wait.min(RETRY);
        }
        if live.iter().any(|(_, on)| on.attention()) {
            wait = wait.min(SWEEP);
        }
        if live.iter().any(|(_, on)| *on == On::Output) {
            wait = wait.min(OUTPUT_POLL);
        }
        if let Some(e) = app.local_watches.next_expiry() {
            wait = wait.min(Duration::from_millis(u64::try_from(e.saturating_sub(at)).unwrap_or(0)));
        }
        tokio::select! {
            e = events.recv() => match e {
                Ok(e) => {
                    let Some(pane) = e.pane else { continue };
                    let ons: Vec<On> = live.iter().filter(|(p, _)| *p == pane).map(|(_, on)| *on).collect();
                    if ons.is_empty() {
                        continue;
                    }
                    if let Some(seen) = seen_by(&app, pane, &e.kind, &ons).await {
                        app.local_watches.see(pane, &seen, now());
                    }
                }
                Err(RecvError::Lagged(_)) => check_all(&app, false).await,
                Err(RecvError::Closed) => return,
            },
            _ = app.local_watches.changed.notified() => {}
            _ = tokio::time::sleep(wait) => check_all(&app, true).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;

    fn ask(on: On, pane: PaneId) -> Ask {
        Ask {
            session: "c-mine".into(),
            session_name: Some("mine".into()),
            on,
            pane,
            exit: None,
            pattern: None,
            note: Some("ship it once it's done".into()),
            expires_in_ms: None,
            wait: false,
        }
    }

    fn output(pattern: &str) -> Ask {
        Ask { pattern: Some(pattern.into()), note: None, ..ask(On::Output, 4) }
    }

    /// A pane's log dir with `lines` printed, and where its output stands.
    fn printed(name: &str, lines: &str) -> (PathBuf, u64) {
        let dir = std::env::temp_dir().join(format!("local-watches-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut log = PaneLog::open(dir.clone()).unwrap();
        log.append(lines.as_bytes()).unwrap();
        (dir, log.end())
    }

    fn print(dir: &Path, more: &str) {
        PaneLog::open(dir.to_owned()).unwrap().append(more.as_bytes()).unwrap();
    }

    /// One read of the log for watch `id`, as `scan_outputs` makes it.
    fn read(s: &Store, dir: &Path, now: i64) -> bool {
        let mut fired = false;
        for (id, _, at, pattern) in s.outputs() {
            let re = Regex::new(&pattern).unwrap();
            if let Some((to, hit)) = scan(dir.to_owned(), at, &re) {
                fired |= s.scanned(&id, to, hit.as_deref(), Some("log stream"), now);
            }
        }
        fired
    }

    fn working() -> Seen {
        Seen { title: Some("Worker".into()), attention: Some(Attention::Working), ..Seen::default() }
    }

    fn idle() -> Seen {
        Seen { attention: Some(Attention::Idle), ..Seen::default() }
    }

    fn texts(s: &Store) -> Vec<String> {
        s.untaken().into_iter().map(|(_, _, t)| t).collect()
    }

    #[test]
    fn a_watch_fires_once_with_the_pane_its_title_and_the_note_apart() {
        let s = Store::default();
        let made = s.make(ask(On::Idle, 4), &working(), 0).unwrap();
        assert_eq!(made.state, WatchState::Live);
        assert_eq!(made.expires_ms, Some(24 * HOUR));
        let id = made.id.unwrap();
        assert!(id.starts_with("wch_"), "{id}");
        assert_eq!(s.list(Some("c-mine")).len(), 1);
        // Another pane's news, or this one still working, isn't it.
        assert!(!s.see(5, &idle(), 1));
        assert!(!s.see(4, &working(), 1));
        assert!(s.see(4, &idle(), 2));
        assert_eq!(
            texts(&s),
            ["agent.idle: pane %4 (\"Worker\"): its agent finished its turn and waits for a prompt.\n\
              Your note: ship it once it's done"]
        );
        assert!(s.list(None).is_empty(), "a fired watch isn't live");
        // Once: it doesn't fire again.
        assert!(!s.see(4, &idle(), 3));
        s.delivered(&id);
        assert!(s.untaken().is_empty());
        // Dropped before the session took it: sent again.
        s.untold(&id);
        assert_eq!(s.untaken().len(), 1);
    }

    #[test]
    fn exits_and_commands_match_their_code() {
        let s = Store::default();
        let exited = |c| Seen { exited: Some(Some(c)), ..Seen::default() };
        s.make(Ask { exit: Some(3), note: None, ..ask(On::Exited, 7) }, &Seen::default(), 0).unwrap();
        assert!(!s.see(7, &exited(0), 1), "only exit 3");
        assert!(s.see(7, &exited(3), 2));
        assert_eq!(texts(&s), ["pane.exited: pane %7: its process exited, exit code 3."]);
        s.make(Ask { note: None, ..ask(On::CommandEnded, 8) }, &Seen::default(), 0).unwrap();
        assert!(s.see(8, &Seen { ended: Some(None), title: Some("build\nmore".into()), ..Seen::default() }, 3));
        assert_eq!(
            texts(&s)[1],
            "pane.command.ended: pane %8 (\"build\"): a command in its shell finished, no exit code."
        );
    }

    #[test]
    fn what_already_happened_makes_no_watch() {
        let s = Store::default();
        let made = s.make(ask(On::Idle, 4), &Seen { title: Some("Worker".into()), ..idle() }, 0).unwrap();
        assert_eq!(made.state, WatchState::AlreadyMet);
        assert_eq!(made.id, None);
        assert_eq!(
            made.met.as_deref(),
            Some("pane %4 (\"Worker\"): its agent finished its turn and waits for a prompt")
        );
        let exited = Seen { exited: Some(Some(1)), ..Seen::default() };
        assert_eq!(s.make(ask(On::Exited, 5), &exited, 0).unwrap().state, WatchState::AlreadyMet);
        // An exit with another code than asked isn't it.
        let other = s.make(Ask { exit: Some(0), ..ask(On::Exited, 5) }, &exited, 0).unwrap();
        assert_eq!(other.state, WatchState::Live);
        assert!(
            s.make(ask(On::Idle, 9), &Seen { gone: true, ..Seen::default() }, 0).unwrap_err().contains("no such pane")
        );
        assert_eq!(s.list(None).len(), 1);
    }

    #[test]
    fn a_watch_expires_with_one_last_message_and_is_forgotten_after() {
        let s = Store::default();
        let a = Ask { expires_in_ms: Some(30 * 60_000), ..ask(On::NeedsInput, 4) };
        let id = s.make(a, &working(), 0).unwrap().id.unwrap();
        assert!(!s.expire(29 * 60_000));
        assert!(s.expire(30 * 60_000));
        assert_eq!(
            texts(&s),
            [
                "agent.needs_input: pane %4 (\"Worker\"): this watch ended without firing; its time ran out at 1970-01-01T00:30Z.\n\
              Your note: ship it once it's done"
            ]
        );
        assert!(s.list(None).is_empty());
        assert!(!s.see(4, &Seen { attention: Some(Attention::NeedsInput), ..Seen::default() }, 31 * 60_000));
        // Never more than a week, and remembered for a week after it ends.
        let long = s.make(Ask { expires_in_ms: Some(u64::MAX), ..ask(On::Idle, 5) }, &working(), 0).unwrap();
        assert_eq!(long.expires_ms, Some(EXPIRES_MAX_MS as i64));
        assert!(s.has(&id));
        s.expire(30 * 60_000 + EXPIRES_MAX_MS as i64);
        assert!(!s.has(&id));
    }

    #[test]
    fn a_pane_that_closes_ends_its_watches() {
        let s = Store::default();
        s.make(ask(On::Idle, 4), &working(), 0).unwrap();
        assert!(s.see(4, &Seen { gone: true, ..Seen::default() }, 1));
        assert_eq!(
            texts(&s)[0].lines().next().unwrap(),
            "agent.idle: pane %4 (\"Worker\"): it closed, so this watch ended without firing."
        );
    }

    #[test]
    fn list_and_cancel_are_a_sessions_own_and_a_session_has_a_most() {
        let s = Store::default();
        let mine = s.make(ask(On::Idle, 4), &working(), 0).unwrap().id.unwrap();
        let other = s.make(Ask { session: "c-other".into(), ..ask(On::Idle, 4) }, &working(), 1).unwrap().id.unwrap();
        assert_eq!(s.list(Some("c-mine")).iter().map(|r| &r.id).collect::<Vec<_>>(), [&mine]);
        assert_eq!(s.list(None).iter().map(|r| &r.id).collect::<Vec<_>>(), [&other, &mine], "newest first");
        assert_eq!(s.list(None)[1].targets, ["agent.idle on pane %4 (on this machine)"]);
        assert!(s.cancel("c-mine", Some(&other)).is_empty(), "not another session's");
        assert_eq!(s.cancel("c-mine", Some(&mine)), [mine]);
        for i in 0..PER_SESSION {
            s.make(ask(On::Idle, i as PaneId + 10), &working(), 2).unwrap();
        }
        assert!(s.make(ask(On::Idle, 1), &working(), 3).unwrap_err().contains("cancel one first"));
        assert_eq!(s.cancel("c-mine", None).len(), PER_SESSION);
        assert!(texts(&s).is_empty(), "no message for a cancelled watch");
        assert_eq!(s.list(None).len(), 1);
    }

    /// A wait's watch: its message is the call's, never a session's queue,
    /// it's gone once the call has it or ends, and it isn't kept.
    #[tokio::test]
    async fn a_waits_watch_fires_to_its_call_and_is_gone_after() {
        let s = Store::default();
        let id = s.make(Ask { wait: true, ..ask(On::Idle, 4) }, &working(), 0).unwrap().id.unwrap();
        let fired = s.fired(&id);
        tokio::pin!(fired);
        assert!(futures_util::poll!(fired.as_mut()).is_pending());
        assert!(s.see(4, &idle(), 1));
        assert!(s.untaken().is_empty(), "no session's queue gets it");
        assert_eq!(
            fired.await.as_deref(),
            Some(
                "agent.idle: pane %4 (\"Worker\"): its agent finished its turn and waits for a prompt.\nYour note: ship it once it's done"
            )
        );
        assert!(!s.has(&id));
        // The call ends without it: the watch is gone, and nothing is told.
        let id = s.make(Ask { wait: true, ..ask(On::Idle, 5) }, &working(), 2).unwrap().id.unwrap();
        drop(Waiting::new(&s, id.clone()));
        assert!(!s.has(&id) && s.list(None).is_empty());
        assert!(!s.see(5, &idle(), 3));
        // Cancelled from elsewhere, its call hears it.
        let id = s.make(Ask { wait: true, ..ask(On::Idle, 6) }, &working(), 4).unwrap().id.unwrap();
        s.cancel("c-mine", None);
        assert_eq!(s.fired(&id).await, None);
    }

    #[test]
    fn a_waits_watch_isnt_kept_or_counted() {
        let dir = std::env::temp_dir().join(format!("local-watches-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir);
        for i in 0..PER_SESSION {
            s.make(ask(On::Idle, i as PaneId + 10), &working(), 0).unwrap();
        }
        let id = s.make(Ask { wait: true, ..ask(On::Idle, 4) }, &working(), 0).unwrap().id.unwrap();
        assert!(s.has(&id));
        assert!(!Store::open(&dir).has(&id), "not kept: a restart ends its call");
        assert_eq!(Store::open(&dir).list(None).len(), PER_SESSION);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watches_and_their_messages_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("local-watches-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir);
        let live = s.make(ask(On::Idle, 4), &working(), 0).unwrap().id.unwrap();
        s.make(ask(On::Exited, 5), &Seen::default(), 0).unwrap();
        s.see(5, &Seen { exited: Some(Some(0)), ..Seen::default() }, 1);
        drop(s);
        let s = Store::open(&dir);
        assert_eq!(s.list(None).iter().map(|r| &r.id).collect::<Vec<_>>(), [&live]);
        assert_eq!(
            texts(&s),
            ["pane.exited: pane %5: its process exited, exit code 0.\nYour note: ship it once it's done"]
        );
        assert!(s.see(4, &idle(), 2));
        assert_eq!(Store::open(&dir).untaken().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_output_watch_reads_new_lines_only_and_fires_once() {
        let (dir, end) = printed("new", "predicate: ERROR boom\nold line\n");
        let s = Store::default();
        let seen = Seen { out_at: Some(end), title: Some("log stream".into()), ..Seen::default() };
        let made = s.make(output("ERROR"), &seen, 0).unwrap();
        assert_eq!(made.state, WatchState::Live, "old output isn't already met");
        assert!(!read(&s, &dir, 1), "a line printed before the watch never fires it");
        print(&dir, "\x1b[31mfine\x1b[0m\nall quiet\n");
        assert!(!read(&s, &dir, 2));
        print(&dir, "an \x1b[1mERROR\x1b[0m: it broke\nERROR again\n");
        assert!(read(&s, &dir, 3));
        assert_eq!(texts(&s), ["pane.output.matched: pane %4 (\"log stream\"): an ERROR: it broke"]);
        assert!(s.list(None).is_empty());
        print(&dir, "ERROR a third\n");
        assert!(!read(&s, &dir, 4), "once");
        assert_eq!(texts(&s).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_output_watch_sees_a_line_printed_in_pieces_and_quotes_it_short() {
        let (dir, end) = printed("pieces", "");
        let s = Store::default();
        s.make(output("^done: 12$"), &Seen { out_at: Some(end), ..Seen::default() }, 0).unwrap();
        print(&dir, "done: 1");
        assert!(!read(&s, &dir, 1), "the line so far isn't it");
        print(&dir, "2\n");
        assert!(read(&s, &dir, 2));
        assert_eq!(texts(&s), ["pane.output.matched: pane %4 (\"log stream\"): done: 12"]);
        let s = Store::default();
        s.make(output("x"), &Seen { out_at: Some(end), ..Seen::default() }, 0).unwrap();
        print(&dir, &format!("  {}  \n", "x".repeat(300)));
        assert!(read(&s, &dir, 3));
        let t = &texts(&s)[0];
        assert_eq!(t.chars().filter(|c| *c == 'x').count(), 200, "{t}");
        assert!(t.ends_with('…'), "{t}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_regex_is_refused_at_create() {
        let s = Store::default();
        let seen = Seen { out_at: Some(0), ..Seen::default() };
        let e = s.make(output("(unclosed"), &seen, 0).unwrap_err();
        assert!(e.starts_with("match pattern: regex parse error"), "{e}");
        assert!(s.make(Ask { pattern: None, ..output("") }, &seen, 0).unwrap_err().contains("needs match pattern"));
        assert!(s.list(None).is_empty());
    }

    #[test]
    fn an_output_watch_resumes_from_where_it_read_after_a_restart() {
        let state = std::env::temp_dir().join(format!("local-watches-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        std::fs::create_dir_all(&state).unwrap();
        let (dir, end) = printed("restart", "ERROR before\n");
        let s = Store::open(&state);
        s.make(output("ERROR"), &Seen { out_at: Some(end), ..Seen::default() }, 0).unwrap();
        print(&dir, "nothing yet\n");
        assert!(!read(&s, &dir, 1));
        drop(s);
        // Down for a while: the daemon's pane printed, and its watch is kept.
        print(&dir, "ERROR while down\n");
        let s = Store::open(&state);
        assert_eq!(s.list(None).len(), 1);
        assert!(read(&s, &dir, 2), "output since the watch was made counts, kept offset or not");
        assert_eq!(texts(&s), ["pane.output.matched: pane %4 (\"log stream\"): ERROR while down"]);
        assert_eq!(Store::open(&state).untaken().len(), 1, "its message is kept");
        let _ = std::fs::remove_dir_all(&state);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
