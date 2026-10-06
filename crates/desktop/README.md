# desktop

Tauri 2 desktop app: a window and tray icon for macOS, Linux, and Windows. Embeds the web client and wraps it with native features like the updater and deep links.

**Excluded from the workspace:** has its own `Cargo.lock` and can release independently (#388). Pinned versions of some dependencies (sha2, getrandom) differ from the workspace to avoid tight coupling.

**Dependencies:** tauri, serde, proto (for types).

**Start reading:** [`src/main.rs`](src/main.rs) for the window setup, and `tauri.conf.json` for the app configuration.
