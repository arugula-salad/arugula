# Decisions

The decisions that still constrain the code, one entry each. Before you change
one of these, read its entry; if you change it, change the entry in the same PR.

Each entry says what was decided, why, where it lives in the code today, and
where it came from. Milestone and spike codes (M2, S5 …) are looked up in
[docs/plan-archive.md](docs/plan-archive.md), which is the old PLAN.md kept as
written. Anything here that the archive says differently has changed since;
this file is the one that matches the code.

Not here: what each milestone built (the archive), how to build and test
([docs/development.md](docs/development.md)), what the product does
([docs.arugula.io](https://docs.arugula.io/)).

## Protocol and transports

### One protocol, two transports
The web client, the CLI, the TUI and the API speak one protocol. On a
WebSocket it is JSON text frames for control messages and binary frames for
terminal bytes; on the Unix socket (a named pipe on Windows) the same messages
travel length-prefixed. A transport never shapes the protocol or the core, so
a new way to reach a daemon is an adapter.
Where: `crates/proto/src/lib.rs` (`ClientMsg`, `ServerMsg`, `Frame`), `crates/daemon/src/server.rs` (`/ws`), `crates/daemon/src/pipe.rs`.
From: [Protocol](docs/plan-archive.md#protocol-one-protocol-two-transports), [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox).

### Terminal bytes are offsets into a log
Output frames are `[u8 kind][u32 pane][u64 offset][bytes]`, big-endian. Every
byte has an offset in the pane's log, so a client resumes by offset, and
`tail`, replay and scrollback restore are seeks. A client that is within 1 MB
of the log's end gets a replay; anyone further behind gets a snapshot.
Why: asciicast has no offsets and inflates the data.
Where: `crates/proto/src/lib.rs` (`FrameKind`, `HEADER_LEN`), `crates/mux/src/pane.rs`.
From: [Decisions table](docs/plan-archive.md#decisions-on-the-briefs-open-questions), M1, M2.

### Some names outlive the rename to Arugula
The product was renamed (#509). Names that something stored or running
depends on stay "illogical" for good: the domains signed or hashed into
certificates, rosters, proofs and push and call tokens, the Noise prologue,
the sync key's HKDF info, the key file header, the checkpoint magic, agent
blocks' MCP token MAC (`illogical block N`), control's hash of a deleted
machine (`illogical gone daemon`), the
browser's IndexedDB `illogical-device`, and the names a restarted daemon or
another version finds running panes and sandboxes by (holder socket, Windows'
pane pipe, systemd scopes, `illogical-eph-`, the resident service). The
state, config, data and cache directories made under the old name are used
where they are. Names that only cross between versions (headers,
`ILLOGICAL_*` variables, service and binary names, download names,
`window.__illogicalApp`) were accepted under both names from the bridge
release (0.25) until every install was on 0.26 or later; since #534 they
are Arugula's alone.
Where: `crates/core/src/rename.rs`, `crates/e2e/src/frozen.rs` (pins the signed domains as hex), "Frozen (#504)" comments.
From: #504, #534.

### The server sends whole layouts, and deltas for summaries
`State` carries the full tree and a `rev` on every layout change (trees are
small). Pane summaries and attention, which change often and across many
hosts, go as field-level `Delta`s.
Where: `crates/proto/src/lib.rs` (`ServerMsg::State`, `Delta`).
From: [M23](docs/plan-archive.md#m23-pane-summaries), S16 (in the [spikes list](docs/plan-archive.md#s16-swarm-spike-summary-cost-fleet-connections-canvas)).

### Wire snapshots are formatter VT bytes plus fix-ups, zstd-compressed on request
On attach or resync a client gets VT bytes that rebuild the pane in a fresh
terminal, because xterm.js needs VT bytes. libghostty's formatter writes most
of them; `wire.rs` fixes what it gets wrong (cursor, saved cursor, blank-cell
colours, hyperlinks, Kitty keyboard flags, alt-screen order). A client that
asks gets them as `SnapshotZstd`, with scrollback capped at what it keeps
(`history`). After a resync a client asks again with `history: 0`: the screen
alone, with a visible "output skipped" rule, so a flood can't keep it
resyncing. Visible-first attach was measured (S10) and is not built.
Where: `crates/vt/src/ghostty/wire.rs`, `crates/proto/src/lib.rs` (`AttachPane`), `crates/mux/src/pane.rs`.
From: [S1/S5 follow-ups](docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named), #1, #49, #53.

### Flow control: acks, a window, and a log that absorbs slow clients
A client that attaches with `acks` sends `ack{pane, offset}` about every 64 KB
and is held to a 512 KB unacked window; past it, it is held back and catches
up from the log (or gets `resync` if the log no longer has the gap). A slow
client never pauses the program: the log absorbs its output. The program is
paused only when the daemon itself falls behind, through a bounded queue (64
chunks), and client requests are served before that queue, so Ctrl-C stops a
flood at once. Clients that don't ack (`attach`, the tmux front end, the share
viewer) get `resync` on a full queue.
Where: `crates/mux/src/pane.rs` (`ACK_WINDOW`, `PROGRAM_QUEUE`).
From: [Protocol, flow control](docs/plan-archive.md#protocol-one-protocol-two-transports), #52.

### Size: a tab has one size, set by its owner
The client that last claimed a tab sets its cols and rows, and the PTYs
follow. Only editors size a tab. A client claims when it opens or switches to
the tab, when it asks ("use this size", the `sized-elsewhere` button), or when
it types there; a plain view (a window resize, a split) only sizes a tab it
owns or nobody does. Other viewers render at the real size, letterboxed on the
desktop and scaled on the phone. The size is never smaller than the split tree
needs. The plan first said "per pane"; the code is per tab.

Typing takes the size only once the owner has been idle for 3 s
(`SIZE_HOLD`; the owner's typing and taking the size both count). Until then
the newcomer types into the owner's size. Such a claim carries `typed: true`;
an older daemon ignores the field and takes it at once. Focusing a window
doesn't claim (web or TUI), and a remote pane claims its host's tab only when
typed into, not because this window owns its home tab. `illogical attach` and
an iTerm2 window resize still claim at once: they are one terminal's own size
changing.
Why: one person moving between devices; "smallest wins" would make the desktop
suffer whenever the phone is open. The hold is #333: two editors typing in
turn, or one person with two windows, resized the PTY at every handover, and
each resize is a SIGWINCH and a redraw. tmux's "smallest client wins" as a
pairing mode isn't done; add it per session if pairing asks for it.
Where: `crates/core/src/mux.rs` (`Mux::view`, `SizeHold`), `crates/proto/src/lib.rs` (`ClientMsg::View`), `crates/mux/src/mux/clients.rs`.
From: [Size arbitration](docs/plan-archive.md#size-arbitration), [M1](docs/plan-archive.md#m1-multiplexer), [M5](docs/plan-archive.md#m5-tmux-control-mode--cc-front-end), #333.

### TypeScript types are generated from `proto`
`web/src/proto.gen.ts` comes from the Rust types (`ts-rs`, `just proto-ts`),
and CI fails when it is stale. The hand-written `web/src/proto.ts` is being
retired.
Where: `crates/proto/src/ts.rs`, `justfile` (`proto-ts`), `web/src/proto.gen.ts`.
From: #387 (the arch tracker), #396.

## State and durability

### The log is the truth; checkpoints are a cache
A pane's history is raw output in 4 MB segments with a sidecar index
(resizes, restores). A checkpoint (GHOSTSNP, zstd, tagged with the engine
that wrote it) speeds up restore, but one that doesn't decode is discarded
and the log tail replayed, because the format changed incompatibly once
without a version bump. Retention is 256 MB a pane.
Where: `crates/mux/src/store.rs` (`RETAIN_BYTES`, `PaneLog`), `crates/vt/src/ghostty.rs`.
From: [S5](docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named), [M2](docs/plan-archive.md#m2-durability).

### The state dir holds secrets: 0700 and 0600
Logs and checkpoints hold what was typed or echoed, tokens included. The state
dir is `0700` and its files `0600`; retention is enforced;
`illogical purge %p` deletes a pane's history. Logs synced from other hosts
are sealed with a key only the home daemon holds.
Where: `crates/mux/src/store.rs`, `crates/mux/src/perm.rs`, `crates/daemon/src/seal.rs`.
From: [M2](docs/plan-archive.md#m2-durability), [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox) (M4c).

### Layout is a file written atomically
`layout.json` holds sessions, tabs, splits, machines and every block's config.
It is written to a temp file, renamed, and the directory fsynced, debounced
to 250 ms, with a schema version. Grants (`acl.json`) are written the same way.
Where: `crates/mux/src/store.rs`, `crates/mux/src/acl.rs`.
From: [M2](docs/plan-archive.md#m2-durability), M12.

### Restart policies decide what runs after a restore
Scrollback always comes back. A pane's policy decides what runs in it: `none`
(press Enter for a shell), `shell` (the default: a login shell in the last
OSC 7 directory), `rerun` (the foreground command, asking first by default),
`hook` (a stored command), `resume` (the agent conversation that was running:
`claude --resume <id>`; the default for a pane running Claude Code). Restoring
feeds a fresh engine the checkpoint and log tail, then writes a reset and a
dim "restored" rule before the new process starts.
Where: `crates/proto/src/lib.rs` (`Policy`), `crates/mux/src/resume.rs`, `crates/mux/src/store.rs`.
From: [Restart policies](docs/plan-archive.md#restart-policies-m2), [Restoring scrollback](docs/plan-archive.md#restoring-scrollback), #146.

### Panes outlive the daemon
Each pane's program runs under a shim, so the daemon can be restarted or
upgraded without ending shells. On Linux with systemd the PTY master waits in
the FD store and each pane has its own transient scope; elsewhere the shim
holds the master and lends it to whichever daemon connects, ending the pane
after a grace period if none does. Stopping the daemon still ends its panes.
The shim writes the exit status because a restarted daemon is not the
program's parent. On Windows the shim is a pty host (ConPTY).
Where: `crates/mux/src/shim.rs`, `crates/mux/src/holder.rs`, `crates/mux/src/sys.rs`, `crates/mux/src/conpty.rs`, `crates/daemon/src/install.rs`.
From: [M2b](docs/plan-archive.md#m2b-in-place-daemon-upgrade-start-of-daily-use), S3, #35, [M58](docs/plan-archive.md#m58-panes-survive-daemon-restarts-on-windows-221).

## The mux and panes

### One task decides the order of every change
The mux task owns the layout, the panes and the connected clients; every
client message, API call and pane notice goes through it. A pane's VT thread
owns libghostty's terminal (it is `!Send`), and PTY output, attaches, resizes
and exits arrive on one channel, so a snapshot and the live output after it
are always in order and the log sees the same bytes.
Where: `crates/mux/src/mux/mod.rs`, `crates/mux/src/pane.rs`.
From: [Cargo workspace](docs/plan-archive.md#cargo-workspace), M1.

### The model is pure: `core` has no I/O
Sessions hold tabs, tabs hold a split tree of panes. Clients change them by
sending intents, applied by pure functions in `crates/core`, and the tree
stays valid (ratios sum to 1, no empty splits). IDs follow tmux (`$session`,
`@tab`, `%pane`) and are never reused. Ratios are stored; cells are derived.
There are no local-only panes: every terminal belongs to some daemon.
Where: `crates/core/src/mux.rs`, `crates/core/src/layout.rs`, `crates/core/src/tree.rs`.
From: [Architecture](docs/plan-archive.md#architecture), [M1](docs/plan-archive.md#m1-multiplexer).

### The terminal engine sits behind `VtEngine`
libghostty-vt is the only engine, behind a trait (`feed`, `resize`,
`snapshot`, …) so the daemon never touches Ghostty types directly. The Ghostty
commit is pinned.
Why: it ships a formatter that turns state back into VT bytes and keeps more
state than xterm's serializer.
Where: `crates/vt/src/lib.rs`, `crates/vt/src/ghostty.rs`.
From: [Decisions table](docs/plan-archive.md#decisions-on-the-briefs-open-questions), S1.

### The mux names no integration
`arugula-mux` holds the core (the mux task, panes, blocks and their registry,
the access model, history, the state directory) and `arugulad` the binary
around it: the edge, MCP, and every integration. The line runs one way: the
daemon names the mux, never the other. Nothing in the mux names an HTTP
server or client, MCP, SSH, the embedded web client or control. Three
mechanisms carry what crosses it: registration (the daemon adds a `BlockKind`
per block type to `BlockKinds` before the mux starts), traits (what the mux
calls out to is a trait in `Config`, in `mux/outside.rs`, and `Config::at_exit`
for the daemon's own cleanup), and features (`arugula-mux/labs`, which the
daemon's `labs` forwards). `just mux-guard`, run by `just check` and
`just check-core`, fails if `axum`, `hyper`, `reqwest`, `rmcp`, `rust-embed` or
`russh` is in `cargo tree -p arugula-mux`, with or without `labs`.
Why: a core that can't reach an integration can't depend on one by accident, so
it stays testable and movable without them, and the order of changes stays
decided in one task.
Where: `crates/mux/src/mux/outside.rs`, `crates/mux/src/block.rs` (`BlockKinds`), `crates/mux/src/mux/config.rs` (`Config`), `crates/daemon/src/main.rs` (the wiring), `justfile` (`mux-guard`), `crates/mux/README.md`.
From: [docs/mux-crate.md](docs/mux-crate.md), #459 (the design, #387 item 4), moved in #460 and #701–#705.

### A block is a leaf with a type; a terminal is the first type
Every leaf of the tree is a block with a `type`. A terminal keeps its own fast
path (PTY bytes, offsets, snapshots). Every other type implements `Block`:
config saved in `layout.json`, JSON state pushed to clients, attention through
the same notices as terminals (so badges, "needs you" and push work for all of
them), a text rendering for `capture`/history/search, and methods. A pane
(`%N`) is a terminal block; OSC 133 command ranges are *command marks*, never
"blocks".
Where: `crates/mux/src/block.rs`, `crates/mux/src/pane.rs`, `crates/mux/src/mux/blocks.rs`.
From: [M6](docs/plan-archive.md#m6-non-terminal-blocks-after-m4b-m5-is-independent-of-it), [Architecture](docs/plan-archive.md#architecture).

### Layout belongs to a host; a remote block references a pane elsewhere
Each daemon owns its own tree. A tab can hold a `remote` block, `{host,
pane}` and nothing else, whose terminal, size, policy and history stay on the
other daemon. The client asks the other host for the pane and then records
the reference here; the daemon never talks to the other one on a client's
behalf, and closing the reference closes only the reference.
Where: `crates/daemon/src/remote.rs`.
From: [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), #17.

### Shell integration is injected the way Ghostty does it
bash `ENV`, zsh `ZDOTDIR` and fish `XDG_DATA_DIRS` load scripts that report
prompts, commands, exit codes and the directory (OSC 133, 7, 633), without
touching the user's dotfiles; a pane can switch it off.
Where: `crates/mux/src/shellint.rs`, `crates/mux/src/osc.rs`.
From: [M3](docs/plan-archive.md#m3-structure-and-cli).

## Access and identity

### One function says what an intent needs
Every request has an author and every session an access list. Roles per
session are `viewer`, `editor` and `owner`; the daemon's owner owns every
session. `illogical_core::access::need` is the one decision function, and the
daemon enforces it on every path: the WebSocket (in the mux, per message), the
HTTP API (`authz.rs`), the Unix socket, the channel and the tmux front end.
The unit of sharing is the session; tabs, blocks and machines inherit it.
Grants are data (`acl.json`) with an audit log (`audit.jsonl`).
Where: `crates/core/src/access.rs`, `crates/mux/src/acl.rs`, `crates/daemon/src/authz.rs`, `crates/mux/src/mux/who_may.rs`.
From: [M12](docs/plan-archive.md#m12-principals-and-roles), [Multiplayer track](docs/plan-archive.md#multiplayer-track-m12m15-added-2026-10-01).

### Operations are declared once, and listed in two checked-in files
An operation (a route a client calls, and sometimes an MCP tool or kind) is one
`Op` declaration in `proto` (name, method, path, types, `ACCESS`, `DRIVES`,
`CREDENTIAL`) and one `Handle` impl in the daemon; its HTTP route, its access
check, its MCP row and the CLI's call come from that. A GET is read-only for an
MCP token unless it declares `CREDENTIAL`. Two tests compare every operation's
access (`ops-access.txt`) and what a read-only token reaches
(`ops-read-only.txt`) against files in `crates/daemon/tests/fixtures/`, so a
change shows in review (`ARUGULA_BLESS=1` rewrites them). A new route in
`api.rs` is an operation unless it is on `HAND_WRITTEN` with a reason. The
design and what doesn't fit it are in [docs/operations.md](docs/operations.md).
Where: `crates/proto/src/op.rs`, `crates/daemon/src/ops/`, `crates/daemon/src/mcp/ops.rs`.
From: #451, #572, #387.

### Who can reach a daemon
The daemon listens on loopback; `tailscale serve` adds `Tailscale-User-Login`
and strips any copy a client sends, and direct tailnet connections are
identified by asking tailscaled (WhoIs). Host must be a loopback or tailnet
name (DNS rebinding). The Unix socket maps the peer's uid to the owner. A
device of the control account the daemon joined is the owner too.
Where: `crates/daemon/src/access.rs`, `crates/daemon/src/tailscale.rs`, `crates/daemon/src/localauth.rs`.
From: [Architecture](docs/plan-archive.md#architecture), [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), S2.

### A guest types on your machine only by trust, or in a VM
Write access to a local shell is code execution as your uid. A guest's new
panes run in a VM (or join the tab's), and driving one of the owner's local
panes needs a per-pane, time-limited trust grant. Anything that types into a
pane, including uploads, checks both the role and `MayDrive`; a new route that
types must be added to that check by path.
Where: `crates/daemon/src/authz.rs` (`Api::MayDrive`), `crates/mux/src/mux/who_may.rs`, `crates/daemon/src/upload.rs`.
From: [M14](docs/plan-archive.md#m14-safe-write-access), [M70](docs/plan-archive.md#m70-images-into-terminal-panes-249).

### Only the owner's agent reaches panes over ssh
The fixed `SSH_AUTH_SOCK` path links only to an agent forwarded by the box's
owner. A guest's forwarded agent is never used in the owner's panes, so `git
push` doesn't depend on who attached last.
Where: `crates/cli/src/ssh.rs` (the agent link), `crates/daemon/src/guest_ssh.rs`.
From: [SSH track](docs/plan-archive.md#ssh-track-s28-m51m53-added-2026-10-04), [M65](docs/plan-archive.md#m65-a-pane-for-a-guest-who-has-only-openssh-198-decided-2026-10-04).

## Clients

### The daemon serves the web client, on purpose
`web/dist` is embedded in the daemon (`rust-embed`), so any browser works
against any machine and there is one artifact to install. The price is that a
UI change needs a daemon release. This is not on the list to change; don't
split the web client out by accident.
Where: `crates/daemon/src/server.rs`, `web/`.
From: [Cargo workspace](docs/plan-archive.md#cargo-workspace), #387 (point 4).

### No client links daemon code
`cli` (TUI, tmux front end), `desktop` and `web/` speak the wire through
`illogical-proto`. `core` has no internal dependencies, `proto` depends only
on `core`, `vt` and `e2e` stand alone, and `control` shares only `e2e`.
Where: `Cargo.toml`, `crates/*/Cargo.toml`.
From: #387.

### Only the web client and the desktop app get new features
The TUI (`illogical tui`) and the tmux `-CC` front end are kept working and
tested, but they are frozen: no new features there.
Why: every feature in the web client lands once instead of three times.
Where: `crates/cli/src/tui/`, `crates/cli/src/tmux/`, `web/`, `crates/desktop/`.
From: #387; [S26](docs/plan-archive.md#s26-how-native-can-it-get-141), [M31](docs/plan-archive.md#m31-illogical-tui), [M5](docs/plan-archive.md#m5-tmux-control-mode--cc-front-end).

### The web client draws terminals through one file
xterm.js lives behind `terminal-view.ts` (WebGL on visible panes, DOM for the
rest) so the renderer can be swapped later (M8, gated, not built). Layout
changes are intents sent to the daemon and never applied locally.
Where: `web/src/terminal-view.ts`.
From: [M1](docs/plan-archive.md#m1-multiplexer), [M8](docs/plan-archive.md#m8-client-terminal-engine-ghostty-web-and-local-echo).

### The desktop app is a window and a supervisor; the daemon stays a service
The app (Tauri 2) loads the UI from the local daemon, bundles `illogicald` and
`illogical`, and registers the daemon as a login service (SMAppService on
macOS). Sessions outlive the window: if the app owned the PTYs, quitting it
would end every pane. `desktop` depends only on `proto`.
The app never replaces a daemon that's there (#392): the bundled copy only
installs one on a machine with none, and a daemon updates itself (#391). At
launch the app checks the daemon's protocol number against its range (#390),
and a daemon that reports none must be 0.19.0 or newer, the titlebar's
release (#317). Otherwise its window stays on the app's setup page, which
says which side is behind and offers that side's update; the app never shows
a daemon's page it doesn't match.
An app update that replaces the app in place (macOS, the AppImage) restarts
the app by itself once nobody's using it (#561): no window of ours focused
and, on macOS, no input anywhere for five minutes, no huddle live and no
dialog or file picker open. Running on from a replaced bundle breaks the
macOS services the app reaches through it (the file picker, #556), and the
panes are the daemon's, so a restart loses nothing; the windows come back
on their pages without taking focus. The tray's *Restart to update* stays
for doing it sooner. Windows' installer closes the app as it installs, so
there's nothing to wait for there.
On macOS the app's launch agent runs the daemon in the bundle, from a plist
the bundle's signature covers, so `arugulad install -- FLAGS` can't write
its flags into that plist as it does into its own plist or unit: it keeps
them in the state dir's `daemon-args.json`, and a daemon that agent starts
with no flags reads them from there (#550).
Where: `crates/desktop/src/main.rs`, `crates/desktop/src/service.rs`,
`crates/desktop/src/compat.rs`, `crates/desktop/src/updates.rs`, `crates/daemon/src/install.rs`.
From: [Desktop track](docs/plan-archive.md#desktop-track-s25-m46m48-added-2026-10-04), [S26](docs/plan-archive.md#s26-how-native-can-it-get-141), #317, #390, #392, #550, #561.

### Clients run ssh; daemons don't
The CLI, the TUI and the desktop app run the system `ssh` themselves, where
the user's agent and prompts are. A daemon never relays ssh (that would make
it a hub and put the user's ssh credentials in a background service). ssh is
used to install and join a box to control; control reaches it after that.
Only the target `user@box` is stored.
Where: `crates/cli/src/ssh.rs`, `crates/cli/src/hosts.rs`.
From: [SSH track](docs/plan-archive.md#ssh-track-s28-m51m53-added-2026-10-04), M51, M52.

## Agents and blocks

### An agent block is an ACP client
It speaks the Agent Client Protocol (JSON-RPC on the agent's stdio) to
whatever agent server the block names (Claude Code through
`claude-agent-acp`, Codex, Fountain, any ACP command). One protocol for every
agent, so no per-agent adapter. A permission request puts the block in
`needs-input`; an unanswered question or approval waits indefinitely. "Always"
rules are kept by the daemon in `rules.json`, per machine, never in Claude
Code's settings.
Where: `crates/daemon/src/agent/mod.rs`, `crates/daemon/src/rules.rs`.
From: [M6b](docs/plan-archive.md#m6b-agent-blocks-acp-clients), S7, #166.

### A Claude Code block logs in as whoever started it
A block's adapter used to get the daemon's environment, so under launchd it
read the default login whatever login its caller had, and a stale one failed
every first turn with nothing to say which. A Claude Code block's config now
has `claude_config_dir`, which its local adapter gets as `CLAUDE_CONFIG_DIR`
over the daemon's and the login shell's. `illogical agent` sends the CLI's
(`""` for none: the default login). `illogical mcp` sends its client's in an
`Illogical-Claude-Config-Dir` header, on the local socket only, and
`start_agent` puts it on the block. With neither, the block takes it from what
it's opened beside: an agent block's own `claude_config_dir` (a lead's
subagents share its login), or a terminal's foreground program's environment,
else its shell's (`procinfo::env_var`, that one variable from
`/proc/PID/environ`). macOS 26 shows no other process's environment, so there
*Start an agent…* beside a terminal falls back to the daemon's login. Opened
conversations (M33) keep the daemon's directory, where the index found them;
a VM's Claude Code keeps its token file. An `acp` block takes it too (it may be
the adapter run by hand); Codex and Fountain refuse it.

The directory is a path, so it's kept in `layout.json` and a restarted daemon
starts the adapter with it again. Keys and tokens in a caller's environment
(`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`) are
not passed on: they'd have to be kept on disk to survive a restart. There's no
daemon-wide setting for agents' directory: `CLAUDE_CONFIG_DIR` in the
service's or the login shell's environment already is one, and a second would
hide whose login a block used.

Each local start notes which login the adapter got (`{"e": "login"}` in the
block's log, so a rebuilt block knows too), naming any key or token variable
by name only. A turn or session that fails with ACP's `authRequired` (-32000),
or a message about authenticating, says in the transcript and the card's
error which login it used and how to log in to that one
(`CLAUDE_CONFIG_DIR=… claude`, or `env -u CLAUDE_CONFIG_DIR claude`, then
`/login`); a VM block names its token or key file.
Where: `crates/daemon/src/agent/defs.rs` (`claude_config_dir`, `login_hint`), `crates/daemon/src/agent/mod.rs` (`local_login`, `auth_failed`), `crates/mux/src/mux/blocks.rs` (`agent_login`), `crates/daemon/src/mcp/tools.rs` (`start_agent`), `crates/cli/src/cmd/agent.rs`, `crates/cli/src/mcp.rs`.
From: #379.

### Attention comes from signals, hooks and a fallback
Each pane is `idle | working | needs-input | done`, from notification
sequences (OSC 9/777/99, BEL), Claude Code hooks (`illogical hook`, which also
turns a permission request into an approval card), and a quiet-output
heuristic. A program that reports its status (OSC 7501, #720) beats the
screen rules and the quiet heuristic for its pane; hooks and open asks still
beat it, since they carry the question and how to answer it. Every answer has an author. Web Push goes to the phone when no
client is focused on the pane.
Where: `crates/mux/src/mux/attention.rs`, `crates/mux/src/status.rs`, `crates/cli/src/hook.rs`, `crates/daemon/src/push.rs`.
From: [M3](docs/plan-archive.md#m3-structure-and-cli), [M29](docs/plan-archive.md#m29-team-answers), M24.

### MCP is served by the daemon, with the API's own checks
`/mcp` (Streamable HTTP) is part of the owner's API: the owner over the
socket (`illogical mcp` bridges stdio), loopback or the tailnet, or per-client
bearer tokens that can be revoked. Tools cover everything the CLI does; the
MCP client's own tool permissions are the guard, so tool annotations matter.
Agent blocks get it injected, scoped to their tab. Every tool answers by name
whether or not it is listed.
Where: `crates/daemon/src/mcp/mod.rs`, `crates/daemon/src/mcp/tools.rs`, `crates/daemon/src/mcp/tokens.rs`.
From: [M16](docs/plan-archive.md#m16-an-mcp-server-illogical-as-tools-for-any-agent), S14.

### The Claude Code plugin calls the installed CLI; its hooks stand aside for settings ones
`plugins/claude-code` (with `.claude-plugin/marketplace.json` at the root, so
the repo is its marketplace) bundles `arugula mcp`, the hooks `arugula hooks
install` writes and a short skill. It ships no binary and isn't in any
release artifact: every command is the user's `arugula` on PATH, so the
plugin follows main and the CLI follows releases. Its hooks are
`HOOKS_SNIPPET` exactly (a test holds them to it), plus one SessionStart check
that says once when `arugula` isn't on PATH. Claude Code runs a plugin's hook
beside an identical settings.json one (it dedupes only across settings
files), so each `arugula` hook command, run with `CLAUDE_PLUGIN_ROOT` pointing
at this plugin, does nothing when a settings file has the same hook: the
settings one, which predates the plugin, is the one that runs. `hooks install`
adds no hooks while the plugin is enabled, and `hooks status` names it. For
the MCP server, Claude Code counts a plugin server with the same command as a
configured one (`claude mcp add arugula -- arugula mcp`) as a duplicate and
keeps the configured one; one registered by path (the setup page's Add to
Claude Code) isn't, and `hooks status` says to remove it. The plugin's tools
are `mcp__plugin_arugula_arugula__*`, so with the plugin enabled `hooks
install` adds its permission rules under that name too.
Where: `plugins/claude-code/`, `crates/cli/src/hooks.rs` (`plugin_copy_stands_aside`, `PLUGIN`), `crates/cli/src/hook.rs`, `crates/cli/src/ask.rs`, `crates/cli/src/cmd/attention.rs`.
From: #229.

### Each browser block gets its own origin
A dev server in a machine runs code an agent wrote. Block 42 is served at
`b-42.<domain>`, at `/`, so blocks can't read each other or the app, and dev
servers need no base path. Through control the same origin comes from control
with no DNS or certificate on the machine.
Where: `crates/daemon/src/sites.rs`, `crates/daemon/src/browser.rs`.
From: [M6a](docs/plan-archive.md#m6a-browser-blocks), [M50](docs/plan-archive.md#m50-blocks-through-control-150-after-s27).

### Forge blocks use the person's own CLI, and agents draft while people send
One `forge` block type with a provider adapter (GitHub through `gh`, Forgejo
through `tea`, GitLab through `glab`); illogical stores no forge tokens. An
agent's write (comment, approve, merge, close) becomes a draft on the block;
the owner or an editor sends it, as themselves. Freshness is conditional
polling, with webhooks relayed by control's GitHub App. Launch ships GitHub PR
blocks only; the other providers move into Labs (#457, not yet built).
Where: `crates/daemon/src/forge/`.
From: [Forge track](docs/plan-archive.md#forge-track-s23-m36m40-added-2026-10-02), [M36](docs/plan-archive.md#m36-forgejo-pull-request-blocks-88), #457.

### Uploads land in a per-pane folder on the pane's host
A file from any client goes to `illogical-uploads/<pane>/` under
`$XDG_RUNTIME_DIR` (else `$TMPDIR`, else `/tmp/illogical-<uid>`), mode 0700,
files `O_EXCL` 0600, named by the daemon, swept after 24 h, capped at 20 MB a
file and 200 MB a host. The daemon pastes the path into the pane. The state
dir is never used: it holds secrets and a VM pane can't see it.
Where: `crates/daemon/src/upload.rs`, `web/src/upload.ts`.
From: [Images track](docs/plan-archive.md#images-track-s32-m70m72-added-2026-10-05), [M70](docs/plan-archive.md#m70-images-into-terminal-panes-249).

### A workspace block asks chant, and keeps a few reads of its own
A workspace block reads through the workspace's own chant and its contract-1
documents; chant decides what the workspace is and who reads it. An audit
against chant's workspace spec (INTENTIUS/chant 74b1a5b97, #305) found
smaller deviations. One was fixed: the fingerprint runs git with
`GIT_OPTIONAL_LOCKS=0` and `diff-index`, so polling never takes `index.lock`,
and its checksum covers `refs/chant/*` and `refs/heads/chant/work/*`. The
rest stay, each with a comment at the spot:
- Which chant (ws-021). The reader only finds a chant to start (`$CHANT`,
  `node_modules/.bin` here or above, PATH); chant hands off to the root's
  pinned one or refuses (`root-chant-required`, `reader-too-old`), and the
  block shows the code.
- Studio-box gates come from hud's work board (`apps/hud.rs`): the workspace
  is inside the box, where Arugula can't run chant. The board's gate fields
  are `status`'s.
- hud passes the email's local part as `onBehalfOf` (ws-080), since hud
  takes only a display name; a workspace with `identity.attribution:
  identified` refuses it in chant. The block's own approvals pass principals.
- Standing permission rules stay outside `chant audit --agents`
  (`rules.rs`): they are Arugula's, answering every ACP agent, and writing
  them into a harness's settings would allow those tools outside Arugula.
- Agent questions stay in Arugula, not chant `points` (ws-091): one person
  answering a hook can't meet a points record's rules.
- A workspace's decision points are answered on the swarm's rail, as its
  gates are approved (#621): a question `points --open` lists is a `Gate`
  with a `point` source, answered by `chant workspace points answer` with
  the block's principals and `--relayed-by`. An op gate that asks a point
  (chant#3170) is that question, never a `chant approve`. This is the
  approvals inbox behold#472 asked for, so behold doesn't build one.
- A member's agent sessions are `ls --json`'s `agents` for the member
  (#590), from the `ls` the block already runs. A chant that lacks the
  field, any before INTENTIUS/chant#3628 (no release has it yet), gets the
  reader's own parse of the declaration's file instead, `.json` or `.jsonc`
  (its comments and trailing commas taken out), read in the working tree
  where chant reads it at base. Drop that parse once the block's chant
  floor has the field.
- *Changes* on a member (#619) joins what chant can't know, and nothing
  else: a run in `graph --intent`'s `why` is linked to the open agent block
  whose recent turns wrote that run id, and a lease's holder to the open
  agent block of the workspace running as that session. The lease itself
  is `status`'s, matched to the walk's `work` nodes by record id, as the
  read contract joins documents; a lease on no work item the walk names
  isn't shown.
- `dist/fountain.yaml` and `src/agents` are read by hand: agent-specs is a
  chant project, not a workspace, and asking chant would mean a build.
- A forge issue keeps its own branch, not a chant work lease: the issue isn't
  a chant record.

The decisions a gate enforces come from `graph --intent <member dir>`
(#617), so chant does the covering and the block joins nothing (ws-052).
It's read per member with a gate waiting, outside the full read, and kept
until `HEAD` or the lifecycle ref moves (once per gate while nobody looks): it runs `git log` over the member, about a
second on a small repository and 40 s or more on chant's own, where the 30 s
limit on a host command cuts it off and the card says so. A member card
opened (#618) gets the same read, and the run ledger, only while a client
draws the block.

A member card joins a few things itself, accepted beside the pane mapping
because chant has no document that does them (#618):
- A run's pane: the run id Arugula writes (`arugula-<pane>-<ms>`), named
  only when the agent block in that pane recorded that run. chant knows the
  agent session; only Arugula knows the pane.
- A run on a member: its `agent` is a session the declaration binds to the
  member (`agents`), or chant's intent walk joined the run to the member's
  commits.
- A lease on a member: `status`'s lease is in the member's ledger, or its
  item is a work node in the member's intent graph with its own
  `constrains` (chant's statement that it covers the member).
- A lease's pane: the run whose `lease` is the lease's token, and that run's
  pane. A lease is never linked by its holder's name.
Where: `crates/daemon/src/labs/workspace/model.rs` (`READER`, `FINGERPRINT`, `INTENT`, `member_why`) and `why.rs`, `apps/hud.rs`, `rules.rs`, `cli/src/ask.rs`, `fountain/wear.rs`, `forge/issue.rs`.
From: [M34](docs/plan-archive.md#m34-chant-workspace-blocks-73), #303, #305, #617, #618, #619.

### A workspace block's graph is behold, run by the block
The estate graph (resources, drift, the ops track) is behold's to draw; the
block doesn't read chant's `graph`. *Graph* on a member card (the owner's)
has the block start `behold serve <root> --port 0` on this host and frame it
through the block's own site, as an editor block frames code-server. It's
the Workspace block's process, not a Browser block beside it or a new block
type: a pick in behold has to reach this block (to open the member's Shell
or Changes), the block's env and fingerprint drive it, and its lifetime is
the block's. behold runs in its own process group, stopped as a group when
the block closes or the daemon stops.
behold gets the environment a Shell pane in the member gets (the user's
login shell over the daemon's), so it sees the credentials that shell has
and Arugula adds none. It's started with no `--env`, which would read live
at startup and on each change: the block's env reaches it through the frame,
`gates=<env>` for its gate strip, and the graph's env only when the person
asks for a live read. A later env change is a `behold:view` message
carrying `gates` (and `env` when live), so the frame never loads again.
behold listens on loopback and answers only its loopback names; the site
proxy hands it `Host: localhost:<port>` and its own
origin, so no `--allow-host` is needed. A settled read tells behold
(`POST /api/refresh?notify=1`), so behold doesn't poll the repo too. Framed,
behold draws gates without an approve button: the block approves (one
approve path per gate). `$ARUGULA_BEHOLD` picks the behold to run. This
host only for now.
Where: `crates/daemon/src/labs/workspace/graph.rs`, `web/src/blocks/workspace-graph.tsx`.
From: #620, INTENTIUS/behold#474.

### A recipe is a Claude Code subagent file, worn as M44 wears a Fountain agent
The agents a person offers to other people's agents (#403) are
`.claude/agents/NAME.md` files, read as they are and never written, so the
same file is a subagent in the person's own Claude Code. Claude Code's ACP
adapter drops the SDK's `agent` option, so a block can't select one: it wears
it through M44's `Worn` instead. The prompt is appended after a line naming
the agent, and model, `tools`, `disallowedTools` and `permissionMode` are
applied (a block's own `permission_mode` wins). Skills are preloaded into the
prompt, as Claude Code does for a subagent: a recipe's `tools` can leave out
the Skill tool, and a plugin alone was never read in a real run. Named MCP
servers come from the scopes Claude Code reads (local, `.mcp.json`, user),
through Fountain's by-reference server code. The session gets
`--strict-mcp-config`, because an agent other people will task mustn't
carry the account's claude.ai connectors (Gmail and the like), and a real
run showed those come along otherwise. Offering is per machine and the
owner's (`<state>/agent-offers.json`). A card is A2A 1.0 with an
`arugula://` JSON-RPC interface and a required channel extension. Fountain's
`agent --as` became `--as-fountain`, so `--as` names a recipe.
Where: `crates/daemon/src/labs/agents/`, `crates/daemon/src/labs/fountain/wear.rs` (`Worn`), `crates/cli/src/cmd/agents.rs`.
From: [S34](https://github.com/arugula-salad/arugula/issues/397), M76 (#398), #403.

### A machine reaches another's agents as its account, through the relay
The team's catalog (M77) is read by the daemon itself, not by a page that
happens to be open, so its agents can use it. A daemon signs its relay
socket and its directory read with its own key, and control routes it
exactly as it would route its person. On the far end a daemon certificate
gets in as `account:<id>`, never as the owner (on a team box every owner is
`Owner`, and a daemon reaching for agents gets the agents and nothing
more). Its channel answers `/api/a2a/` only: no mux, no panes, no hands.
Another account's machine is trusted through the root a checked team roster
names for that account, not through first sight, because a teammate's
roster is already verified. A teammate's own machine is reachable only while it
offers agents (the A2A-only grant). It names those accounts in a separate
`agents` list. Control routes their daemons with it and lists it to them,
but never to their browsers, so a person's fleet doesn't gain machines it
can't open. A daemon keeps one standing channel to each machine it reaches
(#856), every request on it numbered, and closes it once it's idle a few
minutes.
Where: `crates/daemon/src/peer.rs`, `crates/daemon/src/e2e.rs` (`serve_agent_caller`, `agents_only`), `crates/daemon/src/control.rs` (`agent_callers`, `known_root`), `crates/control/src/auth.rs` (`Reacher`), `crates/control/src/teams.rs` (`daemon_may_reach`).
From: [S34](https://github.com/arugula-salad/arugula/issues/397) q3, q7; M77 (#399); #856.

### A task waits for the machine's own account, on a card held by a block
Tasks from another account's agent (M78) wait until the receiving machine's
own account allows them (M79). The check is by account, not principal:
every owner of a team is `Owner` on its machines, so on a team box an
owner from another account would otherwise allow tasks for agents that
aren't theirs. The card can't be on the task's agent block (the mux refuses
cards there), so the Team agents block holds them, one at a time. One is
opened in session `a2a` when none is open, so a card always has a place,
gets attention and is pushed. Answering it goes through the routes that see
the channel's caller (`act`, a block's `answer`), and those refuse a device
of another account. (A conversation's grant answers its own pane's asks
under its own rule, #857, and never these cards.) *Allow for an hour* is a grant for that account and
agent, kept on disk, listed and revocable. The caller polls rather than
streams, and a channel request answers within seconds; tasks take minutes.
Where: `crates/daemon/src/labs/agents/tasks.rs`, `block.rs` (the hub), `delegate.rs`, `crates/daemon/src/api.rs` (`foreign`, `is_agents`).
From: [S34](https://github.com/arugula-salad/arugula/issues/397) q4, q6; M78 (#400), M79 (#401).

### Threads live on the daemon that owns the pane; conversations on their home
Pane and session threads are kept in `<state>/threads/`, outside the pane's
own directory (closing a pane keeps its conversation), with the pane's access
rules. Team channels are ciphertext in control (MLS), and voice is WebRTC on a
session, signaled through the session's daemon, with control only handing out
short-lived TURN credentials.

A conversation (#855) is a third kind of thread beside them, not a pane or
session thread moved: those stay as they are, because their access, unread
marks, invites and push are all the pane's. A conversation has a global id
(`ConvId`, `c-` and 32 hex digits) and lives on its home machine, the one it
was made on, never in control: its record in `<state>/threads/conv-<id>.json`
and its timeline in `conv-<id>.jsonl`, with read marks in the shared
`reads.json`. Its people are the record's, not a session's: the owner
(`Owner`) and members (`Editor`), each reading from when they joined unless
added with history. Only the machine's owners make one (Jake, 2026-10-13);
only its owner renames, deletes, adds and takes out; a member may leave.
Being a member is how someone reaches the home machine (Jake, 2026-10-13,
#856): a member who couldn't reach it before is added with their account's
root (from a roster the home checked, or the one the owner's browser
pinned), so the home lets their devices in and control routes them there,
as a share would, but for its conversations only: they have no role in its
sessions. Members see everything linked (Jake, after S36); the pane itself,
its terminal and its own thread, stays with the people it's shared with.
Membership is a drive grant on every linked pane through the conversation's
lead (Jake, 2026-10-13), so the invite, the members list and linking a pane
into a conversation with members all say so.
Where: `crates/mux/src/labs/threads.rs`, `crates/mux/src/labs/convs.rs`, `crates/mux/src/mux/thread_ops.rs`, `crates/daemon/src/ops/convs.rs`, `crates/mux/src/labs/calls.rs`, `crates/daemon/src/control.rs` (`Acl::reach` in `others`).
From: [Talk track](docs/plan-archive.md#talk-track-s30-m61m64-added-2026-10-05), [M61](docs/plan-archive.md#m61-threads-on-panes-and-sessions-240), [S36](spikes/s36-conversations/README.md) q1 and q4, M81 (#855), M82 (#856).

### A conversation's home drives a linked pane through the relay, by a grant on that pane
A conversation links panes on its home machine and on others (#856). The
pane's machine keeps the grant (`<state>/conv-grants.json`: the home's
device, the pane and its epoch, the conversation, who made it), and the
home keeps the link (`L1`, `L2`…, the machine's device id, never its name,
the pane, its epoch, a title). Someone who may drive the pane makes the
grant from their own device, through that machine's `conv.grant` (an
editor's role and drive trust), so "you can only link a pane you may drive"
is checked where the pane is, not at home; the home then links it after
checking, over the relay, that it's let in. The home's daemon channel
(`serve_agent_caller`) is let through for a granted pane's prompt, wait,
capture, `events?pane=N` and `attention/act` naming that pane, written
plainly, and nothing else, with the grant maker's rights on the pane: none
of those needs more than an editor's, so that is the cap. A grant is listed
on the pane and revoked there; a revoked one (or one whose pane closed) is
kept a week, so the home hears "revoked" (410) and closes the link for good
rather than taking silence for offline. A closed link stays in the
conversation, and linking the pane again is a new link. A teammate's own
machine lets the home's daemon in only while a grant names it, for those
panes only (not its agents), found through a roster it verified or a root a
share pinned, and names its account in a `links` list that control routes
as it routes `agents`. It's the relay channel and not A2A tasks: a link is
an existing pane people work in, driven turn by turn for as long as the
conversation lasts.
Where: `crates/daemon/src/labs/links.rs`, `crates/daemon/src/e2e.rs` (`serve_agent_caller`), `crates/daemon/src/control.rs` (`link_callers`, `link_accounts`), `crates/daemon/src/peer.rs`, `crates/mux/src/labs/convs.rs`, `crates/control/src/teams.rs` (`daemon_access`).
From: [S36](spikes/s36-conversations/README.md) q2 and q4, M82 (#856).

### A conversation hears its links at home, and answers their cards through the pane's machine
What a linked pane does is posted in its conversation by the home (#857):
one watcher per open link, on the mux's event bus for a pane at home or
`/api/events?pane=N&type=attention,exit&follow=1` followed over the
standing channel for one elsewhere (its grant lets that through), started
again with the daemon. The pane's own attention is the signal, filtered by
rules S36's run forced: a turn ends only after the pane worked; only an ask
(`ReasonKind::Ask`) is something to answer, so Claude Code's "waiting for
your input" a minute later is dropped; the same line twice is one; and a
turn's lines are coalesced (an ask answered within two seconds, by a
standing rule say, is counted, and the turn's end says "worked 40s, 3
approvals"). The turn's summary is its last assistant message, carried in
its `done` reason: an agent block's own, or the one Claude Code's `Stop`
hook gives `arugula attention done` (`last_assistant_message`, else the
transcript's last). Events are typed messages (`ThreadMsg.event`: kind,
link, ask), so the page shows cards and lines, not text to parse; each card
is pushed to the conversation's people from the home, which they all reach.
A card is answered by its ask's id through the pane's machine's own
`attention/act`, never "whatever it asks now", so a late answer can't land
on a newer prompt. The home records the member who answered; the pane's
machine records them "via conversation …" with the grant maker's rights.
That is the grant's rule, not the account's: a request a grant let through
may answer its own pane's asks whichever account the home is (M79's
other-account check guards the machine's own task cards, which stay the
owner's), capped at an editor's: an agent's invites and task consent are
refused, and a grant maker who is only an editor must still be one.
Someone outside the owner's teams added to a conversation gets control's
share offer, which names the conversation (`AccessList.reasons`).
Where: `crates/daemon/src/labs/events.rs` (`Rules`, the watchers, `act`), `crates/daemon/src/api.rs` (`ConvVia`, `act_for_conv`), `crates/daemon/src/peer.rs` (`follow`), `crates/mux/src/mux/thread_ops.rs` (`notify_card`), `crates/cli/src/cmd/attention.rs`, `crates/control/src/teams.rs` (`daemon_access`).
From: [S36](spikes/s36-conversations/README.md) q5 and q8, M83 (#857).

## Federation and control

### Every daemon is a peer; control is the only front door
A daemon owns its terminals and serves the page, the protocol and the CLI.
Control's page lists every joined machine, direct or relayed; a daemon's own
page shows that daemon, plus its tailnet `hosts.json` for setups without
control. No daemon is a hub: a joined daemon doesn't list or relay for the
account's other machines. Terminal bytes go between a client and the daemon
that owns the terminal.
Where: `crates/daemon/src/hosts.rs`, `crates/daemon/src/control.rs`, `crates/control/src/api.rs`.
From: [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), [No special machines track](docs/plan-archive.md#no-special-machines-track-s27-m49m50-added-2026-10-04), [M49](docs/plan-archive.md#m49-the-cli-and-the-daemon-page-without-a-hub-149).

### Control can refuse service but can't read
Terminal output, input, scrollback, snapshots and push payloads are end to end
encrypted; control sees metadata only (who, which host, when, sizes, names).
Every client-to-daemon stream through control is one Noise channel
(`Noise_IK_25519_AESGCM_SHA256`), on the relay and on direct paths from
control's page, with one channel per viewer so revoking closes it. A page a
daemon serves itself still uses plain `/ws`, inside the tailnet's WireGuard.
Where: `crates/e2e/src/channel.rs`, `crates/control/src/relay.rs`, `crates/daemon/src/e2e.rs`.
From: [Control track](docs/plan-archive.md#control-track-s15-m17m22-added-2026-10-01), [M18](docs/plan-archive.md#m18-relay-and-end-to-end-encryption), docs/control-e2e.md.

### Trust is a chain of device approvals
A new device must be approved by a device the account already trusts (the
first is trusted on enrollment). Daemons verify the chain themselves back to
the account's pinned root, so control can't add a reader; team rosters are
signed by an owner's device of the previous version.
Where: `crates/e2e/src/cert.rs`, `crates/e2e/src/team.rs`.
From: [M17](docs/plan-archive.md#m17-illogical-control-accounts-devices-enrollment-directory), [M19](docs/plan-archive.md#m19-teams-sharing-roles-team-daemons-invites), S15.

### A refused approval names the check, and isn't a turn-down
When control refuses an approval, its 403 carries a `reason` code beside the
sentence (`revoked`, `approver_untrusted`, `bad_signature`, `cant_approve`,
`recovery_for_machine`, `no_chain`), and the page acts on the code. Control,
the daemon and the CLI find the failed check with one function, so they
agree. A removed key keeps #330's words: it is refused at join with 410, and
at approval with 403 `revoked`. Closing the prompt after a refusal leaves the
join waiting; only *Cancel* turns a machine down. A browser checks its own key
against the account's trust before it offers *Approve*.
Why: "doesn't check out" left people with no way forward, and a failed
approval was recorded as a rejection.
Where: `crates/e2e/src/cert.rs` (`Trust::refusal`, `Refusal`), `crates/control/src/api.rs` (`approval_ok`), `web/src/control.ts` (`REFUSED`, the `untrusted` phase).
From: #327, #330.

### A machine's join link carries a secret; the account isn't compared
`arugulad join`, Getting started and `arugula login` put a secret of their own
after the code in the approval link (`#join=CODE.SECRET`), which control never
sees. The approving device sends a MAC under it over the account and root it
pinned and the machine's keys, and the machine pins that root when it checks
out, asking nothing. A wrong MAC ends the join and drops the key (one guess
per code); a missing one refuses unless `--account` names the account. The
secret is 16 characters (80 bits), not the 8 first proposed: control sees the
MAC, so it could guess a shorter secret offline. A shorter typed code would
take a PAKE. Older daemons and CLIs, and a control that doesn't say
`join_secret`, keep comparing the account's fingerprint.
Why: a compare only works if the person actually compares, and over ssh it
meant going back to the terminal to type `y`.
Where: `crates/e2e/src/cert.rs` (`join_secret`, `join_mac`), `crates/daemon/src/control.rs` (`join_start`, `join_finish`, `AccountCheck`), `crates/cli/src/control.rs` (`login`), `web/src/ui/control.tsx` (`JoinPrompt`), `web/src/control.ts` (`approveJoin`, `signInNext`), [docs/control-e2e.md](docs/control-e2e.md#joining-a-machine-the-secret-in-its-link).
From: #760, #765.

### A new browser shows a code; without its answer it asks to check the account
A browser, phone or app window waiting to be approved shows a code
(`LOOKUP.SECRET`, as text and as a QR code of a `#device=` link) instead of
its fingerprint. The approving device scans or types it and sends a MAC under
the secret over the account, the root it pinned and the new device's keys;
the new device pins the root only when the MAC checks out. A wrong MAC is
refused and the secret dropped (one guess per code). An approval with no MAC
(a page from before codes, or `just dev-control`'s headless device) isn't
refused outright, nor taken on trust: the new browser shows the account's
fingerprint and asks the person to check it (*It matches*). The scanner uses
the browser's `BarcodeDetector`, else jsQR, loaded only then.
Why: the compare was easy to skip, and the new browser took the root on
control's word. Refusing a MAC-less approval outright would strand anyone
approving from a tab opened before the deploy.
Where: `web/src/control.ts` (`enroll`, `vouched`, `approveDevice`, `DEVICE_HASH`), `web/src/ui/control.tsx` (`Waiting`, `DevicePrompt`, `Unvouched`), `web/src/ui/scan.tsx`, `crates/control/src/api.rs` (`enroll`, `approve`, `device`), [docs/control-e2e.md](docs/control-e2e.md#approving-a-new-browser-or-phone-the-code-it-shows).
From: #761, #765.

### Every team invite link carries a one-time key; admitting someone compares nothing
An *Ask me first* link carries a one-time key in its fragment, as a presigned
link does, and the owner's device signs it (`arugula team ask invite v1`,
with the role and when it was made). Asking to join sends the key's signature
over the asker's account and root. The owner's page shows only requests whose
proof checks out against an invite one of the team's owners signed, and the
prompt says "<name> used your invite link (made <when>, for <role>)", with no
fingerprint. Admitting signs the same v1 roster as before, so daemons need no
feature for it. Links and requests from before were dropped at the update
rather than kept on a compare path.
Why: the fingerprint was 16 hex digits of someone else's device, which
nobody checked; the link already reaches the right person, so make it prove
that.
Where: `crates/e2e/src/team.rs` (`AskInvite`), `crates/control/src/teams.rs` (`invite`, `accept_invite`), `web/src/e2e/team.ts` (`askOk`), `web/src/control.ts` (`makeInvite`, `acceptInvite`, `loadTeams`), `web/src/ui/control.tsx` (`AdmitPrompt`), [docs/control-e2e.md](docs/control-e2e.md).
From: #762, #765.

### A passkey with PRF is a device: one wherever it syncs
Where a passkey gives PRF output, the browser's device keys come from it
(two fixed salts, then HKDF), so clearing site data and signing in again,
or a new computer the passkey syncs to, is the same device with no
approval. That device signs the root it pinned once it's approved, and a
browser that derives its keys checks that signature before it takes the
root, so control still can't hand it an account of its own. A browser that
is already a working device keeps its own keys. A removed device, or one
whose signature is missing or wrong, gives way to an ordinary approval.
Why: the approval (#761) is the step people trip on, and a synced passkey
already lets its holder sign in. The cost is written down: whoever holds
the synced passkey is that device, on every machine it reaches.
Where: `web/src/e2e/keys.ts` (`adoptPrfKeys`, `PRF_SALTS`), `web/src/control.ts` (`takePrfKeys`, `rejoin`, `savePin`, `usePasskey`), `crates/control/src/passkey.rs`, `crates/control/src/api.rs` (`pin`), `crates/e2e/src/cert.rs` (`device_pin_body`), [docs/control-e2e.md](docs/control-e2e.md#prf-re-deriving-a-browsers-keys).
From: #764, #765.

### Control's code is open, only the hosted control is supported; the tailnet stays first class
`arugula-control` lives in this repo and the hosted one
(control.arugula.io) runs the same code. For now only the hosted one is
supported: a control you run yourself gets no help (lex00, 2026-10-08).
People who want a setup of their own use the tailnet without control,
which stays first class: tailnet users skip control, or enroll and keep
direct connections. Free for one person, per seat for teams, sandboxes by
usage.
Where: `crates/control/`, `packaging/control/`.
From: [Control track](docs/plan-archive.md#control-track-s15-m17m22-added-2026-10-01), M22, #662.

## Build and release

### One switch for what a stranger doesn't get: the `labs` flag
The `labs` flag turns on huddles and chat, Fountain, studio apps and chant
workspaces, VMs and sandboxes, guest ssh, Forgejo and GitLab and the swarm's
extra views. It is the first named flag in the daemon's config, and the
mechanism #347's per-feature flags extend:
- **The file** is `flags.json` in the state dir, `{"flags": {"labs": true}}`.
  The top-level object has room for the rest of #347's config; a write keeps
  every key and flag it doesn't know, so an older or newer daemon's aren't
  lost. It is written by a temporary file and a rename, private to the owner.
  A file that doesn't parse is logged once and read as every flag at its
  default; only an explicit set replaces it (keeping the old one as
  `flags.json.bad`).
- **The registry** is `arugula_proto::flags::FLAGS`: a name, a sentence and a
  default. `get`, `set` and `all` are the only way in; an unknown name is an
  error to set and off to get. #347 adds a row per feature and gates that
  feature's routes, blocks and MCP tools on `get`; the file, the CLI, the
  route and Settings need no change.
- **Reads are never cached.** `get` reads the file on every call (a request,
  a tool list, a forge open; none is in a loop), so a flag flips with no
  restart, whoever flipped it. That keeps what #385 had, without a cache to
  invalidate between the daemon and `arugulad flags` in another process.
  Two writers at once can lose one change, since each reads the file, changes
  a flag and renames; they are a person at a switch and one at a terminal,
  and the loser sees the flag as it now is, so there is no lock.
- **Three ways to set it:** `arugulad flags [NAME [on|off]]` works on the
  state dir, so it works with the daemon stopped; `flags.list` and `flag.set`
  (`GET /api/flags`, `PUT /api/flags/{name}`) are owner operations (#572) with
  no MCP tool, since an agent has no business with the owner's settings; the
  web client's *Labs…* in the session menu uses them and reloads the page,
  because `HostFeatures` and the Labs chunk are read at load.
- **The old `labs` file** (#385: empty, present means on) is read once more
  and retired. A starting daemon moves it into `flags.json` (unless that
  already says `labs`) and removes it. Until then `get` counts the file as
  `labs` on when `flags.json` has no `labs`, so a CLI newer than its daemon,
  or a daemon not yet restarted, still sees Labs. That fallback goes in a
  later release, like the rename bridges (#534).
- **The `labs` cargo feature (#452) is unaffected.** `labs::enabled` is still
  `BUILT && flags::get(…)`; a build without Labs can set the flag and
  nothing follows, and *Labs…* says so.
It was a file, not an environment variable, because `shellenv::clean()` drops
every `ARUGULA_*` a shell prints and the desktop app starts the daemon from
a fixed plist; that holds for `flags.json` too. The state dir survives
`arugulad uninstall`, install.sh and the desktop app's updates. Labs is per
machine, visibility rather than enforcement (a machine without it still
answered the routes, until #347: see "Each flag gates its feature" below),
and `HostFeatures.labs` tells the page.
Flags per account and per team on control (#347's last item, #464's option
2) were decided on 2026-10-12: **control pushes, and the machine can
override.** See "Control pushes Labs flags; the machine overrides" below.
Where: `crates/proto/src/flags.rs`, `crates/proto/src/op.rs` (`FlagsList`, `FlagSet`), `crates/daemon/src/ops/flags.rs`, `crates/daemon/src/labs/mod.rs` (`enabled`), `crates/daemon/src/main.rs` (`flags_cli`, the startup migration), `crates/daemon/src/args.rs` (`labs_command`), `crates/daemon/src/mcp/mod.rs`, `crates/cli/src/main.rs`. The dialog is now `web/src/ui/developer.tsx` (next entry).
From: #385, #342, #347, #387, #464.

### A flag for each Labs feature, and Developer settings to turn them on
Chat on a machine shouldn't need Fountain, studio and the rest on with it, so
the one `labs` flag (above) is nine: `chat`, `huddles`, `vms`, `fountain`,
`studio`, `workspaces`, `guest-ssh`, `swarm-themes`, `forges`. Each is a row
in `FLAGS`, off by default.
- **`labs` is no longer a flag; in the file it means all of them.** `labs`
  in `flags.json`, or the old empty `labs` file, is the answer for every flag
  the file doesn't name itself. A machine that had Labs keeps everything, one
  feature turned off there leaves the rest, and the test suites' one switch
  (`testkit::labs`, `e2e/labs.ts`) still turns it all on. `PUT
  /api/flags/labs` and `arugulad flags labs on` are refused: nothing offers
  it.
- **Each gate reads its own flag.** `labs::on(dir, flags::NAME)` in the
  daemon; `flags::on(dir)` gives the whole set from one read, for what lists
  many (`tools/list`, both `--help`s). As before this is visibility, not
  enforcement, except for Forgejo and GitLab, which are refused. (#347 made
  it enforcement: the next entry.)
- **`HostFeatures` says which.** `labs` is now "any flag on", and `flags`
  names those that are. A page on a daemon from before reads `labs` for every
  flag, which is what it meant there. `Client.flag(name)` is the page's one
  question; `has()`, `hasThreads()` and `hasCalls()` stand as they were.
- **Developer settings, unlocked first.** The owner's dialog (session menu,
  palette, the phone's sheet) has a switch per flag and applies it without a
  reload: the Labs chunk loads when a flag first needs it. The menus offer it
  only where `flags.json` exists (`HostFeatures.dev`), which `arugulad flags`
  makes, so a stranger doesn't meet unfinished features. The desktop app's
  *Daemon* menu and `/#developer` open it regardless.
Not done: per-flag enforcement of routes and block constructors (done since,
next entry), and flags per team on control (#347).
Where: `crates/proto/src/flags.rs`, `crates/daemon/src/labs/mod.rs` (`flags`, `on`), `crates/daemon/src/hosts.rs` (`features`), `crates/daemon/src/mcp/tools.rs` (`UNLISTED*`), `crates/cli/src/main.rs` and `crates/daemon/src/args.rs` (`LABS_*`), `web/src/client.ts` (`flag`), `web/src/ui/developer.tsx`, `crates/desktop/src/daemon.rs`.
From: #665, #464, #347.

### Each flag gates its feature, not just its listing
A flag off used to hide its feature and leave it working for anyone who
called the route, named the MCP tool or typed the hidden command. Now
everything about the feature follows the flag (#347):
- **Operations** name their flag (`Op::FLAG`), and `ops::flagged` refuses
  one by every way in, HTTP and MCP by name alike, as a 404 that says how to
  turn it on. That is the route list: threads (chat); machines and
  sandboxes (vms); Fountain's agents; studio; ssh guests (guest-ssh). The
  A2A routes (agents) and the forge webhooks already did; an invite from a
  thread asks `chat` too.
- **Block types** a flag owns (Fountain, the agent catalog, app,
  workspace) are wrapped (`labs::Flagged`) so a new one is refused; an agent
  block that is or wears a Fountain agent checks `fountain`. `POST
  /api/blocks` asks `labs::block_flag` first, so an app block doesn't ask
  the studio before it's refused.
- **The mux** checks `vms` before it makes or borrows a machine or shares
  one with a tab, and `huddles` before a join. Leaving, muting and
  signalling a huddle already on still work, so turning the flag off
  doesn't strand anyone in one.
- **What was made with the flag on stays.** A block restored at start-up
  isn't asked (as a Forgejo block wasn't), threads stay on disk and in the
  page's state, a VM pane in a saved layout comes back. Turning a flag off
  stops new use; it doesn't destroy anything, so on again finds it all.
- **One sentence, from the daemon.** `labs::flag` / `flag_off` in the mux
  say "X is a Labs feature that's off on this machine: its owner turns it
  on with `arugulad flags NAME on` there, or in Developer settings". The CLI
  prints the daemon's error, so a hidden command run with `--host` or
  `--ssh` against a machine without the flag says the same about that
  machine; the CLI keeps no list of its own for this.
- **A build without Labs is unchanged.** `labs::flag` is `Ok` there, and
  each feature's twin says it isn't built, as before.
Where: `crates/mux/src/labs/mod.rs` (`flag`, `flag_off`), `crates/daemon/src/ops/mod.rs` (`flagged`), `crates/daemon/src/mcp/ops.rs`, `crates/proto/src/op.rs` (`FLAG`), `crates/daemon/src/labs/mod.rs` (`Flagged`, `block_flag`), `crates/mux/src/mux/machines.rs`, `crates/mux/src/mux/call_ops.rs`, `crates/daemon/tests/integration/flag_gates.rs`.
From: #347, #665.

### Control pushes Labs flags; the machine overrides
An account sets Labs flags for every machine it owns, and a team's owners
for the team's machines, on control's page (Teams…). Control sends both to
each joined machine with its certificates, and nudges the machines a change
reaches. The machine's own word still wins, so a flag can be forced on or
off locally:
- **Precedence, first match wins:** the machine's `flags.json` (the flag by
  name, then `labs` there, then the old marker); control's word for the
  machine's account; control's word for its team; the flag's default.
  Account before team because it's the narrower say: the account owns the
  machine, and a team reaches only the machines added to it.
- **Team flags reach the team's machines only** (`daemons.team`), not the
  members' own machines: a team owner doesn't set what runs on someone
  else's laptop.
- **`guest-ssh` and `agents` are local-only** (`Flag::local_only`): only the
  machine's `flags.json` turns them on or off. They open a way in, or act
  for the machine, so its owner decides there. The daemon ignores control's
  word on them, control refuses to store them (`wire::LOCAL_ONLY_FLAGS`,
  checked against the registry by a daemon test), and its page doesn't
  offer them.
- **Strangers never see Labs on control's page.** The flag lists show only
  for someone with a machine that has Developer settings: a daemon whose
  state dir is unlocked adds `dev-settings` to the features it sends
  control, and `GET /api/flags` answers `dev`.
- **Tri-state locally.** A flag `flags.json` doesn't name follows control;
  *Follow control* in Developer settings (`PUT /api/flags/{name}` with
  `{"on": null}`, `arugulad flags NAME unset`) takes the machine's word
  back. Where `labs` spoke for it, the rest are written out first, so they
  stay on.
- **Kept beside, not in, `flags.json`:** the daemon writes control's answer
  to `flags.control.json` in the state dir and removes it when the machine
  isn't in control, so a machine that leaves (or never joined) is as before.
  A control from before this sends no `flags`, and the daemon keeps what it
  had. Reads stay uncached: `flags::get` reads both files.
- **Control doesn't know the list.** It stores any short lowercase name
  (`labs_flags`, owner `account:ID` or `team:ID`), and a daemon ignores one
  it doesn't have; the page offers the list it was built with (`FLAGS` in
  `proto.gen.ts`). Developer settings says where each flag comes from.
Where: `crates/proto/src/flags.rs` (`explain`, `write_control`, `unset`), `crates/control-wire/src/team.rs` (`TrustAnswer::flags`), `crates/control/src/labs_flags.rs`, `crates/control/src/api.rs` (`daemon_trust`), `crates/daemon/src/control.rs` (`refresh`), `web/src/ui/control.tsx` (`LabsFlagList`), `web/src/ui/developer.tsx`.
From: #347 (decided by Jake, 2026-10-12; who sees the lists and the local-only flags on PR #828, the same day).

### Zig 0.16 builds Ghostty; the toolchain is pinned
libghostty-vt needs Zig, pinned in `.mise.toml`; cargo runs under `mise exec`
(the `just` recipes do it), and the daemon embeds `web/dist`, so `just web`
comes before any cargo build.
Where: `.mise.toml`, `justfile`, `rust-toolchain.toml`.
From: [S5](docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named), M2.

### Windows is a full platform, built on GitHub's runners
The daemon runs panes on Windows (ConPTY, named pipes, a logon task), not just
drives them. Unix-only code stays behind `cfg` gates, and Windows CI runs
clippy and tests on every change (`illogical-control` is excluded).
Where: `crates/mux/src/conpty.rs`, `crates/daemon/src/pipe.rs`, `.github/workflows/windows.yml`.
From: [Windows track](docs/plan-archive.md#windows-track-s29-m54m60-added-2026-10-05), M55-M60.
