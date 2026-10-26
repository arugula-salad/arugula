# Labs

What a machine offers only when the owner turns its flag on.
These features work, but a stranger doesn't have what they need (the
studio, a sandbox provider, our own chant setup), so the public docs at
[docs.arugula.io](https://docs.arugula.io/) don't describe them at all.
This page is the team's description of them. It was the *Labs* sections
of the public features, teams, advanced and control pages until those
moved to the site.

Managoat was here until it left Labs (#942, F5). Its daemon side was
removed on 2026-10-18 and control's on 2026-10-23; agents in the cloud run
on hosted workspaces now ([managoat.md](managoat.md)). Huddles,
voice calls on a session, were here until they were removed (2026-10-18,
DECISIONS.md "Huddles are removed"); a `huddles` flag left in `flags.json`
does nothing.

## Turning a feature on

Each feature has a flag, off on a new install:

| Flag | Turns on |
| --- | --- |
| `chat` | The chat page (chat itself is control's), and bringing old threads into control |
| `vms` | VM tabs and panes, machines and the sandboxes page |
| `studio` | Studio app blocks |
| `workspaces` | Chant workspace blocks |
| `guest-ssh` | *Invite over ssh…* for a guest with only OpenSSH |
| `forges` | GitLab and Forgejo pull request and issue blocks (GitHub's are always on) |
| `agents` | Offering Claude Code subagents from this machine as A2A agents, and the team's agent catalog (#403) |
| `docs` | Doc blocks: a report, plan or write-up an agent builds beside its pane, section by section (#962), kept in a library you can list and reopen (#1492) |

A flag also lists its feature's tools, commands and options in `arugula mcp`,
`arugula --help` and `arugulad --help`. Without it they aren't offered, and
they don't work either: the feature's routes, its blocks, its MCP tools called
by name are refused, saying how to turn the
flag on (`arugulad flags NAME on` on that machine, or Developer settings). A
hidden CLI command run against a daemon without the flag says the same, so
with `--host` or `--ssh` it's the other machine's flag that counts (#347).
A block made while the flag was on still comes back after a restart; a new
one isn't made.

| Flag | What is refused without it |
| --- | --- |
| `chat` | Nothing on the machine: it shows the Chat page, and, joined to control, brings old threads into it (see [Threads and chat](#threads-and-chat)) |
| `vms` | `/api/machines`, `/api/sandboxes`, a new VM pane or tab, a borrowed sandbox (`run --sandbox`), *Share machine with tab* |
| `studio` | `/api/studio…`, app blocks, `show` kind `app` |
| `workspaces` | chant workspace blocks |
| `guest-ssh` | `/api/guests` (list, invite, revoke) |
| `forges` | Forgejo and GitLab blocks and webhooks |
| `agents` | `/api/a2a/…`, the agent catalog block, recipes, `delegate` |
| `docs` | new Doc blocks, the `doc` tool by name, and the Doc library's `/api/docs…` (list, open, delete) |

```
arugulad flags                # the flags, and which are on
arugulad flags chat on        # and `off`
```

Or, as the machine's owner, in the web client: *Developer settings…* in the
session menu (and the command palette, and the phone's sheet) has a switch
for each flag, and the page follows a switch at once, with no reload. The
menus offer it only on a machine that has asked: one where `arugulad flags`
has been run, or a flag set. The desktop app's *Daemon* menu has *Developer
settings…* either way, and so does the page at `/#developer`.

The flags are `{"flags": {"chat": true}}` in `flags.json` in the state
directory, which is `$ARUGULA_STATE_DIR` if set, else
`$XDG_STATE_HOME/arugula`, else `~/.local/state/arugula`; on Windows,
`%LOCALAPPDATA%\arugula\state`. `arugulad flags` writes it directly, so it
works with the daemon stopped. The state directory is kept by
`arugulad uninstall`, the installer and the updates, so the flags survive
them.

Before each feature had a flag there was one switch, and it still counts:
`labs` in `flags.json` means every flag the file doesn't name is on. So a
machine that had Labs keeps all of it, and turning one feature off there
leaves the rest. (The empty `labs` file of 0.24 to 0.28 was moved into
`flags.json` by daemons up to 0.44; it isn't read any more.)

- **Per machine.** They're read by the machine that serves the page, so every
  machine you want a feature on needs its own. Someone you share a session
  with sees VMs or studio apps on your machine if it has them on, and not
  otherwise. Chat is control's and follows control's flag.
- **No restart.** The daemon reads the flags whenever it's asked.
- **Each feature still needs its own setup.** Studio needs a link, VMs a
  sandbox provider, guest ssh its listener; a flag lets its
  feature be shown and used, and sets nothing up.

## What labs unhides

- **CLI (`arugula --help`):** `studio`, `app`, `workspace`,
  `agents`, `guests`, `machines`, `sandboxes`;
  `agent --vm/--machine/--as`,
  `run --vm/--vm-tab/--image/--sandbox`, and
  `share --guest/--rw/--reusable/--relay/--addr/--name`
  (`LABS_COMMANDS`, `LABS_OPTIONS` in `crates/cli/src/main.rs`).
- **Daemon (`arugulad --help`):** `--guest-ssh`, `--guest-ssh-host`,
  `--studio-file`, `--wisp-url`,
  `--wisp-token-file`, `--loam-url`,
  `--loam-key-file`
  (`LABS_OPTIONS` in `crates/daemon/src/args.rs`).
- **MCP:** the kinds `show:app`, `show:workspace` and `list:agents`, and
  `start_agent`'s `recipe` and `vm` (`UNLISTED` and `UNLISTED_ARGS` in
  `crates/daemon/src/mcp/tools.rs`). Unlisted tools called by name are
  refused without their flag. Chat's MCP tools (`read_thread`, `post_thread`)
  aren't labs: they follow the grants in control (see
  [Threads and chat](#threads-and-chat)).
- **Web client:** `Client.has("vms" | "studio")` and
  `hasLabs()` and `flag(name)` in `web/src/shell/client.ts`: VM panes and tabs,
  *Sandboxes…*, *Split (local)* and a VM tab's *Machine* items, studio
  apps, *Open as workspace*, *Invite over ssh…*, threads and chat.
  The swarm's city, hive and timeline views left Labs (2026-10-26): every
  swarm offers them under *Theme*, and `swarm-themes` is a retired flag.
- **Machines (VMs).** A *machine* (`--machine mN`, `mN:PATH`, `--machine
  local`, MCP `run`'s `machine`) is a VM or sandbox from the provider
  (`crates/mux/src/labs/machine.rs`). Examples that use them: `arugula open
  --machine m2 :3000`, `arugula edit --machine m2 ~/app`, `arugula view
  m2:src/main.rs`, `arugula fs cat m2:~/log.txt`, `arugula agent --machine
  m3 …`, and `arugula --host s1 ls` for a sandbox with a resident daemon
  (`arugula hosts promote`), which wakes it.

### Where machines come from

One provider per daemon (the `Provider` trait, `crates/mux/src/provider/`):

- **A Sprites API** (`crates/daemon/src/labs/sprites.rs`): a wispd
  (`--wisp-url`, default `http://127.0.0.1:7788`, token in
  `--wisp-token-file`, default `~/.local/share/wisp/token`) or Fly's
  sprites. The default.
- **loam** (`crates/daemon/src/labs/loam.rs`), when `--loam-url` (or
  `LOAM_API_URL`) is given, e.g. `https://api.loam.inevitable.fyi`. Its
  project key (`loam_…`, from the loam operator) is read from
  `--loam-key-file` (`ARUGULA_LOAM_KEY_FILE`), default
  `~/.config/arugula/loam-key`: one line, mode 0600, beside the other
  credentials in that directory, and never served by `fs`. A missing or
  empty key leaves VM panes off, with a warning in the log. Machines are
  made from loam's `base` template, or from an image pinned by digest
  (`--image ghcr.io/…@sha256:…`; any other `--image` names a template),
  labelled `owner=arugulad`, with an idle rule of pause after five minutes.

A VM pane's terminal is a detachable exec session on its machine, so it
outlives a daemon restart and reattaches from the bytes the pane has. On
loam a pane that has been quiet (nothing typed, nothing printed) for five
minutes lets go of its session so the sandbox can pause (memory kept); its
next keystroke or resize wakes it and reattaches, replaying what was
printed meanwhile (up to 1 MiB). A sandbox still awake past its idle
timeout is followed again, and one whose idle rule is `none` is never let
go of. A command that prints nothing for that long pauses with the
sandbox and carries on when it wakes.

Changing provider doesn't move machines: a VM pane restored on a daemon
whose provider doesn't have its machine finds it gone and starts on a new
one, as after a lost machine.

## Threads and chat

Chat is control's (#978, DECISIONS.md "Chat lives in control, in plain
text"): a team's channels and DMs, and a thread about each pane and each
session, held by control so they work with every machine asleep. It's
behind control's own `chat` flag, the account's or a team's
(`POST /api/flags`); the API is under `/api/chat`
(`crates/platform/src/chat.rs`, control's platform since M138), and the
pages read it in
`web/src/shared/chat-store.ts`, `web/src/shared/chat/rooms.tsx` and
`web/src/shared/chat/room.tsx`. The Chat page is the platform page's
(`app.arugula.io`, M153); control's page keeps each pane's and session's
thread beside it and links to the rest, and since M155 draws no chat page
of its own.

**No machine keeps chat (M125).** A machine's `chat` flag does one thing
now:
- Joined to control, with control's flag on too, it brings the threads and
  conversations an older daemon kept (`<state>/threads/`) into control
  once, then renames the folder `threads.imported/` (M124,
  `crates/daemon/src/chat_import.rs`). A machine that skipped the release
  with the import still does this. Not joined, the folder stays where it
  is, unread. An older daemon is marked "Update to bring its threads into
  chat" under Devices and machines.

What the daemon still does for chat is say who may read each pane's and
session's thread (`POST /api/daemon/chat/readers`, from
`crates/mux/src/mux/chat_readers.rs`): the same people as the pane or
session, a private pane's only its owner, a "from now" share from when it
was made. Control holds the messages; the machine decides who reads them.

- **Threads on panes and sessions.** *Thread* in a pane's menu, *Session
  thread* in the session menu, or the bubble on a pane opens its room
  beside the panes (the whole screen on a phone). *Quote selection in
  thread* copies the selected output into the message, so it reads with
  the machine asleep; its chip opens the pane only for someone who may
  reach the machine. Unread counts show on the pane's bubble and the
  session button.
- **The Chat page**, on the platform page (*Chat* beside *Panes* and
  *Swarm* on control's page, the phone's sheet and the command palette open
  it there, at `/chat`): the team's channels and DMs, then the threads on
  panes and sessions by machine. *Activity* lists what mentions you; the
  search field searches every room you can read, in control. `@name`
  notifies them, on their phone too, and a tap opens the room at the
  message (`#chat=<room>&msg=N` on control's page, which sends it on, M154).
  A control with no platform page, and a machine on its own (the tailnet
  stays first class for terminals, not for chat), offer no Chat at all
  (M155; until then that page said chat needs control, S41 decision 7).
- **Agents in chat** (#1095, then M197/M198): off by default. An agent reads
  and posts only where its machine's owner gave it a `threads.read` or
  `threads.post` grant, as the owner's agent and labelled so; `@agent` is
  text otherwise. [control.md](control.md) has the grants; "What chat does
  for agents" below says what each feature below means for them.
- **Conversations** (#855) were rooms of their own on one machine. Brought
  into control they're read-only archives (S41 decision 3); a new one is a
  channel or a DM.
- **Gone with the machine's store:** an `@` of someone who can't read the
  thread no longer offers to invite them (#297), and `search` and MCP
  `history` find terminal output only.

### What chat does

Everything here is the platform page's (and the phone's), one implementation
in `web/src/shared/chat/`, served by the platform in control
(`crates/platform/src/chat.rs` and `chat/`). Control holds the text, so all
of it works with every machine asleep. [control-e2e.md](control-e2e.md#chat)
says what is stored.

**Writing**
- **Markdown** in every message: lists, quotes, links, tables, and fenced
  code with colours (a small scanner of ours, not an editor's parser) and a
  copy button. Enter sends, Shift+Enter is a new line, and Enter inside an
  open fence adds a line; Ctrl/Cmd+Enter sends from there. ↑ in an empty
  composer edits your last message.
- **Emoji:** `:joy:` draws as 😂; typing `:ta` after a space offers a list
  (recent, then common, first); the picker has search and categories, and
  remembers recent picks on the device. A message of up to three emoji draws
  large. The table is loaded only when a message or the composer needs it.
- **Files and images:** paste, drop or 📎. Up to 10 files of 10 MB each on a
  message, 256 MB per team, 30 uploads a minute. A file is uploaded first
  and waits, its uploader's alone, for an hour until a message names it.
  Pictures show inline and open in a viewer; video and audio play in the
  browser (and seek); anything else is a card to download. The bytes are in
  control's database (`chat_files`), and only readers of the room get them
  (404 to anyone else). The type is read from the bytes, not the name or the
  upload: SVG and HTML are text and download, never run, and every response
  is `nosniff` and sandboxed. Search finds file names.
- **Link previews:** a link becomes a card, drawn when the message is read.
  A link to one of our own objects (an issue, run, deploy or hosted
  workspace on control's hosts) shows its title and state, read as the
  reader, so only a member of the project gets a card; others get the link.
  A GitHub issue or PR of a linked repo is read from the mirror. Any other
  web link is **fetched by control**, server-side: it reads the page's Open
  Graph title, description and image, from public addresses only (private,
  loopback, link-local and similar addresses are refused at every redirect),
  in 10 s and 1 MB, HTML only. Results are cached a day. The image is
  fetched and served by control, so a reader never contacts the site. The
  message's author can take its previews off; a team's owner can turn web
  previews off for the team (the room's details). Docs and panes are the
  machine's, so their links stay links.
- **Reactions:** click a chip to add yours or take it back, or use the
  hover toolbar (three quick emoji, the picker, and ⋯). A message holds 20
  kinds of emoji. Reactions are refused in a read-only room, an archive and
  on a deleted message. Nobody is pushed for one.

**Knowing who is there**
- **Avatars:** control fetches an account's GitHub picture by its login,
  keeps it a day and serves it itself (`/api/chat/avatar/{account}`), so no
  page loads GitHub's hosts and the CSP is untouched. Only the account and
  those who share a team with it get it; for anyone else it's a 404, so a
  picture can't be used to find out an account exists. Only small
  PNG/JPEG/GIF/WebP are kept. The letter block shows until it loads.
- **Presence:** a page beats every 15 s with whether it was used in the last
  10 minutes; the answer is the people visible to the reader. Active, away
  (idle, or chosen) and offline (no page, or chosen: *Appear offline* in the
  account menu). A closed app is offline within a minute. Only the times and
  the chosen mode are stored. A machine's agents show their machine's dot.
- **Typing:** the composer pings while the text changes (at most every 3 s,
  never when empty). Others with the room open see up to three names, then
  "Several people are typing…", for 6 s. It is a short-lived row in
  `chat_events`, so it reaches every platform machine and is never
  replayed, and dropped within seconds. Writing `@arugula` in a project's
  channel, a reply thread in one, or an issue's or PR's thread makes the
  composer's ping say `warm` (once per message; a reply thread's composer
  sends only that, never typing), and control wakes the project's agents box,
  so it's up by the time the message is sent: only for someone who could
  ask, never in a DM, and at most once a minute per project.

**Being told, and not**
- **Levels:** per room, *all messages*, *mentions only* or *nothing*. The
  defaults are by kind in Settings: DMs all, channels mentions, threads on
  panes, sessions and objects mentions. A channel on mentions counts its
  mentions and shows other unread as bold. The old mute is the `nothing`
  level.
- **Do not disturb:** pause every chat push for 30 min, 1 h, 2 h, until
  tomorrow or a typed time, or on a weekly schedule in your time zone. Unread
  counts carry on. *All notices* also holds the inbox and deploy-approval
  pushes. Account and ops alerts (agent tokens, failed deploys) and the
  pushes machines relay (a pane needs you) always go. The check is in the
  platform, before the push, so it covers Web Push, the desktop and the
  phone alike.
- **`@channel` and `@here`:** `@channel` notifies everyone in the room,
  `@here` only those whose presence is active (a page of theirs used in the
  last minutes, not away or appearing offline). Both count as mentions. A team's owners can
  keep them for owners (a post by anyone else is refused, 403), and the
  composer asks first past six people.
- **Keywords:** words (up to 20, a whole word or phrase) that notify you as a
  mention would; kept per account in Settings.
- **Mark unread:** from a message's menu. The room's read mark moves back to
  before it, on all your pages, and the page holds its *New* line there
  until you leave the room.
- **Saved for later:** a star on the message, with an optional reminder
  (in 1 h, tomorrow 9:00 your time, or a time up to a year ahead; one push).
  *Saved* beside *Activity* lists To do and Done. Private to you; control
  keeps which message, never a copy (500 per account).

**Rooms**
- **Topic and description** of a channel, written by its members (or its
  owners, if the team says so); the header shows the topic, and the tooltip
  and the switcher too. **Pins:** anyone who may post pins a message (50 a
  room); the header's 📌 lists them, newest first. A topic change and a pin
  post a small system line ("alice pinned a message").
- **Starred rooms and sections:** star a room to move it to the top; make
  sections of your own, drag rooms into them (on the phone, the room's
  *move*), collapse and sort by name or recent activity. One document per
  account, so the computer and the phone agree; the last write wins.
- **The agents chip:** one chip in the room header ("3 machines' agents")
  lists whose machines' agents can read and post and what each may do. When
  a grant first lets a machine into a channel or a DM, the room gets one
  system line saying so. Revoking says nothing.
- **System lines** are ordinary numbered messages, dim and centred. They are
  never unread, pushed, searched, edited, reacted to or pinned.
- **Hover toolbar and the rest:** quick emoji, picker, and ⋯ (quote, copy
  link, edit, delete, mark unread, save, pin; a long-press on a phone). A
  room scrolled up stops following and offers "↓ N new messages"; Esc reads
  and jumps to the newest. *Messages: Compact* in Appearance is a look kept
  on the device.

**What chat does for agents.** Agents use chat through `read_thread` and
`post_thread` under the `threads.read` and `threads.post` grants, and
nothing else of the above. What they get:
- Reactions: `post_thread` with `react_to_seq` and `emoji` (and `remove`);
  `read_thread` shows each message's reactions. Theirs are the owner's,
  labelled with the machine, and apart from the owner's own.
- Files: `post_thread`'s `files` (paths the agent may read, at most 10 of
  10 MB), and `read_thread` lists each message's attachments with name,
  type, size and control's address.
- Pins: `post_thread`'s `pin_seq` (and `remove`). `read_thread` begins with
  the room's topic, description and pins, marks pinned messages, and shows
  system lines as `* alice set the topic: …` (the platform's own as
  "Arugula"). An agent can't set a topic.
- Markdown, emoji shortcodes, `@login`, `@channel` and keywords work in what
  they write, under the same team rule as their owner's role. It is the
  owner's reach and role that decides.
- No tool for previews (they get the URL text), presence, avatars, levels,
  DND, mark unread, saved, typing, sidebar or starring: those are a
  person's own state, or no use to an agent, and a grant shouldn't reach
  them. Nothing wakes an agent, and a reaction pushes nobody.

### Chat's API

All under `/api/chat` unless said, with the session cookie (a page; the
`cli` column of `crates/control/routes.toml` lists the few the CLI may
call), and each only for someone who may read the room. The older routes
(rooms, messages, read, search, live) are in `routes.toml`; the ones added
for the features above:

| Route | What it is for |
| --- | --- |
| `GET`/`PUT /sidebar` | The caller's starred rooms, sections, collapse and sort (one document, checked by the platform) |
| `POST /presence` | The page's 15 s beat; answers the people visible to the caller. Own mode (auto, away, offline) rides on it |
| `GET /avatar/{account}` | A GitHub picture, for the account itself and people sharing a team with it; 404 otherwise |
| `POST /rooms/{id}/files`, `GET /rooms/{id}/files/{file}` | Upload a file for the next message (bytes as the body, `?name=`); serve one to the room's readers (`?download=1` saves, `Range` honoured) |
| `GET /rooms/{id}/previews?seqs=` | Link previews under those messages, for the caller |
| `POST /rooms/{id}/messages/{seq}/previews` | The message's author takes its previews off or back on |
| `GET /previews/image` | The image of a page that was read for a preview, fetched by control |
| `GET`/`PUT /teams/{team}/previews` | Whether web links in a team's chat get previews: read by members, set by an owner |
| `POST`/`DELETE /rooms/{id}/messages/{seq}/reactions` | Add the caller's emoji (body `{emoji}`) or take back `?emoji=`; neither is an error twice |
| `PUT /rooms/{id}/topic` | A channel's topic and/or description; posts a system line. 403 if the team left it to owners |
| `GET /rooms/{id}/pins`, `PUT`/`DELETE /rooms/{id}/messages/{seq}/pin` | The pins, newest first; pin or unpin, by whoever may post |
| `POST /rooms/{id}/level` | The caller's level for the room (all, mentions, nothing, or back to their default) |
| `POST /rooms/{id}/mute` | The old switch: mute is `nothing`, unmute the default |
| `GET`/`POST /api/notify/prefs` | The caller's default levels, snooze, quiet hours (with time zone) and *all notices* |
| `POST /rooms/{id}/unread` | Move the caller's read mark back to before `seq` (their pages only) |
| `GET /saved`, `PUT`/`PATCH`/`DELETE /rooms/{id}/messages/{seq}/saved` | The caller's saved list; save (with an optional reminder), mark done, remove |
| `POST /rooms/{id}/typing` | The composer's ping |
| `GET`/`PUT /keywords` | The caller's keywords |
| `GET`/`PUT /teams/{team}/settings` | What a team's owners decided: `@channel`/`@here` and topics for owners only. Members read, owners set |

The machine's agents call, with the machine's signature and a grant, under
`/api/daemon/chat`: `POST /rooms/{id}/files` and `GET /rooms/{id}/files/{file}`
(`threads.post` and `threads.read`), `POST`/`DELETE /rooms/{id}/messages/{seq}/reactions`
and `PUT`/`DELETE /rooms/{id}/messages/{seq}/pin` (`threads.post`), beside the
rooms, messages and search of [control.md](control.md). They act as the
machine's owner, labelled with the machine, within the owner's reach.

## Chant workspaces

A workspace block shows a chant workspace beside your terminals: the gates
waiting for a person, a card for each member, and the workspace's records.
From a member's card you open a shell, an agent, its changes or its graph
there, or run one of its ops. The block reads the workspace through chant's
read contract (contract 1), with the workspace's own chant, so what it shows
is what chant says (`crates/daemon/src/labs/workspace/`,
`web/src/shell/blocks/workspace.tsx`). DECISIONS.md has what it reads and joins
itself ("A workspace block asks chant, and keeps a few reads of its own").

### Opening one

A workspace is a directory with a `chant.workspace.json` or
`chant.workspace.jsonc`. When a pane's folder holds one, its menu offers
*Open as workspace*, which opens the block beside the pane. From the CLI:

```
arugula workspace                     # the workspace here, in a new tab
arugula workspace ~/src/estate --split right
arugula workspace --env staging       # watch staging's gates and releases
```

It prints the block's id, then a line with the members, records and gates
waiting. An agent opens one with MCP's `show` (kind `workspace`, the root
as an absolute path).

The block runs the first chant it finds: `$CHANT` in the daemon's
environment, else `node_modules/.bin/chant` in the root or a parent, else
`chant` on `PATH`. chant then decides whether it may read the declaration.
When it can't, the block shows chant's reason code and what to do:
`declaration-missing` (run `chant workspace init`), `root-chant-required`
(run `npm install` at the root), `reader-too-old`, `contract-unknown`.

The bar names the workspace and its root, the environment watched, the
chant version, who approvals are recorded as, and whether it's *live*.
Below it are *Waiting on you* (the gates), *Members* (a card each, with its
kind, directory and tags such as `gate waits`, `2 leased`, `3 undecided`)
and *Records*. The block reads again on open, on ↻, after an approval, and
when the repository changes and then holds still for a moment, so a `chant
run` or an agent at work costs one read, not one per edit. A release or a
gate reached reads at once: a gate shows about a second after `chant run`
stops at it. It keeps watching while nobody looks, so a gate still reaches
the rail, the phone and push.

### Gates

A gate waiting for a person is on the block under *Waiting on you*, and is
attention (a `gate` reason) on the rail, the phone and push:

```
delivery: ship waits at gate approve-ship
0/1 approvals · just now · expires 10/8/2026, 6:59:54 PM
Enforces toy-001: A person approves each ship
plan sha256:3f9a1c0e7b2d · no release yet
```

The card names the decision the gate enforces, from `chant workspace graph
--intent <member dir>`. That read can land a moment after the gate, so the
card says *Reading the decisions…* until then. With no current decision
covering the member it says so, and on a large repository the read can pass
the host's 30 s limit on a command, which the card also says. The last line
has the plan the approval binds and the member's last release in the
watched environment (`web/src/shell/ui/gate-why.tsx`).

*Approve* runs the `chant approve` line `chant workspace status` printed
for the gate, in the member's directory. That line has `--plan <digest>`,
so the approval is for the plan you were shown, and `--sign` for a gate
that needs a signed approval (`crates/daemon/src/gate.rs`). The owner and
drivers approve, from the block, the rail, the phone's sheet or a push;
watchers see the cards without buttons. Approving doesn't run
the op: run it again to walk through the gate.

*Expire* turns a gate down: `chant approve <op> <gate> [--env E] --expire`
clears the waiting gate without approving it, so the next run of the op
stops there again. It's on the block's gate card, the phone's sheet
and the push, for chant gates only (hud has no route for it).

### Who chant records

chant records each approval as a principal: a forge identity such as
`github:alice`, or a signer. By default the owner approves as their Arugula
name, passed as is. Where the workspace wants a forge identity or a signer
(`identity.attribution: identified`), name them. The owner sets this with
*as …* on the bar, or when opening the block:

```
arugula workspace --actor github:alice --principal sam@example.com=github:sam-h
```

`--actor` is the owner's principal; `--principal NAME=PRINCIPAL`
(repeatable) is an editor's, by their Arugula name. MCP's `show` takes the
same as `actor` and `principals`. chant runs on the owner's machine with
the owner's credentials, so an editor's approval goes as `--actor <their
principal> --relayed-by <the owner's>` (chant 0.103.1 and newer). A gate
that needs a signed approval can't be relayed: the owner approves it in the
block, or the editor runs the `chant approve` line with `--actor` where
their own key is. When chant refuses for want of a principal, the error
says which flag to set.

### The environment

A block watches one environment's gates and releases: `local`, unless it
was opened with `--env`. The menu on the bar lists the environments chant
has releases for on `chant/lifecycle`, plus `local`, and *other…* for one
it doesn't list. A pick reads again, and the block keeps it in its config.
The owner and drivers switch it. Gates in the other environments aren't
read: each is another `status`, another chant process on every read.

### Member cards

The owner's buttons on a member's card:

- *Shell*: a terminal in the member's directory, beside the block.
- *Agent*: an agent block there, as the member's agent session (below).
- *Changes*: what changed in the member (below).
- *Graph*: behold's estate graph on this member (below).
- *Run op*: the member's ops as buttons, and a box for another name. It
  opens a terminal beside the block running `chant run <op>` in the
  member's directory. A gate's card has *Run {op}* for the op it stopped.

A member that is a workspace of its own has *Open* instead, which opens it
in a block of its own.

*Why* (owner and drivers) opens the card on why the member is the way it
is, and who works on it:

```
delivery                                   chant
gate waits  1 leased  2 undecided
DECISIONS
toy-001 decided  A person approves each ship
toy-002 proposed Releases go out on Fridays  Review in hud
Replaced: toy-000 Ship on every merge
2 commits changed delivery while no decision covered it.
LEASES
W-001 held by shipper · until 7:29 PM  pane %3
RUNS
arugula-3-1791422365955 running · as shipper · W-001 · just now  pane %3
```

- Decisions: those covering the member, ranked by chant, with the ones they
  replaced. A proposed one has *Review in hud*, which opens
  `<hud>/decisions#<id>`; the owner's first click asks for hud's address,
  and the block keeps it.
- Undecided commits: how many changed the member while no decision covered
  it (`intent-commit-undecided`). The `undecided` tag counts the same. It's
  a tag, not attention: only gates are.
- Leases: `status`'s, in the member's ledger or on a work item covering it.
- Runs: agent runs on the member from about the last two weeks (`chant
  workspace runs`), those of the sessions its declaration binds and those
  chant's walk joined to its commits.

A run or lease links to its pane only when the agent block there recorded
that run and the pane is still open; a run id from another daemon, or one
typed by hand, links nowhere. Viewers see the tags but not *Why*: opening a
card is a block call, and a viewer's are refused.

### Agents on a member

*Agent* starts an agent block in the member's directory as the agent
session the declaration binds to the member, the first of the member's
`agents` in `chant workspace ls --json`; with a chant whose `ls` doesn't
list them, the block reads the declaration (`.json` or `.jsonc`) itself
(`crates/daemon/src/agent/chant.rs`):

- The agent runs with `CHANT_AGENT` set to that session, so chant judges
  its writes by the session's scope. Claude is also told on its system
  prompt.
- Each turn is a run in the workspace's run ledger: `chant workspace runs
  start` when the prompt goes, `runs end` with the stop reason, tokens and
  cost when the turn ends. The run id is `arugula-<pane>-<ms>`. A failed
  write is a note in the transcript; the turn goes on.
- Each prompt carries a note naming the trailers the turn's commits end
  with, so `graph --intent` joins a commit to the run that made it:

  ```
  Chant-Agent: app
  Chant-Run: arugula-7-1790848800000
  ```

  It's part of the prompt, so Codex and any other ACP agent get it; its
  `_meta` (`arugula/chantRun`) keeps it out of the transcript. A member
  with no agent session gets only `Chant-Run`. A prompt starting with `/`
  goes alone, as typed, with no note; its run is still recorded.

### Changes on a member

*Changes* on a member card opens a diff block of the member's directory,
from where the branch left the default one (the merge base with
`origin/HEAD`, `main` or `master`) to the working tree. On the default
branch itself it compares the working tree with `HEAD`, and says committed
work isn't shown. A menu on the bar compares with the last commit, the last
3 or 10, or a revision typed in, and goes back to the branch point; the
block keeps the choice.

With a file open, each hunk names what made its added lines
(`crates/daemon/src/labs/workspace/why.rs`):

```
@@ -1,8 +1,8 @@
why-001 The server answers on one port
Run arugula-2-1791429314234 (app, claude-code)
1 not committed yet; app holds the lease on W-001
```

- The decision the hunk's commit or run carries out, else the one chant
  ranks first for the file; hover for its state.
- The run, linked to its agent pane when an open agent block on this daemon
  wrote it.
- Lines not committed yet, with who holds a lease on the work covering the
  file, if anyone.

A hunk that only removes lines names nothing. A plain diff block never runs
chant.

### The estate graph

*Graph* on a member card (the owner's) shows behold's estate graph in the
block, on that member. The first time, the block starts `behold serve
<root> --port 0` on this host and frames it through the block's own site,
as an editor block frames code-server, so it needs `--block-listen`. It runs
`$ARUGULA_BEHOLD` (a command) if the daemon has it, else
`node_modules/.bin/behold` in the root, else `behold` on `PATH`. *Graph* on
another member moves the view there (`crates/daemon/src/labs/workspace/graph.rs`,
`web/src/shell/blocks/workspace-graph.tsx`).

The graph opens on the source. *live {env}* on its bar reads the block's
environment live, with this host's credentials. The gates behold draws
follow the block's environment, without an approve button: the block's gate
cards approve. A click on a member in behold opens a Shell there; *a click
opens* on the bar switches that to Changes, or to nothing.

behold gets the environment a Shell pane in the member gets (the login
shell's variables), and Arugula adds no credentials. It listens on loopback
only, and only the owner starts or reaches it. It runs until the block
closes or the daemon stops, either of which stops its process group. One
that exits by itself shows the end of its log and *Try again*. It isn't
available for a workspace on a VM; the block says so.

## Agent recipes

The first part of the AGENTS track (#403): agents one person offers to other
people's agents. With `agents` on (M76, #398):

- **A recipe is a Claude Code subagent file**, as it is:
  `.claude/agents/NAME.md` in a project, or `~/.claude/agents/NAME.md`. Its
  frontmatter (`name`, `description`, `tools`, `disallowedTools`, `model`,
  `permissionMode`, `skills`, `mcpServers`) and its body, the prompt, are
  all Arugula reads. Nothing writes to it.
- **`arugula agent --as NAME`** (MCP: `start_agent {recipe}`) runs one in an
  agent block here, in the current project. The prompt is appended to
  Claude Code's, after a line naming the agent, because Claude Code's
  adapter doesn't select a subagent. The model, tool lists and permission
  mode are applied, and so are the skills: each `SKILL.md` is preloaded
  into the prompt, as Claude Code does for a subagent, and the whole
  skill is put in a local plugin for its other files. The MCP servers
  are named ones (from the project's local config in `~/.claude.json`,
  its `.mcp.json`, then the user's `~/.claude.json`) and ones written into
  the recipe. Their `${VAR}`s come from your shell environment, and
  credentials go by reference, never on a command line.
  Nothing else comes along: the session runs with `--strict-mcp-config`,
  so the account's claude.ai connectors are left out too. The block's
  header shows what came along and what didn't, and why. (Wearing a
  Managoat agent here, `--as-fountain`, was retired on 2026-10-15: a block
  saved wearing one comes back saying so, and doesn't start.)
- **Offering one** is the machine owner's choice: `arugula agents offer
  NAME [--dir PROJECT]` (`--stop` to stop), kept in
  `<state>/agent-offers.json`. Nothing is offered by default. `arugula
  agents recipes` lists the recipes a project has, offered or not.
- **Cards:** `arugula agents` (`GET /api/a2a/agents`) lists what's offered
  here as A2A 1.0 agent cards, and `arugula agents card NAME` shows one.
  A card's interface is `arugula://MACHINE/api/a2a/agents/NAME`, JSON-RPC,
  with a required extension saying it's reached over Arugula's own
  channel. That interface takes no tasks yet: every method answers A2A's
  `UnsupportedOperationError` until M78 (#400). The cards are for anyone
  who reaches the machine, and offering is the owner's alone.

### The Agents page

**Agents…** in the session menu, or the phone's sheet, opens a page at
`#agents` (the owner's). It has two tabs:

- **Recipes** lists every recipe this machine has: yours in
  `~/.claude/agents`, and those in `.claude/agents` of each project it
  knows. A project is known if you added it on the page, if a recipe is
  offered from it, or if a pane here is open in it.
  - **New recipe** and **Edit** use a form (name, description, model, tools,
    permission mode, skills, MCP servers, prompt) that writes the file as
    Claude Code reads it. An edit keeps any keys the form doesn't know, and
    MCP servers written inline in the file. A new recipe never replaces a
    file that's already there.
  - **Run here** opens an agent block wearing the recipe.
  - **Offer** and **Stop offering** put it in, or take it out of, the
    team's catalog.
  - **Delete** removes the file, after asking, and its offer with it.
  - Writing and deleting are this machine's own account's.
- **Team** (`#agents=team`) shows the team's catalog by machine and owner,
  tasks from other people waiting for you (*Allow once*, *For an hour*,
  *Deny*), and standing grants with *Revoke*.

### The team's catalog (M77)

- **Team agents…** (the session and pane menus, the owner's) opens a block with
  every agent offered on the machines this one reaches: the account's own,
  its teams', and teammates' own machines that offer something. Cards are
  grouped by machine and named with their owner. A machine that's offline,
  or doesn't answer, keeps the cards it last had, marked as such. Below
  them, a project's recipes, with *Offer* and *Stop offering*. *Run here*
  runs one of this machine's offered recipes beside the block. *Run there*
  runs one another of your account's machines offers: it starts there, in
  the project it's offered from, in a session named after it, and the page
  goes to it. The Agents page's **Team** tab has the same as *Run* on each
  of your account's agents, and `arugula agents run AGENT [--on MACHINE]
  [PROMPT]` does it from a shell, finding the machine that offers it (this
  one first). Another account's agents only take tasks.
- **For agents:** MCP `list {kind: agents}` (with `query`, `fresh`). **For
  people:** `arugula agents catalog [--fresh]`. Both read
  `GET /api/a2a/catalog` (the owner's), which is kept in
  `<state>/agent-catalog.json` and asked again once it's a minute old.
- **How it's read:** this daemon asks each machine's `GET /api/a2a/agents`
  itself, through control's relay, signed with its own key (`peer.rs`).
  Control never sees a card. Another account's machine is trusted only
  through the root a checked team roster gives for that account.
- **Who gets in:** a machine lets in other machines' daemons as their
  account, never as the owner, for `/api/a2a/` and nothing else. Those are
  this account's machines and its team's. Once it offers something with
  the flag on, they're also the members' of every team its owner is on.
  That's the A2A-only grant: control routes those accounts' daemons to it
  (`AccessList.agents`) and lists it to them, but never to the people's
  browsers. Stopping every offer takes it away again.

### Tasks and consent (M78, M79)

- **Sending:** an agent uses MCP `delegate` (`send`, `get`, `answer`,
  `cancel`, each with `agent`, and `machine` if more than one machine
  offers it). A person uses `arugula agents send MACHINE AGENT TEXT`
  (`here` for this machine, `-` for whichever offers it) and `arugula
  agents task MACHINE AGENT TASK [--answer TEXT | --cancel]`.
- **This machine's own agents** take tasks too: the same JSON-RPC, answered
  here without control, as this machine's own account (no consent card).
  Leave the machine out and the one that offers the agent is used, this
  one first. Either way it's A2A 1.0 JSON-RPC over the
  same relay channel as the catalog, as this machine's account. `send`
  waits (five minutes by default) for the task to finish or ask something,
  and comes back with its reply and its patch's stat. The whole result,
  patch included, is kept in `<state>/a2a/results/TASK.json`, because the
  other machine cleans up once it's been read.
- **Receiving:** a task runs the offered recipe as an agent block in session
  `a2a`, in a git worktree of the offer's project at its `HEAD`
  (`<state>/a2a/work/TASK`). A permission it asks for is answered on that
  block, by this machine's people. A question it asks goes back to the
  caller as `input_required`. When it ends, the reply and a patch against
  that commit are its artifacts. New binary files are left out and named.
  Once the caller has read the ended task, its block closes and its
  worktree goes.
- **Consent:** a task from another account waits until this machine's own
  account allows it. The card is held by the Team agents block, one is
  opened in session `a2a` if none is, and it's pushed like any card. The
  choices are *Allow once*, *Allow for an hour* (that account's tasks for
  that agent) and *Deny*. The answer is checked by account, so a team's
  other owner, though `Owner` on a team box, can't allow tasks there. Grants
  are listed in the block, with *Revoke*, and by `arugula agents grants
  [--revoke ACCOUNT AGENT]`. `arugula agents waiting`, `allow TASK
  [--hour]` and `deny TASK` work from the CLI too.
- **Durable:** tasks are `<state>/a2a/tasks/TASK.json`, and a restarted
  daemon follows the running ones and puts the waiting ones' cards back.
  Grants are `<state>/a2a/grants.json`.
- **The work comes back (M80):** `delegate {kind: review}` (or `arugula
  agents review TASK`) applies a finished task's patch to a scratch
  worktree of your checkout at its `HEAD` (`~/.cache/arugula/review/TASK`)
  and opens a diff block on it, so nothing here changes until you've looked.
  `delegate {kind: apply}` (or `arugula agents apply TASK`) applies it to
  the checkout itself with `git apply --3way`. The result names every
  change it made, and every file with conflicts to resolve, hunks that
  didn't apply (as `.rej` files, when nothing merged), and new binary files
  the task left out. It also says when your checkout lacks the commit the
  task started from.
- **Or a pull request:** `delegate {kind: send, pr: true}` (or `agents send
  --pr`) is for work on the agent's own project, not yours (fixing a bug
  you found in its code). The task's worktree is a
  new branch, `a2a/AGENT-TASK`, off its origin's default branch as just
  fetched, and the agent is told, after its own prompt, to commit there,
  push, and open a pull request with the project's own tooling, without
  merging it. That runs with the receiving machine's forge credentials, as
  its owner, so the consent card says so for another account's task. The
  reply ends with the link, and a `pr` artifact carries it, the branch, its
  commits and whether origin has them. The patch comes too. Once the task
  is read, the branch goes from the project's checkout only if origin has
  it.
- **Cleaned up** once its own caller reads it, this machine's owner
  included for a task they sent (before, only another account's tasks
  were).
- **Not yet:** files that aren't in a repository. There's no
  streaming (`SendStreamingMessage`): the caller polls `GetTask`.
