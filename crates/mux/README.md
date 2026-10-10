# arugula-mux

The daemon's core as a library: the task that orders every change to the
layout, the panes it owns, the blocks and their registry, the access model
(`acl`), history, the state directory (`store`) and the leaves under them
(`paths`, `perm`, `sys`, the pane shim and holder, ConPTY). `arugulad` is the
binary around it; [docs/mux-crate.md](../../docs/mux-crate.md) is the design
and [DECISIONS.md](../../DECISIONS.md) ("The mux names no integration") the
rule.

Depends on `arugula-core`, `arugula-e2e`, `arugula-proto` and `arugula-vt`. It must not name
any integration: no HTTP server or client, no MCP, no SSH, no embedded web
client, no control. `just mux-guard` fails if `axum`, `hyper`, `reqwest`,
`rmcp`, `rust-embed` or `russh` appear in its dependency tree, and `just check`
and `just check-core` run it.

How an integration reaches the mux, since the mux can't reach it:

- **Block kinds.** A `BlockKind` per `BlockType`, added to `BlockKinds`
  (`src/block.rs`) before the mux starts and handed over in `Config::kinds`.
  Browsers, agents, editors, forges and the rest are the binary's; the mux
  only asks the registry to make one.
- **Traits.** What the mux calls out to is a trait in `src/mux/outside.rs`
  (`Notify`, `People`, `AgentSessions`, `IdeLink`, `PaneGone`), plus
  `Provider` for machines, all held in `Config`. The binary implements them.
- **At exit.** `Config::at_exit` is what the binary runs when the mux shuts
  down, after the save.
- **The other way.** The routes and tools call in through `MuxHandle`, which
  sends `Cmd::Api` (`src/mux/api_calls.rs`) to the task, so nothing but the
  mux task changes the layout.

Start with `src/mux/mod.rs` (the task and `MuxHandle`), then `src/mux/config.rs`
and `src/mux/outside.rs` (what the binary supplies), `src/pane.rs` (a process
on a PTY and its VT thread) and `src/block.rs`.

The `labs` feature carries what Labs puts inside the mux task (threads and
huddles, `src/labs/`), with a twin for each item when it's off, so call sites
have no `cfg`. `arugulad`'s `labs` forwards it (`labs = ["arugula-mux/labs", …]`),
so the two are on together.

Its tests are unit tests beside the code; the daemon's integration tests
(`crates/daemon/tests/`) cover it from outside. `just check-core` clippies and
runs both without Labs, and `just check` with.
