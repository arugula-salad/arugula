//! Chat held by control (M122, S41): a team's channels and DMs, and the
//! threads about a pane or a session, in plain text. Control numbers the
//! messages, keeps read marks, resolves mentions, pushes and searches;
//! pages read it all over `/api/chat/…` with the session cookie, and hear of
//! new messages on one Server-Sent Events stream ([`ChatLive`]).
//!
//! Daemons hold the panes, never the talk about them. What they tell
//! control is who may read each pane's or session's thread
//! ([`CHAT_READERS`], decision 2 of S41).
//!
//! These types are new with M122, so nothing older reads them; the web
//! client's copy is generated (`just proto-ts`, the `ts` feature).

use serde::{Deserialize, Serialize};

/// A daemon says who may read its panes' and sessions' threads (`POST`),
/// [`ChatReaders`], answered [`ChatReadersStored`].
pub const CHAT_READERS: &str = "/api/daemon/chat/readers";

/// The most text a message carries, and the most of a pane it quotes.
pub const MAX_TEXT: usize = 8 * 1024;
pub const MAX_QUOTE: usize = 16 * 1024;
/// How much of a message a push shows (decision 5).
pub const PUSH_CHARS: usize = 200;

/// What a room is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    /// A team's, by name; its people are the team's roster.
    Channel,
    /// Two teammates.
    Dm,
    /// The thread about a pane, on its machine.
    Pane,
    /// The thread about a session, on its machine.
    Session,
    /// An old conversation brought in from a machine, read-only (M124).
    Archive,
}

/// What a daemon's thread is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatTargetKind {
    Pane,
    Session,
}

impl ChatTargetKind {
    pub fn kind(self) -> ChatKind {
        match self {
            ChatTargetKind::Pane => ChatKind::Pane,
            ChatTargetKind::Session => ChatKind::Session,
        }
    }
}

/// Someone, as the reader sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPerson {
    pub account: String,
    pub name: String,
}

/// A room as one person sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRoom {
    pub id: String,
    pub kind: ChatKind,
    /// The team a channel or DM is in.
    pub team: Option<String>,
    /// A channel's name (without `#`); a pane's or session's room is
    /// named `pane 2`, `session 1`.
    pub name: String,
    /// The machine a pane's or session's room is on, and its name.
    pub machine: Option<String>,
    pub machine_name: Option<String>,
    /// The pane or session id there.
    pub target: Option<String>,
    /// A DM's other person.
    pub with: Option<ChatPerson>,
    /// The newest message's number, and the reader's read mark.
    pub last: i64,
    pub read: i64,
    /// Messages after the read mark (not the reader's own), and how many
    /// of those mention the reader.
    pub unread: u64,
    pub mentions: u64,
    /// The reader turned its pushes off.
    pub muted: bool,
    /// A channel its team's owners archived: read, not written.
    pub archived: bool,
    /// Whether the reader may post here now (a DM whose other person left
    /// the team, an archive, an archived channel: no).
    pub writable: bool,
}

/// `GET /api/chat`: every room the reader sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRooms {
    pub rooms: Vec<ChatRoom>,
}

/// `GET /api/chat/rooms/{id}`: a room and who sees it (whom `@` reaches).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRoomDetail {
    pub room: ChatRoom,
    pub people: Vec<ChatPerson>,
}

/// A pane a message names, as the poster sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPaneRef {
    pub machine: String,
    pub pane: String,
}

/// A pane a message names, as the reader sees it: whether its chip opens
/// for them (they may reach the machine), and the machine's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPane {
    pub machine: String,
    pub machine_name: String,
    pub pane: String,
    pub opens: bool,
}

/// A message as the reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatMessage {
    /// Its number in the room. Messages brought in from a machine before
    /// the room had any of its own are numbered 0 and below (M124).
    pub seq: i64,
    pub at: u64,
    /// The author's account, and their name now. Both are empty for a
    /// deleted account.
    pub author: String,
    pub name: String,
    pub text: String,
    /// Lines the poster chose from a pane, copied in (decision 1).
    pub quote: Option<String>,
    pub pane: Option<ChatPane>,
    /// The accounts it mentions.
    pub mentions: Vec<String>,
    pub edited: Option<u64>,
    /// When it was deleted: its text, quote and pane are gone.
    pub deleted: Option<u64>,
    /// Brought in from a machine's own threads (M124): the name it had
    /// there.
    pub imported: Option<String>,
}

/// `GET /api/chat/rooms/{id}/messages?after=N` (oldest first after N),
/// `?before=N` (the page before N), or neither (the newest page);
/// `limit` is at most 200.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessagesQuery {
    #[serde(default)]
    pub after: Option<i64>,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatMessages {
    pub room: ChatRoom,
    /// Oldest first.
    pub messages: Vec<ChatMessage>,
    /// There are more in the direction asked.
    pub more: bool,
}

/// `POST /api/chat/teams/{team}/channels`: owners and editors make them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatNewChannel {
    pub name: String,
}

/// `POST /api/chat/teams/{team}/dms`: the DM with a teammate, made the
/// first time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatNewDm {
    pub with: String,
}

/// `POST /api/chat/targets`: the thread about a pane or a session, made
/// the first time by someone who may read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatOpenTarget {
    pub machine: String,
    pub kind: ChatTargetKind,
    pub target: String,
}

/// `PATCH /api/chat/rooms/{id}`: a channel renamed or archived, by its
/// team's owners.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRoomChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub archived: Option<bool>,
}

/// `POST /api/chat/rooms/{id}/messages`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPost {
    pub text: String,
    /// A pane the message is about: only one on a machine the poster may
    /// reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pane: Option<ChatPaneRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub quote: Option<String>,
}

/// Why control pushed someone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatPushWhy {
    Mention,
    Dm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPushed {
    pub account: String,
    pub why: ChatPushWhy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPosted {
    pub seq: i64,
    /// The accounts it mentions.
    pub mentions: Vec<String>,
    /// `@words` that named nobody who sees the room.
    pub unreached: Vec<String>,
    /// Whom control pushed (not those with the room open, muted, or pushed
    /// for it in the last minute).
    pub pushed: Vec<ChatPushed>,
}

/// `PATCH /api/chat/rooms/{id}/messages/{seq}`: the author's new text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatEdit {
    pub text: String,
}

/// `POST /api/chat/rooms/{id}/read`: read up to here (it only moves
/// forward).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRead {
    pub seq: i64,
}

/// `POST /api/chat/rooms/{id}/mute`: no pushes from this room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatMute {
    pub muted: bool,
}

/// `GET /api/chat/search?q=&team=`: words (each a phrase, `mea*` a
/// prefix) and filters (`in:#channel`, `from:@login`, `has:quote`), over
/// the rooms the reader sees, or one team's; newest first, at most 50.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub team: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatHit {
    pub room: String,
    pub message: ChatMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatFound {
    pub found: Vec<ChatHit>,
}

/// `POST /api/chat/live/open`: the room a page's stream has open (`None`:
/// none, or the page is hidden). Nobody is pushed for a room they have
/// open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatOpen {
    pub stream: String,
    pub room: Option<String>,
}

/// What `GET /api/chat/live` sends, one JSON object per event (no event
/// names). Every event but `hello` has an id, so `EventSource` resumes
/// with `Last-Event-ID` after a drop; a stream that can't be resumed (it
/// fell too far behind, or control restarted) gets `resync`: reload the
/// rooms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChatLive {
    /// First, once per connection: this stream's id, for [`ChatOpen`].
    Hello {
        stream: String,
    },
    /// A new message: fetch it (`after`).
    Message {
        room: String,
        seq: i64,
    },
    /// A message was edited or deleted: fetch it again.
    Changed {
        room: String,
        seq: i64,
    },
    /// A room appeared, changed or went: reload the rooms.
    Room {
        room: String,
    },
    /// The reader's own read mark moved (another of their pages).
    Read {
        room: String,
        seq: i64,
    },
    Resync {},
}

/// What a daemon says of a thread's readers: for each pane or session it
/// names, the whole list, which replaces the last one control had. Those it
/// doesn't name keep theirs (a pane that closed keeps its readers).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReaders {
    #[serde(default)]
    pub targets: Vec<ChatReaderList>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReaderList {
    pub kind: ChatTargetKind,
    pub target: String,
    #[serde(default)]
    pub readers: Vec<ChatReader>,
}

/// One account that may read a thread, and from when (ms; its "from now"
/// floor, if it has one): it doesn't see messages from before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReader {
    pub account: String,
    #[serde(default)]
    pub from: Option<u64>,
}

/// How many lists control kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReadersStored {
    pub stored: usize,
}

/// The most lists in one [`ChatReaders`], and readers in one list.
pub const MAX_READER_TARGETS: usize = 500;
pub const MAX_READERS: usize = 500;
