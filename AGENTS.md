# Working on Arugula

Arugula keeps terminals and agent sessions alive on a machine and lets you
reach them from a browser, a phone, the desktop app or a shell. One daemon,
`arugulad`, per machine owns the panes; every client attaches to it.

Building, running and releasing are in [docs/development.md](docs/development.md);
tests in [docs/testing.md](docs/testing.md); contributing in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Size the work to the task

Match the effort to what was asked. A question, an investigation or a one-off
gets the smallest thing that answers it: no new tests, no pull request and no
full `just check` unless the task asks for them. New tests are for changes to
behaviour that ships, and then only for that behaviour, not the code around
it. When unsure, do the small version and say what you left out.

## Crates

Each has a `README.md` with where to start reading.

- `crates/core`: the multiplexer's state, independent of PTYs and networking:
  sessions, tabs, split trees, the intents that change them, the cell layout.
- `crates/proto`: the wire protocol and the HTTP API's types, shared by the
  daemon and every client.
- `crates/vt`: a pane's terminal state on libghostty-vt (`VtEngine`).
- `crates/e2e`: end-to-end encryption between client devices and daemons
  (Noise IK; design in [docs/control-e2e.md](docs/control-e2e.md)).
- `crates/mux`: `arugula-mux`, the daemon's core as a library: the mux task
  (`mux/`), panes, blocks and their registry (`block.rs`, `BlockKinds`), the
  access model (`acl`), history, the state directory (`store`), the pane's
  process helpers, `provider`, the shell integration and the files it can read
  (`fs`). The mux names no
  integration: no HTTP or MCP, and nothing the binary adds (agents, browsers,
  forges). The binary reaches it through `BlockKinds`, the traits in
  `mux/outside.rs`, `Config::at_exit` and `MuxHandle`; `just mux-guard` checks
  its dependency tree (see [its README](crates/mux/README.md)). Its `labs`
  feature is on when the daemon's is.
- `crates/daemon`: `arugulad`, the binary: the edge and the integrations
  around `arugula-mux`, wired together in `main.rs`.
- `crates/cli`: `arugula`, the CLI. It doesn't link `arugula-vt`, so it
  builds without libghostty and Zig.
- `crates/control`: Arugula control's hub: accounts, devices, the directory
  and the relay. It runs the platform in process, or is its gateway when
  the platform runs as its own (`arugula-platform`, M140).
- `crates/platform`: control's platform (#1150): billing, Cloud, chat,
  projects and grants are here, and the `arugula-platform` binary. It
  reaches the hub only through `HubPort`, and the hub reaches it through
  `PlatformPort`, in process or over the private network (`remote`).
- `crates/control-common`: what the hub and the platform both use: the SQLite
  opener, API errors, rate limits, analytics and the signed principal.
- `crates/control-wire`: the enrolment, routing, relay, push and TURN messages
  between daemons and control (and the CLI's join), one type each, so both
  sides build from the same definition.
- `crates/forge-github`: the GitHub reader (M160): GitHub's REST API onto
  the forge model (PRs, issues, checks, reviews, events, which Forgejo and
  GitLab map onto too), with its tokens from a `TokenSource`. The daemon's
  forge blocks read through it with the person's `gh` token; the platform
  will with an installation or one-use user token. It knows no blocks.
- `crates/testkit`: the harness for the daemon's integration tests.
- `crates/desktop`: the desktop app (Tauri). Outside the Cargo workspace, with
  its own `Cargo.lock`, so it releases on its own (#388).
- `web/`: the web client (TypeScript, Preact, xterm.js). The daemon embeds its
  build.
- `vendor/libghostty-vt-sys`: libghostty's sys crate, vendored so the build can
  apply `patches/` to Ghostty.

## The daemon, by layer

Paths are under `crates/daemon/src/`, except those marked `mux:`, which are in
`crates/mux/src/` (`arugula-mux`). The rule: the mux names no integration, so
nothing marked `mux:` names the edge or the block kinds.

- **Edge:** the embedded web client and `/ws` (`server.rs`), the HTTP API
  (`api.rs`), MCP (`mcp/`), share links (`share.rs`), file uploads (`upload.rs`), the dial-out transport
  (`dial.rs`), end-to-end channels from client devices (`e2e.rs`), enrolment in
  control (`control.rs`), Windows' named pipe (`pipe.rs`).
- **Who may:** who may talk to the daemon (`access.rs`, `localauth.rs`),
  principals and grants (`mux:acl.rs`), the API for someone who isn't the owner
  (`authz.rs`), standing permission rules (`rules.rs`), file modes (`mux:perm.rs`).
- **Operations:** `ops/`: each operation (a route, its access, its MCP tool
  or kind) handled once (`mod.rs` holds the list and `HAND_WRITTEN`), with
  `mcp/ops.rs`; declared in `crates/proto/src/op.rs`
  ([docs/operations.md](docs/operations.md)).
- **The mux:** `mux/` (`mux:`, with `block.rs`, `history.rs`
  and `agentenv.rs`): the task that owns the layout, the
  panes and every client (`mod.rs`), with one file per area: `attention.rs`,
  `clients.rs`, `blocks.rs`, `api_calls.rs`, `who_may.rs`, `chat_readers.rs`,
  `machines.rs`, `info.rs`, `config.rs`, and the traits it calls out
  through, `outside.rs`. Chat is control's (M125): the mux only lists who
  reads each pane's and session's thread (`chat_readers.rs`), and, in the
  daemon,
  gates waiting for a person (`gate.rs`), hands (`hand.rs`), Web Push
  (`push.rs`).
- **Panes** (`mux:`): a process on a PTY and its VT thread (`pane.rs`),
  Windows' pseudoconsole (`conpty.rs`), what survives a restart (`store.rs`),
  keeping terminals open across restarts (`holder.rs`, `shim.rs`, `sys.rs`),
  prompts and commands in the output (`osc.rs`), what programs say they're
  doing (OSC 7501, `status.rs`), shell integration
  (`shellint.rs`, `shellenv.rs`), what a pane is busy with (`classify.rs`),
  process info (`procinfo.rs`), history (`history.rs`), resuming an agent's
  conversation (`resume.rs`), named keys (`keys.rs`).
- **Blocks:** what every block provides and the registry of kinds (`mux:block.rs`), agents (`agent/`),
  browsers (`browser.rs`) and block sites (`sites.rs`, `tls.rs`, `ports.rs`,
  and `block_channel.rs` for control's block origins),
  editors (`editor/`), review (`review/`), files (`fs.rs`, with its types in `mux:fs.rs`), forges (`forge/`),
  workspaces (`labs/workspace/`), studio apps
  (`labs/apps/`),
  conversations (`conversations/`), the IDE bridge (`ide/`, with the mux's half in `mux:ide/`), invites
  (`invite/`), remote blocks (`remote.rs`), configured agent harnesses
  (`mux:inventory.rs`).
- **Reach:** other daemons (`hosts.rs`), machines that aren't this host
  (`mux:labs/machine.rs`), sandbox providers (the trait in `mux:provider/`; the
  Sprites adapter, `labs/provider_tunnel.rs`, `labs/resident.rs` and
  `labs/sandbox.rs` in Labs), synced history (`sync.rs`, `seal.rs`),
  tailscaled (`tailscale.rs`), outgoing TLS roots (`roots.rs`).
- **Lifecycle:** the command line (`args.rs`), startup (`main.rs`), a
  hosted workspace's box (`hosted/`: `--hosted`, its own keys per box,
  enrolling, redialling on resume, busy and idle for control), `install`
  (`install.rs`), Getting started (`setup.rs`), updates (`update.rs`,
  `selfupdate.rs`), `_host` (`mux:host.rs`), malloc settings (`mux:heap.rs`), paths
  through links (`mux:paths.rs`), opt-in telemetry (`telemetry.rs`, the
  answer in `arugula_proto::telemetry`, crash files from its `crash` panic
  hook, [docs/telemetry.md](docs/telemetry.md)).

## Invariants

- The mux task decides the order of every change. Other tasks send it a
  `Cmd` and wait for the answer (`crates/mux/src/mux/mod.rs`).
- libghostty's terminal is `!Send`: each pane's VT thread owns it, and
  everything else asks that thread (`crates/mux/src/pane.rs`, `crates/vt`).
- The web client's wire types are generated: after changing a type it uses,
  run `just proto-ts`. CI fails if `web/src/shared/proto.gen.ts` is stale.
- Every request passes the access checks before it reaches the mux
  (`access.rs`, `authz.rs`, `arugula_core::access`).
- The daemon serves the web client itself, embedded in the binary
  (`server.rs`). That is on purpose (#387).
- `web/src` is three folders (M147): `shell/` (keys, the daemon client,
  control's trust UI, the fleet, terminals, blocks, the swarm), `platform/`
  (chat and Cloud workspace views, projects, account settings) and
  `shared/` (tokens, common controls, wire types, views both draw).
  `platform/` imports only `platform/` and `shared/`, `shared/` only
  itself, and platform views reach the page through `PlatformApi`
  (`shared/platform-api.tsx`). `just web` refuses anything else
  (`web/scripts/check-imports.mjs`, M148).

## Clients

The web client and the desktop app are the clients that draw panes; the CLI
scripts them and `arugula attach` shows one in a terminal. The TUI and the
tmux -CC front end were removed (DECISIONS.md "The TUI and tmux -CC are
removed").

## Labs

A flag per feature turns on what a stranger doesn't get (#385, #464, #665):
`sandbox-providers`, `chant-workspaces`, `additional-forges`,
`agent-delegation` (DECISIONS.md "Four Labs flags stay, renamed; the rest
graduate or retire"). Their old names, `vms`, `workspaces`, `forges` and
`agents`, are aliases forever (`flags::ALIASES`, mirrored for control as
`wire::FLAG_ALIASES`): every name that comes in goes through
`flags::canonical`/`lookup`, and only new names are listed. A feature that
leaves takes its row out of `FLAGS` and puts its name in `flags::RETIRED`,
so an old file naming it is ignored: `fountain` (daemon-side Managoat,
removed: DECISIONS.md "Daemon-side Managoat is removed"), `huddles`
(DECISIONS.md "Huddles are removed"), and, graduated or retired in the same
rethink, `chat`, `docs`, `guest-ssh` (now a sharing setting), `swarm-themes`
(the swarm's views, under Theme) and `studio` (its connector). They're named
flags in `flags.json` in the state
dir (`arugula_proto::flags`: the `FLAGS` registry, `get`, `on`, `set`, `all`;
read on every call, never cached), set by `arugulad flags chant-workspaces on` or the
owner's *Developer settings…* in the web client (`flags.list` and `flag.set`
operations). Control can push flags too, per account and per team
(`flags.control.json` beside it); the machine's own `flags.json` wins, then
the account's, then the team's (DECISIONS.md). A new Labs feature adds a row to `FLAGS` and gates itself on
it, refusing as well as hiding (#347): `Op::FLAG` on its operations (refused
by `ops::flagged`), `labs::Flagged` on its block types, and
`labs::flag(dir, flags::NAME)` anywhere else, which gives the refusal that
says how to turn it on. `labs` in the file, or the old empty `labs`
file (which a daemon moves into it at startup), means every flag the file
doesn't name. `ARUGULA_STATE_DIR` moves the state dir. A `labs` cargo feature (on by default) compiles Labs
code in or out: it lives in `crates/daemon/src/labs/`, and core reaches it
only through the surface in `labs/mod.rs`, which explains the pattern. Studio apps, chant workspaces, and VMs (the Sprites adapter, machines, resident
daemons, the provider tunnel and the tailnet sandbox supervisor) and guest ssh
(`labs/guest_ssh.rs`; `russh` is an optional dependency, and a build without
Labs answers `/api/guests` with 501), and the Forgejo and GitLab forges
(`labs/forgejo.rs`, `labs/gitlab.rs`, `labs/tea.rs`, `labs/forge_live.rs`,
and `forge/labs_forges.rs`, the block's own connecting, which needs its
fields; GitHub's PR and issue blocks are core, and `labs::forge_allowed`
refuses the others unless Labs is on, in a block's creation, `open_config`
and each read, so a saved one comes back unavailable; with Labs off a bare
`OWNER/REPO#N` is GitHub's) are there (#452 to #457). The `Provider` trait stays
in core (`provider/`); a build without Labs never has a provider, and a saved
VM pane comes back exited, with the reason. `just check-core`
runs clippy and the tests of `arugulad` and `arugula` with the feature off.
The GitHub forge (`forge/`, surface `forges.rs`) and the editor server with
Claude Code's IDE (`editor/` and `ide/`, surface `editors.rs`) are features of
their own, `forge` and `editor`, on by default the way `labs` is, and built
on the same pattern: one surface, a twin for each item, no `cfg` at call sites.
`labs` turns `forge` on (Forgejo and GitLab are forges); `just check-features`
lints the daemon with neither, and with each alone.

## Commands

- `just bootstrap`, then `just web`: toolchains, web dependencies, and the web
  build the daemon embeds.
- `just check`: what CI runs: `just test` (nextest, doctests, the web
  typecheck), then the web import rule and its tests (`just web-imports`),
  the `proto-ts` check, fmt and clippy.
- `just e2e`: the browser specs. `just e2e-webkit`: WebKit's.
- `just dev`: a dev daemon on 7682 (state in `~/.local/state/arugula-dev`)
  and Vite on 5173, leaving your daily daemon alone.
- `just proto-ts`: regenerate `web/src/shared/proto.gen.ts`.
- In a worktree, set `CARGO_TARGET_DIR` to a directory of its own, so
  parallel builds don't share one `target/`, and put it under `~/.cache`
  (`~/.cache/arugula-<worktree>-target`), not in or beside the worktree:
  a machine can keep its caches on a disk of their own, and its cleanup
  knows where to look. A new one builds cold;
  `just build-cache` sets up caches its worktrees share (Ghostty's Zig
  build, sccache), per machine (docs/development.md, "Caches for
  worktrees").
- Dev builds carry line tables only, and dependencies no debug info
  (Cargo.toml says how to get full info for a debugger). Changing that
  rebuilds the whole target dir, both ways.
- One suite runs at a time per machine: `just test`, `just check` and
  `just e2e` build, then wait for `scripts/suite-lock` while another worktree's
  tests run. Two at once wedge macOS's syspolicyd, and the tests fail by the
  hundred. Waiting can take a while, so run them in the background (or an
  Arugula pane). Agents building side by side: `CARGO_BUILD_JOBS=4`.
  Run suites through `just`: a `cargo nextest run` of your own skips the
  lock.

## Binaries

No screenshots, recordings, builds or archives in the repo, on any branch:
every clone downloads them for good. A PR's pictures go in its description
(drag them in on GitHub), not in a commit or a `pr-assets/` branch, which a
ruleset now refuses. The only binaries are the desktop app's icons and the
web favicon; `scripts/check-binaries` fails on any other, and on any file
over 1 MiB. It runs on every PR (`.github/workflows/binaries.yml`) and in
`just check`.

## Milestone codes

Comments carry codes like M43 and S21. They name milestones and spikes from
the original plan; the index at the top of
[docs/plan-archive.md](docs/plan-archive.md) says what each was. When you
touch a module anyway, replace its leading "M43:" tag with a plain name.

## Decisions

The decisions that still constrain the code are chant decision records, one
file each in [docs/decisions/](docs/decisions/) (`aru-001` …), indexed by
section in [DECISIONS.md](DECISIONS.md). A reference to `DECISIONS.md "Some
title"` names the record with that title. Before you change one of these,
read its record; if you change it, change the record in the same PR (its
body, and `constrains` when the code it names moves), and run `just
decisions` to rewrite the index, which `just check` checks. A new decision
is a new record with the next id. `chant workspace records --current` lists
them, and `chant workspace check` validates them against the kind.
