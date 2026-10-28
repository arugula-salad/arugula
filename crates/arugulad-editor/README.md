# arugulad-editor

The daemon's editor server and Claude Code's IDE, split out of
`arugulad-base` (#232) so an edit here rebuilds this crate and the binary,
not the daemon:

- `src/editor/`: editor blocks (M27, VS Code as code-server), the editors
  that join the swarm (`link.rs`, `presence.rs`), and Arugula's VS Code
  extension (`ext/`, packed by `vsix.rs` for `arugulad _vsix`).
- `src/ide/`: the daemon as Claude Code's IDE (M28): the link to the relay
  (`mod.rs`), the relay itself (`relay.rs`, `arugulad _ide_relay`), and the
  diff cards (`cards.rs`).

`src/lib.rs` is the whole of its surface: `register()`, which the binary
calls when its `editor` feature is on, and the implementations of
`arugulad_base::editors::{Editors, Ide}`. The daemon names nothing in here.
The IDE's operations (`GET`/`PUT /api/ide`, mentions) are here too
(`src/ide/ops.rs`): `impl Handle<Editor> for IdeGet`, routed with
`op_of::<IdeGet, Editor>()` (docs/operations.md, "A feature crate's
operations").

The code here still names what it uses as `crate::…` (`crate::task`,
`crate::mux`, `crate::server::App`): `lib.rs` brings those modules in from
`arugula-mux` and `arugulad-base`.
