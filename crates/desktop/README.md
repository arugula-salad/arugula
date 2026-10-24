# Arugula desktop

The desktop app (M46, Tauri 2): the daemon's own web client in a native
window, with a tray, updates, deep links and a bundled daemon for first
install.

It's outside the Cargo workspace, with its own `Cargo.lock`, so it builds and
releases on its own (#388). It depends on `arugula-proto` by path.

Start with `src/main.rs`, then `src/service.rs` and `src/updates.rs`.
`src/daemon.rs` is the *Daemon* menu (tray, app menu, Dock) and the
notification when control drops this machine. `src/notices.rs` shows what
control would push this device (chat, the machines' panes, projects), while
the app runs, once control's page turns *Notify this device* on: a webview
has no Web Push. Only the app's own pages,
the daemon's, control's and the platform page's stay in the window; other
sites go to the browser.
`src/frames.rs` is why a block's frame stays too, and `src/platform.rs`
why the platform page does (on the origin control's `/control.json` says,
with the same window permissions as control's page).
`just desktop-check` checks it.
