# Arugula desktop

The desktop app (M46, Tauri 2): the daemon's own web client in a native
window, with a tray, updates, deep links and a bundled daemon for first
install.

It's outside the Cargo workspace, with its own `Cargo.lock`, so it builds and
releases on its own (#388). It depends on `arugula-proto` by path.

Start with `src/main.rs`, then `src/service.rs` and `src/updates.rs`.
`src/daemon.rs` is the *Daemon* menu (tray, app menu, Dock) and the
notification when control drops this machine. Only the app's own pages,
the daemon's, control's and the platform page's stay in the window; other
sites go to the browser.
`src/frames.rs` is why a block's frame stays too, and `src/platform.rs`
why the platform page does (on the origin control's `/control.json` says,
with the same window permissions as control's page).
`just desktop-check` checks it.

Passkeys in the window (D2, macOS): a WKWebView gets WebAuthn only for a
site the app is associated with. A release built with the Developer ID
provisioning profile (`APPLE_PROVISIONING_PROFILE`, `just desktop-macos`)
carries it as `Contents/embedded.provisionprofile` and is signed with
`webcredentials:control.arugula.io` (`scripts/macos-sign`); control names
the app back in `/.well-known/apple-app-site-association`. Only then does
control's page in the window keep its passkey button (`src/cloud.rs`,
`passkeys_on`). Without the profile the app is built as before.
