---
schema: 1
id: "aru-051"
title: "Chat lives in control, in plain text; terminal content doesn't"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Chat lives in control, in plain text; terminal content doesn't"
options:
  - id: "a"
    label: "Chat lives in control, in plain text; terminal content doesn't"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S41"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s41-chat-in-control/README.md"
  - title: "#1096"
    url: "https://github.com/arugula-salad/arugula/issues/1096"
  - title: "Talk track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#talk-track-s30-m61m64-added-2026-10-05"
  - title: "M61"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m61-threads-on-panes-and-sessions-240"
  - title: "S36"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s36-conversations/README.md"
  - title: "#855"
    url: "https://github.com/arugula-salad/arugula/issues/855"
  - title: "#856"
    url: "https://github.com/arugula-salad/arugula/issues/856"
  - title: "#1095"
    url: "https://github.com/arugula-salad/arugula/issues/1095"
decided_by: "jhgaylor"
decided_on: "2026-10-19"
reviews: []
constrains:
  - "path:crates/platform/src/chat.rs"
  - "path:crates/platform/src/db/chat.rs"
  - "path:crates/control-wire/src/chat.rs"
  - "path:crates/daemon/src/chat_import.rs"
  - "path:crates/mux/src/mux/chat_readers.rs"
  - "path:crates/daemon/src/control.rs"
---

# Chat lives in control, in plain text; terminal content doesn't

Chat is a team's, not a machine's (Jake, 2026-10-19, #978): control holds a
team's channels and DMs and the threads about a pane or a session, in plain
text, so people talk with every machine asleep. It's one case of the platform
plane's rule ("Two planes, two trust rules", #1122): plaintext for what has
to exist while every machine is asleep, listed in `crates/control/tables.toml`.
A team's channels belong to its default project ("The project is the unit
of platform sharing"); DMs and pane or session threads belong to no project.
Within control, chat is the platform process's ("Control is two processes: a
hub and a platform", #1150, M138); push stays the hub's, and the platform
asks it through `HubPort` (`push_with`, to every subscription the account
has, as chat's pushes always went). Who is in which team, which machine is
whose and who may reach it come from the hub too, kept by the platform and
dropped as the hub's change feed names them (kept for as long as the
feed is read): a roster, lock, machine, share or flag change reaches chat
within a feed tick plus a cache tick (`arugula_platform::cache::DELAY`,
11 s at most, about a second in practice, and 2 s at most while the feed
is read). Control numbers messages per
room, keeps read marks, resolves mentions against who sees the room, pushes
(who, where and the first 200 characters), searches with SQLite FTS5, and
tells open pages over one Server-Sent Events stream each (`Last-Event-ID`
resumes it). A message may name a pane and quote lines the poster chose
from it; those lines are copied into control, and nothing else of the pane
is. A pane's or session's thread is read by whom its machine last said
(`POST /api/daemon/chat/readers`, each with a "from now" floor), within
whom control routes to that machine at all; before the machine has said,
by its owner and its team's owners. A DM belongs to a team: one person
leaving makes it read-only for the other and gone for them. Messages are
kept until deleted: an author or an owner deletes one (its number stays,
empty); deleting an account deletes its DMs and its machines' threads and
blanks its messages elsewhere; a team that goes with its founder takes its
rooms. It's behind control's `chat` Labs flag, the account's or a team's.
Agents aren't in chat (M121); a machine with no control has no chat
(decision 7): since M125 no daemon keeps threads, and a page without
control's chat (a machine's own page, or control's flag off) shows a Chat
page that says chat needs control, when the machine's own `chat` flag is on.
Where: `crates/platform/src/chat.rs`, `crates/platform/src/db/chat.rs`, `crates/platform/src/cache.rs`, `crates/control-wire/src/chat.rs`.
From: [S41](spikes/s41-chat-in-control/README.md) (decisions 1–10, all as recommended), M122 (#1096).

The daemon kept its own pane and session threads (M61) and its M81
conversations in `<state>/threads/` until M125 took the store out (with its
API, `ServerMsg::Thread`, `State.threads`, thread hits in `/api/search`,
the thread ACL floors, its mention pushes and the invite from a thread,
#297). M124 brings them into control once: a daemon in control with `chat` on posts them signed as itself
(`POST /api/daemon/chat/import`), then moves `threads/` to
`threads.imported/` so a second start, or an older daemon after a
downgrade, has none. Old messages keep their numbers, or go before a room's
own (0 and below) when it was used in control first; control takes each
once by its old number, so a retry or a split import is safe, and they
count as read. An author with an account that sees the room stays theirs;
anyone else shows under the machine's owner by their old name. Conversations
become read-only `archive` rooms in the owner's account, read by their
members from when each joined (decision 3). After the import the daemon
keeps control's reader lists current. A session's viewer stays read-only
(Jake, 2026-10-19): their reader entry says `watching`, so the room isn't
writable for them and a post is refused (403, `watching`). A daemon from before says no
`chat-import` in its features, and control's page tells its owner "Update
to bring its threads into chat". The import stays after the store went
(decision 9): it reads the old files from disk itself, so a machine that
skips the release with the import still brings its threads in, and one not
in control leaves them on disk, unread. Where: `crates/daemon/src/chat_import.rs`,
`crates/mux/src/mux/chat_readers.rs`, `chat::daemon_import` in the platform.

A conversation (#855, until M125) was a third kind of thread beside them, not
a pane or session thread moved. It had a global id (`ConvId`, `c-` and 32
hex digits) and lived on its home machine, the one it was made on, never in
control: its record in `<state>/threads/conv-<id>.json` and its timeline in
`conv-<id>.jsonl` (which is what M124's import reads), with read marks in
the shared `reads.json`. The rest of this paragraph is what it was; in
control it's a read-only archive room. Its people are the record's, not a session's: the owner
(`Owner`) and members (`Editor`), each reading from when they joined unless
added with history. Only the machine's owners make one (Jake, 2026-10-13);
only its owner renames, deletes, adds and takes out; a member may leave.
Being a member is how someone reaches the home machine (Jake, 2026-10-13,
#856): a member who couldn't reach it before is added with their account's
root (from a roster the home checked, or the one the owner's browser
pinned), so the home lets their devices in and control routes them there,
as a share would, but for its conversations only: they have no role in its
sessions. Members see what's quoted in it; a pane itself, its terminal and
its own thread, stays with the people it's shared with. A conversation links
no panes and has no lead: agents are out of chat (#1095, below).
Where (until M125): `crates/mux/src/labs/threads.rs`, `crates/mux/src/labs/convs.rs`, `crates/mux/src/mux/thread_ops.rs`, `crates/daemon/src/ops/convs.rs`, `crates/daemon/src/control.rs` (`Acl::reach` in `others`).
From: [Talk track](docs/plan-archive.md#talk-track-s30-m61m64-added-2026-10-05), [M61](docs/plan-archive.md#m61-threads-on-panes-and-sessions-240), [S36](spikes/s36-conversations/README.md) q1 and q4, M81 (#855), M82 (#856), M121 (#1095).
