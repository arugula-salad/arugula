# illogicald

The daemon owns terminals and blocks. One per machine, shared by browsers, editors, the desktop app, and agents.

**The core invariant:** the mux task in `src/mux/mod.rs` decides the order of every change. Panes get their own VT threads (pane owns libghostty's `!Send` terminal). Every API route goes through access checks before reaching the mux.

**Architecture:** the daemon is organized in layers (see `AGENTS.md`):
- **Edge:** WebSocket, HTTP API, MCP, relay, share, guest SSH, dial-out, IDE, and file upload.
- **Who may:** Access control, ACL, authorization, and permissions.
- **Core loop:** Mux task and its submodules (`config`, `attention`, `clients`, `info`, `blocks`, `api_calls`, `who_may`, `thread_ops`, `machines`, `call_ops`), plus main loop and threading rules.
- **Panes:** PTY, VT thread, history, shell integration, and OSC signal handling.
- **Blocks:** Forge, agent blocks, Fountain, editors, conversations, apps, reviews, and invites.
- **Reach:** Hosting, dialing, sync, and Tailscale.
- **Lifecycle:** Install, setup, and updates.

**Dependencies:** axum, tokio, proto, vt, core, and many others (see `Cargo.toml`).

**Start reading:** [`src/main.rs`](src/main.rs) for the clap arguments and overall structure. Then see `AGENTS.md` for the layer map and the files in each layer.

**Tests:** integration tests in `tests/`, property tests for core state in `crates/core/`.
