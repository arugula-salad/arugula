# S46 click-through sheet: real providers, real devices

What the local runs can't show: whether each password manager honours related origins, and whether
it really asks for Face ID, Touch ID, a PIN or a password before it sets UV. About 2 minutes per
row. The README says why each row matters; this sheet is only the steps.

## Before you start

1. On geek (or any machine with `cloudflared` and Node 20+), run
   `spikes/s46-passkey-approver/harness/tunnel.sh`. It prints three URLs, with QR codes:
   **RP**, **app** and **stranger**. They last until you press Ctrl-C. Nothing touches control,
   DNS or Fly. If the tunnels stop, start them again; the URLs change, and the passkeys you made
   before are for the old URLs only (start that row again).
2. Each passkey you make is saved as "S46 test" for a `*.trycloudflare.com` host. Delete them when
   you're done (step 7).
3. For 1Password and Bitwarden rows, turn the provider's passkey saving on and the others off.
   On the iPhone that's Settings > General > AutoFill & Passwords. On a Mac browser, it's the
   extension's own setting. Write down the versions.

## Each row

1. Open **RP**. Copy the "What this browser says" block into the row's notes (only the first row
   per browser needs it).
2. Tap **1. Make a passkey**. Choose the provider under test. Note whether it asked for Face ID,
   Touch ID, a PIN or a password.
3. Tap **2. Sign with it here** on RP. *Expected:* worked, `UV=1`, log check `verified`.
4. Open **app** and tap **2. Sign with it here**. This is the question. *Expected if the
   provider honours related origins:* worked, origin is the app's URL, log check `verified`.
5. Tap it again straight away. Did it ask again (Face ID, Touch ID, PIN or password), or just
   sign? `UV=1` with no prompt at all, on the second try, is the dishonest case.
6. Open **stranger** and tap **2. Sign with it here**. *Expected:* `SecurityError` (or
   "NotAllowedError" with no passkey offered). **If it signs, mark the row REFUSE:** that
   provider ignores the RP check, and approvals must not accept its passkeys.
7. Use **Copy row** after steps 3 to 6 and paste each into the table below, filling in `asked?`.
   At the end, delete every "S46 test" passkey in each provider.

## Rows to run

Run the starred rows first; they decide the default path. The rest decide who gets the fallback
(the hop to control's page) and who gets a warning.

| # | Device | Browser | Provider | Why |
|---|---|---|---|---|
| ★1 | iPhone | Safari | iCloud Keychain | Most approvals come from a push on a phone |
| ★2 | Mac | Safari | iCloud Keychain | |
| ★3 | Mac | Chrome | Google Password Manager | |
| ★4 | Mac | Chrome | iCloud Keychain (Chrome's "use iCloud Keychain" choice) | No source either way; AuthenticationServices checks the RP itself |
| ★5 | Mac | Chrome | 1Password extension | 1Password staff: ROR not supported. Does it fail cleanly, or sign anyway? |
| ★6 | iPhone | Safari | 1Password (iOS AutoFill provider) | Safari checks ROR; does 1Password then refuse? |
| 7 | Mac | Chrome | Bitwarden extension, 2026.10.0 or later | ROR shipped 2026-10-15 |
| 8 | iPhone | Safari | Bitwarden (iOS AutoFill provider) | |
| 9 | Mac | Firefox (152+) | iCloud Keychain (macOS passkeys) | Firefox shows its own related-origins prompt: note its wording |
| 10 | Mac | Firefox | 1Password extension | |
| 11 | iPhone | Chrome | iCloud Keychain | All iOS browsers are WebKit; non-Safari AutoFill had a UV bug |
| 12 | Win11 VM on geek | Edge or Chrome | Windows Hello | |
| 13 | Android, if one is to hand | Chrome | Google Password Manager | |
| 14 | Mac | Chrome | GPM: step 5 within 30 s of step 4 | Does GPM ever skip its PIN or Touch ID because you just unlocked? |

## App windows (needs Jake's go-ahead first)

The app windows can't load the tunnel pages with passkeys: the macOS app gets WebAuthn only for
`webcredentials:` domains it's signed for (control.arugula.io today). So these rows run against the
real control and platform pages, **after** control serves `/.well-known/webauthn` listing
`https://app.arugula.io` (the README's step 1, a prod change: ask first). Until a page on
app.arugula.io asks for a passkey, that file changes nothing.

| # | Window | What to try | Expected |
|---|---|---|---|
| A1 | macOS app (Developer ID build, macOS 15+) | DK5's approve-in-place on app.arugula.io | Unknown: does WKWebView apply related origins on top of the app's association with control.arugula.io? |
| A2 | Windows app (WebView2), Win11 VM | the same | Probably works (Windows Hello through webauthn.dll; WebView2 is Chromium) |
| A3 | Linux app (WebKitGTK) | none | No WebAuthn at all (`results/probe-webkitgtk.txt`): it hands off to the browser or the phone |

## Results

| when | browser (UA) | provider | page | step | result | flags | origin | log check | asked? |
|---|---|---|---|---|---|---|---|---|---|
| | | | | | | | | | |
