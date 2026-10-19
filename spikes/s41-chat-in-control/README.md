# S41: chat in control: team channels, DMs and pane threads held by control in plain text (#1092)

Run 2026-10-19 on geek, against `main` at 0.47.0 (b82bf2d8). Issue #1092; tracker #978. Supersedes S38
(#979, PR #991).

**Result: go.** Control already has what chat needs: who is in which team (signed rosters it checks),
who may reach which machine (`teams::may_reach`), push it can send itself (`push::notify`), and one
SQLite database with FTS5 compiled in. The prototype added about 900 lines to `crates/control` and
nothing to the daemon. The prototype ran end to end on a local rig: one control and two machines, with
both machines stopped ([results/flow.txt](results/flow.txt)).
- jake and alex are in team *acme*. box-a is jake's machine, in acme. box-b is alex's own machine,
  not in the team.
- Before anything stopped, box-a had an M61 thread on a pane (2 messages, one quoting the pane) and a
  session thread (1 message), on its own disk, made the way they are today.
- Each person captured a pane over the relay, end to end, as the page does before quoting.
- **Both daemons were stopped by PID, and control's directory showed both offline.** With them off:
  - box-a's old threads were imported into control, with their numbers kept;
  - jake made `#general` and posted `@alex the mean test fails on box-a`, quoting box-a's pane;
  - alex's live stream (SSE) had it 2 ms later. alex's phone got the mention push, which the rig opened
    with the phone's keys: "jake mentioned you in #general" and the text;
  - alex posted in `#general`, quoting a pane on box-b, then opened a DM with jake. Each wrote there,
    and the `@alex` in the DM was pushed too;
  - alex read box-a's imported pane thread and answered in it, with box-a still off;
  - unread and mention counts, read marks and search (`mean`, `FAILED` inside a quote) all came
    from control;
  - sam, signed in but not in acme, saw no rooms, got 404 for `#general`, and had no search hits.
- **Pane chips:** jake can't reach box-b, so in jake's view box-b's chip says `opens: false`. alex
  sees both chips open: box-b is his, and box-a is the team's.
- **What control holds** ([results/dump.txt](results/dump.txt)): rooms, messages and quotes in plain
  text, read marks, and the pushes it sent. The line the pane printed that nobody quoted (`running 3
  tests`) isn't in the database's bytes. Quoted lines are.
- **2,000 idle live streams** cost control 21 kB each: 34 MB of RSS became 77 MB (a debug build).
  One post reached all 2,000 in 78 ms.

The prototype is throwaway code in the second commit of this branch. It is not for main:
- `crates/control/src/db/chat.rs`: the tables (rooms, DM members, messages, an FTS5 index, read marks,
  the pushes sent) and their queries;
- `crates/control/src/chat.rs`: the API, who sees what, mentions, pushes, search, the SSE stream, and
  the import;
- the routes in `main.rs` (10 lines), the schema in `db.rs` (3), and one call in `account.rs` (deleting
  an account erases its chat).

The rig is [rig.ts](rig.ts) (the whole flow, writing `flow.txt`) and [dump.py](dump.py) (what the
database holds). No web UI was built: everything ran over HTTP, as the page would call it, with
`web/fixtures/device.ts` as each person's browser.

**Not proven:**
- any UI: the Chat page, the drawer on a pane, the phone layout, notification taps;
- a daemon importing its own threads, signed, on upgrade. The rig read box-a's files and posted them
  as its owner;
- M81 conversations' migration. None were made on the rig;
- the per-pane reader list (decision 2). The prototype gates pane threads by `may_reach`;
- live delivery through Fly's proxy, a release build's numbers, and `Last-Event-ID` resume;
- removing anything (q7 is an inventory, not a run);
- `just check`: clippy is clean and the tests of `arugula-control` pass (148, unchanged). Nothing
  else was run, and nothing else was touched.

## Answers

### q1: schema and API in control

**Tables** (the prototype's, with what the product adds):

| table | columns | notes |
|---|---|---|
| `chat_rooms` | id, scope, kind, name, machine, target, created_by, created, archived, last_seq | `kind` is `channel`, `dm`, `pane`, `session` or `archive` (q6). `scope` is a team id, or `account:<id>` for a thread on a machine in no team. A pane or session room is unique by (kind, machine, target), and a channel by (scope, name). |
| `chat_dm_members` | room, account | A DM's two people. A channel's members are the team's roster, not rows. |
| `chat_messages` | room, seq, author, at, text, ref_machine, ref_pane, quote, mentions, imported, (edited, deleted) | Control numbers messages per room, in the same transaction as `last_seq`. `mentions` is account ids, resolved at post time. |
| `chat_fts` | FTS5 over text and quote | Kept by triggers (q5). |
| `chat_reads` | room, account, seq | Moves forward only. Your own post marks you read. |
| `chat_pushes` | account, room, seq, why, at | A log of what was pushed (the prototype's), for rate limits and "you were notified". |
| (product) `chat_reader_lists` | machine, kind, target, accounts, at | Who may read a pane's or session's thread, as the machine last said (q3, decision 2). |

**The API** (session cookie, or a CLI device's signature, as every `/api/` route takes; the
prototype's paths):

| route | what |
|---|---|
| `GET /api/chat` | Every room I see, with `last`, `read`, `unread` and `mentions`, and a DM's other person. |
| `POST /api/chat/teams/{team}/channels` | Make a channel. Owners and editors may; viewers read and post. |
| `POST /api/chat/teams/{team}/dms` | The DM with a teammate, made on first use. |
| `POST /api/chat/targets` | The thread on a pane or session, `{machine, kind, target}`, made on first use by someone who sees it. |
| `GET /api/chat/rooms/{id}/messages?after=N` | A page of 200. Each message is shown as the reader sees it: names, and whether its pane chip opens. |
| `POST /api/chat/rooms/{id}/messages` | `{text, pane?: {machine, pane}, quote?}`. Returns its number, who it mentioned and who was pushed. |
| `POST /api/chat/rooms/{id}/read` | `{seq}`. |
| `GET /api/chat/search?q=` | q5. |
| `GET /api/chat/live` | q2. |
| `POST /api/chat/import` | q6 (the product's is a daemon's signed call). |

The product adds editing and deleting your own message, renaming and archiving a channel (owners), and
leaving a channel's notifications (a per-person mute, not membership). It also adds the wire types in
`arugula-control-wire`, generated into `web/src/proto.gen.ts`.

**Limits:**
- A message is at most 8 KB of text, and a quote at most 16 KB, as `MAX_TEXT` and `MAX_QUOTE` are
  today.
- Posts go through `limit.rs` like other account actions: say 60 a minute per account, and a cap for
  all of control.
- At most 200 channels per team, and 1 GB of chat per team (the owner pays, as for seats).

**Retention and deletion** (decision 6):
- **Messages are kept until someone deletes them.** An author deletes their own message: its row stays
  as "deleted", with no text, so numbers and read marks hold. Team owners may delete any message in
  their team's rooms.
- **Deleting an account** (`account::delete_account`, which the prototype hooked) does three things:
  - it deletes the account's DMs, both sides, since the other person can't answer anyone;
  - it blanks the text, quotes and mentions of its messages elsewhere, keeping the rows as "a deleted
    account";
  - it deletes its read marks.
- **Disbanding a team** deletes every room in its scope and their messages
  (`Db::erase_chat_scope`, written but not wired in the prototype).
- **Leaving a team:** your messages stay. Your DMs in that team become read-only for the other person
  and disappear for you.
- **Litestream's copies** age out with the rest of control's backups, as for every other row. The
  privacy page says so.

### q2: live delivery

**Server-Sent Events from control: `GET /api/chat/live`** (decision 4).
- **One stream per page.** It says `{room, seq}` for each new message in a room the person sees, and the
  page fetches what it hasn't got. The prototype checks `may_see` per subscriber per message. The
  product keeps each stream's room set in memory, refreshed when a roster or reader list changes.
- **Resuming:** each event carries an id (control's message counter), so `EventSource` resumes with
  `Last-Event-ID` after a drop. A stream that falls more than 1,024 behind gets `resync` and reloads
  the room list.
- **Why SSE, not the relay socket:** `/api/relay/m` is admitted, counted and billed as relay traffic.
  It carries Noise channels to daemons, and it isn't open when no machine is. SSE is plain HTTP with
  the session cookie, reconnects by itself, and needs no client library.
  - It runs over HTTP/2 at Fly's edge, so it doesn't use one of the browser's six HTTP/1.1
    connections.
  - The desktop app and the phone's PWA load control's page, so they get it the same way.
- **Pages served by a daemon** (tailnet, no control page) reach control cross-origin. They'd need CORS
  with credentials for control's API. Decision 7 says whether they get chat at all.
- **Cost on Fly:** control is one `shared-cpu-1x` machine with 1 GB (`packaging/control/fly.toml`).
  - An idle relay socket costs about 28 kB (#174). An idle SSE stream measured 21 kB.
  - 2,000 open pages would be about 45 MB, and one post reached all of them in 78 ms with a database
    check each.
  - Open files are the limit, as for the relay (`soft_limit = 6000`). An SSE stream is one more
    connection per open page, beside its relay socket, so plan on two per page. At today's size it's
    free; at a few thousand concurrent pages, raise the soft limit or move the relay to its own machine
    first.
  - **Storage:** a message is about 300 bytes, plus about the same again in the FTS index. A million
    messages is under 1 GB on the volume, and Litestream ships the same.

**Desktop app:** native notifications for mentions come from Web Push, if the webview delivers it.
Where it doesn't (not checked here for any platform), the app reads the same SSE stream. The app's
`needs_input` notifications stay as they are.

**Editor extension:** it has no chat (`crates/daemon/src/editor/ext/`), so nothing changes.

### q3: pane references

A message may carry `pane: {machine, pane}` and a `quote` (the text the poster chose).
- **Posting:** you may reference only a pane on a machine you may reach. The quote is read end to end
  by your page (`GET /api/panes/{id}/capture` over the relay), and you pick the lines. Control sees the
  lines once you post them, and nothing else of the pane (decision 1).
- **Reading a message with a pane chip:** control says whether the chip opens for this reader
  (`opens`). If it does, it opens the pane on its machine, end to end, when that machine is up. If it
  doesn't, the reader sees the machine's name, the pane number and the quote, and the chip is inert.
  The rig showed both.

**Who sees a pane's or a session's thread** (decision 2): today the daemon decides per pane. It
uses session roles, private panes, "from now" floors and the quote's own pane (`thread_role`,
`acl::thread_floor`). Control knows less:
- `may_reach` says who may reach the *machine*. A person shared one session (M30) may reach the whole
  machine, so gating by it would show them every pane's thread there. That's what the prototype does.
- **Recommended:** the daemon tells control, for each pane and session that has a thread, who may read
  it, and keeps that current. It does this beside its access list (`POST /api/daemon/access`), as
  `chat_reader_lists`. Control gates by the last list it was given, which still holds while the machine
  sleeps. With no list yet, only the machine's owner (and team owners, for a team machine) reads.
  - "From now" floors become a `from` number per person in the list.
  - A pane that closes keeps its last list, so its thread stays readable by the same people, as today.

**What a reader without rights on the pane sees:** a pane thread they can't read doesn't exist for
them (404, as for sam). A message in a channel that quotes a pane they can't open shows the quote
and an inert chip.

### q4: mentions and push

- **Resolving:** `@word` is matched, case-insensitively, against the room's audience by roster name,
  login and display name. The audience is a channel's roster, a DM's two people, or a pane thread's
  readers. A mention of someone who can't see the room doesn't resolve, and the author is told who it
  didn't reach (today's `unreached`).
- **Stored as account ids:** `mentions` in the message, and counted per reader in `GET /api/chat`.
  Renaming someone changes nothing.
- **Pushes, by control** (`push::notify`, encrypted per RFC 8291 by control itself):
  - a mention: "jake mentioned you in #general";
  - anything in a DM: "jake wrote to you";
  - the body is the first 200 characters of the text (decision 5);
  - the tag is `chat-<room>`, so a phone shows one notification per room, replaced as more come.
- **Not pushed:** your own posts, a room you have open (the page says so on its stream, as the daemon's
  presence does today), and more than one push per room per person per minute.
- **`@here` and `@channel`:** not in the first cut.
- **The daemon's own thread pushes** (`notify_mentions`, `notify_card`, `notify_reply`,
  `notify_added` in `thread_ops.rs`) go away with the store (q7).
- **Taps:** `sw.ts` and `tap.ts` learn `#chat=<room>` in place of `(pane|session)-N` with a daemon.

### q5: search

**Server-side, with SQLite FTS5, over the rooms you see.**
- The prototype indexes text and quotes with triggers. It searches the visible rooms with each word as
  a quoted phrase, so FTS5's syntax isn't the person's to type, newest first, 50 hits.
- This replaces `chat-find.tsx`'s load-every-thread-on-every-machine index, which can't work once
  threads aren't on machines.
- **How much:** words and prefixes (`mea*`), within one team's rooms or all of yours, and filters for
  `in:#channel`, `from:@alex`, `has:quote`. There's no regex and no search of pane output (control
  doesn't have it).
- **Cost:** about the size of the text again, in the same database and backups.
- **The daemon's `/api/search`** stops returning thread hits (`history.rs`). MCP `history` loses them
  too, which is consistent with agents being out of chat.

### q6: migration

**Pane and session threads (M61): imported into their pane's or session's room in control, by the
daemon, on upgrade.**
- **The rig did it by hand:** box-a's `pane-2.jsonl` and `session-2.jsonl`, read from its state dir,
  became rooms `pane 2` and `session 2` on box-a. The old numbers became control's (`same_numbers`).
  Authors were mapped: `owner` became the machine's owner, `account:<id>` became that account, and
  anyone else (a `tailnet:` login, a deleted account) was kept by name as the importer, marked
  imported.
- **The product's import:**
  - The daemon, joined and seeing control's `chat` feature, posts each thread signed as itself
    (`DaemonAuth`) to `POST /api/daemon/chat/import`. Control takes it only for that machine's own
    targets.
  - If the room already exists in control (someone used it from a new page first), the old messages go
    *before* control's: numbered `seq <= 0`, shown as "Earlier, on box-a". Read marks (always > 0) are
    unaffected, and every imported message counts as read.
  - The daemon moves `threads/` to `threads.imported/`, so it doesn't import twice, and so an older
    daemon after a downgrade starts with no threads rather than a fork.
  - It sends the reader list (q3) for each target in the same call.
  - A daemon with no control keeps its threads on disk, unread by anyone (decision 7).

**M81 conversations: archived read-only rooms** (decision 3).
- Each `conv-<id>.json`/`.jsonl` becomes a room of kind `archive` in its owner's account scope. Its
  readers are the conversation's members at the time, and its title is kept.
- Its timeline is kept as text: a person's message as it was, a lead's answer as "lead: …", and an
  event or card as one line ("L1 finished: …"). Links, grants and the lead's state aren't migrated,
  since agents are out.
- No one posts in an archive. The members continue in a channel or a DM. There's no team to put an
  M81 conversation in, since its members came from any account, and inventing a team per conversation
  is worse than an archive.

**What an old daemon does** (one that doesn't know control holds chat):
- It keeps serving its own threads over its own socket to old pages, as now, and to nobody else.
- A new page (control's) reads chat from control only, and shows no badge or drawer from an old
  daemon's `state.threads`.
- Control's page marks such a machine "update to bring its threads into chat", from its `features`.
- An old daemon has no way to post into control's rooms, which is fine: posting is a person's, from a
  page, and pages are new.

### q7: what comes out

Sizes from today's tree. "Out" is deleted; "changed" is trimmed in a shared file.

**Agents out of chat** (decision 9: first, and on its own):

| what | where | lines |
|---|---|---|
| The lead (M84) | `crates/daemon/src/labs/lead.rs` | 866 out |
| Links and grants (M82) | `crates/daemon/src/labs/links.rs`, `conv-grants.json` | 455 out |
| Events and cards (M83/M85) | `crates/daemon/src/labs/events.rs` | 1,268 out |
| Their wiring | `labs/mod.rs` (resume, no-labs twins), `ops/convs.rs` (`link`, `lead`, `act`, grants), `e2e.rs` (`link_only`, `ConvVia`), `api.rs` (`act_for_conv`/`act_for_lead`), `control.rs` (`link_callers`, `link_accounts`, `granted`), `peer.rs` (`find_by_id`, `request_within`, `follow`) | about 700 changed |
| MCP | `Scope::Lead`, `LEAD_ONLY`, `links` list kind, `lead::route`/`after`, `adopt`; **`read_thread` and `post_thread`**, `THREAD_INSTRUCTIONS`, `CHAT_TOOLS` (decision 8) | about 300 changed |
| `@agent` follow-ups | `thread_ops.rs` (`to_agent`, `calls_agent`), `ops/threads.rs` (`tell_agent`). M29's inbox and `Api::FollowUp` stay: forge issues use them | about 80 changed |
| Control's side | `AccessList.links`/`reasons`, `AccessReason`, `daemon_offer_reasons`, the conversation offer push | about 130 changed |
| Proto | `ConvLink*`, `ConvGrant`, `ConvEvent*`, `ConvAsk`, `ConvLead`, `LeadChange`, the 7 link and lead ops | about 400 changed |
| Web | `ui/links.tsx` (365), `ui/conv-events.tsx` (244) out; `@lead`/`@agent` in `threads.tsx`, `people()` | about 650 |
| Tests | `chat_lead.rs`, `chat_watch.rs`, `chat_diffs.rs` (500); events and links unit tests; `chat-lead`, `chat-links`, `chat-events` specs (1,032, 22 tests) | about 1,550 out |

That's about 4,000 lines of Rust and 1,600 of web and e2e, with nothing to build in their place.

**The daemon's thread store and API** (after the import has shipped):

| what | where | lines |
|---|---|---|
| The store | `crates/mux/src/labs/threads.rs` | 408 out |
| Posting, access, unread, pushes | `crates/mux/src/mux/thread_ops.rs`, `labs_off.rs` | 524 out |
| Handlers and MCP | `crates/daemon/src/ops/threads.rs` | 233 out |
| Conversations (M81) | `crates/mux/src/labs/convs.rs`, `ops/convs.rs` | 559 out |
| Mux wiring | `Api::Thread*`, `Conv`, `Convs`, `CanRead`, `ThreadPlace`; `acl.rs` `ThreadFrom`/`thread_floor`; `history.rs` thread hits; `State`/`Delta.threads`/`convs`; `ServerMsg::Thread` | about 500 changed |
| Proto | `ThreadTarget`, `ThreadMsg`, `Quote`, `ThreadSummary`, `Conversation`, `ConvMember`, the 3 thread and 7 conversation ops; `proto.gen.ts` | about 700 changed |
| Invites from a thread (#297) | `invite/mod.rs`, `InviteRequest.thread` | about 80 changed. An invite from a chat room becomes control's: a room link in the share push |
| Tests | `threads.rs`, `chat_convs.rs` (887); unit tests; fixtures `mcp-tool-list.json`, `ops-access.txt`, `ops-read-only.txt` | about 1,300 out or changed |

That's about 1,700 lines of Rust out and 1,300 changed, plus tests.

**The order:**
1. **Agents out first** (M121). It depends on nothing here, it's most of the deletion, and it leaves
   the M81 conversations as plain timelines, which is what q6 archives.
2. **Control's chat, and clients on it** (M122, M123), behind control's `chat` flag for a team or
   account. The daemon's store still runs for old pages.
3. **The import** (M124), shipped in a release and run on Jake's machines.
4. **Then the daemon's store comes out** (M125), one release after the import, so a machine that
   skipped a release still imports. Its `threads.imported/` stays on disk.

`read_thread`/`post_thread` go in step 1 (decision 8). Nothing proxies them to control.

### q8: clients

The Chat page talks to control: `api()` in `web/src/control.ts`, with a new `ChatStore` holding the
rooms, the messages fetched, and the SSE stream.

| file | lines | what changes |
|---|---|---|
| `web/src/ui/chat.tsx` | 737 | The sidebar goes from "machines, then sessions, then panes" to "channels, DMs, threads on panes". The "Not here: offline" note and `convRows` go. `PanePeek`/`ChatDetails` still read the pane from its machine, and say "box-a is asleep" when it is. |
| `web/src/ui/threads.tsx` | 841 | `ThreadBody`, `Compose` and `Message` read from the store. `Quote` gains `machine`. `reveal` finds the pane's machine through the fleet. `@agent`/`@lead`, `InviteOffer` and `landed` go. `people()` is the room's audience, from control. |
| `web/src/fleet.ts` | 562 | It stops carrying thread summaries. `FleetPane.unread`/`mention` come from the store, keyed by (machine, pane), for the swarm's corners and the pane badges. It still carries pane metadata. |
| `web/src/ui/chat-find.tsx` | 496 | Search calls `GET /api/chat/search`. Activity is mentions of me from `GET /api/chat`. |
| `web/src/ui/places.tsx` | 240 | `sources()` goes. `chatUnread()` reads the store. Routes are `#chat=<room>`. |
| `web/src/client.ts` | 1,754 | `loadThread`, `postThread`, `markThreadRead`, `onThread`, `convs`, `conv*` (about 215 lines) go. |
| `web/src/ui/convs.tsx`, `links.tsx`, `conv-events.tsx` | 910 | Out. Archives (q6) open in the ordinary room view, read-only. |
| `web/src/sw.ts`, `tap.ts`, `main.tsx` | | Taps open `#chat=<room>`. Pushes now come from control (`control: true`), not the daemon. |
| `web/src/ui/phone.tsx` | 392 | Nothing of its own: the Chat button shows on control's `chat` flag rather than a daemon's `hasThreads()`, and the chat page's phone layout (#889) stays. |
| `web/src/hand.ts` | | Unchanged (S33's phone hand has no chat). |

**Who "me" is:** the control account, not each daemon's presence principal (`chatMe()`, `meMatcher`).

**Gating:** control's `chat` flag for the account or team (`labs_flags`, which already has a `chat`
row) replaces each daemon's `chat` flag.

**e2e:** `chat.spec.ts`, `threads.spec.ts`, `chat-messages.spec.ts` and `chat-convs.spec.ts` (37
tests) move onto a control fixture (`team-fixture`, as `chat-links` uses) and are rewritten. The three
agent specs go in M121. `tap.spec.ts` and `labs-off.spec.ts` change their routes.

## The rig

- One local `arugula-control` with a fake GitHub and a fake Web Push service (`--push-host`).
- Two people, each a headless browser device (`web/fixtures/device.ts`, the web client's own e2e code):
  jake, who founds team *acme*, and alex, invited as an editor and admitted. A third person, sam,
  signs in late and is in no team.
- Two machines:
  - box-a, jake's, joined into acme;
  - box-b, alex's own.

  Each has Labs on, its own state dir, port and log, and is reached through the relay.
- A pane on each prints a fixed text (a failing test on box-a, a deploy line on box-b).
- Both daemons are stopped with SIGTERM to their process group, by PID, and waited for, before any
  chat happens.
- alex's push subscription is signed by his device, as the page signs it. The rig decrypts what
  the fake push service got with the phone's keys (RFC 8291), as `teams.spec.ts` does.
- The environment is stripped of `ARUGULA_*`/`CLAUDE*`. Every call names a rig port, and every
  process is stopped by PID.

## Decisions for Jake

Each is a choice, with a recommendation.

1. **A quoted pane.**
   - **(a) The lines the person picks are copied into the message, in control (recommended).** It's
     their choice to share them, it reads with the machine asleep, and it's what a quote is today.
   - (b) Only a reference (machine, pane, a line range), with the text fetched end to end by each
     reader who may reach the pane. Terminal text never enters control, but the quote is blank when
     the machine sleeps, and for anyone without rights.
2. **Who reads a pane's or a session's thread.**
   - **(a) Whom the daemon says may read it, kept in control and updated by the daemon; only the
     owner until it has said (recommended).** It's today's rule, and it holds while the machine
     sleeps.
   - (b) Anyone who may reach the machine through control (the prototype). It's simpler, but someone
     shared one session reads every pane's thread on that machine.
   - (c) Everyone in the machine's team. It's simplest, but a private pane's talk is the team's.
3. **M81 conversations.**
   - **(a) Archived, read-only rooms for their members, events and the lead's answers as text
     (recommended).**
   - (b) Moved into a team's channel, where there is a team that holds all its members. Most don't
     have one.
   - (c) Not migrated: the files stay on the home machine, unread.
4. **Live delivery.**
   - **(a) Server-Sent Events from control, one stream per page (recommended).**
   - (b) A control channel inside `/api/relay/m`. That's one socket per page, but chat becomes relay
     traffic, counted and billed, and there's none while no machine is up.
   - (c) Polling every few seconds, as Cloud workspaces do.
5. **What a push says.**
   - **(a) Who, where, and the first 200 characters (recommended).** Control can read it anyway, and a
     lock screen is where people read chat.
   - (b) Who and where only, "jake mentioned you in #general".
6. **Retention.**
   - **(a) Kept until deleted. Deleting an account deletes its DMs and blanks its messages, and
     disbanding a team deletes its rooms (recommended).**
   - (b) A retention period a team owner sets (say 90 days, a year, forever), with a daily sweep.
7. **Chat without control** (the tailnet stays first class).
   - **(a) Chat needs control. A machine with no control has no chat, and its old threads stay on
     disk (recommended).** Chat is a team feature, and teams are control's.
   - (b) Keep the daemon's thread store for machines without control: two chat systems, and every
     client speaks both.
8. **MCP `read_thread` and `post_thread`.**
   - **(a) Removed with the rest of agents in chat (recommended).**
   - (b) Proxied by the daemon to control, posting as the machine's owner. That's an agent in chat
     again, under a person's name.
9. **The order of removal.**
   - **(a) Agents out first, on their own; the daemon's store last, a release after the import
     (recommended).**
   - (b) Everything at once, when the import lands. That's one milestone fewer, but a machine that
     skips a release loses its threads.
10. **DMs.**
    - **(a) A DM belongs to a team. It's read-only, then gone, when either person leaves
      (recommended).** It matches "channels and DMs belong to a team", and leaving a team ends it.
    - (b) One DM per pair of accounts across all their teams, kept while they share any team.

## The milestones (from M121, for #978)

M121–M126 are unclaimed. Checked 2026-10-19: GitHub issues and PRs (the highest is M120), remote
branches, and `.claude/worktrees` (`cw-m114` … `cw-m120`). Each builds on the one before unless it
says otherwise. Nothing from this branch merges as is.

- **M121: agents out of chat.** Independent of the rest.
  - Remove the lead, links, grants, events and cards (`lead.rs`, `links.rs`, `events.rs`) and their
    wiring in the daemon, MCP (`Scope::Lead`, `read_thread`, `post_thread`), control (`AccessList.links`
    and `reasons`, offer reasons) and proto.
  - Remove `@agent`/`@lead` from threads, and `links.tsx` and `conv-events.tsx` from the web.
  - Remove the three agent specs and three integration test files.
  - DECISIONS: the M82–M85 entries go, and a line records that agents are out of chat until they come
    back through #959/#1002 grants. `docs/labs.md` changes to match.
  - **Done when:** `just check` and the e2e suite are green. A pane thread and an M81 conversation still
    post and read on the daemon. `@agent` in a thread is plain text. The MCP tool-list fixture has no
    chat tools. About 4,000 lines of Rust are gone.
- **M122: chat in control.**
  - The tables (q1) in `SCHEMA` and `migrate()`, the API, and its wire types in `arugula-control-wire`
    and `proto.gen.ts` (`just proto-ts`).
  - Limits and rate limits, mentions, read marks, FTS5 search, and pushes by control (q4).
  - Deletion with the account and the team, and the per-pane reader lists from daemons (q3,
    decision 2).
  - The SSE stream with `Last-Event-ID` and an in-memory room set per stream.
  - Behind control's `chat` flag.
  - DECISIONS: "Threads live on the daemon that owns the pane" becomes "Chat lives in control, in plain
    text; terminal content doesn't". "Control can refuse service but can't read" gains the exception.
    `docs/control-e2e.md` (What control stores, Push) and the privacy page change too.
  - **Done when:** control's tests cover visibility (a channel, a DM, a pane thread by reader list, an
    outsider), mentions, pushes, search, read marks and deletion. A control integration test runs this
    spike's flow with no daemon running.
- **M123: the Chat page on control.**
  - The `ChatStore`, and `chat.tsx`, `threads.tsx`, `chat-find.tsx`, `places.tsx` and `fleet.ts` as in
    q8.
  - Pane chips that open or don't, and quotes with their machine.
  - Badges and swarm corners from the store, push taps to `#chat=<room>`, and the phone layout.
  - The chat specs move onto a control fixture.
  - **Done when:** on `just dev-control`, with every daemon stopped, two people chat in a channel and a
    DM from the web and the phone layout. One quotes a pane and the other's chip opens only where they
    may. A mention push opens the room. All of `just e2e` passes, and the PR has before and after shots.
- **M124: the import.**
  - The daemon's signed import on upgrade (q6), with old messages before control's, reader lists sent
    with it, and `threads.imported/`.
  - M81 conversations become archives.
  - "Update to bring its threads into chat" on control's page for old daemons.
  - Mixed versions tested (`ARUGULA_FEATURES=""` playing the last daemon).
  - **Done when:** a daemon with pane and session threads and a conversation upgrades, and they appear
    in control with their numbers, authors and quotes. A second start imports nothing. A room already
    used in control keeps its own numbers after the old ones.
- **M125: the daemon's store out** (a release after M124).
  - The store, `thread_ops`, `ops/threads`, conversations, `ServerMsg::Thread`, `State.threads`, the
    thread ACL floors, thread hits in `/api/search`, the daemon's mention pushes, and the proto types
    (q7).
  - Pages stop reading `state.threads`.
  - **Done when:** `just check` and `just e2e` are green with about 1,700 more lines gone, and a page
    against a daemon of this release shows chat only from control.
- **M126: the tracker's run, live.**
  - #978's done-when, on the hosted control: with geek and jake-mini asleep, Jake and a teammate chat
    in a team channel and a DM from the phone and the web. They quote a pane, get mention pushes, and
    find geek's old pane threads in their panes' rooms and an old conversation as an archive.
  - Anything the run finds.

## Reproduce

```
export CARGO_TARGET_DIR=$PWD/target
git checkout s41-chat-in-control   # the prototype is the second commit
cargo build -p arugulad -p arugula-control
env -u ARUGULA_SOCK -u ARUGULA_PANE TARGET_DIR=$PWD/target/debug \
  node --experimental-strip-types --no-warnings spikes/s41-chat-in-control/rig.ts $RIG
python3 -I spikes/s41-chat-in-control/dump.py $RIG/control/control.db
```

- `RIG` is any empty directory. The rig runs to the end (about 15 s), writes `$RIG/flow.txt`, and
  stops control and both daemons by PID.
- `S41_STREAMS` sets how many idle live streams it opens at the end (2,000 by default).
- `web/dist` isn't needed: nothing loads a page.
