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

Passkeys in the window (D2, macOS): a WKWebView gets WebAuthn only for a
site the app is associated with. A release built with the Developer ID
provisioning profile (`APPLE_PROVISIONING_PROFILE`, `just desktop-macos`)
carries it as `Contents/embedded.provisionprofile` and is signed with
`webcredentials:control.arugula.io` (`scripts/macos-sign`); control names
the app back in `/.well-known/apple-app-site-association`. Only then does
control's page in the window keep its passkey button (`src/cloud.rs`,
`passkeys_on`). Without the profile the app is built as before.

## A keychain prompt after an update (#1647)

macOS may ask, once, for a keychain password because "Arugula wants to
use your confidential information stored in 'Arugula WebCrypto Master
Key'". The item isn't Arugula's: WKWebView makes it in the login keychain
(account `com.apple.WebKit.WebCrypto.master+io.arugula.desktop`) to wrap
the WebCrypto keys a page keeps in IndexedDB, here the device key
(`web/src/shell/e2e/keys.ts`). Its access list names the build that made
it. Apps up to 0.27.0 were ad-hoc signed, so on a Mac that ran one, the
item names that build by its cdhash, and the first Developer ID build to
read it asks. Every release since 0.27.4 is signed with the same Developer
ID (team HNDPMQPF22, `scripts/macos-sign`), whose designated requirement
is the bundle id and the team, so once it's allowed, later updates don't
ask, and an item a signed build made shouldn't either. `scripts/macos-sign`
refuses to release an ad-hoc app, which would start it over.

What to tell someone who sees it:

- Type the login keychain's password (usually the Mac's login password;
  after a password change outside System Settings it can be the old one)
  and click **Always Allow**. The app may ask a second time, to add our
  team to the item's partition list; **Always Allow** again. It doesn't
  come back.
- **Deny** or **Cancel**: the page can't read the keys it kept, and the
  app asks again at its next launch (restarting it brings the prompt back
  for **Always Allow**). Until it's allowed, this device may not unlock
  its key, and may need approving again.
- Not knowing the password: quit the app, delete **Arugula WebCrypto
  Master Key** from the login keychain in Keychain Access, and open the
  app. WebKit makes a new item with no prompt, but the device key it
  wrapped is gone: sign in to control again and approve the device.

Seen on lex00's Mac on 2026-10-27, `log show --predicate 'process ==
"securityd"'`: the item, made 2026-10-07 by app 0.26.1 (ad-hoc, cdhash
`1be1dd51…`), asked when 0.30.1 first read it, and stopped after
**Always Allow**.
