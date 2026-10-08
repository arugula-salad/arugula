# The daemon's core as a lib crate

A design note for #459 (part of #387, item 4). It proposes what moves out
of `crates/daemon` into a new crate, `arugula-mux`, and what stays in
`arugulad` (the binary and the integrations). It covers the interface
between them, the access checks, the state directory, the API server and
the embedded web client, and gives the order of moves as tickets. No move
changes behaviour, the wire or what is stored.

Measured against `main` at 8922af9 (0.27.0). Line counts are `wc -l` and
include tests. Operations (`docs/operations.md`, #572–#578) are assumed to
have landed first, as agreed: train A, then #578, then #460.

## What we have today

`crates/daemon/src` is 80,417 lines in one binary crate. There is no
`lib.rs`: `main.rs` declares every module, so any module can name any
other through `crate::`. The core the ticket names:

| Module | Lines | What |
|---|---|---|
| `mux/` (12 files) | 5,465 | the task that orders every change, and `Daemon` |
| `pane.rs` | 2,569 | a process on a PTY and its VT thread |
| `store.rs` | 631 | `StateDir`, `PaneLog`, `Event`, `layout.json` |
| `history.rs` | 451 | history and search over the logs |
| `block.rs` | 448 | the `Block` trait, `BlockCtx`, `block::create` |
| `conpty.rs` | 430 | Windows' pseudoconsole |
| `shim.rs` | 361 | the pane shim (`arugulad _shim`) |
| `holder.rs` | 258 | holding terminals across restarts |
| `sys.rs` | 181 | systemd, the FD store |
| **Total** | **10,794** | |

These already depend on more than the ticket's list. Below is every
`crate::` path they name outside themselves, grouped by kind. The grep
covers `mux/`, `pane.rs`, `block.rs`, `store.rs`, `history.rs`, `shim.rs`,
`holder.rs`, `sys.rs` and `conpty.rs`, both inline paths and `use crate::{…}`
blocks.

**Leaves.** These are small modules that name no integration. They have to
move with the core, or it can't compile:

- `osc` (470), `procinfo` (810), `classify` (344), `resume` (145), `keys`
  (4), `paths` (76), `perm` (67), `heap` (35): pane.rs, store.rs and the
  mux call these.
- `shellint` (235) depends on `pane` and `perm`. `shellenv` (471) depends
  on `provider`. `inventory` (317) depends on `shellenv`. `Daemon` holds
  `shell_env` and `inventory`, and reads the inventory to decide whose
  screen to watch (#145).
- `provider/` (295) holds the `Provider` trait. It already stays in core
  under Labs, and pane.rs's `Begin`, `Exec` and `ExecEvent` are in it. It
  names `pane::Spawn` and `fs::FsError`.
- `host.rs` (428, Windows) is the pane host behind `arugulad _host`. pane.rs
  calls `host::{read_frame, write_frame, pipe_name, collect, exe}`, and
  host.rs needs `pipe::Sa` and `pipe::same_user` from the named-pipe server
  in `pipe.rs`.
- Two helpers in `main.rs`: `crate::home()` (pane.rs, conversations) and
  `crate::conpty_command_line` (mux/config.rs).

**The access model.** The mux uses `acl::Principal` (12 files) and
`acl::Acl` through `Config.acl`: `role`, `floor`, `team_role`, `notifies`,
`record`, `forget_session`, `list` and `thread_floor`. They answer who sees
which pane, and they filter each person's `State`.

**Files.** `fs::Scope` sits in `Daemon.fs`, `BlockEnv.fs` and
`MuxHandle.fs`. Blocks use `fs::Target` through `BlockCtx::files`, and the
mux uses `fs::rerun_line`. The same `fs.rs` (870 lines) holds `/api/fs`'s
routes, which name `server::App`.

**Block constructors.** `block::create` names nine types' constructors:
`browser::Browser`, `agent::Agent`, `editor::Editor`, `remote::Remote`,
`review::diff::Diff`, `review::file::FileView`, `forge::ForgeBlock`,
`invite::card::InviteBlock`, and `labs::create_block` for Fountain, App and
Workspace.

**What blocks are given.** `BlockEnv` and `BlockCtx` carry
`mcp::Link`, `invite::Hook`, `block::Secrets` and `rules::Rules` on behalf
of one type each. `mcp`, `secrets` and `rules` are read only in
`agent/mod.rs` (12 sites). `invite` is read only in `invite/card.rs` (2
sites), and it is `Arc<OnceLock<Weak<App>>>`: **the core carries a weak
pointer to `App`.**

**Block-type knowledge in the mux:**

- `remote::parse` checks a remote block's config when it opens
  (`mux/blocks.rs:178`).
- `agent::defs::CLAUDE_CONFIG_DIR` is a constant (`mux/blocks.rs:297`).
- `invite::OWNER_ONLY` and `CLOSE_OWNER_ONLY` are two strings
  (`mux/attention.rs:697`, `mux/clients.rs:117`).
- `editor::presence::Presence::make` makes the block for an editor that
  joined the swarm (`mux/api_calls.rs:242`).
- `agent::transcript_of` renders an agent block's stored log for search
  (`history.rs:118`).

**Editors and the IDE.** The `Block` trait's `link`, `attach` and `detach`
name `editor::link::Link`, and `Api::EditorJoin` carries one. The mux calls
`bind`, `snapshot` and `followers` on it and keeps `Daemon.follows`.

Claude Code's IDE (M28) is the deepest tie:

- `Api::Ide(ide::Event)` comes in, and `mux/attention.rs:298–468`
  (`ide_event`, `diff_answer`, about 170 lines) keeps `Daemon.diffs` and
  `Daemon.ide_conns`.
- The mux calls `Ide::{reply, closed, target, forward, port}` and the
  helpers `ide::{saved, rejected, no_diagnostics, NAME, diff::unified}`.
  `Config.ide` is an `Arc<ide::Ide>`.

**Notifications and people.** The mux calls `control::Control` through
`Config.control`:

- `push` (four sites, each beside `Daemon.push`, the Web Push sender in
  `push.rs`, which uses `reqwest` and `p256`);
- `is_team`, `sandbox_done`, `co_owners`, `team_people` and `owns_here`.

`control.rs` is 3,254 lines and names `forge`, `labs`, `setup`, `dial` and
`sites`.

**Agent sessions.** `mux/config.rs:254` resumes an agent's conversation
through `conversations::Index`, and `mux/info.rs:82` finds Claude Code's
live sessions with `conversations::{Ours, live_in_panes}` (#146). The
`conversations` module is 2,217 lines.

**Pane cleanup.** `mux/clients.rs:356,371` calls `upload::{forget,
forget_on}` when a pane closes, so its uploads go with it.

**Rules.** `mux::start` opens `rules.json` only to put `rules::Rules`
into `BlockEnv` and `MuxHandle`. `rules.rs` names `mcp::same_tool`.

**Labs.** The mux holds `labs::Threads` and `labs::Calls`, and calls
`labs::threads::{names, mentions, calls_agent}`, `labs::vms_unavailable`
and `labs::stop_beholds`. pane.rs calls `labs::machine_start`. Seventeen
`#[cfg(feature = "labs")]` lines sit in the core files:

- `Block::wrote_run`, `BlockCtx::block` and `BlockCtx::update_reason`;
- `Api::GuestInput`, `Api::GuestSize` and `Api::GuestLeft`;
- `mux/thread_ops.rs` and `mux/call_ops.rs`, with their twins in
  `mux/labs_off.rs`.

The other direction matters as much. Integrations reach the core through
`MuxHandle` (`send`, `api`, `events`, and the fields `store`, `provider`,
`daemon_id`, `fs`, `ide`, `shell_env`, `rules` and `inventory`) and through
`Cmd::Api`. Outside `mux/`, code names 48 `Api` variants. The most used are
`Block` (40 sites), `Panes` (16), `Pane` (14), `Open` (6), and `Run`,
`RoleOn`, `MachineOf` and `SessionEnds` (5 each). `store::write_atomic` (36
sites) and `store::now_ms` (33) are used everywhere.

`Daemon` itself is private to `mux/`. No integration touches its fields
directly. They reach them through the mux's own files: `thread_ops.rs`
and `call_ops.rs` for chat and huddles, `attention.rs` for the IDE, and
`api_calls.rs` and `who_may.rs` for editors.

## The line

**`arugula-mux` holds the task that orders every change and what it needs
to run panes and blocks, and names no integration. `arugulad` is the
binary: the edge, the integrations, and `main.rs`, which wires them
together.** Every tie listed above either moves with the core, because
it's a leaf the core needs, or turns into something an integration hands
the core (a registered block kind, or a trait object in `Config`).

The core may still `match` on `arugula_proto::BlockType`. The types are a
closed enum in proto that every client knows, and per-type defaults on
open (`editor_defaults` in `mux/blocks.rs`) are mux data, not integration
code. What it may not do is call into an integration's module.

### What moves into `arugula-mux`

| From `crates/daemon/src` | Lines | Notes |
|---|---|---|
| `mux/` | 5,465 | whole, `labs_off.rs` included |
| `pane.rs`, `block.rs`, `store.rs`, `history.rs` | 4,099 | `block::create` becomes the registry (below) |
| `shim.rs`, `holder.rs`, `sys.rs`, `conpty.rs` | 1,230 | plus `home()` and `conpty_command_line` from `main.rs`, into `sys` |
| `osc`, `procinfo`, `classify`, `resume`, `keys`, `paths`, `perm`, `heap` | 1,951 | leaves |
| `shellint`, `shellenv`, `inventory` | 1,023 | |
| `provider/` | 295 | the trait; adapters stay in Labs |
| `host.rs` (Windows) | 428 | with `pipe::Sa` and `same_user`; the pipe *server* stays |
| `acl.rs` without `pub mod api` | ~545 | `Principal`, `Grant`, `Acl` |
| `fs.rs`'s `FsError`, `Scope`, `Machine`, `Target`, `rerun_line` | ~460 | the routes and handlers stay |
| `labs/threads.rs`, `labs/calls.rs`, `labs/machine.rs` | 436 | behind the crate's own `labs` feature (see [Labs](#labs)) |
| `ide/diff.rs`, plus `ide::Event`, `saved`, `rejected`, `no_diagnostics`, `NAME` | ~230 | the mux's half of the IDE |
| **About** | **16,100** | 20% of the daemon |

Its dependencies:

- workspace crates: `arugula-core`, `arugula-proto`, `arugula-vt`,
  `arugula-e2e` (`Subscriber.device` is an `arugula_e2e::Cert`);
- `tokio`, `serde`, `serde_json`, `futures-util`, `crossbeam-channel`,
  `zstd`, `regex`, `sha2`, `hex`, `tracing`, `anyhow`;
- `nix` on Unix and `windows-sys` on Windows.

It does **not** depend on `axum`, `hyper`, `reqwest`, `rmcp`, `rust-embed`,
`tokio-rustls`, `instant-acme`, `russh` or `arugula-control-wire`. That is
the line in one test: see ticket 6.

### What stays in `arugulad`

Everything else, about 64k lines:

- the edge: `server.rs`, `api.rs`, `mcp/`, `share.rs`, `upload.rs`,
  `dial.rs`, `e2e.rs`, `control.rs`, `pipe.rs` (the server);
- the HTTP access checks: `access.rs`, `localauth.rs`, `authz.rs`,
  `acl.rs`'s `api` (as `acl_api.rs`), `rules.rs`;
- every block type except the ones the core makes itself: `agent/`,
  `browser.rs`, `editor/`, `review/`, `forge/`, `invite/`, `remote.rs` and
  `labs/` (the threads, calls and machine modules above excepted);
- `conversations/`, `ide/` (the relay, lock files and prefs), `gate.rs`,
  `hand.rs`, `push.rs`;
- reach: `hosts.rs`, `sync.rs`, `seal.rs`, `tailscale.rs`, `roots.rs`,
  `sites.rs`, `tls.rs`, `ports.rs`;
- lifecycle: `main.rs`, `args.rs`, `install.rs`, `setup.rs`, `update.rs`,
  `selfupdate.rs`;
- `ops/`, which is all of operations' daemon half (see
  [Operations](#operations)).

## The interface across the line

Three mechanisms, each for one kind of tie. Labs (#452) chose among the
same three, and this note picks each where it fits:

- **Registration** where the choice is made at run time, by data: a
  block's type comes back from `layout.json` or an `OpenRequest`.
- **Traits** where the core calls out to something it must not name:
  notifications, people, agent sessions, the IDE, an editor's link, a
  pane closing.
- **Features** where the choice is made at build time: Labs, and later
  #461's forge and editor.

### Block kinds: a registry

`block::create`'s `match` becomes a table the binary fills before the mux
starts:

```rust
// arugula_mux::block
pub trait BlockKind: Send + Sync + 'static {
    /// Make one from its config (today's `X::create(ctx, config)`).
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String>;
    /// Check a config before the block is placed (today `remote::parse`).
    fn check(&self, _config: &Value) -> Result<(), String> { Ok(()) }
    /// A stored log as text, for history and search (today
    /// `agent::transcript_of`).
    fn stored_text(&self, _dir: &Path) -> Option<String> { None }
}

#[derive(Default)]
pub struct BlockKinds(HashMap<BlockType, Arc<dyn BlockKind>>);
impl BlockKinds {
    pub fn add(&mut self, kind: BlockType, k: impl BlockKind);
}
```

`Config` gets a `kinds: Arc<BlockKinds>` field, and `history::search`
takes it as a parameter. A type that nobody registered answers with what
`block::create` says today: "terminals aren't made here" for `Terminal`,
and "a block type this build doesn't know" for `Unknown` and the rest.

**What a block is given shrinks to what the core knows.** `BlockEnv` and
`BlockCtx` lose `mcp`, `invite`, `secrets` and `rules`. The agent's kind
holds the first three. The invite kind holds the `invite::Hook`. Both are
built in `main.rs`, where `App` exists. A block keeps reading them from
`self` instead of `self.ctx` (14 sites in two files). This removes the
core's only path to `App`.

**Labs' block types** register the same way. `labs::create_block` becomes
`labs::kinds(&mut BlockKinds)`. Its twin (without the `labs` feature)
registers kinds whose `create` returns today's `not_built` text, so a
saved Fountain block in a build without Labs still says "A Fountain block
isn't in this build".

**Editors.** The `Block` trait names `Arc<dyn EditorLink>` instead of
`editor::link::Link`. The trait has what the mux and `Editor` call: `id`,
`info`, `file`, `bind`, `snapshot` and `followers`. `editor::link::Link`
implements it in the binary. `Api::EditorJoin` carries the
`Arc<dyn Block>` the binary made with `Presence::make`, so the mux only
reserves the id and binds it.

### Callouts: traits in `Config`

`Config.control: Arc<control::Control>` and `Daemon.push: Option<Push>`
become two traits:

```rust
// arugula_mux::outside
pub trait Notify: Send + Sync {
    /// Today's `push.send(…)` followed by `control.push(…)`, at each of the
    /// four sites.
    fn notify(&self, pane: PaneId, title: &str, body: &str, extra: Option<Value>,
              to: &(dyn Fn(&Principal) -> bool + Send + Sync));
}
pub trait People: Send + Sync {
    fn is_team(&self) -> bool;
    fn owns_here(&self, account: &str) -> bool;
    fn co_owners(&self) -> Vec<Principal>;
    fn team_people(&self) -> Vec<Principal>;
    /// A hosted sandbox's last session closed (M20).
    fn sandbox_done(&self);
}
```

`Control` implements `People`. The binary's `Notify` holds the `Push` and
the `Control` and does both sends in today's order.

**Four smaller ties** become trait objects of their own:

- `AgentSessions` (`transcript(id) -> Option<PathBuf>` and
  `live(&Ours) -> …`), implemented by `conversations`;
- `IdeLink` (`port`, `reply`, `closed`, and `forward_diff`, which forwards
  a diff to another IDE and returns `true`, or returns `false` to keep it
  here; that is today's `target()` plus `forward(…)`), implemented by
  `ide::Ide`;
- `PaneGone`, implemented by `upload` (today's `forget` and `forget_on`).

`Config.ide` becomes `Option<Arc<dyn IdeLink>>`.

**Moved instead of hidden.** These are cheaper to move than to hide:

- `CLAUDE_CONFIG_DIR` is a string, so it moves to `arugula-mux`'s
  `agentenv`, or to proto.
- `invite::OWNER_ONLY` and `CLOSE_OWNER_ONLY` are strings. The mux already
  decides "is an invite" by `BlockType::Invite`, so they move to the core.
- `rules.json` is opened in `main.rs` instead of `mux::start`. `Rules`
  moves from `MuxHandle` to `App` (`app.mux.rules` becomes `app.rules`, two
  sites in `api.rs`).

### Attention and state: already an interface

Blocks raise attention and push state through `BlockCtx`: `changed`,
`attention`, `reason`, `clear`, `event`, `machine`, `ask` and `withdraw`.
This is the M11 design and doesn't change. Terminals get attention from
the edge through `Cmd::Api`: `Hook`, `Ask`, `Attention`, `FollowUp` and
`Inbox`. The IDE's diff cards stay in the mux, because a diff waiting for
you is attention ordered with everything else. Only the IDE's transport
crosses the line, as `IdeLink` and `Api::Ide`.

### Routes and MCP tools: they never reach the core

The core serves nothing. Every route and MCP tool is in the binary, and
the binary knows every integration, so this split needs no registration
for them. They call the core through `MuxHandle::api` as they do today.
Which integrations a *build* has is a question for features (Labs now,
#461 next). Within the binary, an integration contributes routes the way
`labs::routes` does now: one function that chains its `.op::<O>()` lines
and its hand-written routes.

### Operations

The parts of `docs/operations.md`:

- `arugula_proto::op` (`Op`, `Access`, `Method`, `PathArgs`): stays in
  **proto**, already shared by the daemon and the CLI.
- `ops/`'s `Handle`, `Cx`, `Via`, `OpError`, `FromPath`, `serve`,
  `OpRoutes`, `policy` and `every_op!`: the **binary**. `Cx` holds
  `&Arc<App>` and `Via::Mcp(&Call)`, `serve` is an axum handler, and
  `policy` feeds `authz.rs`.
- `mcp/ops.rs` (`McpOp`, `def`, `kind`, `call`): the **binary**.

So operations and this split don't touch each other's types. Handlers
reach the core only through `MuxHandle`. The only conflicts are import
paths in `ops/*.rs`, which is why #460 follows #578.

### The hard cases

**`Daemon`'s fields.** These stay in the core, all of them. No field moves
behind a trait, because each is read inside the mux task's ordering:

- Chat and huddles (`threads`, `calls`, and `thread_ops.rs` and
  `call_ops.rs`, which read `clients`, `config.acl` and `config.control`):
  `labs/threads.rs` and `labs/calls.rs` move into the core under its
  `labs` feature. Together they are 406 lines and depend only on `store`.
  `control`'s three reads become `People`.
- The IDE (`diffs`, `ide_conns`): the mux's half moves (see the table),
  and the relay stays behind `IdeLink`.
- Editors (`follows`, presence blocks): `EditorLink`.
- `fs`, `shell_env`, `inventory`: they move, being leaves.
- `rules`: leaves the mux (above).

**`App` in `server.rs`.** It stays in the **binary**, whole. It is the
binary's state: `MuxHandle` (from the core) next to `Hosts`, `Shares`,
`Synced`, `Control`, `Acl`, MCP's `Tokens`, `Guests`, `Hands`, `Push` and
`DialOuts`. The core never sees it. It saw it once, through
`invite::Hook`, and that moves into the invite block's kind. `App` gains
`rules` and `ide` (as `Option<Arc<ide::Ide>>`, the concrete type; the
`dyn IdeLink` is a clone of it in `Config`). Everything else reaching the
core through `app.mux` keeps doing so.

**`hosts.rs`'s `HostFeatures`.** The type is in proto and stays there.
`hosts::features(&App)` stays in the **binary**, because it asks
integrations (`sites::get`, `labs::fountain_login_here`,
`labs::studio_here`), which only the binary may name. Two things cross the
line, both read-only: `app.mux.provider.is_some()` for `vms`, and the
state dir, through `app.control.state_dir()` for `labs` (today's path).
`hosts.rs` as a whole (812 lines: other daemons, tokens, invites) is Reach
and stays.

**`MuxHandle`.** Its fields stay public, minus `rules` and `ide`: `store`,
`provider`, `daemon_id`, `fs`, `shell_env` and `inventory` are all core
types after the move. `Cmd` and `Api` stay one enum each in the core. They
are the mux task's inbox, so integration-specific variants (`Ide`,
`EditorJoin`, `DiffAnswer`, `Guest*`) stay as variants carrying core types
or trait objects. Making `Api` extensible would change how every caller
talks to the mux, which is out of scope here.

## Access checks

`authz.rs` stays in the binary. It matches HTTP methods and paths,
operations' `policy` feeds it, and its `Policy` is about routes. So do
`access.rs` (Host and Origin checks, peers) and `localauth.rs`. The
*model* moves: `Principal` and `Acl` (grants, roles, floors, the audit
record). The mux decides per WebSocket message who may do what
(`who_may.rs`) and filters each person's `State` by it (`clients.rs`), so
it can't call out for that. `arugula_core::access` (roles per session)
stays where it is. The invariant in `AGENTS.md` holds unchanged: every
request passes the access checks before it reaches the mux.

## The state directory

Nothing moves on disk. `StateDir` (`layout.json`, `panes/N/`) moves with
`store.rs`. `main.rs` still chooses the root (`--state-dir`,
`ARUGULA_STATE_DIR`) and hands it to `mux::start`. Every other file under
it stays the binary's, at the same path: `hosts.json`, tokens,
`rules.json` (now opened by `main.rs`), control's files, `editors/sock`,
and the `labs` file (read by `arugula_proto::hosts::labs`). Threads'
`threads/` moves with `labs/threads.rs`, and a build without Labs still
never opens it.

## The API server and the web client

`server.rs` stays in `arugulad`: `/ws`, the routers (`router`,
`local_router`, `tunnel_router`, `channel_router`, `editors_router`), the
guard and `cors` middleware, and `#[derive(Embed)] struct Assets` with
`#[folder = "../../web/dist"]`. The path is relative to `crates/daemon`,
which doesn't move, so `just web`, the `debug-embed` feature, release
builds and the desktop sidecar are unchanged. The core crate has no
`rust-embed` and no HTTP. #387 item 4 (the daemon serves the web client,
so any browser works against any machine) is kept by construction: the
binary that serves is still the binary that embeds.

`/ws` uses `pane::{Subscriber, ToClient, client_queue}` and `Cmd`. These
become `arugula_mux` exports, and `server.rs` changes only its `use`
lines.

## Labs

The daemon's pattern (`labs/mod.rs`: one surface, a twin for each item, no
`cfg` at call sites) carries over and repeats once:

- `arugula-mux` gets its own `labs` feature and its own `src/labs.rs`
  surface for what Labs puts inside the mux task. That is `Threads`,
  `Calls`, `thread_names`, `machine_start`, `vms_unavailable`, `not_built`
  and `BUILT`, with their twins (today's `labs/mod.rs` `absent` module,
  the core half). The seventeen `#[cfg(feature = "labs")]` lines in the
  core files move with their files, unchanged.
- `arugulad`'s feature forwards it: `labs = ["arugula-mux/labs",
  "dep:russh"]`. Its `labs/mod.rs` re-exports the core's items (so
  `crate::labs::Threads` still resolves) and keeps the binary's own.
  `labs::kinds` registers Labs' block types.
- `just check-core` adds `-p arugula-mux` to its clippy and nextest lines.

Moving chat and huddles *out* of the mux task (so the core wouldn't need
the feature) would change the order threads and layout changes are seen
in. That is a behaviour change, and not proposed.

## How the moves keep churn down

**Re-export at the binary's root.** `main.rs` declares `pub(crate) use
arugula_mux::{store, pane, block, mux, …};` instead of `mod store;`, so
the binary's `crate::store::…`, `crate::pane::…` and `crate::mux::…`
paths keep resolving. There are 365 such paths to the moving modules
today. Each move's diff is then `git mv`, the crate's `lib.rs`, the
visibility changes and the ties above, not every file that names a moved
module. Rewriting the paths is optional (ticket 7).

**Visibility.** Most of the core's items are already `pub`, because the
modules are private in `main.rs`. What changes is small:

- the `pub(crate)` items the binary names: there are 10 in the moving
  code (`fs.rs` 8, `provider/mod.rs` 2);
- `mux`'s submodules stay private, with their `pub(super)` items;
- `mod` declarations in the new `lib.rs` become `pub mod` where the
  binary reaches in.

**Dead code.** A binary warns about unused `pub` items, but a library
doesn't. After the move, `arugula-mux`'s unused API stops warning, and its
`#[cfg_attr(not(feature = "labs"), allow(dead_code))]` attributes on `pub`
items do nothing. Ticket 5 removes the ones on `pub` items. Expect some unused
code to go unnoticed from then on.

## Proposed tickets

Each is one PR, with no change in behaviour, on the wire or on disk. Each
is done when `just check` and `just check-core` pass, the integration
tests' count under nextest is unchanged, and the unit tests' total across
`arugulad` and `arugula-mux` equals the base's. All come after #578.

1. (replaces #460) **`arugula-mux`, with the leaves and the access
   model.** `crates/mux` (`arugula-mux`, lib `arugula_mux`) with `store`,
   `perm`, `paths`, `keys`, `heap`, `osc`, `procinfo`, `classify`,
   `resume`, `sys` (plus `home`, `conpty_command_line`), `shim`, `holder`,
   `conpty`, and `acl` without its `api` (which becomes the daemon's
   `acl_api.rs`). The daemon depends on it and re-exports each at its
   root. About 4.8k lines moved. **M.** Depends on #578.
   - *Traps:* the shim's `_shim` dispatch stays in `main.rs`, calling
     `arugula_mux::shim::run`, before any thread starts. `conpty.rs` is
     Windows-only, and nothing on `main` compiles Windows (see [Open
     questions](#open-questions)). `acl`'s `pub mod api` is a nested
     module: split it by hand.
2. **Blocks behind a registry, in place.** `BlockKind` and `BlockKinds`.
   `block::create` and `labs::create_block` become registrations made in
   `main.rs`. `BlockEnv` and `BlockCtx` lose `mcp`, `invite`, `secrets` and
   `rules` to the agent's and invite's kinds. `EditorLink` replaces
   `editor::link::Link` in `Block`. `EditorJoin` carries the presence
   block. `remote::parse` becomes `check`, and `agent::transcript_of`
   becomes `stored_text`. No crate move. **M.** Independent of ticket 1.
   - *Traps:* Labs' twin must register refusals with today's exact text,
     which `just check-core`'s tests read. `invite::Hook` is set after
     `App` exists, so the invite kind holds the same `OnceLock`, not a
     value.
3. **The mux's callouts behind traits, in place.** `Notify` and `People`
   replace `Config.control` and `Daemon.push`, plus `AgentSessions`,
   `IdeLink` and `PaneGone`. `CLAUDE_CONFIG_DIR` and the invite strings
   move to core-side modules. `rules.json` opens in `main.rs`, and
   `app.mux.rules` and `app.mux.ide` become `app.rules` and `app.ide`.
   The mux's half of the IDE (`Event`, `diff.rs`, `saved`, `rejected`,
   `no_diagnostics`, `NAME`) splits into a file of its own. **M.**
   Independent of tickets 1 and 2.
   - *Traps:* `Notify` must keep each site's order (Web Push first, then
     control) and filters. The `ide::Ide::forward` call takes the mux's
     `tx`, which `forward_diff` keeps taking.
4. **Panes into `arugula-mux`.** `pane.rs`, `provider/`, `shellint`,
   `shellenv`, `inventory`, `host.rs` (with `pipe::Sa` and `same_user`
   moved to the core's `sys`), `fs`'s `FsError`, `Scope`, `Machine`,
   `Target` and `rerun_line`, and `labs/machine.rs` (behind the crate's new
   `labs` feature and its `src/labs.rs` surface). About 4.8k lines.
   **L, but mostly `git mv`.** After 1.
   - *Traps:* `impl IntoResponse for FsError` can't stay on a type from
     another crate (the orphan rule). It becomes a `From<FsError> for
     ApiError` in the binary, with the same status and text. `host.rs` and
     the pipe security helpers are Windows-only. `pane.rs`'s
     `#[cfg(unix)]` and `#[cfg(windows)]` blocks mean macOS and Linux
     clippy don't see the Windows half.
5. **The mux into `arugula-mux`.** `mux/`, `block.rs` and `history.rs`,
   plus `labs/threads.rs` and `labs/calls.rs` into the core's Labs
   surface. The daemon's `labs` feature forwards `arugula-mux/labs`, and
   `just check-core` adds `-p arugula-mux`. **L, mostly `git mv`.** After
   2, 3 and 4.
   - *Traps:* `mux/attention.rs`'s three tests build reasons with
     `gate::reason`, and `gate.rs` stays in the binary (it names `labs` and
     `review`). They stay in the daemon as tests of `push_reason`, or build
     the `Reason` by hand. `Api`'s `#[cfg(feature = "labs")]` variants
     need the forwarded feature, or a Labs build fails to match them.
6. **Docs and the guard.** `crates/mux/README.md`, `AGENTS.md` ("Crates"
   and "The daemon, by layer": the mux, Panes and Who may move, partly),
   the daemon's README, and a `DECISIONS.md` entry: "the mux names no
   integration". Add a check in `just check` that `cargo tree -p
   arugula-mux -e normal` has no `axum`, `hyper`, `reqwest`, `rmcp`,
   `rust-embed` or `russh`, so the line can't drift back. **S.** After 5.
7. **Optional: name the core by its crate.** Replace `crate::store::…`
   and the rest with `arugula_mux::…` in the binary, and drop the root
   re-exports. Mechanical. **M** in lines, **S** in thought. After 5.
   Unnecessary if the team prefers the re-exports.

After 5, #461 (GitHub forge and editor behind features, build times
measured) starts from a core that names neither.

## Open questions

1. **The name.** `arugula-mux` matches the `mux/` module it's built
   around and the ticket's intent (`illogical-mux` before the rename). The
   cost is that `arugula-core` already calls itself "the multiplexer's
   state", so the two read alike: `arugula_core::Mux` is the layout model,
   and `arugula_mux` is the task and the panes. `arugula-panes` is the
   alternative if that is too close. (`arugula-daemon-core` would be worse
   beside `arugula-core`.)
2. **Windows.** No job on `main` compiles the Windows build. Only
   `app-release.yml`'s `windows` job does, at release. Tickets 1 and 4 move
   `conpty.rs`, `host.rs` and the pipe helpers. Either add a `cargo check`
   on GitHub's Windows runner to `check.yml` first, or have each of those
   PRs checked by hand on Windows.
3. **Keep the root re-exports, or do ticket 7?** Keeping them makes every
   later diff smaller. Dropping them makes it obvious at each use which
   crate a name comes from.
4. **The IDE's diffs in the core.** This note keeps them in the mux,
   because they're attention. The other choice is to make each waiting
   diff a block of its own type, so all of the IDE stays out of the core.
   That is a visible change (a new block type), so it's not proposed here.
   Say if it's wanted later.

## Things that surprised me

Read these before reviewing the tickets:

- The daemon is 80k lines now (the ticket said 71k). The core as drawn is
  about 16k.
- The core already holds a `Weak<App>`, through `invite::Hook` in
  `BlockEnv`. It's the only path from the core back to the binary's state.
- Chat and huddles, though Labs, have to be in the core crate, because
  they live in `Daemon`'s fields.
