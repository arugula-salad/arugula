// Generated from crates/proto by `just proto-ts`: don't edit. Change the
// Rust types and run it again; CI checks this file is current.

export type PaneId = number;
export type TabId = number;
export type SessionId = number;
export type NodeId = number;
export type ClientId = number;
export type MachineId = number;

/**
 * `POST /api/attention/act`: do something about one pane's reason, or
 * several at once ("allow all 3", "dismiss all 11"). Each pane needs
 * editor on its session.
 */
export type ActRequest = { action: Action, pane?: number, panes?: Array<number>, 
/**
 * The ask it answers (`AskRef::id`); without one, whatever the pane
 * asks now.
 */
id?: string, 
/**
 * `answer`: the card's fields.
 */
content?: Record<string, unknown>, 
/**
 * `allow`: `once` (default) or `always`.
 */
option?: string, 
/**
 * `allow` `always` for Claude Code in a terminal (M29): which of its
 * suggestions to keep (default the first).
 */
suggestion?: number, 
/**
 * `deny`: why, for the agent.
 */
message?: string, 
/**
 * `accept` (M28): the file as it should be saved, when someone
 * changed the proposal first.
 */
text?: string, };

export type ActResponse = { results: Array<ActResult>, };

export type ActResult = { pane: number, ok: boolean, error?: string, };

/**
 * Something done about a reason (`POST /api/attention/act`).
 */
export type Action = "allow" | "deny" | "answer" | "dismiss" | "continue" | "accept" | "reject" | "rerun" | "expire";

/**
 * How much a pane prints (M23).
 */
export type Activity = { 
/**
 * Bytes of output a second, over the last second or so.
 */
bps: number, 
/**
 * When it last printed anything (ms since the epoch); 0: not since
 * the daemon started.
 */
last_ms: number, };

/**
 * The agent working on an issue, and what it made.
 */
export type AgentLink = { branch: string, 
/**
 * The worktree, absolute.
 */
worktree: string, 
/**
 * What the branch was made from.
 */
base: string, 
/**
 * The agent block.
 */
block: number, 
/**
 * `claude`, `codex`, …
 */
agent: string, at_ms: number, 
/**
 * Its pull request, once there is one (found once).
 */
pr?: number, pr_url?: string, 
/**
 * The PR block opened beside the agent.
 */
pr_block?: number, };

/**
 * The open question or approval behind an `ask` reason.
 */
export type AskRef = { 
/**
 * What `allow`, `deny` and `answer` name (a permission request's id, or
 * a question's).
 */
id: string, 
/**
 * `approve` (allow or deny it) or `question` (answer or skip it).
 */
what: AskWhat, 
/**
 * Who asks: the agent (`claude`, `managoat`, …).
 */
agent: string, 
/**
 * A permission's tool (`Bash`, `Edit`…), when it's known (#857).
 */
tool?: string, 
/**
 * A question's choices (#859): Claude's AskUserQuestion `questions`
 * (`question`, `header`, `multiSelect`, `options`), so a card away from
 * the pane can be answered with them. Boxed: a
 * reason rides in every attention event.
 */
questions?: import("./ask").Question[], };

export type AskWhat = "approve" | "question";

export type AttachPane = { pane: number, 
/**
 * Offset just past the last byte the client has, or `None` for a fresh
 * view.
 */
offset: number | null, 
/**
 * At most this many rows of scrollback in a snapshot: what the client
 * keeps (`None`: all of it). After a [`ServerMsg::Resync`], `0`: the
 * client keeps what it has and needs only the screen.
 */
history?: number, };

/**
 * Whether a pane wants you: the cheap version of an "agent block".
 */
export type Attention = "idle" | "working" | "needs_input" | "done";

/**
 * What a block is; where one runs is its `host`, not its type. All types
 * share one id space (`%N`) and one place in the layout tree.
 */
export type BlockType = "terminal" | "browser" | "agent" | "editor" | "diff" | "file" | "remote" | "workspace" | "app" | "forge" | "invite" | "agents" | "doc" | "chat" | "team_chat" | "board";

/**
 * Zone or row.
 */
export type BoardEntryKind = "zone" | "row";

export type BoardList = { project: string, boards: Array<ProjectBoard>, };

/**
 * The item a row points at, as it is now: what the page draws beside the
 * row's own status, and the label of its link.
 */
export type BoardLive = { state: BoardLiveState, 
/**
 * `pr` or `issue` (a mirrored one), or `native` for a native issue.
 */
kind: string, 
/**
 * `OWNER/NAME` of a mirrored item; absent for a native issue.
 */
repo?: string | null, number: number, title: string, };

/**
 * What a row's `fi_` or `iss_` pointer is now, from data control already
 * has (the forge mirror, the issues table): a separate fact, never written
 * over the row's stored `status`.
 */
export type BoardLiveState = "open" | "checks_running" | "checks_failed" | "merged" | "closed";

/**
 * [`BOARD_ENTRY_MOVE`]'s body: put the entry just before `before` (a
 * sibling), or last when there is none. A row may go to another `zone`
 * (its own, if absent); a zone moves among zones, never past `proofs`.
 */
export type BoardMoveEntry = { rev: number | null, zone: string | null, before: string | null, };

/**
 * What a row points at. `None` is free text. The ids are the platform's:
 * `fi_…` (a mirrored PR or issue), `iss_…`, `run_…`, `hw_…`.
 */
export type BoardPointer = { "kind": "none" } | { "kind": "forge_item", id: string, } | { "kind": "issue", id: string, } | { "kind": "run", id: string, } | { "kind": "workspace", id: string, } | { "kind": "block", machine: string, pane: string, };

/**
 * [`BOARD_ENTRIES`]' body. Without `id` it makes an entry: `entry` says
 * which. A zone is added before the `proofs` zone (a `proofs` one goes
 * last, and a board has one); a row is added last in `zone`. With `id` it
 * changes that entry: what is absent is left as it is. A change to
 * `title`, `pointer` or `note` names `rev`; a change to `status` alone
 * doesn't.
 */
export type BoardPutEntry = { id: string | null, rev: number | null, entry: BoardEntryKind | null, 
/**
 * A new row's zone.
 */
zone: string | null, title: string | null, 
/**
 * A new zone's kind.
 */
kind: BoardZoneKind | null, 
/**
 * A row's pointer; `{"kind": "none"}` clears it.
 */
pointer: BoardPointer | null, note: string | null, 
/**
 * A row's status; `"none"` clears it.
 */
status: BoardStatus | null, };

export type BoardRow = { id: string, rev: number, pointer: BoardPointer, note: string, status: BoardStatus, 
/**
 * A `fi_` or `iss_` pointer's state now, when the reader may see the
 * item ([`BoardView`] is read with it; a write's answer has it too).
 */
live?: BoardLive | null, 
/**
 * The machine whose agent added the row, if one did (it may remove
 * it; nobody else's agent may).
 */
agent?: ChatAgent | null, created_at: number, updated_at: number, };

/**
 * Where a row stands. `None` says nothing.
 */
export type BoardStatus = "none" | "open" | "working" | "needs_input" | "done" | "failed";

/**
 * A board with its zones and rows, in order.
 */
export type BoardView = { board: ProjectBoard, zones: Array<BoardZone>, };

export type BoardZone = { id: string, rev: number, title: string, kind?: BoardZoneKind | null, rows: Array<BoardRow>, };

/**
 * A zone's kind: `proofs` is the board's last zone. Absent: a plain one.
 */
export type BoardZoneKind = "proofs";

/**
 * A side of a PR: which repository (a fork's, for its head), branch and
 * commit.
 */
export type Branch = { repo: string | null, branch: string, sha: string, };

/**
 * A person's note asking an agent to change its draft.
 */
export type ChangesAsked = { note: string, by: string, at_ms: number, 
/**
 * When the agent drafted again.
 */
revised_ms?: number, };

/**
 * `POST /api/chat/rooms/{id}/members`: a channel's creator or its team's
 * owners add people of the team.
 */
export type ChatAddMembers = { accounts: Array<string>, };

/**
 * The agent that wrote a message (M197): the machine whose grant it acted
 * through, and its name now. The message's author is the machine's owner.
 */
export type ChatAgent = { machine: string, machine_name: string, };

/**
 * `POST /api/chat/presence`: a page's heartbeat, every 15 s while it's
 * open, and the answer is what it shows. `idle`: the page hasn't been
 * used for 10 minutes. `gone`: it's closing (sent as a beacon), so the
 * person shows offline now rather than when the beat goes stale. `mode`
 * sets their choice (absent: leave it). `machines`: whose online state
 * the page wants too (the machines its messages' agents are on).
 */
export type ChatBeat = { idle: boolean, gone: boolean, mode: PresenceMode | null, machines: Array<string>, };

/**
 * A public channel in the browser (#118).
 */
export type ChatChannel = { id: string, name: string, topic: string, 
/**
 * How many of the team are in it.
 */
members: number, 
/**
 * The reader is one.
 */
joined: boolean, archived: boolean, };

/**
 * `GET /api/chat/teams/{team}/channels?q=`: the team's public channels
 * (a private one only shows to its members, in their rooms), by name.
 */
export type ChatChannels = { channels: Array<ChatChannel>, };

/**
 * `PATCH /api/chat/rooms/{id}/messages/{seq}`: the author's new text.
 */
export type ChatEdit = { text: string, };

/**
 * One attached file, as the reader sees it. `url` is control's, for the
 * room's readers only (a session cookie, or an agent's token): the same
 * path with `?download=1` downloads it.
 */
export type ChatFile = { id: string, name: string, 
/**
 * What the bytes were sniffed as, never what the upload said.
 */
mime: string, kind: ChatFileKind, size: number, 
/**
 * An image's size in pixels, when its header gave it.
 */
width?: number, height?: number, url: string, };

/**
 * How a file is shown: `Image`, `Video` and `Audio` inline (only types
 * that are safe to render: never SVG or HTML), anything else as a card
 * that downloads.
 */
export type ChatFileKind = "image" | "video" | "audio" | "file";

/**
 * `POST /api/chat/rooms/{id}/threads/{seq}/follow`: follow the thread, or
 * stop. Answers its first message.
 */
export type ChatFollow = { following: boolean, };

export type ChatFound = { found: Array<ChatHit>, };

export type ChatHit = { room: string, message: ChatMessage, };

/**
 * `GET`/`PUT /api/chat/keywords`: the words that notify this account as a
 * mention does (#114), lowercased, at most [`KEYWORDS_MAX`], each at most
 * [`KEYWORD_CHARS`] long.
 */
export type ChatKeywords = { words: Array<string>, };

/**
 * What a room is.
 */
export type ChatKind = "channel" | "dm" | "pane" | "session" | "archive" | "object";

/**
 * `POST /api/chat/rooms/{id}/level`: the reader's level for the room; none
 * is "use my default".
 */
export type ChatLevel = { level: NotifyLevel | null, };

/**
 * What `GET /api/chat/live` sends, one JSON object per event (no event
 * names). Every event but `hello` has an id, so `EventSource` resumes
 * with `Last-Event-ID` after a drop, on whichever of the platform's
 * machines it reaches; a stream that can't be resumed (it fell too far
 * behind, or missed more events than control keeps) gets `resync`: reload
 * the rooms.
 */
export type ChatLive = { "type": "hello", stream: string, } | { "type": "message", room: string, seq: number, } | { "type": "pinned", room: string, } | { "type": "changed", room: string, seq: number, } | { "type": "reacted", room: string, seq: number, } | { "type": "reply", room: string, parent: number, seq: number, } | { "type": "threadread", room: string, parent: number, seq: number, } | { "type": "room", room: string, } | { "type": "read", room: string, seq: number, } | { "type": "object", room: string, target: string, } | { "type": "session", room: string, ses: string, } | { "type": "typing", room: string, account: string, name: string, } | { "type": "sidebar", } | { "type": "saved", } | { "type": "resync", };

/**
 * A message as the reader sees it.
 */
export type ChatMessage = { 
/**
 * Its number in the room. Messages brought in from a machine (M124)
 * keep their old numbers, unless the room already had messages of its
 * own: then they're numbered 0 and below, before control's.
 */
seq: number, at: number, 
/**
 * The author's account, and their name now. Both are empty for a
 * deleted account.
 */
author: string, name: string, text: string, 
/**
 * Lines the poster chose from a pane, copied in (decision 1).
 */
quote: string | null, pane: ChatPane | null, 
/**
 * The accounts it mentions, and the agents (`machine:<id>`, M198).
 */
mentions: Array<string>, edited: number | null, 
/**
 * When it was deleted: its text, quote and pane are gone.
 */
deleted: number | null, 
/**
 * Brought in from a machine's own threads (M124): the name it had
 * there. Someone with no account here (a tailnet login, an agent) is
 * shown under the machine's owner, by this name.
 */
imported: string | null, 
/**
 * Written by an agent on the author's machine (M197): shown as
 * "<name>'s agent on <machine_name>".
 */
agent: ChatAgent | null, 
/**
 * Posted for the author by a product (M237): a hud box's name, for a
 * review written there. Shown as "<name> via <via>". [`SYSTEM_VIA`]
 * with no author: a line the platform wrote (#122), shown apart, as
 * plain text.
 */
via?: string, 
/**
 * Who reacted with which emoji (#104), the first reaction first.
 * Empty for a deleted message and for archives brought in.
 */
reactions: Array<ChatReaction>, 
/**
 * Files and images attached (#107), in the order they were chosen.
 * Empty for a deleted message: its files went with it.
 */
files: Array<ChatFile>, 
/**
 * Pinned (#119).
 */
pinned: boolean, 
/**
 * Not something said but something that happened (#119): `author` set
 * the topic or pinned a message. Drawn dim and centered; never
 * counted unread, pushed, edited or reacted to.
 */
system?: ChatSystem, 
/**
 * A reply (#106): the number of its thread's first message. The
 * room's list has it only with `also`; the thread has it always.
 */
parent?: number, 
/**
 * A reply that was also sent to the channel.
 */
also: boolean, 
/**
 * The thread of replies this message starts, once it has any.
 */
thread?: ChatThread, };

export type ChatMessages = { room: ChatRoom, 
/**
 * Oldest first. With `thread`, its replies.
 */
messages: Array<ChatMessage>, 
/**
 * There are more in the direction asked.
 */
more: boolean, 
/**
 * With `thread`: the message that starts it, deleted or not.
 */
parent?: ChatMessage, };

/**
 * `POST /api/chat/rooms/{id}/mute`: no pushes from this room.
 */
export type ChatMute = { muted: boolean, };

/**
 * `POST /api/chat/teams/{team}/channels`: owners and editors make them.
 */
export type ChatNewChannel = { name: string, 
/**
 * Invite-only and out of the browser (#118); else public.
 */
private: boolean, topic: string, 
/**
 * Everyone who joins the team later joins it too.
 */
auto_join: boolean, };

/**
 * `POST /api/chat/teams/{team}/dms`: the DM with a teammate, or (#117,
 * with `and`) with up to seven more, made the first time: the same people
 * again reopen it.
 */
export type ChatNewDm = { with: string, and: Array<string>, };

/**
 * `POST /api/chat/live/open`: the room a page's stream has open (`None`:
 * none, or the page is hidden). Nobody is pushed for a room they have
 * open. A page with several chat blocks (#123) has several: `room` is the
 * first and `more` the rest.
 */
export type ChatOpen = { stream: string, room: string | null, more?: Array<string>, 
/**
 * The thread open beside the room (#106), by its first message's
 * number: replies in it aren't pushed for.
 */
thread?: number, };

/**
 * `POST /api/chat/targets`: the thread about a pane or a session, or an
 * object (M196, `machine` empty), made the first time by someone who may
 * read it.
 */
export type ChatOpenTarget = { machine: string, kind: ChatTargetKind, target: string, };

/**
 * A pane a message names, as the reader sees it: whether its chip opens
 * for them (they may reach the machine), and the machine's name.
 */
export type ChatPane = { machine: string, machine_name: string, pane: string, opens: boolean, };

/**
 * A pane a message names, as the poster sends it.
 */
export type ChatPaneRef = { machine: string, pane: string, };

/**
 * Someone, as the reader sees them.
 */
export type ChatPerson = { account: string, name: string, 
/**
 * They've paused their notifications (do not disturb, #115): a moon
 * on their avatar. Only that it's on, never until when or why.
 */
dnd: boolean, };

/**
 * One pinned message, in `GET /api/chat/rooms/{id}/pins` (#119).
 */
export type ChatPin = { message: ChatMessage, 
/**
 * Who pinned it, and when.
 */
by: string, by_name: string, at: number, };

/**
 * `GET /api/chat/rooms/{id}/pins`: the room's pins, the newest first,
 * as the reader sees the messages.
 */
export type ChatPins = { pins: Array<ChatPin>, };

/**
 * `POST /api/chat/rooms/{id}/messages`.
 */
export type ChatPost = { text: string, 
/**
 * A pane the message is about: only one on a machine the poster may
 * reach.
 */
pane?: ChatPaneRef, quote?: string, 
/**
 * Files uploaded for this message (ids from `POST .../files`, #107):
 * the poster's own, for this room, not attached before. A message may
 * be only files.
 */
files?: Array<string>, 
/**
 * "Notify anyway" (#115): in a DM, push the other person though they
 * have paused notifications. For this message only; anywhere else it
 * does nothing.
 */
notify_anyway?: boolean, 
/**
 * A reply in the thread of this message (#106): the number of a
 * message of the room that is not itself a reply.
 */
parent?: number, 
/**
 * With `parent`: also send it to the channel, as Slack's checkbox
 * does. The room's list shows it too.
 */
also?: boolean, };

export type ChatPosted = { seq: number, 
/**
 * The accounts it mentions, and the agents (`machine:<id>`, M198).
 */
mentions: Array<string>, 
/**
 * `@words` that named nobody who sees the room.
 */
unreached: Array<string>, 
/**
 * Whom control pushed (not those with the room open, muted, or pushed
 * for it in the last minute).
 */
pushed: Array<ChatPushed>, };

/**
 * Who's around, for one reader: themselves and everyone who shares a team
 * with them (nobody else is named, online or not), and the asked-for
 * machines they may see that are connected. Machines' agents show as
 * their machine, not as a person.
 */
export type ChatPresence = { 
/**
 * The reader's own choice.
 */
mode: PresenceMode, people: Array<ChatPresent>, machines_online: Array<string>, };

/**
 * One teammate's presence.
 */
export type ChatPresent = { account: string, state: PresenceState, };

/**
 * Why control pushed someone.
 */
export type ChatPushWhy = "mention" | "dm" | "keyword" | "message" | "thread";

export type ChatPushed = { account: string, why: ChatPushWhy, };

/**
 * `POST`/`DELETE /api/chat/rooms/{id}/messages/{seq}/reactions` (and the
 * same under `/api/daemon/chat/` for an agent, see [`agent_reactions`]):
 * add the caller's reaction, or take it back. The emoji is the body of a
 * post and `?emoji=` of a delete. Both answer the message ([`ChatMessage`]),
 * and doing either twice is not an error.
 */
export type ChatReact = { emoji: string, };

/**
 * One emoji on a message, as the reader sees it.
 */
export type ChatReaction = { emoji: string, 
/**
 * All who reacted with it; `who` names the first [`MAX_REACTORS_SHOWN`].
 */
count: number, 
/**
 * The reader has: a person's own reaction, or for an agent reading,
 * its own (never the owner's, nor the agent's for the owner).
 */
mine: boolean, who: Array<ChatReactor>, };

/**
 * Someone who reacted: an account, and the machine's agent if it was
 * the agent reacting for them.
 */
export type ChatReactor = { account: string, name: string, agent: ChatAgent | null, };

/**
 * `POST /api/chat/rooms/{id}/read`: read up to here (it only moves
 * forward).
 */
export type ChatRead = { seq: number, };

/**
 * A room as one person sees it.
 */
export type ChatRoom = { id: string, kind: ChatKind, 
/**
 * The team a channel or DM is in.
 */
team: string | null, 
/**
 * A channel's name (without `#`); a pane's or session's room is
 * named `pane 2`, `session 1`.
 */
name: string, 
/**
 * The machine a pane's or session's room is on, and its name.
 */
machine: string | null, machine_name: string | null, 
/**
 * The pane or session id there, or an object thread's global id.
 */
target: string | null, 
/**
 * The project an object thread is in (M196).
 */
project: string | null, 
/**
 * A DM's other person (for a group DM, the first of `others`).
 */
with: ChatPerson | null, 
/**
 * A DM's other people now, in account order (#117): one in a 1:1, and
 * a group DM is named after them unless it was renamed (`name`).
 */
others: Array<ChatPerson>, 
/**
 * An invite-only channel (#118), hidden from the browser.
 */
private: boolean, 
/**
 * The newest message's number, and the reader's read mark.
 */
last: number, read: number, 
/**
 * Messages after the read mark (not the reader's own), and how many
 * of those mention the reader.
 */
unread: number, mentions: number, 
/**
 * The reader turned its pushes off (`level` is `nothing`).
 */
muted: boolean, 
/**
 * What the reader is notified of here: their own choice for the room,
 * else their default for its kind (#113).
 */
level: NotifyLevel, 
/**
 * Whether `level` is the reader's own choice for this room (else it's
 * their default), so the menu can offer "use my default".
 */
level_own: boolean, 
/**
 * A channel its team's owners archived: read, not written.
 */
archived: boolean, 
/**
 * Whether the reader may post here now (a DM whose other person left
 * the team, an archive, an archived channel: no).
 */
writable: boolean, 
/**
 * What people wrote about a channel (#119); empty when nobody has.
 */
topic: string, description: string, 
/**
 * How many messages are pinned.
 */
pins: number, 
/**
 * Whether the reader may set the topic: a team can leave it to its
 * owners.
 */
topic_writable: boolean, };

/**
 * A machine whose agents reach a room through its owner's grant (M197).
 */
export type ChatRoomAgent = { 
/**
 * The owner's account, and their name now.
 */
owner: string, owner_name: string, machine: string, machine_name: string, 
/**
 * What it may do here: `threads.read`, `threads.post`, and on a
 * native issue's thread `issues.write`.
 */
powers: Array<string>, };

/**
 * `PATCH /api/chat/rooms/{id}`: a channel renamed or archived, by its
 * team's owners; a DM renamed (`name`).
 */
export type ChatRoomChange = { name?: string, archived?: boolean, };

/**
 * `GET /api/chat/rooms/{id}`: a room and who sees it (whom `@` reaches).
 */
export type ChatRoomDetail = { room: ChatRoom, 
/**
 * Who sees the room: a channel's or a group DM's members (#118).
 */
people: Array<ChatPerson>, 
/**
 * The reader may add and remove a channel's members: its creator and
 * its team's owners.
 */
can_manage: boolean, 
/**
 * The agents a live grant lets read or post here (M197), for everyone
 * in the room to see: "Jake's agents on geek can read and post here".
 */
agents: Array<ChatRoomAgent>, };

/**
 * `GET /api/chat`: every room the reader sees.
 */
export type ChatRooms = { rooms: Array<ChatRoom>, 
/**
 * Unread replies in the threads the reader follows, and how many
 * mention them (#106): the Threads view's badge, which the rooms'
 * own counts leave out.
 */
threads_unread: number, threads_mentions: number, };

/**
 * `PUT /api/chat/rooms/{id}/messages/{seq}/saved`: save the message for
 * later (again: it changes the reminder, and un-does "done"). `remind_at`
 * (ms since the epoch, in the future, at most [`SAVED_REMIND_MAX_MS`] away)
 * is when to push the reader once.
 */
export type ChatSave = { remind_at: number | null, };

/**
 * `GET /api/chat/saved`: the reader's saved messages in rooms they still
 * see, the ones to do newest saved first, then the done ones.
 */
export type ChatSaved = { items: Array<ChatSavedItem>, };

/**
 * `PATCH .../saved`: mark a saved message done, or not.
 */
export type ChatSavedDone = { done: boolean, };

/**
 * One saved message, with its room (the reader still sees it).
 */
export type ChatSavedItem = { room: string, seq: number, saved_at: number, 
/**
 * When it was marked done.
 */
done: number | null, 
/**
 * When it will push the reader (or did: `reminded`).
 */
remind_at: number | null, reminded: boolean, message: ChatMessage, };

/**
 * A section the account made.
 */
export type ChatSection = { 
/**
 * Made by the page that creates it (`s_` and hex), stable under rename.
 */
id: string, name: string, 
/**
 * Room ids, in the order the person dragged them.
 */
rooms: Array<string>, };

/**
 * `GET /api/chat/sidebar`, and the body of `PUT` (which replaces it).
 */
export type ChatSidebar = { 
/**
 * Starred rooms, in the order they were starred.
 */
starred: Array<string>, 
/**
 * The account's own sections, top to bottom, below Starred.
 */
sections: Array<ChatSection>, 
/**
 * Groups shown shut, by key: `starred`, a section's id, or a built-in
 * group's (`<team>/channels`, `<team>/dms`, `machine/<id>`, `archives`).
 */
collapsed: Array<string>, 
/**
 * Groups not sorted by name, by the same keys.
 */
sorts: { [key in string]: ChatSort }, };

/**
 * How a group's rooms are ordered.
 */
export type ChatSort = "name" | "recent";

/**
 * What a system line tells of (#119).
 */
export type ChatSystem = { 
/**
 * `topic` (its `text` is the new topic, empty when cleared),
 * `description` (likewise), `pin` or `unpin`.
 */
kind: string, 
/**
 * The pinned message's number, for `pin` and `unpin`.
 */
target: number | null, };

/**
 * What a daemon's thread is about.
 */
export type ChatTargetKind = "pane" | "session" | "object";

/**
 * `GET`/`PUT /api/chat/teams/{team}/settings`: what a team's owners
 * decide about its chat (#114). Anyone in the team reads it, for the
 * composer; only an owner sets it.
 */
export type ChatTeamSettings = { 
/**
 * `@channel` and `@here` are for owners only.
 */
broadcast_owners: boolean, 
/**
 * Only owners set a channel's topic (#119).
 */
topic_owners: boolean, };

/**
 * The replies to a message, as one reader sees them (#106).
 */
export type ChatThread = { 
/**
 * Replies not deleted, and when the newest was written (ms).
 */
replies: number, last: number, 
/**
 * Who replied, the newest first, at most [`THREAD_REPLIERS`].
 */
repliers: Array<ChatPerson>, 
/**
 * The reader follows the thread. Always false to an agent.
 */
following: boolean, 
/**
 * The reader's mark in the thread (a room number), and the replies
 * after it that aren't theirs (only while they follow), and how many
 * of those mention them.
 */
read: number, unread: number, mentions: number, };

/**
 * One thread the reader follows, in [`ChatThreads`].
 */
export type ChatThreadItem = { room: string, 
/**
 * The message that starts it, with its [`ChatThread`].
 */
parent: ChatMessage, 
/**
 * The newest reply, for the list's preview.
 */
latest: ChatMessage | null, };

/**
 * `POST /api/chat/rooms/{id}/threads/{seq}/read`: read the thread up to
 * here (a room number; it only moves forward). Answers the thread's first
 * message, with its [`ChatThread`].
 */
export type ChatThreadRead = { seq: number, };

/**
 * `GET /api/chat/threads`: the threads the reader follows in rooms they
 * see, the most recently active first (at most [`THREADS_LISTED`]), and
 * all their unread: the badge on the Threads view.
 */
export type ChatThreads = { threads: Array<ChatThreadItem>, unread: number, mentions: number, };

/**
 * `PUT /api/chat/rooms/{id}/topic`: what a channel's members (or, when
 * the team says so, its owners) write about it. What's left out stays;
 * an empty one clears it. Each change posts a system line. Answers the room.
 */
export type ChatTopicSet = { topic?: string, description?: string, };

/**
 * `POST /api/chat/rooms/{id}/typing`'s body, which may be left out (#111).
 * `warm` (M262): the composer's text asks `@arugula`, so the project's
 * agents box is woken now, before the message is sent. A page sends it
 * once per composing burst. `show` false: only that, not typing (a reply
 * thread's composer, whose typing nobody is shown).
 */
export type ChatTyping = { warm: boolean, show: boolean, };

/**
 * `POST /api/chat/rooms/{id}/unread`: mark as unread from `seq`: the read
 * mark goes to just before it, backwards if need be (#116). The answer is
 * the room, as the reader now sees it.
 */
export type ChatUnread = { seq: number, };

export type Check = { name: string, source: CheckSource, state: CheckState, allow_failure: boolean, 
/**
 * Absolute (Forgejo's own are relative to the site; the adapter makes
 * them whole).
 */
url: string | null, description: string | null, run: RunRef | null, };

export type CheckSource = "status" | "action" | "check_run" | "pipeline_job";

export type CheckState = "queued" | "running" | "success" | "failure" | "cancelled" | "skipped" | "neutral" | "action_required" | "manual";

export type Child = { weight: number, node: Node, };

/**
 * Control messages from a client.
 */
export type ClientMsg = { "type": "attach", panes: Array<AttachPane>, zstd?: boolean, acks?: boolean, 
/**
 * The client encodes keys for the kitty keyboard protocol (M31).
 * Unused since #806: programs asking are told it's there whoever
 * is attached. Kept so older clients and daemons still agree.
 */
kitty_keys?: boolean, } | { "type": "ack", pane: number, offset: number, } | { "type": "detach", panes: Array<number>, } | { "type": "view", tab: number, cols: number, rows: number, zoom: number | null, claim: boolean, typed?: boolean, 
/**
 * The person pressed "Use this window's size" (#1502).
 */
asked?: boolean, } | { "type": "intent", id: number | null, intent: Intent, } | { "type": "pane", pane: number, op: PaneOp, } | { "type": "focus", pane: number | null, } | { "type": "ping", id: number, } | { "type": "subscribe", summary: boolean, } | { "type": "follow", pane: number, on: boolean, } | { "type": "block_patches" } | { "type": "hand", tools: Array<HandTool>, name?: string, } | { "type": "hand_reply", id: number, result?: unknown, error?: string, };

/**
 * A command the shell integration reported.
 */
export type CommandInfo = { text: string | null, cwd: string | null, exit: number | null, started_ms: number, ended_ms: number | null, 
/**
 * Stream offsets of its output: `tail --from start`.
 */
start: number, end: number | null, 
/**
 * Who started it (M13), when someone other than the owner might have.
 */
by?: string, };

/**
 * A machine's standing with Arugula control (#325): whether and where
 * it's joined, whether control is reachable, or that control dropped it.
 * The page, the tray and `arugula status` all show this.
 */
export type ControlState = { 
/**
 * `not_joined`, `joined` or `dropped`.
 */
state: "not_joined" | "joined" | "dropped", 
/**
 * The control it's (or was) joined to.
 */
url?: string, 
/**
 * `account` or `team`.
 */
kind?: "account" | "team", 
/**
 * The team's name, or the account's login (empty until control says).
 */
name?: string, 
/**
 * Joined: reachable through control now (its relay socket is up; for a
 * sandbox behind a provider's proxy, the last refresh worked).
 */
connected: boolean, 
/**
 * When control last answered (ms since the epoch).
 */
seen_ms?: number, 
/**
 * Joined: what last went wrong talking to control, until it works again.
 */
error?: string, 
/**
 * Dropped: what control said ("not an enrolled daemon (left, or
 * revoked?)", or that its key was removed, #330).
 */
said?: string, 
/**
 * Dropped: when this machine first heard it (ms since the epoch).
 */
dropped_ms?: number, 
/**
 * Dropped: a join waiting for approval, its code (a machine whose key
 * was removed asks to join again with a new key by itself, #330; or
 * someone started one), and where a signed-in device approves it.
 */
code?: string, approve?: string, };

/**
 * An editor's debug session.
 */
export type DebugState = { 
/**
 * `running` or `paused`.
 */
state: "running" | "paused", 
/**
 * Why it stopped: `breakpoint`, `exception`, `step`, ...
 */
reason?: string | null, file?: string | null, line?: number | null, };

/**
 * A decision covering a region, as chant's intent read ranks it
 * (`graph --intent <dir>`'s `why.decisions`, #617): chant does the
 * `member:` and `path:` covering and follows supersession.
 */
export type DecisionRef = { id: string, title: string | null, state: string | null, 
/**
 * How it covers the region: `carried`, `path`, `contract`, `issue`,
 * `member`, or `related` (only through supersession or a commit).
 */
relevance: string, 
/**
 * Not superseded.
 */
current: boolean, 
/**
 * In a state its kind closes, such as ratified.
 */
closed: boolean, };

/**
 * Changes to the last [`State`]: each pane in `panes` is `{id, ...}` with
 * only the fields that changed (a field set to `null` went back to its
 * default, absent); `gone` panes left this client's view. `machines` and
 * `presence` are whole when present. Anything else (sessions, tabs, options,
 * roles) changes with a new `State`.
 */
export type Delta = { panes?: Array<{ id: PaneId } & Partial<PaneInfo>>, gone?: Array<number>, machines?: Array<Machine> | null, presence?: Array<Presence> | null, };

/**
 * Diagnostic counts: errors, warnings, information.
 */
export type Diag = { e: number, w: number, i: number, };

/**
 * An edit an agent proposes, waiting as a diff (M28).
 */
export type DiffInfo = { 
/**
 * What `accept` and `reject` name.
 */
id: string, 
/**
 * The file it changes (whole path), and relative to the pane's
 * directory when it's inside.
 */
file: string, 
/**
 * Lines added and removed.
 */
added: number, removed: number, 
/**
 * The change as a unified diff, cut short when it's long.
 */
text: string, 
/**
 * It makes a new file.
 */
new?: boolean, at_ms: number, 
/**
 * Which IDE shows it: `arugula`, or the one diffs go to.
 */
ide: string, };

export type Dir = "row" | "column";

/**
 * `GET /api/doc-links/ID` (#1066): a shared Doc's URL, with its key.
 */
export type DocLinkUrl = { url: string, };

/**
 * `POST /api/docs/open` (#1493): a Doc from the library in a block. Exactly
 * one of `id` and `link` names it.
 */
export type DocOpenRequest = { 
/**
 * The Doc's id (`docs.list` has them).
 */
id?: string | null, 
/**
 * A link's id, as control's Shared Docs lists it (#1494): the Doc that
 * link shares.
 */
link?: string | null, session?: string | null, split?: number | null, };

/**
 * The block that shows the Doc: the one that had it open, or a new one.
 */
export type DocOpened = { pane: number, };

/**
 * One Doc in the library (#1493): `GET /api/docs`. `open` is the block
 * that shows it now, `shared` its link's dates and id (never its key).
 */
export type DocRow = { id: string, title: string, made: number, updated: number, 
/**
 * Who made it: an agent (`mcp:NAME`), a person's driver name, or
 * `migrated` for a Doc that predates the library.
 */
by: string, open?: number | null, shared?: DocShared | null, };

/**
 * When a Doc was shared with a link, and last updated.
 */
export type DocShared = { made: number, updated: number, 
/**
 * The link's id, as control's list of shared Docs names it (never the
 * key): that list titles its rows from it (UI audit #174).
 */
link: string, };

/**
 * An agent's write, waiting for a person.
 */
export type Draft = { id: string, 
/**
 * Who drafted it (`mcp:claude-code`, `agent`).
 */
by: string, at_ms: number, status: DraftStatus, 
/**
 * Who sent or dropped it, and when.
 */
settled_by?: string, settled_ms?: number, url?: string, 
/**
 * The last send failed: why (it waits again, text kept).
 */
error?: string, } & ({ "method": "comment", body: string, } | { "method": "review", event: ReviewEvent, body?: string, } | { "method": "merge", style?: string, } | { "method": "rerun_checks" } | { "method": "live", on: boolean, });

export type DraftSent = { draft: ForgeDraft, 
/**
 * `via: "browser"`: GitHub's page to open; the draft isn't sent yet.
 */
authorize: string | null, 
/**
 * How it went: `token` (as the person, through GitHub's authorization)
 * or `machine`.
 */
via: string | null, 
/**
 * The machine's name, when one sent it.
 */
machine: string | null, };

export type DraftStatus = "waiting" | "sent" | "dropped";

/**
 * A pane's driver (M13).
 */
export type Driver = { 
/**
 * Principal id (`owner`, `tailnet:<login>`, `account:<id>`).
 */
who: string, name: string, };

/**
 * Where to put something relative to a pane.
 */
export type Edge = "left" | "right" | "top" | "bottom" | "center";

/**
 * What an editor says about itself in summaries (M28, S17's schema):
 * what changes about once in ten seconds. The cursor and the file's text
 * are content and go only to followers ([`ServerMsg::Follow`]).
 */
export type EditorInfo = { 
/**
 * `vscode`, `cursor`, `code-server`, `nvim`, ...
 */
app: string, 
/**
 * VS Code's remote: `ssh-remote`, `dev-container`, ... (`None`: local).
 */
remote?: string | null, 
/**
 * The remote's authority (`ssh-remote+geek`), to open the same file
 * from a desktop editor.
 */
authority?: string | null, 
/**
 * The machine it runs on, as it names itself.
 */
hostname?: string | null, 
/**
 * Diagnostics across the workspace.
 */
diag: Diag, 
/**
 * Files with unsaved changes.
 */
dirty: number, debug?: DebugState | null, 
/**
 * A file with merge conflict markers that's open.
 */
conflict?: string | null, 
/**
 * How many people follow it now: the editor says so.
 */
followers: number, };

export type Event = { 
/**
 * Provider-prefixed, stable across polls (`fj-1403`).
 */
id: string, at: number, actor: string | null, kind: EventKind, 
/**
 * The forge's own name for it, when it's `other` (or more exact).
 */
what?: string, 
/**
 * Who a request is for.
 */
target?: Reviewer, body?: string, 
/**
 * A push: how many commits, and whether it was forced.
 */
commits?: number, force?: boolean, };

export type EventKind = "commented" | "review_comment" | "reviewed" | "review_requested" | "review_request_removed" | "review_dismissed" | "pushed" | "labeled" | "assigned" | "renamed" | "referenced" | "merged" | "closed" | "reopened" | "branch_deleted" | "milestone" | "mentioned" | "other";

export type ExportedIssue = { issue: NativeIssue, events: Array<IssueEvent>, 
/**
 * Oldest first; deleted ones left out.
 */
comments: Array<IssueComment>, };

/**
 * Someone an export names, with what they're called now.
 */
export type ExportedPerson = { account: string, name: string, };

/**
 * `GET /api/flags`: one row per flag (the `flags.list` operation).
 */
export type FlagInfo = { name: string, 
/**
 * What Developer settings calls it. Absent from a daemon before #665.
 */
title?: string, about: string, 
/**
 * Whether it is on here now.
 */
on: boolean, default: boolean, 
/**
 * Whether this build has what the flag turns on. A build without Labs
 * can set a flag, and nothing follows; Settings says so. Absent from
 * daemons that don't report it, which have it.
 */
built?: boolean, 
/**
 * Where `on` comes from (#347). Absent from a daemon before it, whose
 * flags are all this machine's or their defaults.
 */
source?: FlagSource, 
/**
 * This machine's word on it; absent: it follows control.
 */
local?: boolean, 
/**
 * Control's word for the machine's account, if it has one.
 */
account?: boolean, 
/**
 * Control's word for the machine's team, if it has one.
 */
team?: boolean, 
/**
 * Only this machine sets it; control's word is ignored
 * ([`Flag::local_only`]). Absent: control may set it.
 */
local_only?: boolean, };

/**
 * `PUT /api/flags/{name}`: the body (the `flag.set` operation). `null`
 * takes this machine's word back, so the flag follows control (#347);
 * only a daemon that reports [`FlagInfo::source`] takes it.
 */
export type FlagSetRequest = { on: boolean | null, };

/**
 * Where a flag's state comes from (#347), first match wins: this
 * machine's `flags.json`, control for the machine's account, control for
 * its team, else the flag's default.
 */
export type FlagSource = "default" | "team" | "account" | "machine";

export type FollowChange = { range: [number, number, number, number], text: string, };

export type FollowCursor = { file: string, line: number, col: number, sel?: [number, number, number, number] | null, 
/**
 * The first and last lines in view.
 */
view?: [number, number] | null, 
/**
 * nvim's mode.
 */
mode?: string, };

export type FollowDiagnostic = { range: [number, number, number, number], 
/**
 * `error`, `warning`, `information`, `hint`.
 */
severity: string, message: string, };

export type FollowDiagnostics = { file: string, items: Array<FollowDiagnostic>, };

export type FollowEdit = { file: string, version: number, changes: Array<FollowChange>, };

export type FollowMsg = FollowCursor | { open: FollowOpen, } | { edit: FollowEdit, } | { diagnostics: FollowDiagnostics, } | { gone: true, };

export type FollowOpen = { file: string, version: number, text: string | null, lang?: string, too_big?: boolean, };

export type ForgeContent = { item: string, 
/**
 * `github` (read now, or a few minutes ago) or `mirror`.
 */
from: string, body: ForgeText | null, 
/**
 * Comments, reviews' text and review comments, oldest first. A review
 * comment's thread is the ones whose `reply_to` is its `id`.
 */
comments: Array<ForgeText>, };

export type ForgeDraft = { 
/**
 * The platform's id: `dft_` and 32 hex characters.
 */
id: string, project: string, 
/**
 * `OWNER/NAME`.
 */
repo: string, repo_id: number, number: number | null, 
/**
 * The mirrored item it's on, and its title, when the mirror has it.
 */
item: string | null, item_title: string | null, 
/**
 * `comment`, `review` or `issue`.
 */
kind: string, event: string | null, title: string | null, body: string, 
/**
 * `waiting`, `sending` (a send holds it, M168), `sent`, `discarded`
 * or `failed`.
 */
state: string, 
/**
 * The person it's for: an account id.
 */
author: string, 
/**
 * The agent that wrote it, and the machine it was on (an id, and its
 * name when the machine is still there).
 */
agent: string | null, machine: string | null, machine_name: string | null, created_at: number, updated_at: number, sent_at: number | null, sent_url: string | null, error: string | null, };

export type ForgeDrafts = { drafts: Array<ForgeDraft>, };

/**
 * M40: how the block hears of changes.
 */
export type ForgeLive = "webhook" | "polling";

/**
 * A `tea` login to pick from.
 */
export type ForgeLogin = { name: string, url: string, user: string, };

/**
 * M172: a GitHub block on a repo linked to a project in Arugula control
 * is a pin onto the platform's PR or issue ("The daemon's forge reader
 * stays for unlinked repos; linked repos' blocks are pins", aru-087): it
 * shows what the platform's mirror says, and this machine doesn't read
 * GitHub for it.
 */
export type ForgePin = { kind: ItemKind, 
/**
 * The platform's id for it: `fi_` and 32 hex characters.
 */
global_id: string, 
/**
 * Why the platform couldn't be read just now, while what it said last
 * is shown.
 */
stale?: string, };

/**
 * A forge block's state: the PR or issue, what it wants of you, the agent's
 * drafts, and how it's kept current.
 */
export type ForgeState = { provider: Provider, 
/**
 * M37: `pr` or `issue`.
 */
kind: ItemKind, repo: string, number: number, api: string | null, login: string | null, host: string | null, dir: string | null, loading: boolean, error: string | null, 
/**
 * It can only read, and why (GitLab with no glab login: anonymous).
 */
read_only: string | null, 
/**
 * Logins to pick from, when none (or several) matched.
 */
logins: Array<ForgeLogin>, 
/**
 * "You", on the forge.
 */
me: string | null, pr: Pr | null, 
/**
 * M37: the issue, for `kind: issue`.
 */
issue: Issue | null, 
/**
 * M37: the agent on it.
 */
link: AgentLink | null, 
/**
 * M37: a new issue, before (and after) it went out.
 */
new: NewIssue | null, wants: Array<ForgeWant>, 
/**
 * Whether *Rerun checks* is possible here, and where the failed run
 * is when it isn't (Forgejo).
 */
rerun: Rerun | null, drafts: Array<Draft>, updated_ms: number, polls: number, 
/**
 * M40: pokes heard (webhooks through control or straight here).
 */
pokes: number, reads: number, 
/**
 * A client draws it.
 */
watching: boolean, 
/**
 * The last write's result (sent directly), for the client.
 */
said: string | null, 
/**
 * The forge's rate limit and any backing off (M38: GitHub).
 */
rate: RateLimit | null, 
/**
 * M40: webhook (pokes say when to read) or polling, and why.
 */
live: ForgeLive, 
/**
 * Where the pokes come from, when there's a way for them to
 * (`github-app`, `hook`).
 */
live_via: string | null, live_why: string | null, 
/**
 * When it last heard from the webhook path.
 */
live_heard_ms: number | null, 
/**
 * A webhook this daemon made is on the repository.
 */
hook: boolean, 
/**
 * M172: it's the platform's item, read through Arugula control.
 */
pin: ForgePin | null, 
/**
 * The ways this repository lets a pull request be merged, as the
 * `merge` method names them (UI audit #196: GitHub's `allow_*`), so
 * Merge offers only those. None when it isn't known: the forge
 * doesn't say, or says only to someone who can push.
 */
merge_styles?: Array<string>, };

/**
 * A body, a comment, a review's text or a review comment.
 */
export type ForgeText = { 
/**
 * `body`, `comment`, `review` or `review_comment`.
 */
kind: string, 
/**
 * GitHub's id: the PR's or issue's for a body.
 */
id: number, 
/**
 * A GitHub login.
 */
author: string | null, 
/**
 * Markdown, as GitHub has it.
 */
body: string, 
/**
 * A review comment's file and line.
 */
path: string | null, line: number | null, 
/**
 * The review comment it answers.
 */
reply_to: number | null, 
/**
 * GitHub's times, in milliseconds.
 */
created_at: number, updated_at: number, };

/**
 * What it wants of you, as the client draws it.
 */
export type ForgeWant = { kind: ForgeWantKind, why: string, };

/**
 * What a PR or issue wants of you, as the client draws it: its kind.
 */
export type ForgeWantKind = "review" | "failed" | "changes" | "mention" | "done" | "assigned";

/**
 * A gate that waits for someone (M34): an op stopped before a step until
 * a person approves it. The `gate` reason, its bundle on the swarm's rail,
 * the card and the phone's sheet are all made from this, whichever reader
 * found it (`source`).
 */
export type Gate = { 
/**
 * The member (of a workspace) whose op waits.
 */
member: string, op: string, gate: string, env?: string, 
/**
 * When it started waiting, and when it stops (RFC 3339).
 */
since?: string, expires?: string, 
/**
 * Approvals so far, of how many it needs.
 */
approvals: number, needed: number, 
/**
 * The source's own command for approving it, to show.
 */
command?: string, source: GateSource, 
/**
 * A chant gate: the decisions it enforces, its plan and the member's
 * last release (#617).
 */
why?: GateWhy, };

/**
 * A release, from `status`.
 */
export type GateRelease = { component: string, 
/**
 * When (RFC 3339), who and from which commit.
 */
at: string, actor: string | null, git_sha: string | null, };

/**
 * Where a gate was read, which is how it's approved.
 */
export type GateSource = { "kind": "chant", 
/**
 * The workspace's root, and the member's directory, on that host.
 */
root: string, dir: string, 
/**
 * The machine (sprite) it's on; none for this host.
 */
machine?: string, } | { "kind": "hud", 
/**
 * The box's origin, and the app's name in studio.
 */
box_url: string, app: string, } | { "kind": "forge", 
/**
 * The forge's API base (`https://git.example/api/v1`) and its web
 * address for the PR.
 */
api: string, url: string, number: number, } | { "kind": "point", root: string, machine?: string, 
/**
 * The answer record's id, which `points answer` names.
 */
id: string, 
/**
 * The point's name.
 */
point: string, 
/**
 * What it asks: the point's title (or an ad-hoc question's own
 * text) and what it's about.
 */
question: string, 
/**
 * The answers it takes; empty when the points read didn't say.
 */
choices: Array<PointChoice>, 
/**
 * The answer a model proposed (or leaned to, below its
 * threshold), as one of `choices`' values.
 */
proposed?: string, };

/**
 * What someone about to approve a gate wants to know (#617): the
 * decisions it enforces, the plan it binds and the member's last
 * release.
 */
export type GateWhy = { 
/**
 * The decisions covering the gate's member, most relevant first. None
 * until chant has been asked.
 */
decisions: Array<DecisionRef> | null, 
/**
 * Why they couldn't be read.
 */
note: string | null, 
/**
 * The plan the gate was reached for (`status`'s `planDigest`).
 */
plan_digest: string | null, 
/**
 * The member's latest release in the env watched.
 */
last_release: GateRelease | null, };

/**
 * An ssh invite to a pane (M65). `token`, `command` and the pinning lines
 * are only in the answer that made it; the daemon keeps a hash.
 */
export type GuestInvite = { id: number, pane: number, rw: boolean, reusable: boolean, label: string, created_ms: number, expires_ms: number, 
/**
 * A single-use invite someone has logged in with.
 */
used: boolean, 
/**
 * Guests connected with it now.
 */
sessions: number, token?: string, 
/**
 * What the guest pastes: `ssh` with the host key pinned.
 */
command?: string, 
/**
 * The pinned key as a known-hosts line, for an OpenSSH older than 8.5
 * (no `KnownHostsCommand`).
 */
known_hosts?: string, 
/**
 * The host key's SHA256 fingerprint.
 */
fingerprint?: string, host?: string, port?: number, 
/**
 * Through control's ssh jump host (the daemon is behind NAT).
 */
relay: boolean, 
/**
 * The jump host, as `host[:port]` (with `command` only).
 */
jump?: string, };

/**
 * `POST /api/guests` (M65): an invite to one terminal pane for someone
 * with only OpenSSH.
 */
export type GuestInviteRequest = { pane: number, 
/**
 * They may type (one driver per pane still applies).
 */
rw?: boolean | null, 
/**
 * Good for any number of logins until it ends; else the first spends it.
 */
reusable?: boolean | null, 
/**
 * Seconds until it expires [default: an hour; at most a day, or two
 * hours with `rw`].
 */
ttl_secs?: number | null, 
/**
 * What to call them, on their input [default: `guest`].
 */
label?: string | null, 
/**
 * The address to put in the command [default: the daemon's
 * `--guest-ssh-host`, else its hostname].
 */
host?: string | null, 
/**
 * Through control's ssh jump host (`true`), or straight to this
 * machine (`false`) [default: through control when the daemon is
 * joined to one that has a jump host and no address is named].
 */
relay?: boolean | null, };

/**
 * A tool a hand offers (S33).
 */
export type HandTool = { name: string, description: string, 
/**
 * Its arguments, as a JSON Schema object.
 */
schema: unknown, };

/**
 * The optional parts of a machine, as `GET /api/host` reports them.
 */
export type HostFeatures = { 
/**
 * Some Labs flag is on here (see [`crate::flags`]). Absent from older
 * daemons, and pages treat that as off. Daemons from before each
 * feature had a flag (#665) say only this, so a page reads it for
 * every flag where `flags` is missing.
 */
labs: boolean, 
/**
 * The owner has asked for Developer settings here
 * ([`crate::flags::unlocked`]), so the page offers them.
 */
dev?: boolean, 
/**
 * Browser blocks on ports and editor blocks, on the daemon's own block
 * sites (`--block-listen`): what the daemon's own page can show.
 */
blocks: boolean, 
/**
 * The same blocks through Arugula control's block origins (M50): the
 * daemon is joined to a control with a block domain. Control's page
 * offers blocks when either is on; the daemon's own page only with
 * `blocks`. Older daemons leave it out.
 */
block_channels?: boolean, 
/**
 * VM tabs and panes and *Sandboxes…*: a sandbox provider (wisp).
 */
vms: boolean, 
/**
 * A studio is linked (`arugula studio login`).
 */
studio: boolean, 
/**
 * The names of the Labs flags that are on, whatever else each needs
 * (a login, a provider). `None` from a daemon before #665: see `labs`.
 */
flags?: Array<string>, };

/**
 * `GET /api/host`: who this daemon is.
 */
export type HostInfo = { name: string, version: string, 
/**
 * The app↔daemon protocol it speaks ([`crate::PROTOCOL`], #390).
 * Absent from daemons older than the number, which speak
 * [`crate::PROTOCOL_BASELINE`].
 */
protocol?: number, 
/**
 * Where `tailscale serve` puts the app, when tailscaled told us this
 * node's name (#109): `https://NAME.TAILNET.ts.net`.
 */
tailnet_url?: string, 
/**
 * The owner has come in over the tailnet since the daemon started:
 * serve works (#110).
 */
tailnet_seen?: boolean, 
/**
 * The control this daemon joined, if any (#110).
 */
control?: string, 
/**
 * The team it joined as, if one (#110).
 */
team?: string, 
/**
 * What this machine is set up for (#171, #180): the menus offer only
 * these, or say how to turn them on. Absent from older daemons.
 */
features?: HostFeatures, };

export type Inbox = { entries: Array<InboxEntry>, };

export type InboxEntry = { 
/**
 * The item: `fi_` and 32 hex characters, a native issue's `iss_` id
 * (M202), or a run's `run_` id (M215).
 */
item: string, 
/**
 * One of [`INBOX_REASONS`].
 */
reason: string, 
/**
 * `OWNER/NAME`; a native issue's or a run's project's name.
 */
repo: string, 
/**
 * 0 for a run.
 */
number: number, 
/**
 * `pr`, `issue` or `run`.
 */
kind: string, 
/**
 * `open`, `closed` or `merged`; a run's state.
 */
state: string, title: string, 
/**
 * A GitHub login; a native issue's author's name.
 */
author: string | null, 
/**
 * The item on GitHub; a native issue's or a run's path on the
 * platform page (`/issues/<id>`, `/runs/<id>`).
 */
url: string, 
/**
 * A native issue's or a run's project (M202, M215).
 */
project: string | null, 
/**
 * A `thread_mention`'s room: the thread on the item (M202).
 */
room: string | null, 
/**
 * When it came into the inbox, and when the item last changed.
 */
created_at: number, updated_at: number, seen_at: number | null, done_at: number | null, };

/**
 * Mark an item's entries: `true` sets seen or done (now, if it wasn't),
 * `false` clears it, and one not given stays as it is.
 */
export type InboxMark = { 
/**
 * Only this reason's entry; all of the item's when `None`.
 */
reason: string | null, seen: boolean | null, done: boolean | null, };

export type InboxMarked = { item: string, 
/**
 * How many entries it changed.
 */
marked: number, };

export type Intent = { "op": "new_session", name: string | null, from_pane: number | null, } | { "op": "rename_session", session: number, name: string, } | { "op": "close_session", session: number, } | { "op": "new_tab", session: number, from_pane: number | null, cwd?: string, } | { "op": "rename_tab", tab: number, name: string | null, } | { "op": "close_tab", tab: number, } | { "op": "move_tab", tab: number, session: number, index: number, } | { "op": "split", pane: number, edge: Edge, local?: boolean, cwd?: string, } | { "op": "close_pane", pane: number, } | { "op": "move_pane", pane: number, target: number, edge: Edge, } | { "op": "break_pane", pane: number, session: number, index: number | null, } | { "op": "dock_tab", tab: number, target: number, edge: Edge, } | { "op": "resize_split", split: number, weights: Array<number>, } | { "op": "arrange", tab: number, preset: Preset, main?: number, } | { "op": "focus", tab: number, pane: number | null, } | { "op": "set_option", scope: OptionScope, name: string, value: string | null, };

/**
 * How an invite's push went: `sent` once a subscription took it,
 * `pending` while control can't reach them yet, else `unreachable`.
 */
export type InviteDelivery = "sent" | "pending" | "unreachable";

/**
 * What an invite granted: the role they hold now, and whether this invite
 * gave it (`false`: they held it already).
 */
export type InviteGrant = { session: number, principal: string, name: string, role: Role, granted: boolean, };

/**
 * `POST /api/invite`: share a session with someone and tell them, and only
 * them (#233; the owner's).
 */
export type InviteRequest = { session: number, 
/**
 * `tailnet:<login>`, `account:<id>`, or a name: someone shared with,
 * or in a checked roster.
 */
who: string, role?: Role | null, note?: string | null, 
/**
 * Where it opens (default: the session's first pane).
 */
pane?: number | null, 
/**
 * With history (default: from now on), for a new grant.
 */
history?: boolean | null, 
/**
 * An editor may also type on this machine's pane for so long (M14).
 */
drive_minutes?: number | null, 
/**
 * For an `account:` no grant or pin vouches for: their root device,
 * whose fingerprint the owner checked with them.
 */
root?: string | null, 
/**
 * Their window follows the inviter's: to watch (#1473). Opening the
 * invite starts following whichever of the owner's windows is there.
 */
follow?: boolean | null, };

/**
 * What an invite answers.
 */
export type Invited = { invite: string, grant: InviteGrant, pane: number, delivery: InviteDelivery, 
/**
 * Why it isn't `sent`.
 */
reason: string | null, 
/**
 * Whether they may drive (`drive_minutes`), when that was asked.
 */
drive: boolean | null, };

/**
 * An issue, read whole (M37): the item (no branches), its newest events,
 * and the pull requests that refer to it.
 */
export type Issue = { item: Item, events: Array<Event>, linked: Array<Linked>, };

/**
 * [`ISSUE_AGENT`]'s answer: the issue's thread and the message that asked,
 * and the session it started, if it did (else the thread says why, or
 * asks which box or repo).
 */
export type IssueAgent = { room: string, seq: number, session: string | null, };

/**
 * A comment on an issue: a message in its thread.
 */
export type IssueComment = { 
/**
 * Its number in the thread.
 */
seq: number, 
/**
 * The account that wrote it, or "" once it's deleted.
 */
author: string, agent: ChatAgent | null, at: number, text: string, 
/**
 * Lines it quoted from a pane.
 */
quote: string | null, edited: number | null, };

/**
 * An event on an issue's timeline.
 */
export type IssueEvent = { 
/**
 * Orders an issue's events.
 */
id: number, 
/**
 * `opened`, `edited`, `closed`, `reopened`, `labelled`, `unlabelled`,
 * `assigned`, `unassigned` or `exported`.
 */
kind: string, 
/**
 * The account that did it (an agent's owner's), or "" once it's
 * deleted.
 */
actor: string, agent: ChatAgent | null, at: number, 
/**
 * `edited`: `title` (the old one) and/or `body: true`; `closed`:
 * `reason`; `labelled`/`unlabelled`: `label`;
 * `assigned`/`unassigned`: `assignee`; `exported`: `repo`, `number`.
 */
detail: Record<string, unknown>, };

export type IssueEvents = { issue: string, 
/**
 * Oldest first.
 */
events: Array<IssueEvent>, };

/**
 * Every issue in a project, by number, with its timeline and comments.
 */
export type IssueExport = { project: string, project_name: string, exported_at: number, issues: Array<ExportedIssue>, 
/**
 * Everyone it names (authors, actors, assignees, commenters).
 */
people: Array<ExportedPerson>, };

/**
 * `GET /api/projects/{project}/issues/export?format=`: `json` (when not
 * given) or `md`.
 */
export type IssueExportQuery = { format: string | null, };

/**
 * Where an issue was exported to on GitHub (M205).
 */
export type IssueGithub = { 
/**
 * `owner/name`.
 */
repo: string, number: number, };

/**
 * Where a project's "new issue" goes.
 */
export type IssueHome = { project: string, 
/**
 * `native` (the platform's own issues) or `github` (its linked repo).
 */
home: string, 
/**
 * Someone chose it; otherwise it's the default: `github` while the
 * project has a linked repo, `native` when it has none.
 */
chosen: boolean, 
/**
 * The project's linked repos (`owner/name`).
 */
repos: Array<string>, 
/**
 * How many native issues it has.
 */
native_issues: number, 
/**
 * It has issues in both places: native ones and a linked repo. The
 * page says so.
 */
both: boolean, 
/**
 * Every label on its native issues, open or closed, sorted: what the
 * label menus offer (UI audit #163).
 */
labels: Array<string>, };

/**
 * `PUT /api/projects/{project}/issue-home`: `native`, `github` (only with
 * a linked repo), or `None` for the default again.
 */
export type IssueHomeChange = { home: string | null, };

export type IssueList = { 
/**
 * The project's id (`personal` resolved).
 */
project: string, 
/**
 * Newest (highest number) first.
 */
issues: Array<NativeIssue>, 
/**
 * There are more past the last one (`before` its number).
 */
more: boolean, };

/**
 * A change to an issue's labels or assignees: the whole list, or some
 * added or taken off.
 */
export type IssueListEdit = { "set": Array<string> } | { "add": Array<string> } | { "remove": Array<string> };

/**
 * `PATCH /api/issues/{id}`: what's given changes.
 */
export type IssuePatch = { title: string | null, body: string | null, 
/**
 * `open` or `closed`.
 */
state: string | null, 
/**
 * Why it's closed (with `state: closed`): `completed` (when not
 * given), `not_planned` or `duplicate`.
 */
reason: string | null, labels: IssueListEdit | null, assignees: IssueListEdit | null, };

/**
 * What a patch made of the issue, and the events it wrote (none when
 * nothing changed).
 */
export type IssuePatched = { issue: NativeIssue, events: Array<IssueEvent>, };

/**
 * `GET /api/projects/{project}/issues`'s query. Each that's given narrows
 * the list.
 */
export type IssueQuery = { 
/**
 * `open` (when not given), `closed` or `all`.
 */
state: string | null, 
/**
 * Labels, comma-separated: issues with every one.
 */
labels: string | null, assignee: string | null, author: string | null, 
/**
 * Search words, in the title or the body: each one (`dep*`: a word
 * starting with `dep`).
 */
q: string | null, 
/**
 * Issues numbered below this: the next page.
 */
before: number | null, 
/**
 * At most this many (default 50, at most 200).
 */
limit: number | null, };

export type Item = { kind: ItemKind, number: number, url: string, title: string, body: string, 
/**
 * The author's login.
 */
author: string, state: ItemState, draft: boolean, labels: Array<string>, assignees: Array<string>, base: Branch, head: Branch, 
/**
 * `refs/pull/N/head`: what `diff` and `checkout` fetch (never the
 * head's branch: AGit PRs have none).
 */
head_ref: string, merge_base: string | null, 
/**
 * `None` while the forge works it out.
 */
mergeable: boolean | null, 
/**
 * The forge says branch protection blocks a merge (GitHub only;
 * Forgejo can't tell, so false).
 */
blocked: boolean, 
/**
 * Review requests still pending (Forgejo's `requested_reviewers` also
 * keeps whoever already reviewed: see `forge::forgejo`).
 */
requested: Array<Reviewer>, comments: number, updated_at: number, merged_at: number | null, merged_by: string | null, };

export type ItemKind = "pr" | "issue";

export type ItemState = "open" | "closed" | "merged";

export type Layout = { panes: Array<[number, Rect]>, splits: Array<SplitRect>, };

/**
 * A work lease, from `status`'s `leases` (#618): who holds a work item.
 */
export type Lease = { 
/**
 * The work item's id.
 */
item: string, holder: string, 
/**
 * `active`, or `expired` (it can be claimed again).
 */
state: string, expires_at: string | null, 
/**
 * The member whose ledger holds it; none for the workspace's own.
 */
member: string | null, 
/**
 * The fencing token, which a run under the lease names.
 */
token: string | null, 
/**
 * The Arugula agent pane holding it, when a run of one names its token
 * (or, running, its agent session is the holder).
 */
pane: number | null, };

/**
 * `POST /api/projects/{project}/repos`: link `OWNER/NAME`. A project
 * member who is an admin or maintainer of it on GitHub may. When the
 * GitHub App isn't installed where it lives, the answer is a 409 with
 * `reason: "install_app"` and `install_url`, GitHub's page to install it,
 * which brings them back to the project's settings.
 */
export type LinkRepo = { repo: string, };

/**
 * M37: a pull request that refers to an issue (from the issue's
 * timeline), or one found by its head branch.
 */
export type Linked = { number: number, title: string, state: ItemState, url: string, 
/**
 * Its head branch, when the forge said.
 */
head?: string, };

/**
 * A repo linked to a project, or unlinked and waiting for the purge.
 */
export type LinkedRepo = { 
/**
 * GitHub's id for it.
 */
repo_id: number, 
/**
 * `OWNER/NAME`.
 */
full_name: string, private: boolean, 
/**
 * Who linked it (an account id), when that account is still there.
 */
linked_by: string | null, linked_at: number, 
/**
 * When it was unlinked: nothing is read from GitHub for it since.
 */
unlinked_at: number | null, 
/**
 * When its rows go, unless it's linked again first.
 */
purge_after: number | null, 
/**
 * The project mirrors its PRs' and issues' bodies and comments (M166).
 */
mirror_content: boolean, };

export type LinkedRepos = { 
/**
 * The project's id (`personal` asked is answered with the real one).
 */
project: string, repos: Array<LinkedRepo>, };

/**
 * One TCP port something listens on, reachable from loopback.
 */
export type ListeningPort = { port: number, 
/**
 * The program listening (`node`), when it can be read.
 */
process?: string, 
/**
 * Started from the pane asked about (it or a process under it).
 */
pane?: boolean, };

/**
 * A machine that blocks can run on instead of this host: today a
 * throwaway wisp sprite (a Firecracker microVM) owned by one pane, and
 * deleted when that pane closes.
 */
export type Machine = { id: number, 
/**
 * Who runs it: `wisp`.
 */
provider: string, 
/**
 * The provider's name for it.
 */
sprite: string, 
/**
 * What to call it ("drifting cedar", M7): a display name for the
 * machines we make. The sprite keeps its own name.
 */
name?: string | null, image: string | null, 
/**
 * What it belongs to; the machine goes when that closes.
 */
owner: Owner, state: MachineState, 
/**
 * Someone else's sandbox, borrowed for a shell (M4b): never created or
 * deleted by us; closing its owner only ends our sessions on it.
 */
borrowed?: boolean, 
/**
 * Made for a guest (M14): their principal id, for their quota.
 */
by?: string | null, };

export type MachineState = "starting" | "running" | "gone";

/**
 * Why a member is the way it is, and who works on it (#618): read when
 * its card is opened, kept until the fingerprint moves.
 */
export type MemberWhy = { 
/**
 * The decisions covering it, most relevant first (the gate card's
 * read). None while reading, or when it couldn't be read.
 */
decisions: Array<DecisionRef> | null, 
/**
 * Commits that changed it while no decision constrained it.
 */
undecided: number | null, 
/**
 * Why the decisions couldn't be read.
 */
note: string | null, 
/**
 * Its recent agent runs, newest first: those of the agent sessions the
 * declaration binds to it, and those that made commits in it.
 */
runs: Array<WorkspaceRun>, 
/**
 * Why the runs couldn't be read.
 */
runs_note: string | null, 
/**
 * How long chant's intent read took.
 */
ms: number | null, };

export type NativeIssue = { 
/**
 * `iss_` and 32 hex characters.
 */
id: string, project: string, 
/**
 * Its number in the project.
 */
number: number, title: string, body: string, 
/**
 * `open` or `closed`.
 */
state: string, 
/**
 * A closed one's: `completed`, `not_planned` or `duplicate`.
 */
close_reason: string | null, 
/**
 * The account that opened it (an agent's owner's), or "" once it's
 * deleted.
 */
author: string, 
/**
 * The agent that opened it for its owner.
 */
agent: ChatAgent | null, labels: Array<string>, 
/**
 * Accounts.
 */
assignees: Array<string>, github: IssueGithub | null, created_at: number, updated_at: number, closed_at: number | null, 
/**
 * Who closed a closed one (an agent's owner's account), as its latest
 * `closed` event says: a list's row names them (UI audit #163).
 */
closed_by?: string, 
/**
 * The agent that closed it for its owner.
 */
closed_by_agent?: ChatAgent, };

export type NewBoard = { title: string, };

/**
 * A new issue, before and after it reached the forge.
 */
export type NewIssue = { title: string, body: string, 
/**
 * Who asked for it: a person, `mcp:<client>`, or `an agent`.
 */
by: string, 
/**
 * An agent's: a draft, until a person sends it.
 */
agent: boolean, at_ms: number, status: DraftStatus, settled_by?: string, url?: string, 
/**
 * The last try failed: why.
 */
error?: string, 
/**
 * The agent's pane, where *Ask for changes* sends a person's note.
 */
pane?: number, 
/**
 * The last note asking the agent for changes: it's revising until it
 * drafts again (`revised_ms`).
 */
asked?: ChangesAsked, };

export type NewNativeIssue = { title: string, body: string, labels: Array<string>, 
/**
 * Accounts, each a member of the project.
 */
assignees: Array<string>, };

export type Node = { "type": "pane", pane: number, } | { "type": "split", id: number, dir: Dir, children: Array<Child>, };

/**
 * How much of a room reaches the reader's pushes, and how its unread shows
 * (#113).
 */
export type NotifyLevel = "all" | "mentions" | "nothing";

/**
 * What "needs you" notifications someone other than the owner gets (M29):
 * agents in these sessions, or everything they may edit here ("this team's
 * agents" on a team daemon). The owner always is.
 */
export type NotifyPref = { all: boolean, sessions: Array<number>, };

/**
 * `GET /api/notify/prefs`: the account's notification settings.
 */
export type NotifyPrefs = { 
/**
 * Levels for rooms with none of their own (#113).
 */
channels: NotifyLevel, dms: NotifyLevel, 
/**
 * Replies in the threads the person follows (#106).
 */
threads: NotifyLevel, 
/**
 * Snoozed until (ms since the epoch), while that's ahead.
 */
snooze_until: number | null, 
/**
 * IANA name; the schedule is read in it.
 */
tz: string, quiet: Array<QuietHours>, 
/**
 * Do not disturb holds back every push, not only chat's.
 */
all_notices: boolean, 
/**
 * Do not disturb is on now: snoozed, or inside quiet hours.
 */
paused: boolean, };

/**
 * `POST /api/notify`: opt in or out of a session's agents, or all of them.
 */
export type NotifyRequest = { 
/**
 * One session; none: everything you may edit here.
 */
session?: number, on: boolean, };

/**
 * `POST /api/notify/prefs`: what's given changes, the rest stays.
 */
export type NotifySet = { channels?: NotifyLevel, dms?: NotifyLevel, threads?: NotifyLevel, 
/**
 * Snooze until this time (ms); 0 ends a snooze.
 */
snooze_until?: number, tz?: string, quiet?: Array<QuietHours>, all_notices?: boolean, };

/**
 * `POST /api/conversations/open` (M33): a Claude Code conversation as an
 * agent block.
 */
export type OpenConversationRequest = { 
/**
 * Its id, or a unique prefix.
 */
id: string, 
/**
 * Then `continue` or `fork` it.
 */
then?: string | null, session?: string | null, split?: number | null, from_pane?: number | null, };

/**
 * What opening a conversation answers: its block (`opened`: made now, not
 * there already), and why `then` didn't go through, if it didn't.
 */
export type OpenConversationResponse = { block: number, opened: boolean, conversation: string, error?: string, };

/**
 * `POST /api/blocks`: open a block of any type.
 */
export type OpenRequest = { type: BlockType, 
/**
 * What the type needs to make it (a URL, an agent command).
 */
config?: Record<string, unknown>, 
/**
 * Session name or id, as for `run`.
 */
session?: string | null, 
/**
 * Split this block instead of opening a tab.
 */
split?: number | null, from_pane?: number | null, 
/**
 * Run it on a new throwaway machine of its own.
 */
vm?: boolean | null, 
/**
 * The new machine's image.
 */
image?: string | null, 
/**
 * Run it on this machine [default: the tab's, when splitting in a VM
 * tab; else this host].
 */
host?: number | null, 
/**
 * On this host, even split in a VM tab.
 */
local?: boolean | null, };

/**
 * What opening a block answers.
 */
export type OpenResponse = { block: number, };

/**
 * Where an option lives, as in tmux: the server, a session, a window (tab)
 * or a pane.
 */
export type OptionScope = { "kind": "global" } | { "kind": "session", "id": number } | { "kind": "tab", "id": number } | { "kind": "pane", "id": number };

/**
 * Opaque named strings that clients keep with the layout (tmux's `@user`
 * options: iTerm2's tab grouping and attach guard, `@affinities`). Saved
 * with the layout; an entry goes when what it belongs to does. Only the
 * tmux front end set them, and it was removed (DECISIONS.md "The TUI and
 * tmux -CC are removed"); they stay in the protocol and the saved layout so
 * a layout and an older client still load, and nothing reads them.
 */
export type Options = { global?: { [key in string]: string }, sessions?: Array<[number, { [key in string]: string }]>, tabs?: Array<[number, { [key in string]: string }]>, panes?: Array<[number, { [key in string]: string }]>, };

/**
 * A machine's owner: one pane (M3b), or a tab whose panes share it (M3c).
 * JSON `{"pane": 3}` or `{"tab": 2}`; a bare number (M3b's layout.json) is
 * a pane.
 */
export type Owner = { "pane": number } | { "tab": number };

export type PaneInfo = { id: number, 
/**
 * Identifies this pane's output stream. Offsets are only meaningful
 * within one epoch; a client holding an offset from another epoch (an
 * earlier daemon) must attach with `None`.
 */
epoch: number, 
/**
 * The pane process's working directory, when known.
 */
cwd: string | null, 
/**
 * The foreground command, when it isn't the shell itself.
 */
command: string | null, 
/**
 * Whether a process is running (false while a restored pane waits for
 * Enter).
 */
running: boolean, policy: Policy, 
/**
 * Running now, per the shell integration.
 */
current: CommandInfo | null, 
/**
 * The last command that finished.
 */
last: CommandInfo | null, attention: Attention, 
/**
 * Why it wants you (M24), when it does.
 */
reason?: Reason | null, 
/**
 * Shell integration for shells started in this pane.
 */
integration: boolean, type: BlockType, 
/**
 * The machine it runs on; `None` is this host.
 */
host: number | null, 
/**
 * A question open in a terminal (Claude Code's AskUserQuestion, through
 * its hook), drawn as a card beside it (M6c).
 */
ask?: import("./ask").Ask | null, 
/**
 * Who answered its last question or approval, and how (M29), until
 * it asks again. For agent blocks too.
 */
answered?: import("./ask").Answered | null, 
/**
 * An edit Claude Code in this terminal proposes, waiting as a diff
 * (M28: arugulad as its IDE).
 */
diff?: DiffInfo | null, 
/**
 * Claude Code in this terminal is connected to arugulad as its IDE
 * (M28): lines can be mentioned to it from a followed editor.
 */
claude_ide?: boolean, 
/**
 * Claude Code in this terminal waits for a follow-up (its `arugula
 * inbox` hook, M29): one sent now goes straight in.
 */
inbox?: boolean, 
/**
 * Who is driving it (M13): only their typing reaches it, unless it's
 * in pair mode. `None`: nobody yet (the next to type drives).
 */
driver?: Driver | null, 
/**
 * Its driver typed in it in the last few seconds (#118).
 */
typing?: boolean, 
/**
 * Pair mode: every editor types at once.
 */
pair?: boolean, 
/**
 * Never shown to anyone but the owner (M14).
 */
private?: boolean, 
/**
 * Guests trusted to drive this pane, though it runs on the owner's
 * machine (M14): principal id and until when (ms).
 */
trusted?: Array<[string, number]>, 
/**
 * What it's busy with (M23); `None` for blocks other than terminals
 * and agents.
 */
kind?: WorkKind | null, 
/**
 * What a restart resumes (#146): the agent conversation running in
 * it, as "Claude Code conversation <title>", when its policy says to.
 */
resumes?: string | null, 
/**
 * The git repository it works in, if any (M23).
 */
project?: Project | null, 
/**
 * Its repository's checked-out branch, or a short commit when HEAD is
 * detached (M109).
 */
branch?: string | null, 
/**
 * Output rate (M23).
 */
activity?: Activity | null, 
/**
 * The title its program set (OSC 0/2), if any.
 */
title?: string | null, 
/**
 * The file an editor block shows (M27), relative to its folder.
 */
file?: string | null, 
/**
 * What started it, when that wasn't you: an MCP client (M16).
 */
started_by?: StartedBy | null, 
/**
 * An editor's own report (M28): an editor block's, or someone's
 * editor elsewhere (VS Code, Cursor, nvim) that joined the swarm.
 */
editor?: EditorInfo | null, };

export type PaneOp = { "op": "set_policy", policy: Policy, } | { "op": "purge" } | { "op": "set_integration", on: boolean, } | { "op": "attention", state: Attention, } | { "op": "take_control" } | { "op": "request_control" } | { "op": "give_control", to: string, } | { "op": "release_control" } | { "op": "set_pair", on: boolean, } | { "op": "request_trust" } | { "op": "grant_trust", to: string, minutes: number, } | { "op": "revoke_trust", to: string, } | { "op": "set_private", on: boolean, };

/**
 * A pinned file against the tree read (chant's asset `state`).
 */
export type PinState = "pinned" | "drifted" | "missing" | "stale";

/**
 * The signed-in person, as the hub knows them.
 */
export type PlatformMe = { 
/**
 * Control's id for the account.
 */
account: string, 
/**
 * Its sign-in login (GitHub's), or "".
 */
login: string, 
/**
 * The name it chose, else its login, else "".
 */
name: string, };

export type PlatformProject = { 
/**
 * `prj_` and 32 hex characters.
 */
id: string, name: string, 
/**
 * The team it's the default project of; `None` for the person's own.
 */
team: ProjectTeam | null, };

export type PlatformProjects = { me: PlatformMe, projects: Array<PlatformProject>, 
/**
 * Hosted workspaces are open to this person (M179's allowlist): only
 * then does the shell offer to delegate a project to control (M178).
 * A platform from before leaves it out.
 */
hosted: boolean, };

/**
 * One answer a decision point's question takes (#621), from `points
 * --open`: what `points answer --answer` is given, how to show it (`yes`
 * and `no` for a yes-or-no point) and what the points file says it means.
 */
export type PointChoice = { value: string, label: string, means: string | null, };

/**
 * What a pane does when the daemon restores it. Its scrollback always comes
 * back; this decides what runs in it.
 */
export type Policy = { "kind": "none" } | { "kind": "shell" } | { "kind": "rerun", confirm: boolean, } | { "kind": "hook", command: string, } | { "kind": "resume" };

/**
 * `GET /api/panes/N/ports` (#150): what listens on the machine a port
 * would open on, the pane's own first, then by number.
 */
export type Ports = { ports: Array<ListeningPort>, };

/**
 * A pull request, read whole.
 */
export type Pr = { item: Item, reviews: Array<Review>, checks: Array<Check>, rollup: CheckState | null, 
/**
 * The newest events, oldest first.
 */
events: Array<Event>, };

/**
 * Someone looking at the daemon (M13): one per connected client.
 */
export type Presence = { client: number, 
/**
 * Principal id: one person's clients share it.
 */
who: string, name: string, pic?: string, 
/**
 * The tab it shows, and the pane it's focused on.
 */
tab?: number, pane?: number, };

/**
 * What a person chose for their own presence: `auto` (as their pages
 * say), `away` (shown away while a page is open) or `offline` (shown
 * offline always).
 */
export type PresenceMode = "auto" | "away" | "offline";

/**
 * How a person's presence shows to the teammates who see it: `active` (a
 * page of theirs is open and was used in the last 10 minutes), `away`
 * (open but idle, or set by hand) or `offline` (no page, or they chose to
 * appear so).
 */
export type PresenceState = "active" | "away" | "offline";

/**
 * A whole-tab arrangement for [`Intent::Arrange`].
 */
export type Preset = "columns" | "grid" | "main_stack";

/**
 * The git repository a pane's working directory is in (M23).
 */
export type Project = { 
/**
 * The repository's top directory.
 */
root: string, 
/**
 * Its name: the repository its origin names, else its folder's.
 */
name: string, };

export type ProjectBoard = { id: string, project: string, title: string, 
/**
 * The account that made it; empty once that account is deleted.
 */
created_by: string, created_at: number, updated_at: number, };

export type ProjectMember = { account: string, 
/**
 * What the roster calls them.
 */
name: string, role: string, };

export type ProjectTeam = { id: string, name: string, 
/**
 * The reader's role in it: `owner`, `editor` or `viewer`.
 */
role: string, members: Array<ProjectMember>, };

/**
 * Which forge.
 */
export type Provider = "forgejo" | "github" | "gitlab";

/**
 * Quiet hours on some weekdays: from `from` to `to`, in minutes after
 * midnight in the account's time zone. A window that ends at or before it
 * starts runs past midnight, into the next day (its weekday is the day it
 * starts on).
 */
export type QuietHours = { 
/**
 * ISO weekdays: 1 Monday … 7 Sunday.
 */
days: Array<number>, from: number, to: number, };

/**
 * GitHub's rate limit as last heard, and any backing off (M38).
 */
export type RateLimit = { limit: number | null, remaining: number | null, used: number | null, 
/**
 * When the window resets (epoch seconds).
 */
reset: number | null, requests: number, not_modified: number, 
/**
 * Why it's polling slowly, if it is.
 */
backoff: string | null, };

/**
 * Why a pane wants you (M24): what happened, not just "needs you", so a
 * client can explain it, bundle it with others and act on it. Every
 * `needs_input` and `done` pane has one.
 */
export type Reason = { kind: ReasonKind, 
/**
 * When it started wanting you.
 */
since_ms: number, 
/**
 * One line: the question, the command that failed, what finished.
 */
headline: string, command?: string, exit?: number, 
/**
 * `done` and `failed`: how long the command ran.
 */
duration_ms?: number, 
/**
 * Reasons with the same key are one card on a "needs you" rail ("11
 * failed on build-03"): `failed:<machine>`, `exited:<machine>`,
 * `ask:<project>:<agent>`. `None` never bundles.
 */
bundle?: string, 
/**
 * `ask`: what is asked, and how to answer it.
 */
ask?: AskRef, 
/**
 * `gate`: the gate that waits (the first, if several do), which
 * `allow` approves (M34).
 */
gate?: Gate, 
/**
 * `diff`: the edit that waits (#887), so a card away from the pane (the
 * attention drawer's) can show it and accept it by its id. Boxed: a reason
 * rides in every attention event. A pane's own info leaves it out (its
 * `diff` has it).
 */
diff?: DiffInfo, 
/**
 * What [`api::ActRequest`] can do about it here.
 */
actions: Array<Action>, };

export type ReasonKind = "ask" | "input" | "failed" | "exited" | "done" | "paused" | "errors" | "conflict" | "diff" | "gate";

/**
 * An option chosen or turned down.
 */
export type RecordChoice = { option: string, 
/**
 * The option's label, from the record's options.
 */
label: string | null, 
/**
 * Why it was chosen, or turned down.
 */
why: string | null, };

/**
 * One entry of a record's evidence.
 */
export type RecordEvidence = { title: string | null, 
/**
 * A link.
 */
url?: string, 
/**
 * A workspace file pinned by the hash of its bytes, and how it stands
 * now.
 */
path?: string, pin?: PinState, };

export type Rect = { x: number, y: number, cols: number, rows: number, };

/**
 * Where a remote block's pane lives (#17): a host in the home daemon's
 * list, and the pane's id there.
 */
export type RemoteRef = { host: string, pane: number, };

/**
 * `PATCH /api/projects/{project}/repos/{owner}/{name}` (M166): a linked
 * repo's settings; the answer is its [`LinkedRepo`]. `mirror_content`
 * on keeps its PRs' and issues' bodies and comments on the platform
 * (search finds their text); off deletes them at once. Turning it on
 * takes what linking takes: a member who maintains the repo on GitHub.
 */
export type RepoSettings = { mirror_content: boolean, };

/**
 * What the failed checks' rerun is: through the API (`api`: GitLab, as
 * `rerun_checks`), or on a forge with none (or no login), the run's page.
 */
export type Rerun = { api: boolean, url: string | null, note: string, 
/**
 * GitHub: how many workflow runs it reruns.
 */
runs?: number, 
/**
 * GitLab: the pipeline's id, `null` when no check says. Absent
 * (outer `None`) for the others.
 */
pipeline?: string | null, };

export type Review = { id: string, author: string | null, state: ReviewState, commit: string | null, 
/**
 * Made on an older head.
 */
stale: boolean, at: number | null, body: string | null, url: string | null, };

export type ReviewEvent = "approve" | "request_changes" | "comment";

export type ReviewState = "approved" | "changes_requested" | "commented" | "dismissed" | "pending";

/**
 * Who a review request is for: a person, or a team (`Org/Name`, or just
 * `Name` when the forge doesn't say).
 */
export type Reviewer = { "user": string } | { "team": string };

/**
 * Ordered: an owner can do anything an editor can, and so on.
 */
export type Role = "viewer" | "editor" | "owner";

/**
 * Something a run left behind (M213).
 */
export type RunArtifactView = { name: string, size: number, digest: string, created_at: number, };

/**
 * The deploy a deploy's run applies (M219, UI audit #145): which, to
 * which environment, of what.
 */
export type RunDeployView = { 
/**
 * `dep_…`.
 */
id: string, 
/**
 * Its environment's name.
 */
environment: string, 
/**
 * `commit` or `artifact`.
 */
source_kind: string, 
/**
 * The commit's sha or the artifact's digest.
 */
reference: string, };

/**
 * One change of a run's state.
 */
export type RunEventView = { seq: number, from: string | null, to: string, at: number, 
/**
 * What its reporter said (an exit code, a reason, the pane).
 */
detail: Record<string, unknown>, 
/**
 * The machine that reported it, signed; none when control did.
 */
daemon: string | null, };

/**
 * [`PROJECT_RUNS`]' answer.
 */
export type RunList = { project: string, runs: Array<RunView>, };

/**
 * A page of a run's durable log (M213: hosted workspaces only, kept 30
 * days).
 */
export type RunLogPage = { run: string, 
/**
 * Whether the platform keeps a log for it. Until it does, the run's
 * pane is where its output is.
 */
kept: boolean, text: string, offset: number, next_offset: number, 
/**
 * The run has ended and this page reaches the end of its log.
 */
end: boolean, 
/**
 * Why there's no log, or what to do for the rest.
 */
note: string | null, };

/**
 * What a rerun would act on: an Actions run (its number in the repo) and
 * job.
 */
export type RunRef = { id: string, job: string | null, };

/**
 * `POST /api/run`: a new terminal pane.
 */
export type RunRequest = { 
/**
 * Run with the pane's shell (`$SHELL -l -c COMMAND`); none for just a
 * shell.
 */
command?: string | null, 
/**
 * Run it on a new throwaway machine owned by the pane.
 */
vm?: boolean | null, 
/**
 * In a new tab whose panes all share a new throwaway machine.
 */
vm_tab?: boolean | null, 
/**
 * The machine's image (the provider's default if none).
 */
image?: string | null, 
/**
 * On a sandbox that already exists (the provider's name for it), over
 * a plain exec with no daemon there ("open shell", M4b). The sandbox
 * isn't ours: closing the pane leaves it be.
 */
sandbox?: string | null, 
/**
 * Session name or id; created if no session has that name. Default: the
 * session of `from_pane`, else the first.
 */
session?: string | null, 
/**
 * Split this pane instead of opening a tab.
 */
split?: number | null, 
/**
 * With `split`: the new pane runs where the split pane does (its tab's
 * machine, or the sandbox it has a shell on) instead of this host.
 */
join?: boolean | null, 
/**
 * Where it starts. On a machine, a directory there.
 */
cwd?: string | null, policy?: Policy | null, 
/**
 * Where the request comes from (`$ARUGULA_PANE`): the default session
 * and working directory.
 */
from_pane?: number | null, };

export type RunResponse = { pane: number, };

/**
 * A run, as the CLI and a machine's agents read it.
 */
export type RunView = { id: string, project: string, 
/**
 * The hosted workspace (`hw_…`) it runs on.
 */
workspace: string | null, 
/**
 * `manual`, `schedule` or `event`.
 */
trigger: string, 
/**
 * Which spec, schedule or event: what an event's payload said is
 * untrusted text, for showing.
 */
source: Record<string, unknown>, 
/**
 * The S37 operation it runs (`arugula op v1`), and its SHA-256.
 */
op: string, op_hash: string, 
/**
 * The machine it runs on.
 */
daemon: string, 
/**
 * S37's: `pending`, `approved`, `running`, `succeeded`, `failed`,
 * `denied`, `canceled` or `expired`.
 */
state: string, 
/**
 * The account that started it (an agent's owner's), or the schedule's
 * or rule's id.
 */
started_by: string, 
/**
 * The machine whose agent started it.
 */
started_by_agent: string | null, 
/**
 * The standing grant it runs under; without one it waits for an
 * approval.
 */
grant: string | null, last_seq: number, created_at: number, started_at: number | null, ended_at: number | null, exit_code: number | null, 
/**
 * A release's entry in chant's ledger (M220, aru-098): a link to its
 * line on GitHub, which the platform checked is one.
 */
chant_ledger?: string | null, 
/**
 * The pane it runs in on its machine (its `running` event says): its
 * live view, which the project's people watch end to end.
 */
pane: number | null, 
/**
 * Its hosted workspace's name, deleted or not (UI audit #145): pages
 * show it, with the `hw_…` id beside it.
 */
workspace_name?: string, 
/**
 * The deploy a deploy's run applies (M219), as pages name it.
 */
deploy?: RunDeployView, 
/**
 * Its timeline, oldest first (left out of a list).
 */
events?: Array<RunEventView>, artifacts?: Array<RunArtifactView>, };

export type SendDraft = { 
/**
 * `browser` or `machine` (the default).
 */
via: string | null, 
/**
 * For `browser`: the platform page's path to come back to.
 */
back: string | null, };

/**
 * Control messages from the server.
 */
export type ServerMsg = { "type": "hello", version: string, client: number, state: State, } | { "type": "state", state: State, } | { "type": "size", pane: number, cols: number, rows: number, } | { "type": "resync", pane: number, } | { "type": "error", id: number | null, message: string, } | { "type": "block", block: number, state: unknown, } | { "type": "pong", id: number, } | { "type": "notice", message: string, } | { "type": "control_request", pane: number, who: string, name: string, } | { "type": "trust_request", pane: number, who: string, name: string, } | { "type": "delta", delta: Delta, } | { "type": "follow", pane: number, msg: FollowMsg, } | { "type": "hand_call", id: number, tool: string, args: Record<string, unknown>, from: string, };

export type Session = { id: number, name: string, tabs: Array<number>, };

/**
 * [`SESSION_ACTIONS`]'s body: what to ask of the session's box.
 */
export type SessionAct = { 
/**
 * `answer`, `approve`, `deny` or `cancel`.
 */
kind: string, 
/**
 * The open request it answers (the card's `open.request`).
 */
request?: string, 
/**
 * `answer`'s: the question card's answer (`question_<n>`).
 */
answer?: Record<string, unknown>, 
/**
 * `approve`'s: one of the permission's options.
 */
option?: string, 
/**
 * `deny`'s: why.
 */
reason?: string, };

/**
 * An answer to what's open, on its way to the box.
 */
export type SessionAnswered = { by: string, name: string, 
/**
 * `answer`, `approve` or `deny`.
 */
kind: string, at: number, };

/**
 * A session as its card shows it.
 */
export type SessionCard = { session: string, project: string, 
/**
 * Its thread: the room and the message it hangs off.
 */
room: string, parent: number, 
/**
 * `queued`, `working`, `needs_input`, `done`, `failed`,
 * `unresponsive` or `stopped`.
 */
state: string, 
/**
 * An `unresponsive` one's state before its box went quiet.
 */
was: string | null, 
/**
 * Who asked, and their name now ("" once they're gone).
 */
asker: string, asker_name: string, 
/**
 * The hosted workspace it runs on, and its name ("" once it's gone).
 */
workspace: string, workspace_name: string, repo: string | null, branch: string, created_at: number, updated_at: number, 
/**
 * When the platform last heard from its box.
 */
heard_at: number | null, 
/**
 * When it last stopped running (done, failed, stopped): the clock
 * stops there. `None` while a turn is under way or queued.
 */
ended_at: number | null, last_tool: string | null, last_error: string | null, 
/**
 * Why it failed or stopped, or why it's queued again (its box was
 * rebuilt).
 */
reason: string | null, 
/**
 * What its latest turn left.
 */
turn: SessionTurn | null, 
/**
 * What it asks, while it waits on a person.
 */
open: SessionOpen | null, 
/**
 * The answer to it that its box hasn't taken yet.
 */
answered: SessionAnswered | null, 
/**
 * Someone asked its box to cancel the turn, and it hasn't yet.
 */
cancelling: boolean, 
/**
 * Its box and agent block there, once the box took it: Open goes there.
 */
machine: string | null, block: string | null, 
/**
 * Its workspace's dev servers (Preview), the public ones with their
 * URL.
 */
ports: Array<SessionPort>, 
/**
 * The PR its asker sent (M259): its URL.
 */
pr: string | null, 
/**
 * When its PR merged (M265): the thread is done, and the session ends
 * once a turn under way does. Its card says "merged ✓".
 */
merged_at: number | null, 
/**
 * A turn pushed its branch: its asker may draft its PR.
 */
pushed: boolean, 
/**
 * How many CI rounds went back to it (M260), of at most `ci_max`.
 */
ci_rounds: number, ci_max: number, 
/**
 * CI failed again after its last round, and nobody in its thread has
 * prompted it since: it needs a person.
 */
needs_person: boolean, 
/**
 * What it is (M264): `thread` (the thread's own, `@arugula`), `try` (a
 * Try 3's sibling of `parent_session`) or `review` (`@codex review`
 * of `parent_session`).
 */
role: string, parent_session: string | null, 
/**
 * The agent asked for (`claude`, `codex`), and the one its box ran
 * it on, once it took it (`claude` when the other isn't there).
 */
harness: string, ran: string | null, 
/**
 * A Try 3's sessions, on its parent's card: the parent first, then its
 * siblings, oldest first. Empty for every other session.
 */
tries: Array<SessionTry>, 
/**
 * The one of [`SessionCard::tries`] kept, once one is picked; on a
 * sibling, its parent's pick.
 */
picked: string | null, };

/**
 * A turn's finish step (M261): the command, whether it passed, and when
 * it failed, the last of its output. A failed one isn't pushed.
 */
export type SessionFinish = { ok: boolean, command: string, output_tail: string, };

/**
 * One entry of a session's history, in words.
 */
export type SessionLine = { seq: number, 
/**
 * `state`, `error` or `turn`.
 */
kind: string, from: string | null, to: string, at: number, 
/**
 * Its box reported it, signed (else control recorded it).
 */
signed: boolean, 
/**
 * What it says, in a line.
 */
text: string, };

/**
 * What a session waits on a person for.
 */
export type SessionOpen = { 
/**
 * The request's id, as its box named it.
 */
request: string | null, 
/**
 * `permission` or `question` (`None`: the box didn't say).
 */
ask: string | null, 
/**
 * The words shown.
 */
question: string | null, 
/**
 * A permission's option ids.
 */
options: Array<string>, at: number, };

/**
 * [`SESSION_PICK`]'s body: the session kept, one of the Try 3's.
 */
export type SessionPick = { session: string, };

/**
 * One of a workspace's dev servers.
 */
export type SessionPort = { port: number, label: string, 
/**
 * Its public preview's URL, while it's public.
 */
url: string | null, };

/**
 * A session's pull request, as its asker drafts it ([`SESSION_PR`]).
 */
export type SessionPr = { session: string, 
/**
 * `pdr_…`, once it's kept.
 */
id: string | null, 
/**
 * `owner/name`.
 */
repo: string, 
/**
 * The session's branch, which the PR is from.
 */
head: string, 
/**
 * The branch it goes into: the repo's default, unless its asker says.
 */
base: string, title: string, body: string, 
/**
 * Opened as a draft PR on GitHub.
 */
draft: boolean, 
/**
 * `new` (not kept yet: prefilled), `waiting`, `sending`, `sent` or
 * `failed`.
 */
state: string, 
/**
 * Why the last send didn't get there.
 */
error: string | null, 
/**
 * The PR, once sent.
 */
url: string | null, number: number | null, 
/**
 * The repo's PR template the body started from, by its path.
 */
template: string | null, };

/**
 * [`SESSION_PR`]'s `PUT` body: what its asker wrote.
 */
export type SessionPrEdit = { title: string, body: string, draft?: boolean, base?: string, };

/**
 * [`SESSION_PR_SEND`]'s body: what to send (kept first, when given), and
 * how: `browser` (GitHub's authorization in this browser, coming back to
 * `back`, a path on the platform page) or `machine` (one of the asker's
 * own).
 */
export type SessionPrSend = { edit?: SessionPrEdit, via?: string, back?: string, };

/**
 * [`SESSION_PR_SEND`]'s answer: the PR draft now, and GitHub's
 * authorization page to go to when the send goes through the browser.
 */
export type SessionPrSent = { pr: SessionPr, authorize: string | null, 
/**
 * `token` or `machine`, once sent.
 */
via: string | null, 
/**
 * The machine that sent it.
 */
machine: string | null, };

/**
 * [`PROJECT_SESSION_STATS`]'s answer: of the project's sessions asked for
 * in the last [`STATS_DAYS`] days, how many led to a PR (sent by its
 * asker, or opened from its branch by hand) and how many merged, and the
 * median time from the `@` to the PR.
 */
export type SessionStats = { days: number, sessions: number, prs_opened: number, prs_merged: number, 
/**
 * `None` until one led to a PR.
 */
median_to_pr_ms: number | null, };

/**
 * One of a Try 3's sessions, as its parent's compare view shows it.
 */
export type SessionTry = { session: string, state: string, branch: string, harness: string, ran: string | null, 
/**
 * What its latest turn left: its diffstat and how its finish step went.
 */
turn: SessionTurn | null, pushed: boolean, 
/**
 * Why it failed or stopped.
 */
reason: string | null, updated_at: number, 
/**
 * Queued with no slot on its box for it yet (the box holds three
 * sessions, done ones too).
 */
no_slot: boolean, };

/**
 * [`SESSION_TRY`]'s body: the harnesses of the two siblings, `claude`
 * or `codex` each (`["claude", "codex"]` when it's left out; a box
 * without Codex runs Claude Code in its place).
 */
export type SessionTryAsk = { harnesses?: Array<string>, };

/**
 * What a session's turn left, as its box reported it.
 */
export type SessionTurn = { at: number, pushed: boolean, head: string | null, 
/**
 * `"SHA subject"`, newest first.
 */
commits: Array<string>, 
/**
 * `git diff --stat`, shortened.
 */
diffstat: string | null, uncommitted: number | null, 
/**
 * What the workspace's finish step did after it (M261), when it has
 * one.
 */
finish: SessionFinish | null, };

/**
 * [`SESSION`]'s answer.
 */
export type SessionView = { card: SessionCard, 
/**
 * The reader may stop it and answer it: an owner or editor of its
 * project.
 */
may: boolean, 
/**
 * The reader asked for it: only they send its PR.
 */
asked: boolean, 
/**
 * The platform's clock when it answered, so the page can tell a box
 * that went quiet whatever its own clock says.
 */
now: number, 
/**
 * Its history, oldest first.
 */
events: Array<SessionLine>, };

/**
 * A read-only share of one pane. `token`, `path` and `url` are only in the
 * answer that minted it; the daemon keeps a hash.
 */
export type Share = { id: number, pane: number, created_ms: number, expires_ms: number, token?: string, 
/**
 * `/share/<token>`, on this daemon.
 */
path?: string, 
/**
 * The whole link, on this daemon's tailnet name when it has one.
 */
url?: string, };

/**
 * `POST /api/shares`: a read-only link to one terminal pane.
 */
export type ShareRequest = { pane: number, 
/**
 * Seconds until it expires [default: an hour; at most a week].
 */
ttl_secs?: number | null, };

/**
 * A split's area and how long each child is along the split's direction,
 * so a client can turn a divider drag into new weights.
 */
export type SplitRect = { id: number, dir: Dir, rect: Rect, extents: Array<number>, };

/**
 * Who started a pane or block through MCP (M16).
 */
export type StartedBy = { 
/**
 * `mcp:<client>`, as the pane and history show it.
 */
by: string, 
/**
 * The agent block whose token it came with, if any: that block may
 * drive and close it.
 */
block?: number, };

/**
 * Everything a client needs to draw: sessions in order, each tab's tree
 * and the cell rectangles the server computed for it, and pane details.
 */
export type State = { rev: number, sessions: Array<Session>, tabs: Array<TabView>, panes: Array<PaneInfo>, 
/**
 * Machines that blocks run on, other than this host.
 */
machines: Array<Machine>, 
/**
 * Clients' named options (tmux `@` options), per scope: unused since
 * the tmux front end was removed, kept for older clients.
 */
options: Options, 
/**
 * For someone who isn't the daemon's owner (M12): their role in each
 * session they see, as `[[session, role], ...]` (JSON object keys
 * can't come back as numbers inside a tagged message). Absent for the
 * owner, who owns everything.
 */
roles?: Array<[number, Role]> | null, 
/**
 * Who else is here and where they're looking (M13), within what this
 * client sees.
 */
presence?: Array<Presence>, };

export type TabView = { id: number, name: string | null, root: Node, cols: number, rows: number, owner: number | null, zoom: number | null, 
/**
 * The pane filling the tab for everyone (focus mode, M109), if any.
 * `layout` already shows it.
 */
focus: number | null, layout: Layout, };

/**
 * `GET` and `POST /api/team-pins`: the teams pinned here, and those whose
 * rosters this machine checked.
 */
export type TeamPins = { pins: { [key in string]: string }, checked: Array<string>, };

/**
 * `POST /api/team-pins`: the teams the owner's browser pinned, and those
 * it left (#233; the owner's). Their rosters are checked against these.
 */
export type TeamPinsRequest = { 
/**
 * Team id to `<founder device>.<founder's root>`, as the owner's
 * browser pinned it.
 */
pins: { [key in string]: string }, 
/**
 * Teams pinned here that the owner's account is no longer in: their
 * members stop being nameable.
 */
drop: Array<string>, };

/**
 * `GET /api/telemetry` (the `telemetry.get` operation).
 */
export type TelemetryInfo = { on: boolean, why: TelemetryWhy, 
/**
 * When the owner answered (ms), if they have.
 */
asked?: number, via?: string, install?: string, 
/**
 * Where reports go.
 */
url: string, 
/**
 * Reports are signed as this machine: it's joined to the control they
 * go to.
 */
signed: boolean, 
/**
 * The next report, exactly as it would be sent now (an
 * `arugula_control_wire::telemetry::Report`). While it's off, what a
 * yes would send first: no counts, a placeholder for the install id,
 * and `"preview_only": true`. A daemon from M101 always gives one.
 */
preview?: unknown, };

/**
 * `POST /api/telemetry/note` (the `telemetry.note` operation, M103): what
 * the web client counted since its last note, for the day's counts. Views
 * by `arugula_control_wire::telemetry::View`'s names (one it doesn't know
 * counts as `other`), and how many errors our code hit. Counts only.
 */
export type TelemetryNoteRequest = { views: { [key in string]: number }, web_errors: number, };

/**
 * `PUT /api/telemetry` (the `telemetry.set` operation): the owner's answer,
 * and where they gave it (`web` when not said).
 */
export type TelemetrySetRequest = { on: boolean, via?: string, };

/**
 * Why telemetry is on or off here.
 */
export type TelemetryWhy = "yes" | "not_asked" | "no" | "do_not_track" | "env" | "ci" | "test";

/**
 * What a pane is busy with (M23), for drawing and grouping it without
 * attaching: from the foreground process's command line, else the
 * command the shell integration reported.
 */
export type WorkKind = "shell" | "build" | "test" | "agent" | "server" | "logs" | "editor" | "app" | "pr" | "issue" | "managoat";

/**
 * One current record of a kind the declaration names: a decision, a work
 * item, a session.
 */
export type WorkspaceRecord = { 
/**
 * The kind's name (`decision`, `work`).
 */
kind: string, id: string, title: string | null, state: string | null, 
/**
 * A work item: whether nothing blocks it.
 */
ready: boolean | null, blocked_by: Array<string>, 
/**
 * PinState drift and the like.
 */
warnings: Array<string>, valid: boolean, 
/**
 * What a decision answers.
 */
question?: string, 
/**
 * What it constrains, as the record writes it: `member:<name>`,
 * `path:<path>`.
 */
constrains: Array<string>, 
/**
 * The option chosen, and why. None while proposed.
 */
choice?: RecordChoice, 
/**
 * The options turned down, each with why.
 */
rejected: Array<RecordChoice>, 
/**
 * The records this one replaces, and the one that replaced it (chant
 * derives that from the other's link).
 */
supersedes: Array<string>, superseded_by?: string, 
/**
 * Records that fix this one's consequences without replacing it.
 */
remediated_by: Array<string>, 
/**
 * A work item: the decisions it carries out.
 */
implements: Array<string>, 
/**
 * Who decided, and when (a date); who proposed it.
 */
decided_by?: string, decided_on?: string, proposed_by?: string, 
/**
 * What it cites: links, and workspace files pinned by hash.
 */
evidence: Array<RecordEvidence>, 
/**
 * Whether its author's seal verifies: none when nothing here can say
 * (no seal and no signers file).
 */
attested?: boolean, 
/**
 * How far the commit behind it is trusted: `attested`,
 * `attested-unverifiable-here`, `adopted`, `unattested`.
 */
provenance?: string, 
/**
 * The record's file, from the repository root.
 */
path?: string, };

/**
 * An agent run, from `chant workspace runs` (#618). `WorkspaceRun` in
 * TypeScript, which has a `RunRef` already.
 */
export type WorkspaceRun = { 
/**
 * The `Chant-Run` trailer's value.
 */
id: string, 
/**
 * `running` or `ended`.
 */
state: string | null, 
/**
 * The agent session it ran as, and who it worked for.
 */
agent: string | null, by: string | null, started_at: string | null, ended_at: string | null, 
/**
 * How it ended: done, not_done, failed, cancelled.
 */
outcome: string | null, 
/**
 * The work item it worked on.
 */
unit: string | null, 
/**
 * The lease token it worked under.
 */
lease: string | null, 
/**
 * `<kind>/<id>` of each decision it carried out.
 */
decisions: Array<string>, 
/**
 * The Arugula agent pane that ran it (an Arugula run's id names its
 * block: `arugula-<pane>-<ms>`). The client links it while that pane
 * is open.
 */
pane: number | null, };

/** Each operation's method and path (`arugula_proto::op`), by name. */
export const OPS = {
  "acl.get": { method: "GET", path: "/api/acl" },
  "acl.set": { method: "POST", path: "/api/acl" },
  "adapter.install": { method: "POST", path: "/api/agents/adapters/{kind}/install" },
  "adapters.list": { method: "GET", path: "/api/agents/adapters" },
  "agents.get": { method: "GET", path: "/api/hosts/self/agents" },
  "agents.refresh": { method: "POST", path: "/api/hosts/self/agents/refresh" },
  "conversation.open": { method: "POST", path: "/api/conversations/open" },
  "conversations.list": { method: "GET", path: "/api/conversations" },
  "doc.delete": { method: "DELETE", path: "/api/docs/{id}" },
  "doc.open": { method: "POST", path: "/api/docs/open" },
  "doc_link.delete": { method: "DELETE", path: "/api/doc-links/{id}" },
  "doc_link.get": { method: "GET", path: "/api/doc-links/{id}" },
  "docs.list": { method: "GET", path: "/api/docs" },
  "flag.set": { method: "PUT", path: "/api/flags/{name}" },
  "flags.list": { method: "GET", path: "/api/flags" },
  "fs.list": { method: "GET", path: "/api/fs/list" },
  "fs.recent": { method: "GET", path: "/api/fs/recent" },
  "fs.stat": { method: "GET", path: "/api/fs/stat" },
  "guest.mint": { method: "POST", path: "/api/guests" },
  "guest.revoke": { method: "DELETE", path: "/api/guests/{id}" },
  "guests.list": { method: "GET", path: "/api/guests" },
  "host.add": { method: "POST", path: "/api/hosts" },
  "host.get": { method: "GET", path: "/api/host" },
  "host.remove": { method: "DELETE", path: "/api/hosts/{name}" },
  "host_token.mint": { method: "POST", path: "/api/hosts/{name}/token" },
  "host_token.revoke": { method: "DELETE", path: "/api/hosts/{name}/token" },
  "hosted_setup.get": { method: "GET", path: "/api/hosted/setup" },
  "hosts.list": { method: "GET", path: "/api/hosts" },
  "ide.get": { method: "GET", path: "/api/ide" },
  "ide.mention": { method: "POST", path: "/api/ide/mention" },
  "ide.set": { method: "PUT", path: "/api/ide" },
  "invite.send": { method: "POST", path: "/api/invite" },
  "link.mint": { method: "POST", path: "/api/links" },
  "machine.reset": { method: "POST", path: "/api/machines/{id}/reset" },
  "machines.list": { method: "GET", path: "/api/machines" },
  "mcp_token.mint": { method: "POST", path: "/api/mcp/tokens" },
  "mcp_token.revoke": { method: "DELETE", path: "/api/mcp/tokens/{name}" },
  "mcp_tokens.list": { method: "GET", path: "/api/mcp/tokens" },
  "notify.get": { method: "GET", path: "/api/notify" },
  "notify.set": { method: "POST", path: "/api/notify" },
  "pane.ask": { method: "POST", path: "/api/panes/{id}/ask" },
  "pane.ask_withdraw": { method: "POST", path: "/api/panes/{id}/ask/withdraw" },
  "pane.attention": { method: "POST", path: "/api/panes/{id}/attention" },
  "pane.cd": { method: "POST", path: "/api/panes/{id}/cd" },
  "pane.close": { method: "POST", path: "/api/panes/{id}/close" },
  "pane.detection": { method: "GET", path: "/api/panes/{id}/detection" },
  "pane.diff": { method: "GET", path: "/api/panes/{id}/diff" },
  "pane.drivers": { method: "GET", path: "/api/panes/{id}/drivers" },
  "pane.followup": { method: "POST", path: "/api/panes/{id}/followup" },
  "pane.inbox": { method: "POST", path: "/api/panes/{id}/inbox" },
  "pane.keys": { method: "POST", path: "/api/panes/{id}/keys" },
  "pane.mouse": { method: "POST", path: "/api/panes/{id}/mouse" },
  "pane.permit": { method: "POST", path: "/api/panes/{id}/permit" },
  "pane.ports": { method: "GET", path: "/api/panes/{id}/ports" },
  "pane.process": { method: "GET", path: "/api/panes/{id}/process" },
  "pane.prompt": { method: "POST", path: "/api/panes/{id}/prompt" },
  "pane.send": { method: "POST", path: "/api/panes/{id}/send" },
  "pane.wait": { method: "GET", path: "/api/panes/{id}/wait" },
  "panes.list": { method: "GET", path: "/api/panes" },
  "push.key": { method: "GET", path: "/api/push/key" },
  "push.subscribe": { method: "POST", path: "/api/push/subscribe" },
  "push.test": { method: "POST", path: "/api/push/test" },
  "rule.forget": { method: "DELETE", path: "/api/rules/{index}" },
  "rules.forget_all": { method: "DELETE", path: "/api/rules" },
  "rules.list": { method: "GET", path: "/api/rules" },
  "sandbox.demote": { method: "DELETE", path: "/api/sandboxes/{name}/resident" },
  "sandbox.promote": { method: "POST", path: "/api/sandboxes/{name}/promote" },
  "sandboxes.list": { method: "GET", path: "/api/sandboxes" },
  "setup.agent": { method: "POST", path: "/api/setup/agents/{kind}" },
  "setup.claude": { method: "POST", path: "/api/setup/claude" },
  "setup.control": { method: "POST", path: "/api/setup/control" },
  "setup.control_confirm": { method: "POST", path: "/api/setup/control/confirm" },
  "setup.get": { method: "GET", path: "/api/setup" },
  "setup.tailscale": { method: "POST", path: "/api/setup/tailscale" },
  "share.mint": { method: "POST", path: "/api/shares" },
  "share.revoke": { method: "DELETE", path: "/api/shares/{id}" },
  "shares.list": { method: "GET", path: "/api/shares" },
  "shell_env.get": { method: "GET", path: "/api/hosts/self/shell-env" },
  "shell_env.refresh": { method: "POST", path: "/api/hosts/self/shell-env/refresh" },
  "signin_link.get": { method: "GET", path: "/api/signin-link" },
  "studio.apps": { method: "GET", path: "/api/studio/apps" },
  "studio.follow": { method: "PUT", path: "/api/studio/followers/{app}" },
  "studio.get": { method: "GET", path: "/api/studio" },
  "studio.login": { method: "POST", path: "/api/studio" },
  "studio.logout": { method: "DELETE", path: "/api/studio" },
  "studio.unfollow": { method: "DELETE", path: "/api/studio/followers/{app}" },
  "synced.forget": { method: "DELETE", path: "/api/synced/{host}" },
  "synced.list": { method: "GET", path: "/api/synced" },
  "synced.rotate": { method: "POST", path: "/api/synced/rotate-key" },
  "team_pins.get": { method: "GET", path: "/api/team-pins" },
  "team_pins.set": { method: "POST", path: "/api/team-pins" },
  "telemetry.get": { method: "GET", path: "/api/telemetry" },
  "telemetry.note": { method: "POST", path: "/api/telemetry/note" },
  "telemetry.set": { method: "PUT", path: "/api/telemetry" },
} as const;

export type OpName = keyof typeof OPS;

/** The Labs flags (`arugula_proto::flags::FLAGS`), in order. */
export const FLAGS = [
  { name: "chat", title: "Chat", about: "The chat page, which needs control. Brings this machine's old threads into it.", default: false, local_only: false },
  { name: "vms", title: "VMs and sandboxes", about: "VM tabs and panes, machines and the sandboxes page. Needs a sandbox provider.", default: false, local_only: false },
  { name: "studio", title: "Studio apps", about: "Studio app blocks. Needs a linked studio.", default: false, local_only: false },
  { name: "workspaces", title: "Chant workspaces", about: "Chant workspace blocks.", default: false, local_only: false },
  { name: "guest-ssh", title: "Guest ssh", about: "Invite a guest to a pane over plain ssh.", default: false, local_only: true },
  { name: "swarm-themes", title: "Extra swarm views", about: "The swarm's other views beside the default.", default: false, local_only: false },
  { name: "forges", title: "Forgejo and GitLab", about: "PR and issue blocks for Forgejo and GitLab, beside GitHub's.", default: false, local_only: false },
  { name: "agents", title: "Agent catalog", about: "Offer Claude Code subagents from this machine, and see the ones your team offers.", default: false, local_only: true },
  { name: "docs", title: "Docs", about: "Agents build documents in a block beside their pane.", default: false, local_only: false },
  { name: "boards", title: "Boards", about: "A project's live status board, which needs control.", default: false, local_only: false },
] as const;
