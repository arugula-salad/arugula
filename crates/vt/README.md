# illogical-vt

Server-side terminal state on libghostty-vt. `VtEngine` takes PTY output, resize events, and terminal queries; maintains the screen state; and produces snapshots for xterm.js and checkpoints for disk storage.

Also handles OSC signals, VT compatibility for xterm.js's measured capabilities, and recorded fixtures for testing.

**Dependencies:** libghostty-vt-sys (vendored and patched), zstd (for checkpoints).

**Start reading:** [`src/lib.rs`](src/lib.rs) for the `VtEngine` interface and overview, then [`src/compat.rs`](src/compat.rs) for xterm.js compatibility.
