//! Chat held by control (M122, S41): a team's channels and DMs, and the
//! threads about a pane or a session, in plain text. Control numbers the
//! messages, keeps read marks, resolves mentions, pushes and searches;
//! pages read it all over `/api/chat/…` with the session cookie, and hear of
//! new messages on one Server-Sent Events stream ([`ChatLive`]).
//!
//! Daemons hold the panes, never the talk about them. What they tell
//! control is who may read each pane's or session's thread
//! ([`CHAT_READERS`], decision 2 of S41), and, once, the threads they kept
//! themselves before chat moved here ([`CHAT_IMPORT`], M124).
//!
//! These types are new with M122, so nothing older reads them; the web
//! client's copy is generated (`just proto-ts`, the `ts` feature).

use serde::{Deserialize, Serialize};

/// A daemon says who may read its panes' and sessions' threads (`POST`),
/// [`ChatReaders`], answered [`ChatReadersStored`].
pub const CHAT_READERS: &str = "/api/daemon/chat/readers";

/// A daemon brings in the threads it kept itself (`POST`, M124),
/// [`ChatImport`], answered [`ChatImported`].
pub const CHAT_IMPORT: &str = "/api/daemon/chat/import";

/// What a daemon that brings its threads into control says it understands
/// (its `features`): control's page tells the owner of a machine without it
/// to update it, so its threads come into chat.
pub const CHAT_IMPORT_FEATURE: &str = "chat-import";

/// The `via` of a line the platform wrote in a room, with no author (#122:
/// an agent grant reached the room).
pub const SYSTEM_VIA: &str = "system";

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
    /// The thread on an object (M196), by its global id: a native issue
    /// (`iss_`), a mirrored GitHub PR or issue (`fi_`) or a hosted
    /// workspace (`hw_`). Its people are its project's members.
    Object,
}

/// What a daemon's thread is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatTargetKind {
    Pane,
    Session,
    /// An object, by its global id, on no machine (M196): only opened by
    /// people, never in a daemon's reader list.
    Object,
}

impl ChatTargetKind {
    pub fn kind(self) -> ChatKind {
        match self {
            ChatTargetKind::Pane => ChatKind::Pane,
            ChatTargetKind::Session => ChatKind::Session,
            ChatTargetKind::Object => ChatKind::Object,
        }
    }
}

/// Someone, as the reader sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPerson {
    pub account: String,
    pub name: String,
    /// They've paused their notifications (do not disturb, #115): a moon
    /// on their avatar. Only that it's on, never until when or why.
    #[serde(default)]
    pub dnd: bool,
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
    /// The pane or session id there, or an object thread's global id.
    pub target: Option<String>,
    /// The project an object thread is in (M196).
    #[serde(default)]
    pub project: Option<String>,
    /// A DM's other person.
    pub with: Option<ChatPerson>,
    /// The newest message's number, and the reader's read mark.
    pub last: i64,
    pub read: i64,
    /// Messages after the read mark (not the reader's own), and how many
    /// of those mention the reader.
    pub unread: u64,
    pub mentions: u64,
    /// The reader turned its pushes off (`level` is `nothing`).
    pub muted: bool,
    /// What the reader is notified of here: their own choice for the room,
    /// else their default for its kind (#113).
    #[serde(default)]
    pub level: NotifyLevel,
    /// Whether `level` is the reader's own choice for this room (else it's
    /// their default), so the menu can offer "use my default".
    #[serde(default)]
    pub level_own: bool,
    /// A channel its team's owners archived: read, not written.
    pub archived: bool,
    /// Whether the reader may post here now (a DM whose other person left
    /// the team, an archive, an archived channel: no).
    pub writable: bool,
    /// What people wrote about a channel (#119); empty when nobody has.
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub description: String,
    /// How many messages are pinned.
    #[serde(default)]
    pub pins: u32,
    /// Whether the reader may set the topic: a team can leave it to its
    /// owners.
    #[serde(default)]
    pub topic_writable: bool,
}

/// `GET /api/chat`: every room the reader sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRooms {
    pub rooms: Vec<ChatRoom>,
    /// Unread replies in the threads the reader follows, and how many
    /// mention them (#106): the Threads view's badge, which the rooms'
    /// own counts leave out.
    #[serde(default)]
    pub threads_unread: u64,
    #[serde(default)]
    pub threads_mentions: u64,
}

/// `GET /api/chat/rooms/{id}`: a room and who sees it (whom `@` reaches).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRoomDetail {
    pub room: ChatRoom,
    pub people: Vec<ChatPerson>,
    /// The agents a live grant lets read or post here (M197), for everyone
    /// in the room to see: "Jake's agents on geek can read and post here".
    #[serde(default)]
    pub agents: Vec<ChatRoomAgent>,
}

/// A machine whose agents reach a room through its owner's grant (M197).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatRoomAgent {
    /// The owner's account, and their name now.
    pub owner: String,
    pub owner_name: String,
    pub machine: String,
    pub machine_name: String,
    /// What it may do here: `threads.read`, `threads.post`, and on a
    /// native issue's thread `issues.write`.
    pub powers: Vec<String>,
}

/// The agent that wrote a message (M197): the machine whose grant it acted
/// through, and its name now. The message's author is the machine's owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatAgent {
    pub machine: String,
    pub machine_name: String,
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
    /// Its number in the room. Messages brought in from a machine (M124)
    /// keep their old numbers, unless the room already had messages of its
    /// own: then they're numbered 0 and below, before control's.
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
    /// The accounts it mentions, and the agents (`machine:<id>`, M198).
    pub mentions: Vec<String>,
    pub edited: Option<u64>,
    /// When it was deleted: its text, quote and pane are gone.
    pub deleted: Option<u64>,
    /// Brought in from a machine's own threads (M124): the name it had
    /// there. Someone with no account here (a tailnet login, an agent) is
    /// shown under the machine's owner, by this name.
    pub imported: Option<String>,
    /// Written by an agent on the author's machine (M197): shown as
    /// "<name>'s agent on <machine_name>".
    #[serde(default)]
    pub agent: Option<ChatAgent>,
    /// Posted for the author by a product (M237): a hud box's name, for a
    /// review written there. Shown as "<name> via <via>". [`SYSTEM_VIA`]
    /// with no author: a line the platform wrote (#122), shown apart, as
    /// plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub via: Option<String>,
    /// Who reacted with which emoji (#104), the first reaction first.
    /// Empty for a deleted message and for archives brought in.
    #[serde(default)]
    pub reactions: Vec<ChatReaction>,
    /// Files and images attached (#107), in the order they were chosen.
    /// Empty for a deleted message: its files went with it.
    #[serde(default)]
    pub files: Vec<ChatFile>,
    /// Pinned (#119).
    #[serde(default)]
    pub pinned: bool,
    /// Not something said but something that happened (#119): `author` set
    /// the topic or pinned a message. Drawn dim and centered; never
    /// counted unread, pushed, edited or reacted to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub system: Option<ChatSystem>,
    /// A reply (#106): the number of its thread's first message. The
    /// room's list has it only with `also`; the thread has it always.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub parent: Option<i64>,
    /// A reply that was also sent to the channel.
    #[serde(default)]
    pub also: bool,
    /// The thread of replies this message starts, once it has any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub thread: Option<ChatThread>,
}

/// One attached file, as the reader sees it. `url` is control's, for the
/// room's readers only (a session cookie, or an agent's token): the same
/// path with `?download=1` downloads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatFile {
    pub id: String,
    pub name: String,
    /// What the bytes were sniffed as, never what the upload said.
    pub mime: String,
    pub kind: ChatFileKind,
    pub size: u64,
    /// An image's size in pixels, when its header gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub height: Option<u32>,
    pub url: String,
}

/// How a file is shown: `Image`, `Video` and `Audio` inline (only types
/// that are safe to render: never SVG or HTML), anything else as a card
/// that downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatFileKind {
    Image,
    Video,
    Audio,
    File,
}

/// The most one file is, in bytes. Small while the bytes live in the
/// platform's database (`chat_files`); object storage would let it grow.
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
/// The most files one message carries.
pub const MAX_FILES: usize = 10;
/// The most file bytes a team's chat (a scope) keeps: as above, a
/// database's budget, not a bucket's.
pub const MAX_SCOPE_FILE_BYTES: u64 = 256 << 20;

/// `POST /api/chat/rooms/{id}/files?name=`: the file's bytes are the body.
/// Answers [`ChatFile`]; it belongs to the uploader until a message is
/// posted with its `id` in [`ChatPost::files`] (an hour, else it's swept).
/// Agents use `/api/daemon/chat/rooms/{id}/files` the same way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatUploadQuery {
    pub name: String,
}

/// What a system line tells of (#119).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSystem {
    /// `topic` (its `text` is the new topic, empty when cleared),
    /// `description` (likewise), `pin` or `unpin`.
    pub kind: String,
    /// The pinned message's number, for `pin` and `unpin`.
    pub target: Option<i64>,
}

/// The longest topic and description (#119), in characters.
pub const MAX_TOPIC: usize = 250;
pub const MAX_DESCRIPTION: usize = 1000;

/// `PUT /api/chat/rooms/{id}/topic`: what a channel's members (or, when
/// the team says so, its owners) write about it. What's left out stays;
/// an empty one clears it. Each change posts a system line. Answers the room.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatTopicSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub description: Option<String>,
}

/// One pinned message, in `GET /api/chat/rooms/{id}/pins` (#119).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPin {
    pub message: ChatMessage,
    /// Who pinned it, and when.
    pub by: String,
    pub by_name: String,
    pub at: u64,
}

/// `GET /api/chat/rooms/{id}/pins`: the room's pins, the newest first,
/// as the reader sees the messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPins {
    pub pins: Vec<ChatPin>,
}

/// The replies to a message, as one reader sees them (#106).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatThread {
    /// Replies not deleted, and when the newest was written (ms).
    pub replies: u32,
    pub last: u64,
    /// Who replied, the newest first, at most [`THREAD_REPLIERS`].
    pub repliers: Vec<ChatPerson>,
    /// The reader follows the thread. Always false to an agent.
    pub following: bool,
    /// The reader's mark in the thread (a room number), and the replies
    /// after it that aren't theirs (only while they follow), and how many
    /// of those mention them.
    pub read: i64,
    pub unread: u64,
    pub mentions: u64,
}

/// How many repliers a [`ChatThread`] names.
pub const THREAD_REPLIERS: usize = 5;

/// One emoji on a message, as the reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatReaction {
    pub emoji: String,
    /// All who reacted with it; `who` names the first [`MAX_REACTORS_SHOWN`].
    pub count: u32,
    /// The reader has: a person's own reaction, or for an agent reading,
    /// its own (never the owner's, nor the agent's for the owner).
    pub mine: bool,
    pub who: Vec<ChatReactor>,
}

/// Someone who reacted: an account, and the machine's agent if it was
/// the agent reacting for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatReactor {
    pub account: String,
    pub name: String,
    pub agent: Option<ChatAgent>,
}

/// What a system line says after its author's name, for an agent to read:
/// `set the topic: ship friday`, `pinned message 12`.
pub fn system_text(s: &ChatSystem, text: &str) -> String {
    let at = s.target.map(|t| format!("message {t}")).unwrap_or_else(|| "a message".into());
    match s.kind.as_str() {
        "topic" if text.is_empty() => "cleared the topic".into(),
        "topic" => format!("set the topic: {text}"),
        "description" => "changed the description".into(),
        "pin" => format!("pinned {at}"),
        "unpin" => format!("unpinned {at}"),
        other => other.to_owned(),
    }
}

/// A room's pins for an agent to read, the newest pin first: `[12] Sam:
/// freeze starts friday (pinned by Jake)`.
pub fn pins_line(pins: &[ChatPin]) -> String {
    pins.iter()
        .map(|p| {
            let by = if p.by_name.is_empty() { "someone" } else { p.by_name.as_str() };
            let name = if p.message.name.is_empty() { "someone" } else { p.message.name.as_str() };
            format!("[{}] {name}: {} (pinned by {by})", p.message.seq, p.message.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A message's reactions in one line for an agent to read: `👍 2 (Sam,
/// Jake's agent on geek), 🎉 1 (you)`; empty when there are none. `mine` is
/// the reader's own, which is the agent's when an agent reads.
pub fn reactions_line(rs: &[ChatReaction]) -> String {
    rs.iter()
        .map(|c| {
            let mut who: Vec<String> = c
                .who
                .iter()
                .map(|w| {
                    let n = if w.name.is_empty() { "someone" } else { w.name.as_str() };
                    match &w.agent {
                        Some(a) => format!(
                            "{n}'s agent on {}",
                            if a.machine_name.is_empty() { "a machine" } else { &a.machine_name }
                        ),
                        None => n.to_owned(),
                    }
                })
                .collect();
            if c.mine {
                who.push("you".into());
            }
            format!("{} {} ({})", c.emoji, c.count, who.join(", "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// How many reactors a [`ChatReaction`] names.
pub const MAX_REACTORS_SHOWN: usize = 20;

/// At most this many different emoji on one message.
pub const MAX_REACTIONS: usize = 20;

/// `POST`/`DELETE /api/chat/rooms/{id}/messages/{seq}/reactions` (and the
/// same under `/api/daemon/chat/` for an agent, see [`agent_reactions`]):
/// add the caller's reaction, or take it back. The emoji is the body of a
/// post and `?emoji=` of a delete. Both answer the message ([`ChatMessage`]),
/// and doing either twice is not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatReact {
    pub emoji: String,
}

/// `DELETE`'s query: [`ChatReact`] in the URL.
pub type ChatReactQuery = ChatReact;

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
    /// Read the replies in the thread of this message instead of the
    /// room's list (#106).
    #[serde(default)]
    pub thread: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatMessages {
    pub room: ChatRoom,
    /// Oldest first. With `thread`, its replies.
    pub messages: Vec<ChatMessage>,
    /// There are more in the direction asked.
    pub more: bool,
    /// With `thread`: the message that starts it, deleted or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub parent: Option<ChatMessage>,
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

/// `POST /api/chat/targets`: the thread about a pane or a session, or an
/// object (M196, `machine` empty), made the first time by someone who may
/// read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatOpenTarget {
    #[serde(default)]
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
    /// Files uploaded for this message (ids from `POST .../files`, #107):
    /// the poster's own, for this room, not attached before. A message may
    /// be only files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "ts", ts(as = "Option<Vec<String>>", optional))]
    pub files: Vec<String>,
    /// "Notify anyway" (#115): in a DM, push the other person though they
    /// have paused notifications. For this message only; anywhere else it
    /// does nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub notify_anyway: Option<bool>,
    /// A reply in the thread of this message (#106): the number of a
    /// message of the room that is not itself a reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub parent: Option<i64>,
    /// With `parent`: also send it to the channel, as Slack's checkbox
    /// does. The room's list shows it too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub also: Option<bool>,
}

/// Why control pushed someone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatPushWhy {
    Mention,
    Dm,
    /// One of the account's keyword alerts matched (#114).
    Keyword,
    /// A message in a room the person set to every message (#113).
    Message,
    /// A reply in a thread the person follows (#106).
    Thread,
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
    /// The accounts it mentions, and the agents (`machine:<id>`, M198).
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

/// `POST /api/chat/rooms/{id}/unread`: mark as unread from `seq`: the read
/// mark goes to just before it, backwards if need be (#116). The answer is
/// the room, as the reader now sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatUnread {
    pub seq: i64,
}

/// `PUT /api/chat/rooms/{id}/messages/{seq}/saved`: save the message for
/// later (again: it changes the reminder, and un-does "done"). `remind_at`
/// (ms since the epoch, in the future, at most [`SAVED_REMIND_MAX_MS`] away)
/// is when to push the reader once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSave {
    #[serde(default)]
    pub remind_at: Option<u64>,
}

/// `PATCH .../saved`: mark a saved message done, or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSavedDone {
    pub done: bool,
}

/// How far ahead a reminder may be set, and how many items one account
/// keeps (the oldest done ones go first).
pub const SAVED_REMIND_MAX_MS: u64 = 366 * 24 * 3_600_000;
pub const SAVED_MAX: usize = 500;

/// One saved message, with its room (the reader still sees it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSavedItem {
    pub room: String,
    pub seq: i64,
    pub saved_at: u64,
    /// When it was marked done.
    pub done: Option<u64>,
    /// When it will push the reader (or did: `reminded`).
    pub remind_at: Option<u64>,
    pub reminded: bool,
    pub message: ChatMessage,
}

/// `GET /api/chat/saved`: the reader's saved messages in rooms they still
/// see, the ones to do newest saved first, then the done ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSaved {
    pub items: Vec<ChatSavedItem>,
}

/// `POST /api/chat/rooms/{id}/threads/{seq}/read`: read the thread up to
/// here (a room number; it only moves forward). Answers the thread's first
/// message, with its [`ChatThread`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatThreadRead {
    pub seq: i64,
}

/// `POST /api/chat/rooms/{id}/threads/{seq}/follow`: follow the thread, or
/// stop. Answers its first message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatFollow {
    pub following: bool,
}

/// One thread the reader follows, in [`ChatThreads`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatThreadItem {
    pub room: String,
    /// The message that starts it, with its [`ChatThread`].
    pub parent: ChatMessage,
    /// The newest reply, for the list's preview.
    pub latest: Option<ChatMessage>,
}

/// `GET /api/chat/threads`: the threads the reader follows in rooms they
/// see, the most recently active first (at most [`THREADS_LISTED`]), and
/// all their unread: the badge on the Threads view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatThreads {
    pub threads: Vec<ChatThreadItem>,
    pub unread: u64,
    pub mentions: u64,
}

/// The most threads one [`ChatThreads`] lists.
pub const THREADS_LISTED: usize = 100;

/// `POST /api/chat/rooms/{id}/mute`: no pushes from this room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatMute {
    pub muted: bool,
}

/// `GET`/`PUT /api/chat/keywords`: the words that notify this account as a
/// mention does (#114), lowercased, at most [`KEYWORDS_MAX`], each at most
/// [`KEYWORD_CHARS`] long.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatKeywords {
    pub words: Vec<String>,
}

pub const KEYWORDS_MAX: usize = 20;
pub const KEYWORD_CHARS: usize = 60;

/// `GET`/`PUT /api/chat/teams/{team}/settings`: what a team's owners
/// decide about its chat (#114). Anyone in the team reads it, for the
/// composer; only an owner sets it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatTeamSettings {
    /// `@channel` and `@here` are for owners only.
    pub broadcast_owners: bool,
    /// Only owners set a channel's topic (#119).
    #[serde(default)]
    pub topic_owners: bool,
}

/// How much of a room reaches the reader's pushes, and how its unread shows
/// (#113).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    /// Every message.
    All,
    /// The reader's mentions and DMs.
    #[default]
    Mentions,
    /// Nothing: what muting was.
    Nothing,
}

/// `POST /api/chat/rooms/{id}/level`: the reader's level for the room; none
/// is "use my default".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatLevel {
    pub level: Option<NotifyLevel>,
}

/// Quiet hours on some weekdays: from `from` to `to`, in minutes after
/// midnight in the account's time zone. A window that ends at or before it
/// starts runs past midnight, into the next day (its weekday is the day it
/// starts on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QuietHours {
    /// ISO weekdays: 1 Monday … 7 Sunday.
    pub days: Vec<u8>,
    pub from: u16,
    pub to: u16,
}

/// `GET /api/notify/prefs`: the account's notification settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotifyPrefs {
    /// Levels for rooms with none of their own (#113).
    pub channels: NotifyLevel,
    pub dms: NotifyLevel,
    /// Replies in the threads the person follows (#106).
    pub threads: NotifyLevel,
    /// Snoozed until (ms since the epoch), while that's ahead.
    pub snooze_until: Option<u64>,
    /// IANA name; the schedule is read in it.
    pub tz: String,
    pub quiet: Vec<QuietHours>,
    /// Do not disturb holds back every push, not only chat's.
    pub all_notices: bool,
    /// Do not disturb is on now: snoozed, or inside quiet hours.
    pub paused: bool,
}

/// `POST /api/notify/prefs`: what's given changes, the rest stays.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotifySet {
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub channels: Option<NotifyLevel>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dms: Option<NotifyLevel>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub threads: Option<NotifyLevel>,
    /// Snooze until this time (ms); 0 ends a snooze.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub snooze_until: Option<u64>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tz: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub quiet: Option<Vec<QuietHours>>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub all_notices: Option<bool>,
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
/// open. A page with several chat blocks (#123) has several: `room` is the
/// first and `more` the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatOpen {
    pub stream: String,
    pub room: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub more: Option<Vec<String>>,
    /// The thread open beside the room (#106), by its first message's
    /// number: replies in it aren't pushed for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub thread: Option<i64>,
}

/// How a person's presence shows to the teammates who see it: `active` (a
/// page of theirs is open and was used in the last 10 minutes), `away`
/// (open but idle, or set by hand) or `offline` (no page, or they chose to
/// appear so).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum PresenceState {
    Active,
    Away,
    Offline,
}

/// What a person chose for their own presence: `auto` (as their pages
/// say), `away` (shown away while a page is open) or `offline` (shown
/// offline always).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum PresenceMode {
    #[default]
    Auto,
    Away,
    Offline,
}

/// `POST /api/chat/presence`: a page's heartbeat, every 15 s while it's
/// open, and the answer is what it shows. `idle`: the page hasn't been
/// used for 10 minutes. `gone`: it's closing (sent as a beacon), so the
/// person shows offline now rather than when the beat goes stale. `mode`
/// sets their choice (absent: leave it). `machines`: whose online state
/// the page wants too (the machines its messages' agents are on).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatBeat {
    #[serde(default)]
    pub idle: bool,
    #[serde(default)]
    pub gone: bool,
    #[serde(default)]
    pub mode: Option<PresenceMode>,
    #[serde(default)]
    pub machines: Vec<String>,
}

/// One teammate's presence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPresent {
    pub account: String,
    pub state: PresenceState,
}

/// Who's around, for one reader: themselves and everyone who shares a team
/// with them (nobody else is named, online or not), and the asked-for
/// machines they may see that are connected. Machines' agents show as
/// their machine, not as a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatPresence {
    /// The reader's own choice.
    pub mode: PresenceMode,
    pub people: Vec<ChatPresent>,
    pub machines_online: Vec<String>,
}

/// What `GET /api/chat/live` sends, one JSON object per event (no event
/// names). Every event but `hello` has an id, so `EventSource` resumes
/// with `Last-Event-ID` after a drop, on whichever of the platform's
/// machines it reaches; a stream that can't be resumed (it fell too far
/// behind, or missed more events than control keeps) gets `resync`: reload
/// the rooms.
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
    /// A message was pinned or unpinned, or a pinned one deleted (#119):
    /// read the room's pins and the room again. (A topic changing is
    /// [`ChatLive::Room`].)
    Pinned {
        room: String,
    },
    /// A message was edited or deleted: fetch it again.
    Changed {
        room: String,
        seq: i64,
    },
    /// Reactions to a message were added or taken back (#104): fetch it
    /// again. No room list changes with it.
    Reacted {
        room: String,
        seq: i64,
    },
    /// A reply in a thread was written, edited, deleted or reacted to
    /// (#106): read the thread again (the replies after the newest it has,
    /// and the message at `parent`, whose summary moved). A reply also sent
    /// to the channel is announced as a [`Message`](Self::Message) too.
    Reply {
        room: String,
        parent: i64,
        seq: i64,
    },
    /// The reader's own mark in a thread moved (another of their pages), or
    /// they followed or unfollowed it.
    ThreadRead {
        room: String,
        parent: i64,
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
    /// What a room of kind `object` is on changed (M203: a native issue's
    /// title, body, state, labels or assignees): read it again. Sent to
    /// those who see the room; an object nobody has a thread on has none.
    Object {
        room: String,
        target: String,
    },
    /// Someone is typing in a room (#111): shown for [`TYPING_MS`] and
    /// never stored or replayed. Sent to the room's other readers.
    Typing {
        room: String,
        account: String,
        name: String,
    },
    /// The reader's sidebar changed (another of their pages; #120): fetch
    /// it again. Only they hear it.
    Sidebar {},
    /// The reader's saved messages changed (#116: another of their pages
    /// saved, finished or removed one, or a reminder came due): read them
    /// again. Sent to that account only.
    Saved {},
    Resync {},
}

/// How long a typing ping shows, and the client's gap between pings.
pub const TYPING_MS: u64 = 6_000;
pub const TYPING_PING_MS: u64 = 3_000;

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
/// floor, if it has one): it doesn't see messages from before. `watching`:
/// they watch the session (a viewer), so they read the thread and don't
/// post in it (Jake, 2026-10-19). An archive's members never post.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReader {
    pub account: String,
    #[serde(default)]
    pub from: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watching: bool,
}

/// How many lists control kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReadersStored {
    pub stored: usize,
}

/// The most lists in one [`ChatReaders`], and readers in one list.
pub const MAX_READER_TARGETS: usize = 500;
pub const MAX_READERS: usize = 500;

/// What a daemon brings into control, once, from the threads it kept
/// itself (M61) and its conversations (M81), when it first finds chat on in
/// control (S41 q6). Control takes each message once, by its old number, so
/// a call that's sent again (or split over several) adds only what it
/// hasn't got.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImport {
    /// Pane and session threads, each into its room on this machine.
    #[serde(default)]
    pub threads: Vec<ChatImportThread>,
    /// Conversations, each into a read-only archive for its people.
    #[serde(default)]
    pub archives: Vec<ChatImportArchive>,
    /// Who reads each thread now, stored first, as [`ChatReaders`] does.
    #[serde(default)]
    pub readers: Vec<ChatReaderList>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImportThread {
    pub kind: ChatTargetKind,
    pub target: String,
    pub messages: Vec<ChatImportMessage>,
}

/// A conversation, kept as text (decision 3): its title, its owner and
/// members (by account; others had none), and its timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImportArchive {
    /// Its id on the machine (`c-…`).
    pub id: String,
    pub title: String,
    pub created: u64,
    /// Who read it, and from when (ms; 0 is everything): its owner and its
    /// members with an account.
    pub members: Vec<ChatReader>,
    pub messages: Vec<ChatImportMessage>,
}

/// One message as the machine kept it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImportMessage {
    /// Its number there: 1, 2, 3…
    pub id: u64,
    pub at: u64,
    /// The author's account, if they had one (the machine's owner, or
    /// `account:…`).
    #[serde(default)]
    pub account: Option<String>,
    /// Their name there.
    pub name: String,
    pub text: String,
    /// The lines it quoted, and the pane they came from (on this machine).
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub pane: Option<String>,
    /// The accounts it mentioned.
    #[serde(default)]
    pub mentions: Vec<String>,
}

/// What control took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImported {
    /// Rooms it made or added to.
    pub rooms: usize,
    /// Messages added, and those it had already.
    pub messages: usize,
    pub already: usize,
}

/// The most threads and archives one [`ChatImport`] carries, and the most
/// bytes a daemon puts in one (control takes signed bodies up to 1 MB): a
/// daemon with more sends several, a long thread in parts, oldest first.
pub const MAX_IMPORT_ROOMS: usize = 500;
pub const MAX_IMPORT_BODY: usize = 768 << 10;

// ---------------------------------------------------------------- agents

/// The rooms a machine's agents reach (`GET`, M198): every team channel,
/// DM and object thread its owner's live `threads.read` or `threads.post`
/// grants reach, [`AgentRooms`]. Signed by the machine; MCP's thread tools
/// call it, and the rest below.
pub const AGENT_ROOMS: &str = "/api/daemon/chat/rooms";

/// Searching the rooms an agent may read (`GET`, [`ChatSearchQuery`]),
/// answered [`ChatFound`].
pub const AGENT_SEARCH: &str = "/api/daemon/chat/search";

/// A room's messages as an agent reads them (`GET`,
/// [`AgentMessagesQuery`], answered [`AgentMessages`]), and an agent's
/// post there (`POST` a [`ChatPost`], answered [`ChatPosted`]). `room` is
/// a room's id, or an object's global id (`iss_…`, `fi_…`, `hw_…`) for its
/// thread, made the first time.
pub fn agent_messages(room: &str) -> String {
    format!("{AGENT_ROOMS}/{room}/messages")
}

/// An agent's file uploads (`POST` the bytes, `?name=`, answered a
/// [`ChatFile`]) and, with `/{file}`, its downloads (#107).
pub fn agent_files(room: &str) -> String {
    format!("{AGENT_ROOMS}/{room}/files")
}

/// An agent's reactions to message `seq` (`threads.post`): `POST` a
/// [`ChatReact`], or `DELETE` with `?emoji=`; answered the [`ChatMessage`].
pub fn agent_reactions(room: &str, seq: i64) -> String {
    format!("{AGENT_ROOMS}/{room}/messages/{seq}/reactions")
}

/// An agent's pin of message `seq` (`threads.post`): `PUT` pins it, `DELETE`
/// unpins; answered the [`ChatMessage`].
pub fn agent_pin(room: &str, seq: i64) -> String {
    format!("{AGENT_ROOMS}/{room}/messages/{seq}/pin")
}

/// How a message's `mentions` names a machine's agents: `machine:` and the
/// machine's id. An `@` and the machine's name (`@geek`, `@geek's agent`)
/// mentions them where their `threads.read` grant reaches the room. Nobody is
/// pushed for it, and nothing wakes an agent: they read it with
/// [`AgentMessagesQuery::mentions`].
pub const AGENT_MENTION: &str = "machine:";

/// [`AGENT_ROOMS`]' answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRooms {
    pub rooms: Vec<AgentRoom>,
    /// What the machine's live agent grants reach, among what its owner
    /// reads now.
    pub reach: Vec<AgentReach>,
}

/// A room as a machine's agents see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRoom {
    pub id: String,
    pub kind: ChatKind,
    /// A channel's or DM's team.
    pub team: Option<String>,
    /// A channel's name (without `#`), or an object thread's (`issue #12`).
    pub name: String,
    /// An object thread's global id, and its project.
    pub target: Option<String>,
    pub project: Option<String>,
    /// A DM's other person.
    pub with: Option<ChatPerson>,
    /// The newest message's number, and the newest that mentions this
    /// machine's agents.
    pub last: i64,
    pub last_mention: Option<i64>,
    /// `threads.read`, `threads.post`.
    pub powers: Vec<String>,
    /// Whether its agents may post here now.
    pub writable: bool,
    /// A channel's topic and description (#119).
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub description: String,
}

/// A scope a machine's agent grants reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReach {
    /// `project` (its issues and object threads; a team's default
    /// project, its channels too) or `account` (the owner's DMs).
    pub kind: String,
    pub id: String,
    /// The project's name, or `your DMs`.
    pub name: String,
    /// `threads.read`, `threads.post`, `issues.write`.
    pub powers: Vec<String>,
}

/// A page of [`agent_messages`]: as [`ChatMessagesQuery`], and `mentions`
/// for only those that mention this machine's agents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessagesQuery {
    #[serde(default)]
    pub after: Option<i64>,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub mentions: bool,
    /// The replies in the thread of this message instead of the room's
    /// list (#106).
    #[serde(default)]
    pub thread: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessages {
    pub room: AgentRoom,
    pub messages: Vec<ChatMessage>,
    pub more: bool,
    /// The room's pinned messages, the newest pin first (#119).
    #[serde(default)]
    pub pins: Vec<ChatPin>,
    /// With `thread`: the message that starts it.
    #[serde(default)]
    pub parent: Option<ChatMessage>,
}
