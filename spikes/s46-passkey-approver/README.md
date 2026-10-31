# S46: a passkey as the platform's approver, on both sites, across providers

Native issue Arugula #382 (DK1), on the device-keys board. Jake decided on 2026-10-30 that every
platform-plane "a person meant this" is **one user-verified passkey assertion over the action's
bytes**, with no device signature: deploy approve and deny, standing run grants, and team owner
changes. This spike answers what that needs. It builds no product: the harness and the test sheet
here are throwaway.

Status: research and local real-browser runs are done (2026-10-30). The real-provider rows of
[TEST-SHEET.md](TEST-SHEET.md) wait on Jake's devices.

## Answers in one screen

| # | Question | Answer |
|---|---|---|
| 1 | One RP across control.arugula.io and app.arugula.io | **Go: (a), related origins.** Control serves `/.well-known/webauthn` listing `https://app.arugula.io`. Today's passkeys keep working. Where a browser or provider can't do it (the 1Password extension, Firefox before 152), the page falls back to today's hop to control's page. Don't move the RP to `arugula.io`: `<name>.apps.arugula.io` serves apps that agents write. |
| 2 | Is UV honest? | **Go, with a provider policy.** Honest: iCloud Keychain, Google Password Manager on Android, Windows Hello, security keys with a PIN, and the 1Password apps (since "August 27th", per their 2026-10-19 blog). Not honest: the 1Password, Bitwarden, Proton Pass, KeePassXC and Okta extensions, and Proton Pass and Strongbox native. Refuse the never-honest providers by AAGUID, and warn about 1Password and Bitwarden. A choice for Jake below. |
| 3 | App windows | **macOS: probably, test after step 1** (the window is associated with control.arugula.io, which stays the RP). **Windows: probably yes** (WebView2 uses Windows Hello). **Linux: no.** WebKitGTK 2.52.6 has no `navigator.credentials` at all, measured here. The Linux window hands approvals to the browser or the phone. |
| 4 | Publishing passkey keys | **Go.** The format below is prototyped in [harness/passkey-log.mjs](harness/passkey-log.mjs), and 13 cases pass. Three gaps in what passkey.rs keeps: it doesn't store the raw AAGUID, the RP, or the step-up proof. |
| 5 | Recovery | **No-go as it stands.** Once passkeys approve, two things make a GitHub account enough to approve deploys: the first passkey from a fresh GitHub sign-in (#1386), and removing a passkey with just a session. Fix: removing takes a passkey, and a "first" passkey on an account that had one goes through aru-119's 48-hour reset. |

## q1: the RP ID across control.arugula.io and app.arugula.io

### What was run here

[harness/local-chromium.mjs](harness/local-chromium.mjs) runs Google Chrome 153 and its virtual
authenticator against three https origins that exist only on geek: `rp.s46.test`, `app.s46.test`
and `stranger.s46.test`. They share the parent `s46.test`, as control and app share
`arugula.io`. The harness doesn't touch DNS, a trust store or a privileged port. A CONNECT proxy
in [harness/serve.mjs](harness/serve.mjs) carries the browser's own fetch of the well-known file
too. Output: [results/local-chrome.txt](results/local-chrome.txt).

| On | Asks for | Result |
|---|---|---|
| rp.s46.test | RP `rp.s46.test` | ok, UP UV, origin rp |
| app.s46.test | RP `rp.s46.test` (related origin) | **ok**, origin `https://app.s46.test`, verified against the log |
| stranger.s46.test (not listed) | RP `rp.s46.test` | **SecurityError**: "fetching the .well-known/webauthn resource … was successful, but no listed origin matched the caller" |
| app.s46.test, with the RP's well-known a 404 | RP `rp.s46.test` | **SecurityError**: "an attempt to fetch the .well-known/webauthn resource … failed" |
| app / rp / stranger | RP `s46.test` (option b) | ok on all three, the stranger included |

What the server must do:
- Chrome fetches `https://<rp>/.well-known/webauthn` itself, for each ceremony that needs it. The
  request is `mode=no-cors`, `site=none`, with no Origin and no cookie. So the file must be public
  JSON at that exact path, never behind a redirect or a session. `getClientCapabilities()` says
  `relatedOrigins: true`.
- The assertion's `clientDataJSON.origin` is **`https://app.arugula.io`**, and its `rpIdHash` is
  SHA-256(`control.arugula.io`). Today the hub refuses that pair twice: `client_data` in
  `passkey.rs` wants `origin == site.origin`, and `verify_for` picks the site by origin. The
  verifier needs an explicit map from RP to allowed origins (q4).

[harness/probe-engines.mjs](harness/probe-engines.mjs)
([results/probe-engines.txt](results/probe-engines.txt)) probes two more engines:
- **Playwright's Firefox 155** reports `relatedOrigins: true`. It refuses every ceremony on a page
  whose certificate error was overridden, even a same-origin one, so it can't be tested with this
  rig.
- **Playwright's Linux WebKit** has no `navigator.credentials`.

### Evidence by browser and provider (late October 2026)

V: a vendor or tracker source says so. T: tested here. I: inferred from how the request is routed.
"?" goes on the sheet.

| Browser | iCloud Keychain | Google PM | 1Password | Bitwarden |
|---|---|---|---|---|
| Safari macOS 15+ (Safari 18+) | **yes** V | n/a | ? (Safari checks ROR, then hands off to the AutoFill provider) I | ? same |
| Safari iOS 18+ (every iOS browser is WebKit) | **yes** V | n/a | ? sheet ★6 | ? sheet 8 |
| Chrome desktop 128+ | ? sheet ★4 (Chrome → AuthenticationServices, which checks the RP itself) | **yes** V, T (Chrome 153, virtual authenticator) | **no**: the extension takes the call and doesn't support ROR (staff 2024-12, 2025-06, nothing since) V | **yes from extension 2026.10.0** (ROR in clients#18242, flag removed in #23229, shipped 2026-10-15) V |
| Chrome Android 128+ | n/a | **yes** V | ? | **yes** (android#6777, 2026-04) V |
| Firefox 152+ desktop and Android | ? sheet 9 | n/a | **no** (extension) V | yes from 2026.10.0 V |
| Windows, Chrome or Edge 128+ | n/a | yes V | no (extension) | yes (extension) |
| Windows Hello | probably I, sheet 12 | | | |

Notes:
- **Firefox** shipped ROR in 152 (Bugzilla 2010193). It shows its own prompt, and 153 forbids it
  for conditional (autofill) requests (bug 2043449). Approvals use a modal `get`, so that
  doesn't matter. The 140 ESR line has none.
- A **security key** sees only the RP ID and a hash, so it can't object (I).
- The 5-label limit doesn't matter: `control` and `app` share the label `arugula`.

Sources: passkeys.dev/device-support and /docs/advanced/related-origins;
webkit.org/blog/15865 (Safari 18); web.dev/articles/webauthn-related-origin-requests;
bugzilla.mozilla.org 2010193, 2043449, 2085450; github.com/bitwarden/clients PRs 18242 and 23229
(merged 2026-10-01, checked) and issue 22866; github.com/bitwarden/android PR 6777;
1password.community "WebAuthn support of Related Origin Requests"; support.apple.com/122379
(CVE-2025-24180: AuthenticationServices checks registrable suffixes itself).

### Why (a), and not (b) `arugula.io`

- **(b) widens who can ask for an approval to all of `*.arugula.io`.** Agent-written apps go live
  at `<name>.apps.arugula.io` (planter, `crates/e2e/src/deploy_planter.rs`), and `apps.arugula.io`
  isn't a public suffix. Under RP `arugula.io`, any such app can raise the person's Arugula
  passkey sheet and get a real assertion over bytes it chose; the T row "stranger, RP s46.test:
  ok" is exactly this. Only the verifier's origin list would then stand between that and a forged
  approval, in Rust, in TS, and in anyone else's copy of `verify`. Under (a), the browser refuses
  such a page before any sheet appears (the T row "stranger: SecurityError").
- (b) re-registers every passkey (aru-078's reason). The macOS app would also need
  `webcredentials:arugula.io`, with `apple-app-site-association` served from the apex, which is the
  marketing site.
- (a)'s gaps have a fallback that already works: today's hop to control's `#deploy=` page, where
  the RP is the page's own host. The page tries in place and falls back when (i)
  `getClientCapabilities().relatedOrigins` is false, or (ii) `get` fails with `SecurityError`, or
  with `NotAllowedError` faster than a person could have answered. 1Password-extension and old
  Firefox users get the hop; everyone else approves in place.

What (a) costs:
- **app.arugula.io becomes as security-critical as control's page.** The OS sheet doesn't show the
  bytes being signed; the page does. An XSS there can ask for an approval over other bytes. Give
  the platform page control's CSP, and keep deploy.tsx's habit of hashing what it shows from the
  bytes it's handed.
- **Sign-in stays strict.** `login_finish` and the step-up keep accepting only control's own origin.
  Related origins are for platform actions (`verify_for`), so the platform page can't mint
  sessions.

## q2: does UV mean someone checked?

The threat behind "agents never approve": an agent driving the person's browser (CDP, a browser
extension, Claude in Chrome) can click anything in the page, including a password manager's
in-page panel. It can't pass Face ID, Touch ID, Windows Hello or a security key's PIN. So the UV
flag is the whole proof, and it is only as good as the provider that sets it.

| Provider | UV honest? | Do |
|---|---|---|
| iCloud Keychain (Safari, Chrome on macOS, iOS) | yes: biometric or passcode each time with `required` | accept |
| Google Password Manager, Android | yes: screen lock | accept |
| Google Password Manager, Chrome desktop | probably (GPM PIN or OS lock); nothing found either way | accept; sheet 14 |
| Windows Hello | yes | accept |
| Security keys | yes, with a PIN set; no PIN means `required` fails | accept |
| 1Password apps (iOS, Android, desktop as the OS provider) | yes since "August 27th" (1Password's blog of 2026-10-19; no year given) | accept |
| 1Password **extension** | **no**: "handles request without performing UV, sets UV true" (passkeys.dev known issues, 2026-04-09). 1Password says extension-only UV is next | warn |
| Bitwarden **extension** | **no** (same table). It re-prompted in 2024.6, then dropped it, and staff in 2026-04 said no changes are planned | warn |
| Bitwarden mobile | unknown | warn; sheet 8 |
| Proton Pass (extension *and* native), KeePassXC, Strongbox, Okta Personal extension | **no** | refuse |
| Dashlane | unclear: it says it can't do UV-required passkeys | refuse until shown otherwise |

**What we can tell apart.** Only the AAGUID, given once at registration and unsigned under
`attestation: "none"`. It's a label, not a proof: it can catch an honest vendor's weak provider,
never a forger, and adding a passkey already takes a step-up or GitHub. Worse, **one 1Password
passkey syncs to both** the honest app and the dishonest extension, under the same AAGUID, and an
assertion carries no AAGUID. So no per-assertion rule can single out the extension.

**Choice for Jake:**
1. **Refuse the never-honest AAGUIDs** for approvals (Proton Pass, KeePassXC, Strongbox, zero or
   unknown AAGUIDs). **Warn** on 1Password and Bitwarden passkeys, at registration and beside
   Approve: "this manager's browser extension can sign without Face ID or a PIN; approve from your
   phone or add an iCloud, Google or Windows passkey". *(Recommended.)*
2. **Allow-list only**: approvals take passkeys whose AAGUID is iCloud Keychain, GPM, Windows
   Hello or a known security key. Stronger, but it locks out 1Password users even on their phones.
3. Accept every provider, and say in aru-101 that UV is the provider's word.

## q3: the app windows

- **macOS (WKWebView, #1460).** Passkeys work only for RP IDs the app is associated with
  (`webcredentials:control.arugula.io`: scripts/macos-sign, `apple_site.rs`). Under (a) the RP stays
  control.arugula.io, so the association holds without changes. The open question is whether
  WKWebView applies related origins for a page on app.arugula.io. Safari's WebKit has it since
  macOS 15 (WebKit bug 281092 fixed how it's advertised, in 15.4), but no source covers embedded
  views. It can only be tried against the real pages once control serves the file: sheet row A1.
  If it fails, the window navigates to control's `#deploy=` page, which works today. Under (b) the
  app would need a new entitlement and a re-signed build.
- **Windows (WebView2).** WebAuthn goes through webauthn.dll to Windows Hello, with no setting
  needed (tauri#7926 reports, WebView2Feedback#5460). Windows 11's plugin providers (1Password,
  Bitwarden) show in that sheet, and Windows Hello does their UV (I). Related origins should come
  with Chromium; untested, sheet A2.
- **Linux (WebKitGTK).** Measured, not inferred:
  [harness/probe-webkitgtk.py](harness/probe-webkitgtk.py) under xvfb with geek's
  webkit2gtk-4.1 2.52.6 (Tauri's): `navigator.credentials` and `PublicKeyCredential` are both
  `undefined`, in a secure context, as shipped and with every WebAuthn-named feature flag on
  ([results/probe-webkitgtk.txt](results/probe-webkitgtk.txt)). WebKit bug 205350 is still open;
  it waits on the credentialsd portal. So the Linux window **opens the approval in the person's
  browser**, as sign-in does already (`cloud.rs`), or points at the push on their phone. Today's
  `init_script` reports Linux and Windows alike as `"linux"` and hides the passkey button on both;
  DK5 should tell them apart.
- `tauri-plugin-webauthn` calls native FIDO2, which means security keys only on Linux. It isn't
  worth it for synced passkeys.

## q4: publishing passkey keys (the shape DK3 builds)

A per-account, append-only, hash-chained log. Each event is a text body, like the deploy bodies:

```
arugula passkey log v1
account acc_…
seq 3
prev <hex SHA-256 of event 2, or 64 zeros>
event added | removed
credential <base64url credential id>
rp control.arugula.io
alg -7
key sec1 <base64url>        (ES256: SEC1 uncompressed; `raw` for Ed25519; `spki` for RS256)
aaguid <32 hex, zeros if unknown>
at <ms>
by passkey <credential id> | github | signup | reset | backfill
```

- An event's hash is SHA-256 of its body plus its proof line, so the next `prev` covers both.
- A `by passkey X` event carries X's assertion, with UV, whose challenge is SHA-256 of the body.
  The other `by` kinds are the operator vouching, and the log says which.
- `verify(log, account, assertion, challenge, at, origins)` checks:
  - the whole chain, and every proof against the keys live at that point;
  - that the credential was live at `at`;
  - that it's a `webauthn.get` over exactly `challenge`, from an origin `origins[rp]` allows, and
    not from inside another site's frame;
  - the `rpIdHash`, UP and UV;
  - the signature.
- `origins` is configuration the verifier pins, not log data:
  `{"control.arugula.io": ["https://control.arugula.io", "https://app.arugula.io"]}`.
- `at` must be the platform's recorded time, from the deploy event log. Never the approval body's
  own time: a removed (stolen) passkey could backdate that.

[harness/passkey-log.mjs](harness/passkey-log.mjs) is that function in TS-shaped JS (WebCrypto,
browser and Node). `node harness/passkey-log.test.mjs` passes 13 cases: ES256, Ed25519 and RS256
keys, a key used after its removal, a key used before it was added, a stranger origin, UV unset,
another RP, a framed ceremony, other bytes, a swapped key in event 0, and an add signed by a
removed passkey. The harness page verifies real providers' assertions with it, against the keys
real providers registered.

Against what `passkey.rs` and `db.rs` keep today:

| Log field | Today | Gap |
|---|---|---|
| credential, alg, key | `passkeys.id`, `alg`, `public` (SEC1, raw, or "SPKI" from `aws_lc_rs::PublicKeyComponents::as_der`) | Fits, and WebCrypto imports all three. DK3: pin the RSA encoding with an example file. The code comment says SPKI; check it isn't PKCS#1. Keep the format word in the log line. |
| rp | not stored; `rp_id(site)` at the time (#507) | Store it. Backfill from `created` against the date control moved off control.illogical.widgets.wtf, or verify both hashes once and record which matched. |
| aaguid | only the provider *name*, and only for AAGUIDs the list knows (`note_passkey`) | Store the raw AAGUID; q2's policy needs it. Backfill known names to their AAGUID, else zeros. |
| sign_count | `passkeys.sign_count`, updated on use | Mutable state, not log data. Offline verifiers skip it; synced passkeys report 0 anyway. The hub's live check keeps it. |
| `by passkey` proof | the step-up assertion (`register_step_up`, challenge `arugula add passkey v1` over account + credential) is checked, then dropped | Make the step-up's challenge SHA-256 of the log event's body, and keep the assertion as the event's proof. |
| removed | `DELETE FROM passkeys`; `remove_passkey` needs only a page session | An event with a passkey's proof (q5). |
| signup | a passkey-made account (#102) | `by signup`. |
| account deleted | `DELETE FROM passkeys WHERE account` | Keep the log (or a tombstone with the keys) while any stored approval cites it; otherwise old approvals stop verifying. |

## q5: recovery once passkeys approve

Today a lost-every-passkey person signs in with GitHub again (10 minutes, `REAUTH_MS`) and adds a
first passkey at once (#1386). Device keys are what keep that from being an approval: aru-119's
reset is about devices, with its 48-hour wait. Take the device signature out of approvals and two
paths open:

1. **Fresh GitHub → first passkey → approve**, with no wait. Only a push says it happened.
2. **A session alone removes passkeys.** `POST /api/me/passkeys/{id}/remove` needs a page session
   (`account.rs::remove_passkey`). `drop_passkey` refuses only the account's *last way to sign in*,
   and a GitHub identity counts as one. So a stolen session can remove every passkey, and then
   path 1 applies to anyone holding the GitHub account.

So **"fresh GitHub sign-in, then account reset" is not enough as it stands**: it makes GitHub the
root of approval. What would make it enough:
- **Removing a passkey takes a passkey**: an assertion with UV, from any live passkey of the
  account, over the `removed` event's body. Only an aru-119 reset may drop passkeys without one.
- **A first passkey is immediate only for an account whose log is empty.** For an account that has
  ever had one, a fresh GitHub sign-in *starts* aru-119's reset. The 48 hours, the notices and the
  cancel stay as they are, any live passkey can cancel it, and the new passkey enters the log
  `by reset` when the wait ends. The log then shows every verifier "this owner's passkey came from
  a GitHub reset on that date".
- **Teams:** an owner's reset also tells the team's other owners.
- **Choice for Jake:** may a recovery code add a passkey without the wait? It's the person's own
  offline secret, but recovery codes are machine-plane things under DK2. Recommended: no, codes
  cancel a reset and nothing more. That keeps the planes apart.

## What changes for DK3–DK7

- **DK2 (#383):** record (a), related origins, for aru-078/aru-061, with the `*.apps.arugula.io`
  reason. Record q5's two rules beside aru-119, and q2's provider policy beside aru-101.
- **DK3 (#384):**
  - the log format above, with `rp`, raw `aaguid`, `by`, and proof lines;
  - `origins` as pinned configuration;
  - `at` from the platform's record;
  - store the step-up as a proof;
  - removal as a proofed event;
  - keep the log past account deletion;
  - a "rp unknown" backfill rule.
  
  Use the 13 cases here as its example files.
- **DK4 (#385):**
  - approval v2's challenge is SHA-256 of the body, as today;
  - the hub's `verify_for` accepts `https://app.arugula.io` for RP control.arugula.io;
  - `login` and the step-up keep only control's own origin;
  - apply q2's provider policy at verify time from the log's `aaguid`.
- **DK5 (#386):**
  - **Ship control's `/.well-known/webauthn` first.** It's a public JSON file, no redirect, no
    session, and inert until a page on app.arugula.io asks. Then run the sheet's A rows.
  - Approve in place with the fallback rule from q1.
  - The platform page gets control's CSP.
  - The Linux window hands approvals to the browser.
  - `init_script` should tell Windows from Linux.
- **DK6 (#387) and DK7 (#388):** nothing new beyond DK3's verify. A box pinning the passkey log at
  enrollment must pin the `origins` map too.
- **New, for DK3 or its own issue:** passkey removal behind a passkey, and the aru-119 path for
  re-adding one (q5). Without them, DK4 shouldn't ship.

## Still open (Jake's run, [TEST-SHEET.md](TEST-SHEET.md))

- Rows ★1–★6 decide the default path. 1Password and Bitwarden as iOS providers under Safari are
  the biggest unknowns. So are Chrome on macOS with iCloud Keychain, and how the 1Password
  extension fails: cleanly, or by signing anyway.
- Row 14: whether GPM on Chrome desktop ever skips UV after a recent unlock.
- A1 and A2, the app windows: these need control's well-known on prod first (a prod change; ask).
- Option (b) wasn't run on real providers: trycloudflare.com is a public suffix, so the tunnel hosts
  share no parent. It isn't needed for the recommendation. If Jake wants it, it needs three names
  under a domain he controls (a DNS change), e.g. `rp.s46.<domain>`, `app.s46.<domain>`,
  `other.s46.<domain>` pointing at one tunnel. Ask first.

## Files

- [harness/serve.mjs](harness/serve.mjs): one server for every test origin.
  - the page and `config.json`;
  - `/.well-known/webauthn` on the RP's host;
  - `apple-app-site-association` with `--team`;
  - the passkey log;
  - a CONNECT proxy for local runs.
- [harness/index.html](harness/index.html), [harness/harness.js](harness/harness.js): register on one
  origin, sign on another. Shows UP, UV, BE, BS, the clientDataJSON origin, the AAGUID's provider
  and the log check, and copies a row for the sheet.
- [harness/passkey-log.mjs](harness/passkey-log.mjs), [harness/passkey-log.test.mjs](harness/passkey-log.test.mjs):
  q4's log and verify.
- [harness/local-chromium.mjs](harness/local-chromium.mjs), [harness/probe-engines.mjs](harness/probe-engines.mjs),
  [harness/probe-webkitgtk.py](harness/probe-webkitgtk.py): the local runs, with output in
  [results/](results/). Run with `S46_WEB=<a checkout's web/>` (for its Playwright) and
  `S46_TMP=<a short dir>`: Chrome dies on a long TMPDIR socket path.
- [harness/tunnel.sh](harness/tunnel.sh): three Cloudflare quick tunnels (no account, DNS or cost)
  for the sheet.
- [TEST-SHEET.md](TEST-SHEET.md): the click-through for real devices.
