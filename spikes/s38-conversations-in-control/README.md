# S38: conversations stored in control, end to end encrypted, with a lead that moves between the owner's machines (#979)

Run 2026-10-17 on geek, against `main` at 0.41.0 (1e57d979). Issue #979; tracker #978.

**Result: go, with a per-conversation key rather than MLS.** The prototype ran end to end on a local
rig: one control and two machines of one account, box-a and box-b, reaching each other only through
the relay.
- An M81 conversation made on box-a, linking Claude Code (haiku) in a pane on box-b as `L1`, moved
  into control on box-a's word. Its key was wrapped to the account's three devices, its record and
  timeline went up sealed, and box-a took the lead's lease first (token 1). box-b mirrored it within
  a second.
- `@lead` from box-a: box-a's lead prompted L1 through the relay and answered.
- **A dump of control's database holds no message text:** neither the SQL dump nor the raw bytes of
  the database and its WAL contain any word of the messages ([results/dump.txt](results/dump.txt)).
- box-a was stopped with SIGSTOP, as a laptop sleeps. `@lead` from box-b: box-b took the lease once
  box-a's expired (token 2), started its own lead, drove L1 (now on its own machine) and answered.
- box-a was woken with SIGCONT. The first thing it did was try to post as the lead or a link; its own
  check (the lease, by the wall clock) refused it before control was asked. Its renewal was then
  refused, so it closed its lead. A lead's post sent with its old token was refused by control (409)
  ([results/logs.txt](results/logs.txt)).
- box-a was restarted. It mirrored the conversation and didn't lead. Its owner's `@lead` from box-a
  was answered by box-b's lead, which knew a word said before the move.
- control's log shows which device appended each entry: every lead post after the move came from the
  holder of the lease at that moment ([results/flow.txt](results/flow.txt)).

The prototype is throwaway code on this branch, behind the labs `chat` flag:
- `crates/e2e/src/conv.rs`: the conversation key, sealing, and wrapping to a device's X25519 key;
- `crates/control/src/db/s38.rs` and `crates/control/src/s38.rs`: the record, the log and the lease
  in control, for the conversation's own account's daemons;
- `crates/daemon/src/labs/convstore.rs`: moving a conversation in, the mirror, posting through
  control, and the lease;
- `crates/mux/src/mux/thread_ops.rs`: a post split into compose and commit, and messages numbered by
  control kept in order (`Api::ThreadCompose`, `ThreadIngest`, `ConvPut`);
- small changes in `lead.rs`, `events.rs` and `ops/threads.rs`: posts go through the store, resume
  only on the holder, and a grant names the conversation rather than one machine.

The rig is [rig.ts](rig.ts), [setup.sh](setup.sh), [flow.sh](flow.sh), [c.sh](c.sh),
[restart.sh](restart.sh), [dump.py](dump.py) and [table.py](table.py). No web UI was built: everything
was driven over HTTP.

**Not proven:**
- a member from another account, or any browser or phone reading from control (devices other than
  the daemons got wrapped keys, but nothing unwrapped them);
- key rotation on removal, signed entries and the record's merge when two machines change it at once;
- a real second machine (jake-mini), a real sleep (SIGSTOP stands in for it), and a lease taken
  while the pane's own machine is the one asleep;
- links on more than one machine after the move, events posted by the pane's machine, and cards;
- the Claude Code session carried between machines (the new lead starts fresh, q4);
- `just check`: clippy is clean on the four touched crates, and the tests of `arugula-mux`,
  `arugula-e2e` and `arugula-control` pass (with the prototype's two new ones). The daemon's own
  tests, the web client and `check-core` weren't run.

## Answers

1. **The cipher: a per-conversation key, in epochs, wrapped to each member device's X25519 key.**
   It needs no new code in the browser (WebCrypto has X25519, HKDF and AES-GCM, and every device
   already has its X25519 key), a browser that lost its keys gets them back with PRF and reads all of
   it again, and a lead that moves needs the whole history, which MLS works against. MLS's forward
   secrecy and post-compromise security aren't worth 540 kB of wasm and a second key per device for
   groups of two to five people who want the history.
2. **What control stores:** a record (sealed), an append-only log of sealed entries numbered by
   control, the wrapped keys by device and epoch, and the lease. In the clear: the conversation's id,
   the owner's account, the members' account ids, which device appended each entry and when, sizes
   (padded), and who holds the lease. Entries up to 64 KB; images as sealed blobs beside them.
3. **New members and history:** a member device re-wraps old epochs' keys to the new member's devices
   ("with history"); "from now" starts a new epoch at the join. A removed member keeps what their
   epochs' keys open, which is everything up to the removal; the key rotates, and control stops
   serving them.
4. **The lease:** in control, with a fencing token one higher each time it changes hands, renewed
   every 10 s and lasting 30 s. Writes only the lead's machine makes carry the token, and control and
   the pane's machine refuse a stale one. Validity on the holder is judged by the wall clock, not a
   monotonic clock, which stops during sleep. The new holder starts a fresh Claude Code session with a
   handoff (the last messages, the links, and the lead's own state from the sealed record); it doesn't
   carry the transcript.
5. **Owner's rights:** the holder mints the lead's MCP scope locally, as today. A link's grant names
   the conversation's lead (any machine of the owner's account holding the current lease) instead of
   the home device. It's an S37 standing grant (`arugula exec grant v1`) with a new principal,
   capped at its maker's capabilities.
6. **Events and cards go through the lead's machine:** the holder runs the watchers, as the home does
   now, and seals what they post. A write-only key for pane machines isn't needed yet. With no owner's
   machine online, a card waits, saying so.
7. **Migration:** on upgrade the home moves each conversation in (as the prototype did), takes the
   lease first, and moves its local files aside so an older daemon can't fork it. A pane's machine
   that isn't upgraded keeps working while the home holds the lease, and its link says it needs an
   update before the lead can move.
8. **Clients:** in the first step nothing changes for them. Every owner's machine keeps a mirror and
   serves the same thread API, so the Chat page and the phone read through whichever owner's machine
   is online (on #978's done-when, jake-mini). Reading straight from control (for members of other
   accounts, and with every owner's machine off) comes after, with search and unread counts on the
   device.

## The rig

- One local `arugula-control` with a fake GitHub, and one person, `jake`, with one browser device.
- Two machines of his joined to it: `box-a` (where the conversation is made) and `box-b`. Each has
  Labs on, its own state dir and port, and its own log (`--log-file`; S36's rig lost the logs, since
  tracing writes to stdout). Each reaches the other only through the relay.
- Claude Code 2.1.302 under geek's own login: L1 in a pane on box-b (`--model haiku --permission-mode
  default`), and the leads as agent blocks, on haiku (`S38_LEAD_MODEL`, read by the prototype).
- The environment is stripped of `ARUGULA_*`/`CLAUDE*`, every call names a rig port and token, and
  every daemon was stopped by PID.
- **Lease timings in the prototype:** 15 s, renewed every 5 s, and the holder stops counting itself
  the holder 3 s before expiry by its own wall clock.
- Rig quirks:
  - restarting box-b killed L1's `claude` (the pane came back as a shell, as S36 saw), so L1 lives
    on box-b and box-a is the one restarted;
  - a link now names a machine by device id, not name, since M82;
  - box-a trusts box-b about two minutes after box-b joins, as in S36.

## q1: the cipher

The two candidates, for a conversation of 2–5 people, each with 1–4 devices, plus the owner's
machines (every one of them is a candidate to run the lead, so every one is a member device):

| | MLS (openmls, S30) | Per-conversation key, wrapped per device |
|---|---|---|
| **The unit** | each device a leaf of one group; a commit per change | a 32-byte key per epoch; each device gets it wrapped to its X25519 key (an ephemeral X25519, HKDF-SHA256, AES-256-GCM: HPKE's base mode) |
| **Adding a device** | external commit from the GroupInfo control holds, or an Add and Welcome from a member device; it reads from that epoch on | any device holding the key wraps it to the new device's certificate (32 + 12 + 48 bytes per epoch); it reads everything its member may |
| **Removing a member** | a Remove commit; a new epoch they can't read; forward secrecy and post-compromise security | a new epoch's key, wrapped to everyone else; they keep the old epochs' keys, which open what they could already read |
| **A browser that lost its keys** | its leaf secrets are gone: it rejoins by external commit, from the current epoch, and loses the history unless someone re-shares it. PRF can re-derive the device key, not the ratchet state | PRF re-derives the same X25519 key (#764), so the wrapped keys open again: the whole history comes back with no one online |
| **A machine taking the lead** | needs its own leaf, kept in step through every commit, and the history from before it joined re-shared | unwraps the key and reads the log: what the prototype did |
| **Browser, desktop app, phone** | wasm: 540 kB gzipped; a separate MLS signing key, because WebCrypto signs asynchronously and openmls needs it synchronous; group state persisted to IndexedDB | WebCrypto alone: X25519 `deriveBits`, HKDF and AES-GCM, all in today's targets (the device key is already X25519 for Noise). The WebKit X25519-in-IndexedDB bug is handled as it is for Noise |
| **Daemon** | openmls, its storage provider, commit ordering and fork detection | `arugula_e2e::conv`: 135 lines with its test |
| **What control can do** | drop, delay or fork a channel; it can't add a reader (the roster check) | drop, delay, reorder or withhold entries unless entries are chained (below); it can't add a reader (wraps go only to devices that chain to a pinned root) |

**Recommendation: the per-conversation key.**
- **History is the point here.** A conversation's history is what its lead works from, members are
  added "with history" (#855), and the lead moves to a machine that must read all of it. MLS is built
  so a new member *can't* read the past; every history feature would be re-sharing around it.
- **Losing a browser shouldn't lose a conversation.** With PRF, the wrapped key opens again; with
  MLS, the browser starts from the current epoch.
- **Code:** nothing new in the browser, and about 150 lines in Rust, against openmls in wasm, a
  second signing key per device, and a state store that must never fall behind.
- **What's given up:** forward secrecy inside an epoch, and post-compromise security. A stolen device
  key reads every epoch wrapped to it until the device is revoked, which then rotates the key. That's
  the same exposure the device key already has for the terminals it reaches.

**What the product adds to the prototype's cipher:**
- **Epochs.** A new key when a member or a device is removed, and when someone is added "from now".
  Each entry names its epoch.
- **Signed entries.** Each entry is signed by its author's device key (Ed25519), inside the seal, so a
  member can't post as another. The prototype's entries are only sealed.
- **A hash chain.** Each entry names the hash of the previous entry its author had seen, so a member
  device notices an entry control dropped or reordered. Control can still refuse service, as always.
- **HPKE by name.** The wrap is RFC 9180 base mode (DHKEM X25519, HKDF-SHA256, AES-256-GCM), with the
  conversation, epoch and device in its `info`, so other implementations can check it.

## q2: what control stores

**The tables** (the prototype's three, plus what the product needs):

| table | columns | sealed | clear |
|---|---|---|---|
| `conv_heads` | id, account (the owner), version, record, updated | the record: title, members' names and roles, links, the lead's state (`ConvLead`) | id, owner, version, time |
| `conv_members` | conv, account, joined, history | | which accounts are in it, and from when: control needs them to say who may fetch it and whom to push to |
| `conv_keys` | conv, epoch, device, wrapped | the key | which device holds which epoch |
| `conv_log` | conv, seq, epoch, at, device, ct | the message (text, quote, event, mentions, who) | number, time, the device that appended it, padded size |
| `conv_leases` | conv, holder, token, expires, cursor | | which machine runs the lead, how many times it moved, the last `@lead` handed on |
| `conv_blobs` | conv, id, size, at | the image | size, time |

**What the clear part leaks:** who talks with whom (account ids), when and how much, which device
wrote each entry (so whether a lead or a person wrote it, by elimination), which machine runs the
lead and whether it's online, and how many `@lead` turns there were. It doesn't leak a word of the
conversation, its title, its links or what its panes do. The prototype's table proves the split:
control's columns say "box-b appended entry 8", and only a member reads that it's the lead's answer.

**Sizes and retention:**
- **An entry is 64 KB at most** (the prototype refuses more). A message is at most 8 KB of text plus
  a 16 KB quote today (`MAX_TEXT`, `MAX_QUOTE`), so that's room enough. Entries are padded to 256 B,
  1, 4, 16 or 64 KB, so a size says little.
- **Kept until the conversation is deleted.** Deleting it (its owner) deletes its rows at once.
  *Delete account…* deletes the conversations the account owns, and takes it out of the others.
  Litestream's copies age out with the rest of control's backups.
- **A quota per account** (say 1 GB of entries and blobs), counted against the owner, since the owner
  pays.

**Images (#301):** today an image is uploaded to the pane's machine (`/api/panes/{id}/upload`, 20 MB
a file) and its path pasted. In a conversation it becomes a blob in control's object store, sealed
under a fresh key, whose key, hash, name and type ride in the sealed entry that shows it. Control
sees a blob's size and time. A lead or a link that needs the file gets it from the blob on its own
machine. The prototype didn't build blobs.

## q3: new members and history

**Adding "with history"** (#855's default for the owner's choice): a device of the owner's (or any
member's) that holds the old epochs' keys wraps each to the new member's devices. That's 92 bytes
per epoch per device. It needs one of those devices online once, which is the moment of adding: the
owner is adding them from a device.

**Adding "from now":** the same device starts a new epoch at the join and wraps only that. Before
epochs, "from now" was a read floor the home enforced. Now the cipher enforces it, since the current
key would open everything since the last rotation.

**Re-wrapping beats a history bundle.** A bundle (a member device decrypts the history and seals it
afresh for the newcomer) costs the whole history in size, and someone has to be online with it. A
re-wrap is a few hundred bytes, and the newcomer fetches the log from control.

**A new device of a member** gets every epoch its member may read, wrapped by any of that member's
devices when the device is approved (the approval page already runs on one). A browser that comes back
through PRF needs nothing. A new owner's machine is a new device of the owner, wrapped at its join.

**What a removed member keeps:** everything up to the removal. They held the keys and could have
saved every message. The product:
- rotates the key, so nothing after opens with what they hold;
- has control stop serving them the log (it checks `conv_members`), so even their old keys need a
  copy of control's database to read old entries they never fetched;
- says so when someone is taken out: "They keep what they could already read."

## q4: the lead's lease

**Taking and renewing** (as the prototype does, with the product's timings):
- A lease names the holder (a daemon), a fencing token, an expiry and the cursor (the last `@lead`
  handed to the lead).
- Only a daemon of the conversation's owner's account may take it: the owner's rights stay on the
  owner's machines (#978).
- The holder renews every 10 s; a lease lasts 30 s.
- A machine that finds it free or expired takes it, and the token goes up by one. A daemon that
  restarts is a new holder with a new token, even if its old lease hasn't expired. Its old token died
  with the process, so it waits for the expiry like anyone else.
- A holder that shuts down cleanly releases it, so another takes over at once.
- **It stays where it is.** It doesn't drift back to the home when the home comes back. The owner can
  move it (*Run the lead here*), which releases and takes it.
- **Measured:** with box-a stopped, box-b took the lease 13 s after the `@lead` was posted (the
  prototype's 15 s lease, less the time since box-a's last renewal). Its new lead had prompted L1
  and answered 26 s after the `@lead`, including starting Claude Code
  ([results/flow.txt](results/flow.txt)). With the product's 30 s lease, expect about 30–45 s.

**Fencing.** A machine that wakes up holding a stale lease must not post or drive:
- **In control:** what only the lead's machine writes carries the token: the lead's posts, its links'
  events and its own cards. Control appends it only while that device holds the lease with that
  token. A person's post needs no lease. The prototype's probe (box-a's lead post with token 1, after
  box-b held token 2) was refused with 409.
- **On the pane's machine:** the lead's requests over the relay carry `x-arugula-lease: <conv>
  <token>`. The pane's machine keeps the highest token it has seen per conversation (on disk) and
  refuses a lower one. A token higher than it has seen is checked once with control (cached for the
  lease's renewal interval). The prototype didn't build this half: its L1 was on box-b, the new
  holder.
- **On the holder itself:** it counts itself the holder only until its last renewal's expiry, less a
  margin, **by the wall clock**. Rust's `Instant` is `CLOCK_MONOTONIC` on Linux and
  `mach_absolute_time` on macOS, and both stop while the machine sleeps. A laptop that wakes would
  think its lease still had 20 s left. The prototype uses the wall clock (`store::now_ms`).
- **What it does on losing it:** stops its links' watchers, stops following its lead, and closes the
  lead's block, as box-a did when it woke. The token is what makes this safe. Closing the block just
  frees the machine.
- **The run showed all three layers.** Woken, box-a first tried to post (its watcher or its lead,
  catching up), and its own wall-clock check refused it. A second later its renewal came back 409 and
  it closed its lead. The probe with its old token was refused by control. Restarted, it mirrored
  and took no lease. In control's log, every entry after the move that only a lead's machine writes
  (6, 7, 9) was appended by box-b.

**What moves with the lead:**
- **The timeline:** from control. The new holder has it already, since every owner's machine mirrors.
- **The lead's own state** (`ConvLead`: whom it owes an answer, the links it waits on, the cursor):
  in the sealed record, not on the home's disk, so the next holder knows whom to answer. The
  prototype moved only the cursor, in the lease.
- **The Claude Code session: no. The new machine starts fresh, with a handoff.** M84's replacement
  lead already works this way. It's shown the last 20 messages and the links in its first turn. In
  the run, box-b's new lead answered "what was the word for today, said before the move?" from that.
  - Carrying the session would mean uploading its transcript (`~/.claude/projects/<cwd>/<id>.jsonl`:
    every tool result, so screens of every linked pane, often megabytes), sealed. The other machine
    would need the same working directory path and a Claude Code that resumes it. A machine that died
    can't upload anything, so there would be two paths anyway.
  - **Cost:** the handoff is one prompt of about 2–6k tokens (20 messages, the links, the state):
    well under a cent on haiku, a few cents on a larger model, once per move. A move is rare (a
    machine going to sleep), and the lead's own compacting already throws its memory away on long
    conversations. The record, not the lead's memory, is the conversation (S36 q6).
  - **What's lost:** what the old lead was in the middle of. Its last turn is cut off, and the new
    lead is told so (`Why::Restarted`, extended to "the lead moved here from box-a").

## q5: owner's rights on the owner's machines

- **The lead's MCP scope** is minted by whichever machine holds the lease, as today
  (`Scope::Lead { block, conv }`, M84). Nothing about it names a machine. A scope minted on a machine
  that loses the lease dies with the lead's block, which that machine closes.
- **The grant on a linked pane** names the home device today (`conv-grants.json`, `device`). The
  prototype accepted any grant for the conversation while holding the lease. The product's grant names
  **the conversation's lead**: any daemon of the conversation owner's account, chaining to the root
  the pane's machine pinned for that account, that presents the current lease token (q4). A grant
  made before the move names the home device. The pane's machine reads such a grant as the
  conversation's, once the home tells it (q7).
- **Every check that reads a grant has to change with it.** The second run showed why: box-b, a
  mirror, answered `GET /api/convs/{id}/links` by checking L1's grant against its own device id (the
  grant named box-a), marked L1 "revoked", and the mirror put that closed link in control, where
  box-a took it too. Grants are read in `links::link`, `links::statuses`, `lead::route` and `e2e.rs`'s
  `linked_request`. And only the lease's holder may change a link's state: a mirror's status check
  must report, not close.
- **In S37's terms** it's a standing grant, `arugula exec grant v1`:
  ```text
  resource pane <id> <epoch>
  to lead:<conv id>@account:<owner account id>
  caps instruct view (and approve, if the maker holds it)
  ```
  - `lead:` is a new principal kind beside `account:`, `agent:` and `team:`. It's a daemon acting for
    a conversation, held to the grant maker's capabilities (S37 decision 8), and to the lease.
  - `may(principal, cap, resource)` checks the device's chain to the owner's root, the token, and
    the capability.
  - It needs no new signature format: the grant is S37's, made by the person who links the pane, from
    their device, as now.
- **Members instruct the lead through membership** (S37 q7c): an `@lead` from a member is input to
  the lead. What the lead may do on a link is the link grant's. Moving the lead changes neither.
- **The order of work:** this lands after S37's M88 (signed grants). Until then the prototype's rule
  (a conversation's grant counts for the holder) is enough for links on the owner's own machines, which
  is #978's done-when.

## q6: events and cards

**Who posts a link's events:** today the home's watchers post them (M83). There are two ways:
- **(a) Through the lead's machine.** The holder runs the watchers, over the relay as the home does,
  and seals what they post with the conversation's key. That's what the prototype does: L1's turn
  ending was entry 6, appended by box-b after the move.
- **(b) From the pane's machine.** It can encrypt for a conversation it isn't a member of, with a
  write-only key: the conversation publishes an X25519 public key (its private half sealed in the
  record), and the pane's machine seals each event to it with HPKE. It can write, not read. Control
  would accept such entries from a device the grant names.

**Recommendation: (a).** The watchers exist, the lead needs them on its machine anyway (it's woken by
them), the rules that filter them (a turn ends after work, only asks are cards) run in one place,
and the pane's machine stays a pane's machine. (b) helps only when no owner's machine is online, and
then there's no lead to tell. Keep (b) as the step if a conversation without a lead needs its links'
events.

**Cards when the lead's machine is offline:** a card is answered by its ask's id through the pane's
machine's `act` (M83), sent by the holder under the grant. With no owner's machine online:
- the card already in the conversation stays, and says "waiting for one of Jake's machines to come
  back";
- anyone who may drive the pane itself can answer it on the pane, as always;
- the answer isn't queued in control to be sent later: a late answer to a permission prompt could do
  something nobody meant by then.

Jake's call is below (decision 4).

## q7: migration

**On upgrade, the home moves each conversation in**, as `convstore::upload` did in the run:
1. a new key, wrapped to every device of the owner's account and of each member's account (the roots
   the record keeps, #856);
2. the record sealed, and the timeline appended in order, so control's numbers are the old message
   ids (read marks, cursors and quotes stay valid);
3. the home takes the lease first, with the cursor at the last message, so its lead keeps going with
   nothing lost;
4. the local files move to `<state>/threads/control/`, where they're the mirror's cache. An older
   daemon (a downgrade) doesn't see them, so it can't run a second, forked copy of the conversation.
   On a downgrade it simply has no conversations.

It needs control to say `conv-store` among its features, and the daemon to be joined. A daemon with
no control keeps its conversations local, as M81 made them.

**What a machine that isn't upgraded sees:**

| machine | what happens |
|---|---|
| another of the owner's machines, old | nothing: it never had the conversation. It can't lead until it's updated, and control's page says so |
| a linked pane's machine, old | its M82 grant names the home's device. Links work while the home holds the lease. If another machine takes it, the old machine refuses it, and the link shows "box-b needs an update for the lead to move here" |
| a member's machine (another account), old | it never had the conversation: members reached the home. Its owner reads through control's page or an upgraded machine |
| a web client served by an old daemon | the daemon serves what it always did |
| a web client from a new daemon or control, against an old home | the old home serves its local conversation, as before |

## q8: clients

**First, nothing changes for clients.** Each of the owner's machines keeps a full mirror and serves
the same thread API (`/api/threads/conv-…`, `ServerMsg::Thread`), so the Chat page, the phone and push
work through whichever owner's machine is online. That's #978's done-when (geek asleep, jake-mini up),
and it's what the prototype ran: box-b served the conversation the moment it mirrored it. Only the
conversations list needs to dedupe the same conversation from several machines (by id).

**Then, reading straight from control** (#241's original idea, for conversations only), for:
- members of another account, who today reach the home, and would need one of the owner's machines
  online;
- the owner with every machine asleep, who could at least read and write (no lead).

What it takes:
- control's API for devices (session cookie plus the device's signature) to list a person's
  conversations, fetch entries after N, fetch their wrapped keys, and append;
- the page unwraps with its device's X25519 key (WebCrypto `deriveBits`, as Noise does) and opens
  entries with AES-GCM;
- **unread counts:** read marks (the last entry number) per person per conversation, kept by control
  in the clear. They leak when someone read, not what. Mentions and cards are known only after
  decrypting, so the page counts them;
- **search:** on the device, over what it has opened. That's how chat search works already (client
  side). The desktop app and phone keep an index in IndexedDB;
- **offline reading:** ciphertext and wrapped keys cached in IndexedDB, opened with the device key,
  which is non-extractable there;
- **push:** the holder pushes (the prototype sends a conversation's notifications only from the
  holder, so they aren't sent once per machine). With no holder, control sends a notice that says
  only "a new message in a conversation", as its own notices do.

## Decisions for Jake

Each is a choice, with a recommendation.

1. **The cipher.**
   - **(a) Per-conversation key in epochs, wrapped per device (recommended).**
   - (b) MLS (openmls in wasm, a leaf per device).
2. **The lead's Claude Code session when it moves.**
   - **(a) A fresh session with a handoff from the record (recommended).**
   - (b) The transcript sealed in control and resumed (same path needed; a dead machine can't hand it
     over).
3. **Who posts a link's events.**
   - **(a) The lead's machine, as the home does now (recommended).**
   - (b) The pane's machine, with a write-only conversation key.
4. **A card when none of the owner's machines is online.**
   - **(a) It waits, saying so; whoever may drive the pane can answer it there (recommended).**
   - (b) The answer is queued in control and sent when a machine comes back.
5. **Where clients read.**
   - **(a) Through any owner's machine's mirror first (M94), straight from control later (M97)
     (recommended).**
   - (b) Straight from control from the start (the web client's crypto and the mirror in one go).
6. **Where the lead runs after the home comes back.**
   - **(a) Where it is, until that machine goes away or the owner moves it (recommended).**
   - (b) Back to the home, or a machine the owner prefers, whenever it's online.

## The milestones (from M93, for #978)

Each builds on the previous one unless it says otherwise. M93 to M98 are unclaimed (checked
2026-10-17: GitHub issues and PRs, and the other worktrees). Nothing from this branch merges as is.

- **M93: a conversation's record in control, end to end encrypted.**
  - `arugula_e2e::conv`: HPKE base-mode wraps, epochs, entries signed by the author's device and
    chained by hash, pinned in `frozen.rs`.
  - Control's tables (q2) in `SCHEMA` and `migrate()`, with `control-wire` messages, limits, padding,
    the quota, deletion with the conversation and the account, and the `conv-store` feature.
  - Blobs for images.
  - DECISIONS: "Threads live on the daemon that owns the pane; conversations on their home" becomes
    "conversations in control, as ciphertext". `docs/control-e2e.md`: a section on conversations and
    what holds against control.
  - **Done when:** a test fleet's conversation round-trips through control, a dump of control's
    database holds no message text, and a dropped, reordered or forged entry is caught by a member.
- **M94: every owner's machine mirrors it, and existing conversations move in.**
  - The mirror and posting through control (`Api::ThreadCompose`, `ThreadIngest`, the record's
    version and merge).
  - Migration on upgrade (q7), with the old files moved aside.
  - The conversations list deduped by id across machines.
  - Mixed versions tested (`ARUGULA_FEATURES=""` playing the last daemon).
  - **Done when:** a conversation made on one machine reads and takes posts on another of the
    owner's, from the Chat page, with the first one off; an upgraded home keeps its history.
- **M95: the lead's lease.**
  - The lease in control, the fencing token on the lead's writes, renewal by the wall clock, release
    on shutdown, and *Run the lead here*.
  - The lead's state (`ConvLead`) in the sealed record; a new holder starts a lead with the handoff
    and is told it moved.
  - Watchers and pushes only on the holder.
  - **Done when:** with its machine stopped, another of the owner's machines takes the lease and
    answers `@lead`; the first, woken or restarted, posts nothing; a fenced write with a stale token is
    refused, in tests and on the rig.
- **M96: links follow the lead** (after S37's M88).
  - Link grants to `lead:<conv>@account:<owner>`, as S37 standing grants, capped at their maker's
    capabilities; M82's grants read as the conversation's after migration.
  - The pane's machine checks the lease token (its watermark, and control once for a higher one).
  - Events and cards through the holder (q6), and a card that waits when no owner's machine is online.
  - "Needs an update" for an old pane machine.
  - **Done when:** after a move, the new holder prompts a link on a third machine, and the old holder,
    woken, is refused there.
- **M97: clients read from control.**
  - Control's API for devices, the page's unwrap and open (WebCrypto), and members of other accounts
    reading without the owner's machines.
  - Re-wraps for new devices and for members added with history, and a new epoch on removal and
    "from now".
  - Read marks in control, mentions and search on the device, an IndexedDB cache for offline reading,
    and push with no holder.
  - **Done when:** with every one of the owner's machines off, a member reads and posts from the
    phone, and a member taken out reads nothing posted after.
- **M98: the tracker's run, live.**
  - #978's done-when, in its order: with geek asleep, Jake opens on the phone a conversation made on
    geek that links a pane on geek and one on jake-mini; its history is there; `@lead run the tests on
    jake-mini` is answered by a lead now on jake-mini, which prompts that pane and reports; geek wakes
    and starts no second lead; a dump of control shows no message text.
  - Anything the run finds.

## Reproduce

```
export CARGO_TARGET_DIR=$PWD/target
(cd web && pnpm install && pnpm run build)
cargo build -p arugulad -p arugula -p arugula-control
env -u ARUGULA_SOCK -u ARUGULA_PANE TARGET_DIR=$PWD/target/debug \
  node --experimental-strip-types --no-warnings spikes/s38-conversations-in-control/rig.ts $RIG &
spikes/s38-conversations-in-control/setup.sh $RIG   # L1 on box-b, the conversation on box-a, the link
spikes/s38-conversations-in-control/flow.sh $RIG    # the move, @lead, the dump, the sleep, the restart
```

- `RIG` is any empty directory; the daemons pick a short socket path themselves when it's long.
- L1 runs `claude` under the machine's own login; the leads are agent blocks on haiku.
- `restart.sh RIG box-a` restarts a machine's daemon by its exact PID, with Node on its `PATH` (the
  agent block's adapter needs Node 20+).
- Stop the rig with SIGTERM to the `rig.ts` process, then each daemon `restart.sh` started, the panes'
  `arugulad _shim` processes and their children, by PID.
