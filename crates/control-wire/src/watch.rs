//! Watches (Arugula #631): an agent asks control to wake its session when
//! something happens (a PR merges or closes, its checks finish or fail, or
//! it's ready to merge; a hosted session settles; a thread gets a reply; a
//! run or deploy ends), then ends its turn. Control
//! evaluates the watch where the facts are, and a firing reaches the session
//! as an `agents.mention` that carries [`WatchTag`], held, sent again and
//! acked like a teammate's mention. A watch fires once. One that expires
//! (24 hours by default, at most 7 days) or is cancelled from elsewhere
//! sends one last message saying it ended without firing.
//!
//! A standing watch (Arugula #1280) is an event watch that stays live until
//! it expires or is cancelled: it fires at most once per gap
//! ([`WatchCreate::every_ms`], [`EVERY_DEFAULT_MS`] when none), each time
//! carrying every event that matched since it last fired. With
//! [`WatchCreate::exclude_self`] it skips what the watching session caused
//! itself: its own agent block asking or going idle, its own replies.
//!
//! A watch is keyed to the Claude Code session that made it, never to a
//! pane or a name: a new conversation in the pane doesn't inherit it.
//! Every route here is signed with the machine's key, and control acts as
//! its owner.

use serde::{Deserialize, Serialize};

use crate::events::{self, Home};

/// `POST`: make a watch, [`WatchCreate`], answered [`WatchMade`]. `GET`
/// with [`WatchListQuery`]: the machine's live watches, [`WatchList`].
pub const AGENT_WATCHES: &str = "/api/daemon/watches";
/// `POST`: cancel watches, [`WatchCancel`], answered [`WatchCancelled`].
pub const AGENT_WATCHES_CANCEL: &str = "/api/daemon/watches/cancel";

/// How long a watch lives when it isn't told: 24 hours.
pub const EXPIRES_DEFAULT_MS: u64 = 24 * 60 * 60 * 1000;
/// The longest a watch may live: 7 days.
pub const EXPIRES_MAX_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// The most live watches one session may have.
pub const PER_SESSION: usize = 20;
/// A standing watch's gap when it isn't told: 5 minutes.
pub const EVERY_DEFAULT_MS: u64 = 5 * 60 * 1000;
/// The shortest gap a standing watch may have: a minute.
pub const EVERY_MIN_MS: u64 = 60 * 1000;
/// The longest gap a standing watch may have: a day.
pub const EVERY_MAX_MS: u64 = 24 * 60 * 60 * 1000;

/// What a watch waits for: one of these happening fires it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchTarget {
    /// A pull request on github.com, in a repo linked through Arugula's
    /// GitHub App.
    Pr {
        /// `OWNER/NAME`.
        repo: String,
        number: u64,
        until: PrUntil,
    },
    /// A hosted agent session (`ses_…`, Arugula #1034), until it settles as
    /// `wait` with `agent_session` says: done with nothing queued, waiting
    /// for input, failed or stopped.
    Session { id: String },
    /// A reply in a chat thread: a message in `room` under `parent` (its
    /// root's number), or any message in the room without one, that the
    /// session that made the watch didn't write. With `from`, only one by
    /// that person (their login or name, any case).
    Thread {
        room: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// A run (`run_…`) or a deploy (`dep_…`), until it ends however it ends.
    Run { id: String },
}

/// What a watch on a pull request waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PrUntil {
    /// It merged.
    Merged,
    /// It closed, merged or not.
    Closed,
    /// No check on its head commit is queued or running (Arugula #1033).
    ChecksDone,
    /// A check on its head commit failed: once, however often it flips.
    ChecksFailed,
    /// Its checks passed and GitHub says it can merge cleanly.
    Ready,
}

/// `POST /api/daemon/watches`: a watch for one Claude Code session on the
/// machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WatchCreate {
    /// The Claude Code session to wake, by its id.
    pub session: String,
    /// The session's name as people read it, when it has one: for lists,
    /// never for routing.
    #[serde(default)]
    pub session_name: Option<String>,
    /// Any one of these fires it: at least one, at most
    /// [`TARGETS_MAX`].
    pub targets: Vec<WatchTarget>,
    /// How long it lives, in milliseconds. None: [`EXPIRES_DEFAULT_MS`];
    /// at most [`EXPIRES_MAX_MS`].
    #[serde(default)]
    pub expires_in_ms: Option<u64>,
    /// An event target too (the event bus, Arugula #1230): a type in
    /// [`crate::events::CATALOG`] that the cloud serves. Fires it like any
    /// target. Left out when none, so a watch without one reads as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// With `event`: the type's match fields an event must equal, each one
    /// of its [`crate::events::EventType::matches`].
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub matches: Option<serde_json::Map<String, serde_json::Value>>,
    /// Why the agent watches, with any target: handed back when it fires or
    /// expires; at most [`NOTE_MAX`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Standing (Arugula #1280): an event watch, with no other target, that
    /// stays live after it fires, until it expires or is cancelled. Left out
    /// when false, so a watch that isn't reads as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub standing: bool,
    /// With `standing`: the least time between two of its fires, in
    /// milliseconds. None: [`EVERY_DEFAULT_MS`]; from [`EVERY_MIN_MS`] to
    /// [`EVERY_MAX_MS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_ms: Option<u64>,
    /// Skip events the watching session caused itself: its own agent block
    /// (`pane`) asking or going idle. Its own chat replies never fire a
    /// watch anyway.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_self: bool,
    /// The pane the watching session runs in, as the machine names its
    /// blocks: what `exclude_self` skips. Sent only with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
}

/// The longest [`WatchCreate::note`], in characters.
pub const NOTE_MAX: usize = 200;

/// The most targets one watch may have.
pub const TARGETS_MAX: usize = 10;

/// Where a watch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    /// Waiting.
    Live,
    /// What it waits for had already happened when it was asked: no watch
    /// was made.
    AlreadyMet,
    /// It happened, and the session was told.
    Fired,
    /// Its time ran out.
    Expired,
    Cancelled,
    /// It can't fire any more: a target can't be read now, or the session
    /// is gone.
    Ended,
    /// A state this side doesn't know yet.
    #[serde(other)]
    Other,
}

/// What [`WatchCreate`] made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WatchMade {
    /// `wch_…`; none when it was [`WatchState::AlreadyMet`].
    #[serde(default)]
    pub id: Option<String>,
    pub state: WatchState,
    /// When it expires, in ms since the epoch.
    #[serde(default)]
    pub expires_ms: Option<i64>,
    /// What happened already, for [`WatchState::AlreadyMet`]: ids and
    /// states only ("arugula-salad/arugula#2095 merged by aaron at …").
    #[serde(default)]
    pub met: Option<String>,
}

/// `GET /api/daemon/watches`: which of the machine's live watches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WatchListQuery {
    /// Only this Claude Code session's. None: every session's on the
    /// machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// The machine's live watches, the newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WatchList {
    pub watches: Vec<WatchRow>,
}

/// One watch, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WatchRow {
    pub id: String,
    /// The Claude Code session it wakes.
    pub session: String,
    #[serde(default)]
    pub session_name: Option<String>,
    /// Each target in words ("arugula-salad/arugula#2095 until merged"), so
    /// a reader that doesn't know a newer kind still shows it.
    pub targets: Vec<String>,
    pub state: WatchState,
    pub created_ms: i64,
    pub expires_ms: i64,
    /// A standing watch's gap (Arugula #1280), in milliseconds; none for one
    /// that fires once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_ms: Option<i64>,
}

/// `POST /api/daemon/watches/cancel`: one of a session's watches, or all of
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WatchCancel {
    /// The session whose watches these are: a session cancels only its own.
    pub session: String,
    /// The watch (`wch_…`). None: with `all`, every one of the session's.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub all: bool,
}

/// What [`WatchCancel`] cancelled, by id. The session that asked is told
/// by this answer, and gets no message for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WatchCancelled {
    pub cancelled: Vec<String>,
}

/// On an `agents.mention` that is a watch's message (fired, or ended
/// without firing): which watch, and the one Claude Code session it is for.
/// The daemon gives it to that session alone, framed as Arugula's word and
/// not a teammate's, past the owner's switch for teammates' mentions (the
/// owner's own agent asked for it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WatchTag {
    /// `wch_…`.
    pub id: String,
    /// The Claude Code session's id.
    pub session: String,
}

/// `GET`: the live watches on a mirrored PR (`fi_…`, Arugula #1036), for a
/// person in the project of its repo who can read it: [`WatchesOn`].
pub const ITEM_WATCHES: &str = "/api/forge/items/{id}/watches";
/// `GET`: the live watches on a reply in a thread (its root's number `seq`),
/// or in its whole room, for a person who can read the room: [`WatchesOn`].
pub const THREAD_WATCHES: &str = "/api/chat/rooms/{id}/threads/{seq}/watches";
/// `POST`: the person who owns a watch cancels it (its session is told),
/// answered [`WatchCancelled`].
pub const WATCH_CANCEL: &str = "/api/watches/{id}/cancel";

/// [`ITEM_WATCHES`]' path for one PR.
pub fn item_watches(id: &str) -> String {
    ITEM_WATCHES.replace("{id}", id)
}

/// [`THREAD_WATCHES`]' path for one thread.
pub fn thread_watches(room: &str, seq: i64) -> String {
    THREAD_WATCHES.replace("{id}", room).replace("{seq}", &seq.to_string())
}

/// [`WATCH_CANCEL`]'s path for one watch.
pub fn watch_cancel(id: &str) -> String {
    WATCH_CANCEL.replace("{id}", id)
}

/// The live watches on a PR or a thread, the soonest to expire first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WatchesOn {
    pub watches: Vec<WatchShown>,
}

/// One watch as a teammate sees it (Arugula #1036): whose agent waits, on
/// what, and until when. The session's name only as its owner's
/// `agent_sessions` setting shows it, asked of its machine (aru-133).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WatchShown {
    /// `wch_…`.
    pub id: String,
    /// The account whose agent made it, and its name as people read it.
    pub owner: String,
    pub owner_name: String,
    /// The session's name, when its owner shows it; else none, and it's
    /// "their agent".
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub session: Option<String>,
    /// What it waits for here, in words: "merged", "checks_failed", "a
    /// reply from Ada".
    pub waits: String,
    /// When it ends without firing, in ms since the epoch.
    pub expires_ms: i64,
    /// Whether it's the reader's own, which they may cancel.
    #[serde(default)]
    pub yours: bool,
}

/// `until`'s word for a PR, as the `watch` tools take it: `merged` when
/// none.
pub fn until_word_parse(word: Option<&str>) -> Result<PrUntil, String> {
    match word.map(str::trim).unwrap_or("merged") {
        "" | "merged" => Ok(PrUntil::Merged),
        "closed" => Ok(PrUntil::Closed),
        "checks_done" => Ok(PrUntil::ChecksDone),
        "checks_failed" => Ok(PrUntil::ChecksFailed),
        "ready" => Ok(PrUntil::Ready),
        w => Err(format!("until {w}: a watch on a PR waits until merged, closed, checks_done, checks_failed or ready")),
    }
}

/// A watch's event target as the `watch` tools check it, for
/// [`WatchCreate`]'s `event`, `matches` and `note`: each none when it isn't
/// given, and match none without an event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventTarget {
    pub event: Option<String>,
    pub matches: Option<serde_json::Map<String, serde_json::Value>>,
    pub note: Option<String>,
}

/// Which `watch` tool checks an event target: a machine's, whose daemon
/// holds watches on its own types (local watches), or a hosted session's,
/// which has no panes of its own to watch that way. Not on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watcher {
    Machine,
    HostedSession,
}

/// An event target, checked before it's sent: the type is in the catalog,
/// and a daemon's own only for [`Watcher::Machine`], each match key is one
/// of its match fields. The note, which goes with any target, is at most
/// [`NOTE_MAX`] characters. Match needs an event.
pub fn event_target(
    watcher: Watcher,
    event: Option<String>,
    matches: Option<serde_json::Map<String, serde_json::Value>>,
    note: Option<String>,
) -> Result<EventTarget, String> {
    let matches = matches.filter(|m| !m.is_empty());
    let note = note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
    if let Some(n) = &note
        && n.chars().count() > NOTE_MAX
    {
        return Err(format!("note: {} characters, at most {NOTE_MAX}", n.chars().count()));
    }
    let Some(name) = event.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) else {
        if matches.is_some() {
            return Err("match goes with an event".into());
        }
        return Ok(EventTarget { event: None, matches: None, note });
    };
    let Some(t) = events::lookup(&name) else {
        return Err(format!("event {name}: no such type; watch types lists them"));
    };
    if t.home == Home::Daemon && watcher == Watcher::HostedSession {
        return Err(format!(
            "event {name}: a machine's own event, watched by an agent on that machine; a hosted session watches the cloud's types"
        ));
    }
    for k in matches.iter().flat_map(|m| m.keys()) {
        if !t.matches.iter().any(|f| f.name == k) {
            let fields: Vec<&str> = t.matches.iter().map(|f| f.name).collect();
            return Err(format!("match {k}: {name} matches on {}", fields.join(", ")));
        }
    }
    Ok(EventTarget { event: Some(name), matches, note })
}

/// `watch types`' answer: the catalog, a type to two lines, and whether
/// `watcher` can watch each now (`joined`: its machine has joined Arugula
/// cloud).
pub fn event_types(watcher: Watcher, joined: bool) -> String {
    let lines: Vec<String> = events::CATALOG
        .iter()
        .map(|t| {
            let now = match t.home {
                Home::Cloud if joined => "watchable now",
                Home::Cloud => "not now: this machine hasn't joined Arugula cloud",
                Home::Daemon if watcher == Watcher::Machine => "watchable now, by this machine",
                Home::Daemon => "not here: a machine's own, watched by an agent on that machine",
            };
            let fields: Vec<String> = t.matches.iter().map(|f| format!("{} ({})", f.name, f.meaning)).collect();
            format!("{}: {} [{now}]\n  match: {}", t.name, t.meaning, fields.join("; "))
        })
        .collect();
    lines.join("\n")
}

/// `expires`' words, in ms: `30m`, `6h`, `3d`, at most [`EXPIRES_MAX_MS`].
pub fn expires_ms(word: &str) -> Result<u64, String> {
    let w = word.trim();
    let (n, unit) = w.split_at(w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len()));
    let n: u64 = n.parse().map_err(|_| format!("expires {w}: a number and m, h or d, like 6h"))?;
    let ms = match unit.trim() {
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        _ => return Err(format!("expires {w}: a number and m, h or d, like 6h")),
    };
    match ms {
        0 => Err("expires: more than nothing".into()),
        ms if ms > EXPIRES_MAX_MS => Err(format!("expires {w}: at most 7d")),
        ms => Ok(ms),
    }
}

/// `every`'s words, in ms: `1m`, `5m`, `2h`, from [`EVERY_MIN_MS`] to
/// [`EVERY_MAX_MS`].
pub fn every_ms(word: &str) -> Result<u64, String> {
    let w = word.trim();
    let (n, unit) = w.split_at(w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len()));
    let n: u64 = n.parse().map_err(|_| format!("every {w}: a number and m or h, like 5m"))?;
    let ms = match unit.trim() {
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        _ => return Err(format!("every {w}: a number and m or h, like 5m")),
    };
    match ms {
        ms if ms < EVERY_MIN_MS => Err(format!("every {w}: at least 1m")),
        ms if ms > EVERY_MAX_MS => Err(format!("every {w}: at most 24h")),
        ms => Ok(ms),
    }
}

/// A gap as people read it: `5m`, `2h`, `90m`.
pub fn every_word(ms: u64) -> String {
    match ms {
        ms if ms % 3_600_000 == 0 => format!("{}h", ms / 3_600_000),
        ms => format!("{}m", ms.div_ceil(60_000)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemons_own_event_is_a_machines_to_watch() {
        let pane = || Some(serde_json::json!({ "pane": "4" }).as_object().unwrap().clone());
        let t = event_target(Watcher::Machine, Some("pane.exited".into()), pane(), None).unwrap();
        assert_eq!(t.event.as_deref(), Some("pane.exited"));
        let e = event_target(Watcher::HostedSession, Some("pane.exited".into()), pane(), None).unwrap_err();
        assert!(e.contains("a machine's own event"), "{e}");
        let e = event_target(Watcher::Machine, Some("pane.exited".into()), Some(Default::default()), None);
        assert!(e.is_ok());
        let bad = serde_json::json!({ "issue": "#1" }).as_object().unwrap().clone();
        let e = event_target(Watcher::Machine, Some("pane.exited".into()), Some(bad), None).unwrap_err();
        assert!(e.contains("matches on pane"), "{e}");
    }

    #[test]
    fn a_note_goes_with_any_target_and_a_match_with_an_event() {
        let t = event_target(Watcher::Machine, None, None, Some("  squash-merge  ".into())).unwrap();
        assert_eq!((t.event, t.matches, t.note.as_deref()), (None, None, Some("squash-merge")));
        let long = Some("x".repeat(NOTE_MAX + 1));
        let e = event_target(Watcher::Machine, None, None, long).unwrap_err();
        assert!(e.contains("at most 200"), "{e}");
        let m = serde_json::json!({ "issue": "#1" }).as_object().unwrap().clone();
        let e = event_target(Watcher::Machine, None, Some(m), Some("why".into())).unwrap_err();
        assert_eq!(e, "match goes with an event");
    }

    #[test]
    fn every_is_a_minute_to_a_day() {
        assert_eq!(every_ms("5m"), Ok(5 * 60_000));
        assert_eq!(every_ms(" 2h "), Ok(2 * 3_600_000));
        assert!(every_ms("30s").unwrap_err().contains("like 5m"));
        assert!(every_ms("0m").unwrap_err().contains("at least 1m"));
        assert!(every_ms("25h").unwrap_err().contains("at most 24h"));
        assert_eq!((every_word(EVERY_DEFAULT_MS), every_word(3_600_000), every_word(90 * 60_000)), ("5m".into(), "1h".into(), "90m".into()));
    }

    #[test]
    fn a_watch_that_isnt_standing_reads_as_before() {
        let w = WatchCreate { session: "c-1".into(), ..Default::default() };
        let v = serde_json::to_value(&w).unwrap();
        for k in ["standing", "every_ms", "exclude_self", "pane"] {
            assert!(v.get(k).is_none(), "{k}: {v}");
        }
        let back: WatchCreate = serde_json::from_value(serde_json::json!({
            "session": "c-1", "targets": [], "standing": true, "every_ms": 60_000, "exclude_self": true, "pane": "4"
        }))
        .unwrap();
        assert!(back.standing && back.exclude_self);
        assert_eq!((back.every_ms, back.pane.as_deref()), (Some(60_000), Some("4")));
    }

    #[test]
    fn types_say_who_can_watch_a_daemons_own() {
        let machine = event_types(Watcher::Machine, false);
        assert!(machine.contains("agent.idle: "), "{machine}");
        assert!(machine.contains("[watchable now, by this machine]"));
        assert!(machine.contains("[not now: this machine hasn't joined Arugula cloud]"));
        let hosted = event_types(Watcher::HostedSession, true);
        assert!(hosted.contains("[not here: a machine's own, watched by an agent on that machine]"));
        assert!(!hosted.contains("by this machine"));
    }
}
