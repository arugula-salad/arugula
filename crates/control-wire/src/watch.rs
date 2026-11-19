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
//! A watch is keyed to the Claude Code session that made it, never to a
//! pane or a name: a new conversation in the pane doesn't inherit it.
//! Every route here is signed with the machine's key, and control acts as
//! its owner.

use serde::{Deserialize, Serialize};

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
    /// With `event`: why the agent watches, handed back when it fires; at
    /// most [`NOTE_MAX`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
