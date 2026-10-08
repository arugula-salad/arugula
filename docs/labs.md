# Labs

What a machine offers only when the owner turns its flag on.
These features work, but a stranger doesn't have what they need (a
Fountain login, the studio, a sandbox provider, our own chant setup), so
the public docs at [docs.arugula.io](https://docs.arugula.io/) don't
describe them at all. This page is the team's description of them. It
was the *Labs* sections of the public features, teams, advanced and
control pages until those moved to the site.

## Turning a feature on

Each feature has a flag, off on a new install:

| Flag | Turns on |
| --- | --- |
| `chat` | Threads on panes and sessions, and the chat page |
| `huddles` | Voice calls on a session |
| `vms` | VM tabs and panes, machines and the sandboxes page |
| `fountain` | Fountain agents and the runner view |
| `studio` | Studio app blocks |
| `workspaces` | Chant workspace blocks |
| `guest-ssh` | *Invite over ssh…* for a guest with only OpenSSH |
| `swarm-themes` | The swarm's city, hive and timeline views |
| `forges` | GitLab and Forgejo pull request and issue blocks (GitHub's are always on) |

A flag also lists its feature's tools, commands and options in `arugula mcp`,
`arugula --help` and `arugulad --help`. They all keep working without it, and
just aren't offered; GitLab and Forgejo blocks are refused.

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
an empty `labs` file in the state directory (0.24 and later), or `labs` in
`flags.json`, means every flag the file doesn't name is on. So a machine
that had Labs keeps all of it, and turning one feature off there leaves the
rest. A starting daemon moves the old file into `flags.json` and removes it.

- **Per machine.** They're read by the machine that serves the page, so every
  machine you want a feature on needs its own. Someone you share a session
  with sees chat and huddles on your machine if it has them on, and not
  otherwise.
- **No restart.** The daemon reads the flags whenever it's asked.
- **Each feature still needs its own setup.** Fountain needs a login, studio a
  link, VMs a sandbox provider, guest ssh its listener; a flag only stops
  its feature from being hidden.

## What labs unhides

- **CLI (`arugula --help`):** `fountain`, `studio`, `app`, `workspace`,
  `guests`, `machines`, `sandboxes`; `agent --fountain/--as/--vault/--vm`,
  `run --vm/--vm-tab/--image/--sandbox`, and
  `share --guest/--rw/--reusable/--relay/--addr/--name`
  (`LABS_COMMANDS`, `LABS_OPTIONS` in `crates/cli/src/main.rs`).
- **Daemon (`arugulad --help`):** `--guest-ssh`, `--guest-ssh-host`,
  `--studio-file`, `--wisp-url`, `--wisp-token-file`
  (`LABS_OPTIONS` in `crates/daemon/src/args.rs`).
- **MCP:** `read_thread` and `post_thread`, and the kinds `show:app`,
  `show:workspace`, `show:fountain`, `list:fountain_agents`,
  `list:fountain_agent` (`UNLISTED` in `crates/daemon/src/mcp/tools.rs`).
  Unlisted tools still answer when called by name.
- **Web client:** `Client.has("vms" | "fountain" | "studio")` and
  `hasLabs()`/`hasThreads()` in `web/src/client.ts`: VM panes and tabs,
  *Sandboxes…*, *Split (local)* and a VM tab's *Machine* items, studio
  apps, Fountain, *Open as workspace*, *Invite over ssh…*, threads, chat,
  huddles, and the swarm's city, hive and timeline themes
  (`web/src/swarm/view.tsx`).
- **Machines (VMs).** A *machine* (`--machine mN`, `mN:PATH`, `--machine
  local`, MCP `run`'s `machine`) is a VM or sandbox from the provider
  (`crates/daemon/src/labs/machine.rs`). Examples that use them: `arugula open
  --machine m2 :3000`, `arugula edit --machine m2 ~/app`, `arugula view
  m2:src/main.rs`, `arugula fs cat m2:~/log.txt`, `arugula agent --machine
  m3 …`, and `arugula --host s1 ls` for a sandbox with a resident daemon
  (`arugula hosts promote`), which wakes it.
- **Control:** *New hosted VM* and *Delete this VM* in the host menu,
  offered when control says hosted sandboxes are open (not by the `labs`
  file). Hosted sandboxes run on control's provider, which writes their
  trust files, so their operator can read them; deleting an account
  deletes its hosted VMs.

## Threads, chat and huddles

Chat and huddles are on for a machine that has labs. The file belongs to
the machine, so on a team every team machine needs one. Someone you share
a session on a machine with labs with sees chat and huddles there; on a
machine without it, nobody does.

- **Threads on panes and sessions**. Every pane and every session has
  a thread where the people working on it talk: *Thread* in a pane's menu,
  *Session thread* in the session menu, or the bubble on a pane. Messages
  arrive live on every window and phone. The machine that owns the pane
  keeps them (`<state>/threads/`), so they survive restarts and upgrades,
  outlive the pane, show up in `search`, and never pass through control
  unencrypted. Watchers read and drivers post. A private pane's thread is
  its owner's, and someone shared "from now" sees messages from then on.
  Each person has their own unread count: on the pane's bubble, a dot on
  the session button, and a folded corner in the swarm. `@name` notifies someone, on
  their phone too (a tap opens the thread; team members are reached
  whether or not they're connected). *Quote selection in thread* posts terminal output as a
  quote that stays readable after the pane scrolls; clicking it jumps back
  to the output. `@agent` (or `@claude`) in a pane's thread goes to that
  pane's agent as a follow-up, from whoever may drive it, and agents read
  and answer with the MCP tools `read_thread` and `post_thread`. Only an
  `@` that reached someone is marked in the thread; one that reached no
  one (a name nobody here who can read the thread has, or an `@agent` in a
  session's thread or from someone who can't drive the pane) stays plain
  and the poster, and no one else, is told so under the message (and in
  `post_thread`'s `unreached`).
- **An @ of someone who can't see the thread offers to invite them**. When you, the owner, write "@sam look" and Sam is someone this
  machine knows (shared with elsewhere, or on a roster it checked) but
  can't read that thread, the composer says "Sam can't see this. Invite
  them?" in place of the "nobody" note. One click is the invite, as a
  viewer, with your message as its note: Sam's push opens that thread.
  By default Sam sees that message and what follows in that thread only;
  *Share the whole thread* gives that thread's history instead. Either
  way every other thread of the session starts at the invite, like any
  "from now" share, and a later role change keeps what Sam could read.
  Nobody else's post is offered anything, so an @ never tells an editor
  who exists. Agents get no offer: `post_thread` points them at
  `invite_person`.
- **Huddles**. A voice call on a session, for the people working in
  it: the headphones button by the session's name (or *Start a huddle* in
  the session menu) starts one, and everyone with the session sees it's on
  (the button goes green with how many are in) and joins with a click. Up
  to 5 people. The huddle bar stays in the corner while you move between
  tabs, sessions and chat (where it sits in the sidebar's corner): who's in, who's talking, who's muted,
  *Mute* (Ctrl/Cmd+Shift+Space) and *Leave*. Audio goes straight between
  devices, encrypted end to end (WebRTC, DTLS-SRTP), through a TURN relay
  when there's no direct path; the machine only introduces the members.
  Through control, each device signs its call fingerprints with its device
  key and the others check them, so neither the machine nor control can
  listen in by standing in the middle: one of your own devices shows as
  verified, someone else's as signed, and a mismatch is refused. A
  connection with no device key (over a tailnet, or local) shows as
  unverified. Someone whose access is removed drops out at once. A machine
  restart ends its huddles, and the page joins again when it's back. On an
  iPhone the mic stops while the app is in the background; the bar says
  so. The Linux desktop app's web view has no WebRTC, so the app runs the
  call itself (WebRTC in Rust, Opus, and WebRTC's echo cancellation and
  noise suppression), with the same bar and buttons.
- **Chat: every thread in one place.** *Chat* (beside *Panes* and *Swarm*
  in the bar, the phone's sheet, or the command palette; `/#chat`) is a
  page of its own, laid out like a team chat. On the left: *Activity*,
  any huddles that are on, then each machine's sessions as channels with
  each pane's thread under its session, newest first, unread in bold and
  @mentions counted. Fold a machine away, show only what's unread, or drag
  the sidebar wider; the browser remembers. The panes stay as they were
  under the page; *Panes*, or Back, returns to them.
  - **Messages** carry the poster's picture, run together under one
    heading, and have a line between days and a red *New* line at the
    first one you hadn't read. An agent's are badged. Text takes a little
    Markdown: `code`, fenced code blocks, **bold**, *italic*, links and
    @mentions (yours stand out); anything else, HTML included, stays text.
    Hover a message to quote it in your reply, copy a link to it (the link
    opens the thread at that message), or go to its pane.
  - **Writing:** Enter sends, Shift+Enter makes a new line, and @ offers
    the people here (and `@agent` in a pane's thread). An unsent message
    waits in its thread while you look at another.
  - **The header** says where the thread is and what its pane is doing,
    who's in it, and has the huddle button and *Go to pane* (or *Go to
    session*). ⓘ opens details: the pane's screen, live (or each of the
    session's panes), and the people.
  - **Getting around:** the search field in the bar searches every thread
    on every machine you can read as you type. *Activity* lists messages
    that mention you and agents' answers to you. Ctrl/Cmd+K jumps to a
    channel by name. Alt+↑/↓ moves between channels, Alt+Shift+↑/↓ between
    unread ones, and Shift+Esc marks everything read.

### Who sees threads, and where they live

Every pane and every session has a thread: *Thread* in a pane's
menu, *Session thread* in the session menu, or the bubble on a pane.

- **Who sees what:** the same people as the pane or session. Watchers read;
  drivers and owners post. A private pane's thread is its owner's (on a
  team machine, the team's owners'). Someone you shared with "from now"
  sees messages from then on.
- **Where it lives:** on the machine that runs the pane, like its output.
  Control relays it encrypted, as it does the terminal.
- **@name** notifies them, on their phone too, whether or not they're
  connected (team members included); tapping it opens the thread. **@agent** in a pane's
  thread goes to the agent in that pane as a follow-up, if you may drive
  it; agents answer in the thread.
- **@name of someone who can't see it:** if you own the machine and it
  knows them (shared with elsewhere, or on a team roster it checked), the
  composer offers to invite them. They see your message and what follows
  in that thread, or the whole thread if you choose; no other thread's
  past.
- *Quote selection in thread* (pane menu) posts what you selected in the
  terminal; clicking the quote jumps back to it.

### Huddles: who joins, where the audio goes

A session can have a huddle, a voice call for the people in it: the
headphones button by the session's name, or *Start a huddle* in the
session menu. Up to 5 people.

- **Who can join:** anyone the session is shared with, watchers too. A
  read-only link can't. Someone whose share is removed drops out at once.
- **Where the audio goes:** straight between the devices in the call,
  encrypted end to end. When two devices can't reach each other directly
  it goes through a TURN relay (Cloudflare's, for the hosted control),
  which carries only ciphertext; Cloudflare sees addresses and how much is
  sent. The machine that runs the session only introduces the members.
- **Who you're talking to:** through control, each device signs its call
  fingerprints with its device key. Your own devices show as verified,
  other people's as signed (by the device their machine vouches for), and
  a mismatch is refused. Over a tailnet there are no device keys, so
  members show as unverified.

Huddles don't go through control. Control hands machines short-lived
TURN credentials (from Cloudflare, for the hosted control); a relayed
call's audio is encrypted end to end, so the relay sees only addresses and
volume. Operating TURN for the hosted control is in
[control-ops.md](control-ops.md#turn-for-huddles).

## Chant workspaces

A workspace block shows a chant workspace beside your terminals: the gates
waiting for a person, a card for each member, and the workspace's records.
From a member's card you open a shell, an agent, its changes or its graph
there, or run one of its ops. The block reads the workspace through chant's
read contract (contract 1), with the workspace's own chant, so what it shows
is what chant says (`crates/daemon/src/labs/workspace/`,
`web/src/blocks/workspace.tsx`). DECISIONS.md has what it reads and joins
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
watched environment (`web/src/ui/gate-why.tsx`).

*Approve* runs the `chant approve` line `chant workspace status` printed
for the gate, in the member's directory. That line has `--plan <digest>`,
so the approval is for the plan you were shown, and `--sign` for a gate
that needs a signed approval (`crates/daemon/src/gate.rs`). The owner and
drivers approve, from the block, the rail, the phone's sheet, a push or
`arugula tui`; watchers see the cards without buttons. Approving doesn't run
the op: run it again to walk through the gate.

*Expire* turns a gate down: `chant approve <op> <gate> [--env E] --expire`
clears the waiting gate without approving it, so the next run of the op
stops there again. It's on the block's gate card, the phone's sheet,
`arugula tui` and the push, for chant gates only (hud has no route for it).

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
`web/src/blocks/workspace-graph.tsx`).

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
