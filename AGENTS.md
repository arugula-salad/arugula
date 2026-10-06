# illogical system map

illogical is a multiplexed terminal server and web client: one `illogicald` daemon per machine, shared across browsers, editors, and the desktop app. Every change goes through the mux task for ordering.

## Crate map

- `crates/core`: sessions, tabs, splits, and cell layout.
- `crates/proto`: wire protocol for ClientMsg, ServerMsg, and API types.
- `crates/vt`: server-side terminal state on libghostty-vt.
- `crates/e2e`: Noise protocol for client–control encryption.
- `crates/daemon` (`illogicald`): the multiplexer, panes, blocks, and web server.
- `crates/cli` (`illogical`): CLI, TUI, tmux -CC, and MCP.
- `crates/control`: hosted service for accounts, devices, and the relay.
- `crates/testkit`: helpers for tests only.
- `desktop` (outside workspace): Tauri app, own `Cargo.lock`.

## The daemon's layers

**Edge:** WebSocket (`server.rs`), HTTP API (`api.rs`), MCP tools (`mcp/`), control relay (`control.rs`), share links (`share.rs`), guest SSH (`guest_ssh.rs`), dial-out peers (`dial.rs`), IDE port (`e2e.rs`), and file upload (`upload.rs`).

**Who may:** Access control (`access.rs`, `acl.rs`), authorization (`authz.rs`), local auth (`localauth.rs`), and permissions (`perm.rs`).

**Core loop:** Mux task (`mux/mod.rs` and its submodules: `config.rs`, `attention.rs`, `clients.rs`, `info.rs`, `blocks.rs`, `api_calls.rs`, `who_may.rs`, `thread_ops.rs`, `machines.rs`, `call_ops.rs`), attention and hand-offs (`hand.rs`), block operations (`block.rs`), run loop (`main.rs`), push notifications (`push.rs`), threading rules (`threads.rs`), and method gates (`gate.rs`).

**Panes:** PTY and VT thread (`pane.rs`), terminal history (`store.rs`), shell environment (`shellenv.rs`, `shellint.rs`), process info (`procinfo.rs`), OSC signals (`osc.rs`), pane shim (`shim.rs`), PTY holder (`holder.rs`), line classification (`classify.rs`), conpty (`conpty.rs`), system APIs (`sys.rs`), and resume policy (`resume.rs`).

**Blocks:** Forge integrations (`forge/`), agent blocks (`agent/`), Fountain (`fountain/`), editors (`editor/`), conversations (`conversations/`), applications (`apps/`), workspace (`workspace/`), reviews (`review/`), invites (`invite/`), IDE (`ide/`), file browser (`fs.rs`), and browser block (`browser.rs`).

**Reach:** Hosting (`hosts.rs`), host info (`inventory.rs`), dialing (`provider/`, `sandbox.rs`), sync and seal (`sync.rs`, `seal.rs`), Tailscale (`tailscale.rs`), and resident daemons (`resident.rs`).

**Lifecycle:** Install (`install.rs`), setup (`setup.rs`), host paths (`host.rs`), updates (`update.rs`), and heap management.

## Invariants

- The mux task decides the order of every change: `crates/daemon/src/mux/mod.rs`.
- A pane's VT thread owns libghostty's `!Send` terminal: `crates/daemon/src/pane.rs`.
- Wire types are generated into `web/src/proto.gen.ts` by `just proto-ts` (CI fails if stale).
- Every route goes through the access checks: `crates/daemon/src/access.rs`, `acl.rs`, `authz.rs`.
- The daemon serves the web client on purpose (#387 point 4): static files in `crates/daemon/src/server.rs`, embedded in the binary.

## Client policy

The web client and desktop app get all new features. The TUI (`crates/cli/src/tui/`) and tmux -CC (`crates/cli/src/tmux/`) are kept working and tested but get no new features.

## Labs

An empty `labs` file in the state dir turns on features hidden at launch (#385). It's read by `illogical_proto::hosts::labs(state_dir)`. Set `ILLOGICAL_STATE_DIR` to move the state dir. The cargo feature is planned (#452).

## Commands

- `just check`: Lint and test (nextest, doc tests, web typecheck, control-smoke).
- `just test`: Daemon, web, and control tests.
- `just dev`: Dev loop with separate daemon on 7682.
- `just e2e`: Browser specs for the web client.
- `just e2e-webkit`: WebKit specs on macOS.
- `just proto-ts`: Generate TypeScript types from Rust.
- `just bootstrap`: Install toolchains and web deps.
- `just web`: Build the embedded web client.
- One cargo target per worktree: set `CARGO_TARGET_DIR` (e.g., `~/.cache/illogical-arch/<ticket>`).

## Milestone codes

Comments carry codes like M43 and S21. When you touch a module anyway, replace its leading "M43:" tag with a plain name. See [DECISIONS.md](DECISIONS.md): the decisions that still constrain the code (#444).

## Where decisions live

[DECISIONS.md](DECISIONS.md): the decisions that still constrain the code (#444).
