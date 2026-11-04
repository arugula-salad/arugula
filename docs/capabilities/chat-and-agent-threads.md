---
id: "chat-and-agent-threads"
title: "Chat and agent threads"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/platform-chat/src/chat.rs", "crates/platform-chat/src/chat/agent_threads.rs", "crates/arugulad-control-tools/src/threads.rs", "crates/daemon/src/agent_mentions.rs", "web/src/platform/pages/chat.tsx", "web/src/shell/blocks/team-chat.tsx", "crates/cli/src/cmd/chat.rs"]
paths: ["crates/platform-chat/src/chat", "crates/platform/src/chat.rs", "crates/platform/src/db/chat.rs", "crates/platform/src/db/chat_mention_status.rs", "crates/platform/src/db/chat_threads.rs", "crates/control-wire/src/chat.rs", "crates/daemon/src/chat_readers.rs", "crates/daemon/src/team_chat.rs", "crates/mux/src/mux/chat_readers.rs", "crates/cli/src/cmd/mentions.rs", "web/src/shared/chat", "web/src/shared/chat-store.ts"]
shared: ["crates/platform/src/grants.rs", "crates/platform-chat/src/lib.rs", "crates/daemon/src/forges.rs", "crates/daemon/src/control.rs", "crates/proto/src/sharing.rs", "crates/control/src/gateway.rs"]
tests: ["crates/control/src/chat_wire.rs", "crates/platform-chat/src/chat_rooms_tests.rs", "crates/platform-chat/src/chat_threads_tests.rs", "crates/platform-chat/src/agent_threads_tests.rs", "crates/platform-chat/src/chat_mention_status_tests.rs", "crates/platform-chat/src/chat_mention_redeliver_tests.rs", "crates/arugulad/tests/integration/mention_card.rs", "crates/arugulad/tests/integration/mention_held.rs", "crates/arugulad/tests/integration/mention_agent_block.rs", "crates/arugulad/tests/integration/chat_readers.rs", "web/e2e/chat-control.spec.ts", "web/e2e/chat-mention-status.spec.ts"]
decisions: ["aru-051", "aru-090", "aru-091", "aru-115"]
---
# Chat and agent threads

## What a person can do
A team has channels, DMs and reply threads on messages, and panes, sessions and objects such as an issue have threads too. People post, search and get pushed with every machine asleep. Agents read and post where their machine's owner granted it, and `@<machine>` reaches an agent on that machine.

Chat is on by control's setting (`chat_on`, flag name `chat`): the account's own if it set one, else on when any of its teams turned it on. Otherwise routes refuse with `chat_off`. No machine has a say in this.

## Where it runs
Short paths below are under `crates/platform-chat/src`, or `crates/daemon/src` where noted.

**The platform process owns chat**, in plain text; its core is in `crates/platform/src/chat.rs`. Tables include `chat_rooms`, `chat_members`, `chat_messages`, `chat_fts`, `chat_reads`, `chat_thread_reads`, `chat_mention_status` and `chat_readers` (all in `crates/control/tables.toml`). It reaches the hub only through `HubPort`, for rosters, machines, flags and push. `POST /api/chat/rooms/{id}/messages` goes `post`, `post_as`, `post_full`, `post_replied`, which checks the room (`room_for`), resolves mentions, stores the message under a per-room number, then pushes and announces. Open pages read `GET /api/chat/live`, one Server-Sent Events stream each. The shell's chat block (daemon's `team_chat.rs`) keeps only which room it's on: the page reads messages as the person looking, and the daemon never sees them.

**Who sees a room** (`sight`), with 404 for anyone else:
- A channel: its members who are in the team's latest roster. Public channels can be joined by anyone in the team, private ones by invitation. Owners and editors make channels (`new_channel`).
- A DM: its people, up to eight, while each is in the team (`new_dm`).
- A pane's or session's thread: whom its machine last said may read it (`daemon_readers`, sent by daemon's `chat_readers.rs`), and only while control would route them to that machine. The machine's owner always reads.
- An object's thread: its project's members, and for a private repo's item only those with their own GitHub access (`chat/objects.rs`).

**Reply threads** (`chat/threads.rs`) are one level deep. Replies stay out of the room's list unless also sent to it. A thread has its own followers, read mark and pushes.

**How an agent reads and posts:**
- `list` kind `threads`, `read_thread`, `search_threads` and `post_thread` (`crates/arugulad-control-tools/src/threads.rs`) call `/api/daemon/chat/...` signed by the machine (`chat/agent_threads.rs`). The daemon keeps no thread. An agent a guest stands behind may not use them (`owners_agent`).
- `agent_room` checks on every call that the owner still sees the room and that the machine holds `threads.read` or `threads.post` over its scope: the team's default project for a channel, the object's project for an object thread, the owner's account for a DM (`agent_scope`). Agents don't reach pane or session threads.
- What an agent posts has the owner as author, with the machine beside it. Every room a grant reaches says so (`room_agents`), and a new grant posts one line in the room (`chat/agent_notice.rs`).

**How a mention reaches an agent:**
1. `agent_mentions` matches an `@word` that named no person to a named machine whose live `threads.read` grant reaches the room, and stores `machine:<id>`. Nobody is pushed for it.
2. `plan_tells` also includes the machine of an agent's message being replied to, and on a board's thread the machine of the session that leads the board. It writes a `sending` status to `chat_mention_status`. An agent's post isn't told past a chain of posts to its own machine (`CHAIN_LIMIT`) or a per-thread limit (`AGENT_TOLD_MAX`), and those say `chain`.
3. `deliver` sends `DaemonOp::AgentMention` through `HubPort::daemon_op`, after the post has returned.
4. On the machine, daemon's `forges.rs` calls `agent_mentions::deliver`. A mention naming a session (`@geek/name`) goes to that session or, if none takes it, straight to the machine's inbox. Otherwise it goes to the session that posted in that thread, else the one that last posted in the room, else the inbox. The inbox raises a card for the owner. It never gives a message to the session that wrote it, or to a silenced session (`arugula mentions off`).
5. An agent block takes it as its next prompt. A terminal session gets a follow-up through the hooks inbox. Without hooks it is held as a card the owner delivers or dismisses, and is never typed in unasked (`mention_held.rs`).
6. The daemon reports turns to `PUT /api/daemon/chat/mention-state` (`keep_telling`), and the sender sees the status under their message. An agent's reply in the thread marks it `answered`.

The owner can turn this off for a machine (`agent_mentions` in `sharing.json`, on unless false): the op is then refused and nothing is kept. `@arugula` is reserved and never names a machine. It starts a session on the project's hosted box (`chat/sessions.rs`), which is outside this record.

## Compatibility
- Machines can sleep: `chat-control.spec.ts` stops every daemon and people keep writing.
- A machine offline when mentioned shows `offline`. When it connects, the newest 20 mentions from the last day are told once, oldest first (`redeliver`). One it doesn't answer, or whose message, room or grant is gone, shows `missed`. A platform restart mid-delivery releases `sending` mentions back to `offline` (`release_stale`).
- `mention_answer` still reads older daemons' answers: `routed` absent means `delivered`, and the old fall-back answer shows `fell_back` (`chat_mention_status_tests.rs`). The daemon tolerates a control that doesn't take mention turns (`CloudRefused::OldControl` in `keep_telling`).
- `<state>/mentions.json` keeps who posted where across a daemon restart, and only live sessions keep their place after it (`State::vet`).
- A live stream resumes with `Last-Event-ID`. A deleted message keeps its row and number, so read marks hold.
- Threads an older daemon kept in `<state>/threads/` stay on disk and are no longer read. Rooms brought in by the old import are read-only archives. Before a machine has said who reads a thread, its owner and, for a team's machine, the team's owners do.

## Not known
- aru-091 and aru-115 say agents on personal machines are never woken by other people's mentions, and that mentions are read, not pushed. The headers of `chat.rs` and `chat/agent_threads.rs` also say nothing here wakes an agent. The code hands a teammate's mention to a live session as its next prompt or follow-up, on by default, as the header of `threads.rs` describes. Which is the rule now?
- `docs/control.md` says a machine's agents read mentions over MCP, which leaves out delivery. It is about the inbox, so it may be narrower on purpose.
- What may a teammate's mention cause an agent to do? `framed` in `agent_mentions.rs` labels the text as information, not the person's approval. Whether that is enough is a question for a person.
- What do we promise about the plain-text content control holds, beyond what aru-051 and `tables.toml` record?
- I traced the mention path through tests on a fake control and a fake hub, so behaviour over the real relay comes from reading the code only.
