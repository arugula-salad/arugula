//! Watches (Arugula #631): an agent asks control to wake its session when
//! something happens (a PR merges or closes), then ends its turn. Control
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
}

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
