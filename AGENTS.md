# Working on Arugula

Arugula keeps terminals and agent sessions alive on a machine and lets you
reach them from a browser, a phone, the desktop app or a shell. One daemon,
`arugulad`, per machine owns the panes; every client attaches to it.

Building, running and releasing are in [docs/development.md](docs/development.md);
tests in [docs/testing.md](docs/testing.md); contributing in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Crates

Each has a `README.md` with where to start reading.

- `crates/core`: the multiplexer's state, independent of PTYs and networking:
  sessions, tabs, split trees, the intents that change them, the cell layout.
- `crates/proto`: the wire protocol and the HTTP API's types, shared by the
  daemon and every client.
- `crates/vt`: a pane's terminal state on libghostty-vt (`VtEngine`).
- `crates/e2e`: end-to-end encryption between client devices and daemons
  (Noise IK; design in [docs/control-e2e.md](docs/control-e2e.md)).
- `crates/daemon`: `arugulad`.
- `crates/cli`: `arugula`, the CLI, plus `arugula tui` and
  `arugula tmux -CC`.
- `crates/control`: Arugula control: accounts, devices, the directory and the
  relay.
- `crates/control-wire`: the enrolment, routing, relay, push and TURN messages
  between daemons and control (and the CLI's join), one type each, so both
  sides build from the same definition.
- `crates/testkit`: the harness for the daemon's integration tests.
- `crates/desktop`: the desktop app (Tauri). Outside the Cargo workspace, with
  its own `Cargo.lock`, so it releases on its own (#388).
- `web/`: the web client (TypeScript, Preact, xterm.js). The daemon embeds its
  build.
- `vendor/libghostty-vt-sys`: libghostty's sys crate, vendored so the build can
  apply `patches/` to Ghostty.

## The daemon, by layer

Paths are under `crates/daemon/src/`.

- **Edge:** the embedded web client and `/ws` (`server.rs`), the HTTP API
  (`api.rs`), MCP (`mcp/`), share links (`share.rs`), file uploads (`upload.rs`), the dial-out transport
  (`dial.rs`), end-to-end channels from client devices (`e2e.rs`), enrolment in
  control (`control.rs`), Windows' named pipe (`pipe.rs`).
- **Who may:** who may talk to the daemon (`access.rs`, `localauth.rs`),
  principals and grants (`acl.rs`), the API for someone who isn't the owner
  (`authz.rs`), standing permission rules (`rules.rs`), file modes (`perm.rs`).
- **Operations:** `ops/`: each operation (a route, its access, its MCP tool
  or kind) handled once (`mod.rs` holds the list and `HAND_WRITTEN`), with
  `mcp/ops.rs`; declared in `crates/proto/src/op.rs`
  ([docs/operations.md](docs/operations.md)).
- **The mux:** `mux/`: the task that owns the layout, the panes and every
  client (`mod.rs`), with one file per area: `attention.rs`, `clients.rs`,
  `blocks.rs`, `api_calls.rs`, `who_may.rs`, `thread_ops.rs`, `machines.rs`,
  `call_ops.rs`, `info.rs`, `config.rs` (`thread_ops.rs` and `call_ops.rs` are
  Labs; `labs_off.rs` has their twins). Next to it: threads on panes
  (`labs/threads.rs`), huddles (`labs/calls.rs`), gates waiting for a person
  (`gate.rs`), hands (`hand.rs`), Web Push (`push.rs`).
- **Panes:** a process on a PTY and its VT thread (`pane.rs`), Windows'
  pseudoconsole (`conpty.rs`), what survives a restart (`store.rs`),
  keeping terminals open across restarts (`holder.rs`, `shim.rs`, `sys.rs`),
  prompts and commands in the output (`osc.rs`), shell integration
  (`shellint.rs`, `shellenv.rs`), what a pane is busy with (`classify.rs`),
  process info (`procinfo.rs`), history (`history.rs`), resuming an agent's
  conversation (`resume.rs`), named keys (`keys.rs`).
- **Blocks:** what every block provides (`block.rs`), agents (`agent/`),
  browsers (`browser.rs`) and block sites (`sites.rs`, `tls.rs`, `ports.rs`),
  editors (`editor/`), review (`review/`), files (`fs.rs`), forges (`forge/`),
  Fountain (`labs/fountain/`), workspaces (`labs/workspace/`), studio apps
  (`labs/apps/`),
  conversations (`conversations/`), the IDE bridge (`ide/`), invites
  (`invite/`), remote blocks (`remote.rs`), configured agent harnesses
  (`inventory.rs`).
- **Reach:** other daemons (`hosts.rs`), machines that aren't this host
  (`labs/machine.rs`), sandbox providers (the trait in `provider/`; the
  Sprites adapter, `labs/provider_tunnel.rs`, `labs/resident.rs` and
  `labs/sandbox.rs` in Labs), synced history (`sync.rs`, `seal.rs`),
  tailscaled (`tailscale.rs`), outgoing TLS roots (`roots.rs`).
- **Lifecycle:** the command line (`args.rs`), startup (`main.rs`), `install`
  (`install.rs`), Getting started (`setup.rs`), updates (`update.rs`,
  `selfupdate.rs`), `_host` (`host.rs`), malloc settings (`heap.rs`), paths
  through links (`paths.rs`).

## Invariants

- The mux task decides the order of every change. Other tasks send it a
  `Cmd` and wait for the answer (`mux/mod.rs`).
- libghostty's terminal is `!Send`: each pane's VT thread owns it, and
  everything else asks that thread (`pane.rs`, `crates/vt`).
- The web client's wire types are generated: after changing a type it uses,
  run `just proto-ts`. CI fails if `web/src/proto.gen.ts` is stale.
- Every request passes the access checks before it reaches the mux
  (`access.rs`, `authz.rs`, `arugula_core::access`).
- The daemon serves the web client itself, embedded in the binary
  (`server.rs`). That is on purpose (#387).

## Clients

The web client and the desktop app get new features. The TUI
(`crates/cli/src/tui/`) and tmux -CC (`crates/cli/src/tmux/`) are kept working
and tested, but get no new features.

## Labs

A flag per feature turns on what a stranger doesn't get (#385, #464, #665):
`chat`, `huddles`, `vms`, `fountain`, `studio`, `workspaces`, `guest-ssh`,
`swarm-themes`, `forges`. They're named flags in `flags.json` in the state
dir (`arugula_proto::flags`: the `FLAGS` registry, `get`, `on`, `set`, `all`;
read on every call, never cached), set by `arugulad flags chat on` or the
owner's *Developer settings…* in the web client (`flags.list` and `flag.set`
operations). A new Labs feature adds a row to `FLAGS` and gates itself on
`labs::on(dir, flags::NAME)`. `labs` in the file, or the old empty `labs`
file (which a daemon moves into it at startup), means every flag the file
doesn't name. `ARUGULA_STATE_DIR` moves the state dir. A `labs` cargo feature (on by default) compiles Labs
code in or out: it lives in `crates/daemon/src/labs/`, and core reaches it
only through the surface in `labs/mod.rs`, which explains the pattern. Fountain,
studio apps, chant workspaces, and VMs (the Sprites adapter, machines, resident
daemons, the provider tunnel and the tailnet sandbox supervisor) and guest ssh
(`labs/guest_ssh.rs`; `russh` is an optional dependency, and a build without
Labs answers `/api/guests` with 501), and chat and huddles (`labs/threads.rs`,
`labs/calls.rs`, with the mux's handling in `mux/thread_ops.rs` and
`mux/call_ops.rs`, which need `Daemon`'s fields; a build without Labs answers
the thread routes with 501 and a huddle message with an error, and never
touches the state dir's `threads/`), and the Forgejo and GitLab forges
(`labs/forgejo.rs`, `labs/gitlab.rs`, `labs/tea.rs`, `labs/forge_live.rs`,
and `forge/labs_forges.rs`, the block's own connecting, which needs its
fields; GitHub's PR and issue blocks are core, and `labs::forge_allowed`
refuses the others unless Labs is on, in a block's creation, `open_config`
and each read, so a saved one comes back unavailable; with Labs off a bare
`OWNER/REPO#N` is GitHub's) are there (#452 to #457). The `Provider` trait stays
in core (`provider/`); a build without Labs never has a provider, and a saved
VM pane comes back exited, with the reason. `just check-core`
runs clippy and the tests of `arugulad` and `arugula` with the feature off.

## Commands

- `just bootstrap`, then `just web`: toolchains, web dependencies, and the web
  build the daemon embeds.
- `just check`: what CI runs: `just test` (nextest, doctests, the web
  typecheck), then the `proto-ts` check, fmt and clippy.
- `just e2e`: the browser specs. `just e2e-webkit`: WebKit's.
- `just dev`: a dev daemon on 7682 (state in `~/.local/state/arugula-dev`)
  and Vite on 5173, leaving your daily daemon alone.
- `just proto-ts`: regenerate `web/src/proto.gen.ts`.
- In a worktree, set `CARGO_TARGET_DIR` to a directory of its own, so
  parallel builds don't share one `target/`.
- One suite runs at a time per machine: `just test`, `just check` and
  `just e2e` build, then wait for `scripts/suite-lock` while another worktree's
  tests run. Two at once wedge macOS's syspolicyd, and the tests fail by the
  hundred. Waiting can take a while, so run them in the background (or an
  Arugula pane). Agents building side by side: `CARGO_BUILD_JOBS=4`.
  Run suites through `just`: a `cargo nextest run` of your own skips the
  lock.

## Milestone codes

Comments carry codes like M43 and S21. They name milestones and spikes from
the original plan; the index at the top of
[docs/plan-archive.md](docs/plan-archive.md) says what each was. When you
touch a module anyway, replace its leading "M43:" tag with a plain name.

## Decisions

[DECISIONS.md](DECISIONS.md): the decisions that still constrain the code.
