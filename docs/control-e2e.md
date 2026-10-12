# Arugula control: keys, channels and the relay

Written 2026-10-01, from the control spike ([spikes/s15-control](../spikes/s15-control/README.md)).
This is the design the control track builds: accounts and devices, the
end-to-end relay, teams, hosted sandboxes and push. It turns the control track's promise
(plan-archive.md, "Control track") into mechanisms: **control can refuse service,
but it can't read.** That promise has limits, set out in
[What holds against control](#what-holds-against-control): the client code
control serves, and the first account a machine joins, are trusted.

## Summary

- **One channel type:** every client–daemon stream is a Noise channel,
  `Noise_IK_25519_AESGCM_SHA256`, on the relay *and* on direct paths.
  Browsers run it on WebCrypto alone; daemons and the CLI use `snow`.
- **Every viewer gets its own channel, including on shared sessions.**
  Sessions have no shared key. Revoking someone closes their channel; there
  is no key to rotate. Fanning out one ciphertext at the relay waits until
  a measured need (see [Fan-out](#fan-out-later-if-ever)).
- **Device keys:**
  - a browser keeps a non-extractable X25519 key (for Noise) and an Ed25519
    key (for signing approvals) in IndexedDB;
  - the CLI and daemons keep key files;
  - a passkey's PRF output re-derives a browser's keys after storage is
    lost, and a synced passkey is one device everywhere it syncs (see
    [PRF](#prf-re-deriving-a-browsers-keys)).
- **Trust:**
  - control distributes keys, but each device certificate is signed by a
    device the account already trusts;
  - daemons verify the chain themselves, so control can't add a reader;
  - a joining machine pins its account's root only once the device that
    approved it has answered a secret in the machine's approval link,
    which control never sees ([Joining](#joining-a-machine-the-secret-in-its-link));
  - a new browser, phone or app window does the same with the code it
    shows, scanned or typed on a device you already use
    ([Approving a new device](#approving-a-new-browser-or-phone-the-code-it-shows)).
- **Relay:** control keeps each enrolled daemon's dial-out socket (the daemon's
  dial-out mux) and splices clients onto it. It sees connection metadata and byte
  counts only.
- **Read-only links:** a link carries a one-off device key in its fragment
  (`#k=…`). The daemon lists that key as a viewer of one session until the
  link expires.
- **Push:**
  - the daemon encrypts the notification (RFC 8291) to a subscription
    the device signed;
  - control only adds the VAPID signature and posts it;
  - control's own notices (something waits for approval) it encrypts
    itself, and they say only that.

## Keys

| Holder | Keys | Where | Lost when |
|---|---|---|---|
| Browser / PWA | X25519 `noise`, Ed25519 `sign` | IndexedDB, non-extractable `CryptoKey`s (wrapped where those don't survive: below) | site data is cleared; Safari's 7-day eviction for sites not added to the home screen |
| CLI | X25519 + Ed25519 | `~/.config/arugula/device.key`, 0600 | the file is deleted |
| Daemon | X25519 `noise`, Ed25519 `sign` | `<state>/daemon.key`, 0600 | the state directory is deleted (re-enroll) |
| Recovery code | Ed25519 seed | printed once at first sign-in, never stored | the user loses the paper |

The spike checked in Chrome 153 (desktop, headless) that:

- both key types generate as non-extractable (an export attempt throws);
- they survive a reload from IndexedDB;
- they work with `deriveBits` and `sign`.

Safari and Firefox support both curves (Safari 17+, Firefox 130+). The
phone runs below confirm Safari on iOS.

**WebKit loses X25519 keys in IndexedDB.** Playwright's WebKit
(Safari 26.6's engine, on Linux) stores a record holding an X25519
`CryptoKey` and reads it back as `null`, in the same page load or the next;
Ed25519 and AES-GCM keys come back fine. A Safari that enrolled therefore
made new keys on every later page load, and its approvals were signed by a
device nobody trusted. Now:

- **Keys are checked after they're stored:** read back from IndexedDB, they
  must sign what their public key verifies and agree on an X25519 secret,
  before the browser enrolls. The same check runs on every load.
- **Where the check fails, the keys are kept wrapped:** generated
  extractable, then stored as PKCS#8 encrypted with a non-extractable
  AES-GCM key (`wrapKey`), and unwrapped inside WebCrypto as
  non-extractable on each load. If that fails too, the page says this
  browser can't keep a device key.
- **What wrapping gives up:** script running in the page's origin (an XSS,
  a malicious build) can unwrap the keys as extractable and copy them out,
  to use anywhere until the device is removed. With plain non-extractable
  keys it can only use them while it runs in the page. On disk both forms
  are equally readable to someone with the browser profile.
- **A browser enrolled with keys it lost** says so at boot and offers to
  forget itself and enroll again, approved by another device or a recovery
  code.
- `/key-probe.html` (on control and on every daemon) runs the round trip
  in whatever browser opens it and says plainly whether it works, with the
  Safari version.

**A device certificate** is what control stores and hands out:

```
{ v: 1, account, device: <id>, name, kind: browser|cli|daemon|recovery,
  noise: <x25519 pub>, sign: <ed25519 pub>, created, team? }
  + approver: <device id>, sig: Ed25519(approver.sign, canonical JSON)
```

- The **first device** of an account is self-signed. It is trusted on
  first use, like an SSH host key.
- Every later device, daemon or recovery code is signed by an existing
  device. That is the *New device?* prompt. Since #761 a new browser is
  approved with the code it shows rather than a fingerprint to compare
  (the first 8 bytes of SHA-256 of `noise‖sign`, which a page from before
  codes still shows).
- A daemon's own certificate is signed by the device that approved its
  `join` code. When the approver puts it in a team, that device also
  signs the choice (`illogical team join v1`: the daemon, the team, its
  founder). The daemon pins only a team signed that way.
- **A team's member list** (its roster) is versioned, and each version is
  signed by a device of an owner in the version before. The one exception
  is a **presigned invite**:
  - The owner's device signs `illogical team invite v1` (team, role,
    expiry, a one-time Ed25519 public key) when it makes the link. The
    private half of that key goes only in the link's `#fragment`
    (`#pinvite=<team>.<seed>`). Signing in with GitHub from the link keeps
    the seed in the tab's `sessionStorage` rather than in the `next` URL
    control sees, and pages from before presigned invites don't recognize
    the fragment, so they never send the seed as an invite code.
  - The invitee's device writes the next version (`v: 2`). It is the
    previous version plus the invitee at the end, with the invite listed
    as `spent`. The one-time key signs `illogical team redeem v1` over
    that exact member and version, and signs the whole roster too (`by` is
    the one-time key). No device of the invitee's signs it: devices are
    judged as of now, so revoking that device later would stop every later
    version from checking.
  - Daemons, control and browsers accept it only when all of these hold:
    the invite is signed by a device of an owner in the previous version;
    it isn't for an owner; it's unexpired at the version's `at`, and that
    `at` isn't before the previous version's; it isn't already spent; and
    nothing else changed.
  - Control sees the proof only when the invitee submits it. Because the
    proof names that account and root, control can't move it to an
    account of its own.
  - **Accepted risks:**
    - The link is a bearer token. Whoever holds it joins, which is why it
      lasts a day (control refuses longer), works once and never grants
      owner.
    - Expiry is checked against `at`, which the invitee writes. Control
      also refuses an expired invite by its own clock, but a daemon can't
      tell.
  - **Older daemons** refuse `v: 2`, and would then stop taking any later
    version of that team's roster, removals included. So every daemon
    tells control what it understands on each team call
    (`features=presigned-invites`), and control makes a presigned invite
    only when every daemon that checks the team's rosters has said so. That
    means the team's own machines, plus any machine that asked about the
    team in the last week because a session was shared with it. Otherwise
    the owner gets an *Ask me first* link and a note naming the machines to
    update. Control checks again when the invite is redeemed, since a
    machine may have started checking the team in between.
  - Once a team's history has a `v: 2` version, an older daemon can't
    follow it. Control won't put one into such a team (a join or *Move
    to…*; daemons also report their features when they ask to join and on
    every trust refresh). A daemon that was only shared a session with the
    team gets nothing for the team until it's updated: its members lose
    that session there, rather than members who were removed keeping it.
- An ***Ask me first* invite** carries a one-time key too (#762), so
  admitting someone needs no fingerprint:
  - The owner's device signs `arugula team ask invite v1` (team, role,
    when it was made, expiry, a one-time Ed25519 public key). Its own
    domain, so it can never be taken for a presigned invite. The private
    half goes only in the link's fragment (`#askinvite=<team>.<seed>`),
    kept off control at sign-in as a presigned seed is. Control names the
    invite by the public key.
  - Whoever uses the link signs `arugula team ask v1` (team, their account,
    their root, the key) with the one-time key, and control stores that
    proof with the request. It refuses a request without one, but that's
    only a courtesy: the owner's page checks the invite is signed by a
    device of one of the team's owners and the proof names that account
    and root, and shows nothing else. So control can't put an account of
    its own (or move someone's proof to one) in front of an owner.
  - The prompt says who used the link, when the link was made and for
    which role, all from what the owner's device signed. *Add them* signs
    a plain `v: 1` roster with that role and root, so older daemons follow
    it and no feature gate is needed.
  - **Accepted risk:** as with a presigned link, it's a bearer token for
    asking: anyone who has the link can ask, for a week. The owner still
    says yes to each, by name.
  - Links and pending requests from before #762 were dropped when control
    updated (`#invite=<team>.<code>` now says to ask for a new link).

**Verification on the daemon:**

- The daemon keeps its account's chain (the certificates back to the
  first device) and caches it on disk.
- A client's Noise static key must belong to a certificate that chains to
  that root, isn't revoked, and has a role on the session it touches
  (the multiplayer principals; role grants are signed the same way).
- Control serves the chain, but it can't extend it: it holds no `sign`
  private key.

**Revocation:**

- A revocation is a signed `{revoke: <device id>, at}` from any device of
  the account, or from a team owner's device for team grants.
- Control distributes revocations to daemons as it does certificates.
  Approving devices also push them straight to every daemon they can reach.
- **Accepted risk:** a malicious control can *withhold* a revocation from a
  daemon that no trusted device can reach directly. Control can't add
  readers, but it can delay removing one. As with Tailnet Lock, this is
  written down rather than solved.

**Recovery:**

- At first sign-in, the user gets two recovery codes. Each is an Ed25519
  seed, shown as words, whose certificate (`kind: recovery`) the first
  device signs.
- Any of the account's devices can make new ones. It signs their
  certificates and revokes every code still good, in one request; control
  refuses new codes that don't retire the old.
- A recovery code can sign exactly one certificate (a new first device),
  and daemons then treat it as spent.
- Control never sees the seed.

### PRF: re-deriving a browser's keys

A passkey created with the `prf` extension returns 32 bytes per salt, the
same every time for that credential. Since #764 those bytes are the
browser's device keys, where the passkey gives them:

- **Derivation** (`adoptPrfKeys` in `web/src/e2e/keys.ts`): every passkey
  ceremony (making one, signing in) asks for PRF at two fixed salts,
  `"arugula device noise v1"` and `"arugula device sign v1"`. Each output
  goes through HKDF-SHA256 (no salt, `info` `"arugula device x25519 v1"` or
  `"arugula device ed25519 v1"`) to a 32-byte seed, imported as PKCS#8
  (X25519 and Ed25519). The public halves come from exporting a private
  JWK once; the keys are then stored non-extractable, as `adoptKeys` stores
  any (wrapped where WebKit needs it). The salts and `info` strings are
  frozen: changing one makes every passkey a new device.
- **Recorded per passkey:** the page tells control whether the passkey
  gave PRF (`enabled` when it's made, output at sign-in), kept in
  `passkeys.prf` and listed in `/api/me/passkeys`. Control never sees the
  output.
- **Which browsers take the keys:** a passkey sign-in on a browser that
  isn't a working device of that account (new, site data cleared, keys
  lost). One that is keeps its own keys. A browser waiting for approval
  offers *Use your passkey instead*, which signs in again and takes them.
- **The first time**, the passkey's keys are a new device, named for what
  keeps the passkey when control knows it from the AAGUID ("iCloud
  Keychain passkey", else "Passkey"). It's approved once like any other
  browser (with its code), unless the authenticator gave PRF as the
  passkey was made, at sign-up: then it's the account's first device.
- **The pin.** Once approved, that device signs
  `arugula device pin v1`, the account and the root it pinned
  (`devicePinBody`, `arugula_e2e::cert::device_pin_body`), and control keeps
  it (`POST /api/devices/{id}/pin`, checked against the device's key).
- **Every later time** (storage cleared, another browser the passkey syncs
  to) the browser derives the same keys, finds the device in the account,
  and checks the pin with its own key before it takes the root. Control
  can't make that signature, so it can't hand the browser an account of
  its own, which is what the code does for a browser approved normally
  (below). No approval, no new certificate.
- **A device that was removed, or whose pin is missing or doesn't check
  out,** gives way: the browser drops the passkey's keys, makes its own and
  asks to be approved, as before. Revoked keys never come back, so once the
  passkey's device is removed, that passkey only signs in.
- Where PRF isn't available, losing storage means a new device key and an
  approval from another device. That is safe, just less convenient. **So
  PRF is an improvement, not a requirement:** control's accounts ship either
  way, and nothing changes for a passkey without it.

**The trade-off: a synced passkey is one device.** iCloud Keychain and
Google Password Manager sync the credential, so every device sharing it
derives the same keys and is that device: whoever holds the passkey (or the
account it syncs through) reaches your machines without an approval.
That's the trust the passkey already carries as a login, now carried to the
machines too. Removing the device removes it on all of them at once; a
passkey you don't want to stand for a device is one to remove from the
account.

**Go/no-go per platform:**

| Platform | `extension:prf` | Same device after clearing site data | New computer, same synced passkey | Verdict |
|---|---|---|---|---|
| Chrome, Linux | not yet checked | not yet checked | not yet checked | not yet checked |
| Chrome, Mac | not yet checked | not yet checked | not yet checked | not yet checked |
| Safari, Mac (iCloud Keychain) | not yet checked | not yet checked | not yet checked | not yet checked |
| Safari, iOS | not yet checked | not yet checked | not yet checked | not yet checked |
| Chrome, Android | not yet checked | not yet checked | not yet checked | not yet checked |

The spike saw Chrome 153 (Linux, headless) report `true`.
`passkey.spec.ts` runs the round trip on Chromium's virtual authenticator
(`hasPrf`), which says nothing about real ones.

### Joining a machine: the secret in its link

The chain a daemon checks is only as good as the root it pins, and at
`join` that root comes from control. A control that wanted to read a
machine could approve it itself, into an account of its own (its own
"first device", signing the daemon's certificate), and every check above
would pass. So the daemon doesn't take the root on control's word. Since
#760 nothing is compared for it; the person carries a secret instead:

- **The code has two halves.** Control gives the machine its code (the
  lookup: ten characters derived from the machine's keys, which control
  sees, as before). The machine makes a secret of its own, 16 characters
  of the same alphabet (80 bits), and prints the approval link as
  `#join=<code>.<secret>`. The fragment never reaches control's server.
  Typed, it's one code, `XXXXX-XXXXX.XXXX-XXXX-XXXX-XXXX`; the page splits
  it and sends control only the code. Signing in with GitHub on the way
  keeps the whole link in the tab's sessionStorage and gives control only
  `#join=<code>` to come back to, as a presigned invite's seed is kept.
- **The code commits to the machine's key.** The approving page recomputes
  it from the certificate control shows, so control can't swap in a key of
  its own; the link brings the code from the machine, so nobody compares
  it by eye.
- **The approver binds its answer to the secret.** With the certificate it
  signs, the approving device sends
  `HMAC-SHA256(secret, "arugula join secret v1", account, root, the machine's device id and keys)`
  (`arugula_e2e::cert::join_mac`), where the root is the one that device
  pinned when it enrolled, not control's word now. Control passes it on to
  the machine as it is (`mac` in the approval and in the machine's poll).
- **The machine checks it** against the account control says it's in and
  its own keys, and pins that root without asking anyone: `arugulad join`
  and `arugula login` finish on their own, and Getting started goes from
  the code to joined. Control can't make that MAC for a root of its own,
  or for a key of its own, without the secret.
- **One guess per code.** A wrong MAC ends the join: the daemon asks
  control to drop it (best effort) and removes its key, so the next join
  has a new key, a new code and a new secret. The CLI never saved its key.
  A mistyped secret ends up here too.
- **Offline guessing.** Control sees the MAC, so it could try secrets
  against it offline and then answer for an account of its own. That's
  why the secret is 80 bits rather than the code's 50: out of reach while
  the join lasts (ten minutes). A shorter typed code would need a PAKE
  (CPace or SPAKE2) over it instead of a plain MAC.
- **Without the secret.** A page that has only the code (typed alone, or
  brought by a browser that came to approve the machine, #326) asks for
  the whole code before it offers Approve. An approval that reaches a
  machine with a secret but no MAC is refused unless `--account
  FINGERPRINT` names the account (scripts that pass it keep working).
- **What this rests on:** the machine's side is the daemon's own output
  (terminal, or its local page), which control doesn't serve. The other
  side is the approving device, and it's only as trustworthy as that
  device's own pin (below) and the client code it's running
  ([What holds against control](#what-holds-against-control)).
- **Older daemons and CLIs** don't say `join-secret` when they ask to join
  (`features`, as `presigned-invites` is said), print no secret, and get no
  MAC. The page shows them the old way: the code to check against the
  machine's, and the account's fingerprint (its root device's id, the first
  8 bytes of SHA-256 over its keys, as `1a2b-3c4d-5e6f-7a8b`), which the
  machine then shows and asks about (`y`, or the fingerprint typed;
  *They match* / *They don't* in Getting started; `--account FINGERPRINT`
  without asking). A newer daemon or CLI does the same with a control that
  doesn't say `join_secret` in `/control.json`, so a control that lies
  about that gets the compare, not a pass. Daemons from 0.16.0 and before
  pin without a check, as they always did; control's API keeps working for
  all of them.
- **Trust on first use, still:**
  - an account's first device is self-signed. A browser enrolling as a
    later device no longer takes the account's root on control's word: the
    device that approves it answers the code it shows
    ([below](#approving-a-new-browser-or-phone-the-code-it-shows)), and the
    desktop app signed in from a device takes that device's root
    ([below](#signing-in-the-desktop-app-one-allow)). What's left: a
    browser let in by a recovery code takes the root control names (the
    recovery certificate must chain to it, but control picks the chain),
    and one approved by a page from before codes shows the account's
    fingerprint to compare;
  - a machine joined before 0.17.0 pinned whatever root control sent then.
    Its *Devices and machines…* fingerprint and `control.json`'s
    `trust.root` should be the same; if not, `arugulad leave` and join
    again.
- **Hosted sandboxes** skip the check: control creates the VM through
  its provider and writes the sandbox's `control.json` itself
  (`crates/control/src/sandboxes.rs`). The operator runs that machine, so it
  can read it whatever the keys say. A sandbox is end to end encrypted
  against the network, not against control. Its join has no link, so no
  secret.

### Approving a new browser or phone: the code it shows

The same mechanism as a machine's join (#761). A browser, phone or the
app's window that waits to be approved shows a code instead of its
fingerprint:

- **The code** is `<lookup>.<secret>`, as text and as a QR code of the
  link `#device=<lookup>.<secret>` on control's page. The lookup is its
  keys' code (`joinCode`, ten characters); the secret is 16 more of the
  same alphabet (80 bits), made by the waiting page and kept in its
  `localStorage` with its key id, so a reload shows the same code. The
  secret never reaches control: the waiting page sends only its
  certificate and `features: "device-secret"` with its request, and
  signing in with GitHub from a `#device=` link keeps the link in
  sessionStorage rather than in `next`.
- **The approving device** reads it: a phone's camera opens the link,
  *Scan it* reads it in the page (`BarcodeDetector`, or jsQR where there's
  none), or the rest of it is typed into *New device?* after the lookup,
  which the page shows fixed (the request's keys give it). The lookup
  must be that request's keys' code, so control can't swap the key, and
  sends with the certificate it signs
  `HMAC-SHA256(secret, "arugula device secret v1", account, root, the device's id and keys)`
  (`deviceMac` in `web/src/e2e/cert.ts`), where the root is the one the
  approving device pinned. Control keeps it with the device (`mac`) and
  hands it over with the approval (`GET /api/devices/{id}`).
- **The new device checks it** against the account and root control names
  and its own keys before it pins that root. Control can't make that MAC
  for a root of its own without the secret. A wrong one is refused, and
  the secret is dropped (one guess per code): the browser says to enroll
  again. Offline guessing against the MAC is out of reach for the same
  reason as the join's.
- **A browser that came to approve a machine** (#326) adds its code to the
  link it hands on (`#join=<code>.<secret>&device=<lookup>.<secret>`), so
  the device that approves the machine approves it along with the
  machine's MAC; without the link, its card asks for its code.
- **Older pages:** a page from before codes (a tab left open across a
  deploy) still lists the request with its fingerprint and approves it
  without a MAC; the waiting page has the fingerprint a click away. The
  new browser then doesn't pin the root silently: it shows the account's
  fingerprint and asks the person to check it on the approving device
  (*It matches*, or forget this browser). A waiting page from before
  codes says no `device-secret`, and the approving page shows its
  fingerprint to compare, as before. Control says `device_secret` in
  `/control.json`; without it the waiting page shows its fingerprint.
- **Not covered:** a recovery code approves a browser by itself, with no
  other device to answer a code.

### Signing in the desktop app: one Allow

The desktop app's window signs in through the person's browser
(`crates/control/src/app_login.rs`), and since #758 nothing is compared:

- **No code.** The browser hands the one-time grant to
  `http://127.0.0.1:<port>`, the app's loopback port on the computer the
  browser runs on, and redeeming it takes the verifier only the app holds
  (PKCE). A link someone else started can't sign their app in as you: their
  app isn't listening on your computer. The page still warns when the app
  asked from another network than the browser's.
- **Allow makes the app a device.** The app's own sign-in page (served by
  the app, not control) makes the window's device keys and sends their
  public half with the ask. Control shows them to the browser, and a
  browser that's one of the account's devices signs a certificate for them
  when the person presses Allow. The certificate and the root that browser
  pinned go with the grant to the loopback port, never through control.
  The app hands the keys (PKCS#8, in the redeem link's fragment) and the
  certificate to control's page in its window, which takes the keys as its
  own, checks the certificate is for them, enrolls with it (control accepts
  a certificate already signed by a trusted device), and pins the root the
  browser had rather than control's word for it.
- **If control lied about the keys,** the browser signed keys the app
  doesn't hold, and only the app saw that certificate: the window finds it
  isn't for its keys, and asks to be approved like any new browser. Control
  gets nothing it can use.
- **Falls back to an approval** when there's no certificate: the app
  couldn't make keys (a webview without WebCrypto's X25519 or Ed25519), or
  the browser couldn't sign. The window then makes its own keys and waits
  for a device to approve it, with the fingerprint, as before.
- **What this rests on:** control's page in the browser, which signs, and
  in the app's window, which takes the keys: the same page the
  [last row below](#what-holds-against-control) already trusts. The
  private keys leave WebCrypto once, from the app's sign-in page to the
  window's, through the app (in memory) and a URL fragment the page clears
  at once; they're stored as any browser's are from then on.
- **Older apps and controls:** an app from before #758 sends no keys and
  no `features: ["no-code"]`, so control gives it a code and the page shows
  it, as before. A newer app on an older control gets a code back and shows
  it; that control's page never signs, so the window asks to be approved.

### Sharing with a person: their key on first use

Sharing a session with someone found by their login (#763) is Signal's
model: control says who `@name` is, and there's no other channel here, so
nothing is compared on the way.

- **Trust on first use.** The first share pins their account's root (its
  first device's id) in the sharing browser, in the same `localStorage`
  map (`arugula.control.pins`) that already pins the root of each other
  account whose machines the directory lists. A second map
  (`arugula.control.pin-meta`) keeps when each was pinned, and whether it
  was verified. Control can't rewrite either: they're on your device. The
  machine's grant keeps the root too (it's what the machine checks that
  person's devices against), so a machine you've shared from before also
  counts as a pin: sharing from a second device of yours checks against
  it.
- **A changed key stops the share.** If control names a root for `@name`
  other than the one pinned in this browser, or held by a grant to them on
  this machine, nothing is shared: the dialog says their account key
  changed since you first shared with them, with the date of the first
  share and the date this browser first saw the new key. It remembers the
  new key it saw, so their row in the list says so from then on (never
  just losing its verified mark), and a role change there stops the same
  way, until they're verified.
- **Verifying is optional.** *Verify* beside the person (or on the
  warning): their *Devices and machines…* has *Someone verifying you?*, a
  QR code of `arugula-verify:<account>.<root>` and the same as eight words
  (the first 8 bytes of SHA-256 over `arugula verify v1`, the account and
  the root, a word per byte from a fixed list of 256). Their side uses the
  root their device pinned, not control's word for it. Scanning it, or
  pressing *The words match*, pins that root (replacing a changed one) and
  marks the person verified; the mark goes if the pinned root changes.
- **Accepted risks:** control can name someone else on the very first
  share, unless you verify. Pins live per browser, so a device of yours
  that never shared with them and shares to a machine with no grant to
  them trusts on first use again. Inviting them to a team by link (#762)
  avoids trusting control at all; the dialog says so beside *Verify*.

## What holds against control

Who can read your terminals, depending on what control (the hosted service,
or whoever runs yours) does:

| Control… | Your terminals |
|---|---|
| relays and stores honestly | only your devices and the people you share with read them |
| has its database or relay copied, or logs everything it sees | still unreadable: it holds certificates and ciphertext, never a private key |
| turns malicious **after** your machines and devices are set up, without changing the page it serves | can't add a reader: daemons check every device against the root they pinned. It can withhold revocations and refuse service |
| lies at **join** about which account a machine joins | refused by the machine: the approving device's answer to the secret in its link doesn't match (an older daemon or CLI: caught by the account fingerprint, if you compare it) |
| lies about which account a **new browser or phone** is approved into | refused by that device: the approving device's answer to the code it showed doesn't match (approved from a page from before codes: it shows the account fingerprint to check) |
| lies about who **@name** is when you share with them | believed on the first share unless you verify them; after that the share stops, saying their key changed |
| serves a **modified web client** | can read what that client shows and use its keys while it runs |

The last row is the important limit. The browser client at control's
address is JavaScript that control serves, and a page can't check its own
code. A modified page could send keys or plaintext anywhere. That applies
to:

- any browser or phone that opens control's page;
- **the desktop app once it's joined**: its window then loads
  control's page, signed in through your browser;
- not to a daemon's own page (`http://127.0.0.1:7681`, or its tailnet
  name) or the desktop app before it's joined: the daemon serves those from
  its own binary;
- not to `arugulad` or the CLI.

So the promise is end to end encryption against the relay and against
anyone who gets control's data, with the client code and the join trusted.
To avoid trusting control's page, reach your machines from their own pages
(over the tailnet, or `--direct-url`): a daemon's page talks to the daemon
it came from.

## Channels

**Pattern.** Noise `IK` with `25519`, `AESGCM` and `SHA256`.

- The client always knows the daemon's static key: it comes from the
  directory as part of the daemon's signed certificate. So IK applies, and
  attach takes one round trip:
  ```
  -> e, es, s, ss   + payload   (client static encrypted to the daemon)
  <- e, ee, se      + payload
  ```
- AES-GCM rather than ChaCha20-Poly1305, because WebCrypto has AES-GCM and
  not ChaCha. The browser then ships no crypto code: the whole initiator is
  [about 190 lines](archive/spikes:spikes/s15-control/web/noise.ts) over `crypto.subtle`.
- **Prologue:** `"arugula/1" ‖ daemon id ‖ session id or empty`. If the
  relay splices a client onto the wrong daemon, the handshake fails.
- **Message 1's payload** is encrypted to the daemon's static key only, so
  it is replayable. It may carry only idempotent requests (`hello`, and
  `attach` with offsets). It never carries input, method calls or intents.
  Message 2's payload carries the `hello` tree. A block's channel carries
  its grant there instead ([Block channels](#block-channels)).

**Framing.**

- On WebSocket, one binary message is one Noise message.
- On a mux stream (the relay's splice), each Noise message is
  `len (u32 BE) ‖ bytes`.
- Plaintext starts with one byte: `0` means a whole protocol message
  follows (today's JSON or binary frame), and `1` means more follows. That
  covers snapshots over the Noise limit of 65,535 bytes. Senders chunk at
  16 KB.

**Costs, measured in the spike:**

| | Result |
|---|---|
| Handshake bytes | 96 out and 48 back, plus payloads |
| Handshake CPU, both ends, Rust | about 370 µs |
| Handshake in Chrome, WebCrypto | 0.7 ms (key already loaded) to 6 ms (first use) |
| 1 MB burst | 0.098% wire overhead; 805 MB/s encrypt plus decrypt in Rust; 390–460 MB/s through Node's WebCrypto |
| Keystroke | 1 byte becomes 17 (plus framing); 0.4 µs |

Encrypting the tailnet path too costs nothing anyone will notice, and it
keeps one code path.

**Rekeying.** Noise's 64-bit counter never wraps on a terminal stream.
Channels are re-handshaken on every reconnect anyway.

## Shared sessions: per-viewer channels

The plan suggested a session key wrapped for each member device. The spike
compared that with plain per-viewer channels, for 10 MB of output in 4 KB
writes:

| Viewers | Per-viewer: daemon CPU | Per-viewer: daemon uplink | Session key: CPU | Session key: uplink |
|---|---|---|---|---|
| 2 | 13 ms | 21 MB | 6.5 ms | 10.5 MB |
| 5 | 33 ms | 53 MB | 6.5 ms | 10.5 MB |
| 20 | 130 ms | 211 MB | 6.6 ms | 10.5 MB |

- **CPU doesn't matter** either way.
- **Uplink does, at scale.** With per-viewer channels the daemon sends one
  copy per viewer. A session key saves that only if the *relay* fans the
  one copy out, and that means:
  - the relay does per-viewer flow control (today it's per stream on the
    daemon);
  - snapshots and "from now" history still go per viewer;
  - revoking someone needs a key rotation that every remaining device
    acknowledges.
- **Decision: per-viewer channels.**
  - Teams are for small groups (2–5 people). Five viewers of a busy build
    log cost five times its output on the daemon's uplink, which a home
    connection carries.
  - Per-viewer channels mean one mechanism for direct, relayed and shared
    access, and revoking someone is instant: close the channel, refuse the
    key.

### Fan-out later, if ever

The trigger: relayed sessions regularly have more than 5 viewers, or a
daemon's uplink saturates on shared output. Then:

- add a group frame (output only, sealed with a per-session key and
  distributed through each viewer's channel);
- the relay fans group frames out;
- everything else stays per channel.

Nothing in this spec blocks it.

## The relay

Control's relay is the daemon's dial-out transport with control at the home end:

- An enrolled daemon keeps one WebSocket to `wss://control/dial`,
  authenticated by a Noise handshake with its daemon key. (The spike
  trusts `?pub=`; the real one checks the key against the account's chain.)
- A client connects to `wss://control/c/<daemon id>`. Control checks that
  the account may reach that daemon (metadata it has anyway), opens a mux
  stream and splices the two. It forwards opaque Noise messages and counts
  bytes per account (for fair use and the meter).
- The mux is unchanged from the daemon's dial-out transport: per-stream credit, 256 KB windows, 16 KB
  frames.

**Measured** (spike relay on Fly, `shared-cpu-1x`, 256 MB):

| Path | Keystroke round trip p50 | 1 MB burst |
|---|---|---|
| Loopback, relay on the same machine | 36 µs | 1.1–1.8 ms |
| geek → Fly ord → geek | 51.6 ms | 208 ms |
| geek → Fly ewr → geek | 18.7 ms | 99 ms |
| geek → Fly ewr (echo only) | 9.1 ms | — |
| Phone on cellular → Fly → geek | pending phone run | |

- A relayed round trip is about the client↔relay round trip plus the
  relay↔daemon one. Compared with the direct path, the relay adds about
  one relay↔daemon round trip (minus whatever the triangle saves).
- **So the relay must run in the region nearest the daemon.** From geek,
  ewr adds about 9–19 ms; ord added 52 ms.
- **Hosted control is multi-region on Fly.** A daemon dials the anycast
  name and lands on its nearest machine, which holds its mux. A client may
  land elsewhere; that machine answers with `fly-replay:
  instance=<machine>` so Fly routes the WebSocket to the machine holding
  the mux. Self-hosted control is one process and needs none of this.
- **Capacity:** one relay process held 1,000 concurrent channels to one
  daemon. Each typed once a second, with p50 0.9 ms and p99 7 ms, 0
  failures, and 115 MB RSS on loopback. Streams are cheap; uplink
  bandwidth is the limit.
- **Cost:**
  - a `shared-cpu-1x` relay is about $2 a month and has headroom for
    thousands of channels;
  - egress is $0.02/GB in North America and Europe;
  - an active terminal user-hour is roughly 2–50 MB (typing, an agent's
    output, a few attaches), so **$0.0001–0.001 per active user-hour**.

  Relay traffic can be included in every plan; fair-use caps exist for
  abuse, not margin.

**Found and fixed on the way: Nagle.** `axum::serve` leaves Nagle on, and
the dial-out mux writes frames back to back (OPEN then DATA, DATA then
GRANT), so the second frame waited for the peer's delayed ACK. That was
40 ms on every request and keystroke through `/h/<name>/…` on the home
daemon today: 41.7 ms against 1.4 ms direct. The daemon now sets
`TCP_NODELAY` on every accepted connection, and relayed requests take
2.0 ms. Control's relay must do the same.

### Direct paths, and Chrome's Local Network Access

The client tries the tailnet or LAN URLs from the directory first, then the
relay. The spike found a wrinkle:

- **Chrome (153) blocks a public origin from connecting to a private
  address**, and that includes the tailnet's 100.64.0.0/10:
  `net::ERR_BLOCKED_BY_LOCAL_NETWORK_ACCESS_CHECKS`.
- A page served by control (`https://control.arugula.io`)
  therefore can't open `wss://geek.<tailnet>.ts.net` until the user grants
  the *local network access* permission. Granting it worked in the spike.
- **So control's web client:**
  - asks for local network access the first time the directory lists a
    direct URL, with a line saying why ("connect straight to your machines
    when you're on the same network");
  - falls back to the relay when it's denied, or until it's granted;
  - doesn't need the permission when served by a daemon itself, as today:
    that page is on the tailnet already.

The host chip says "direct" or "relayed" either way.

## Read-only links

- A link is `https://control/s/<link id>#k=<base64url X25519 private key>`.
- Making one: the sharing device generates the key pair. Its public key
  goes to the daemon as a **link principal** (viewer of one session,
  expiry, "from now" by default), signed by the sharer's device.
- Opening one: the page reads the fragment, uses it as its Noise static key
  and connects through the relay. The daemon finds the key among its link
  principals and serves that session read-only.
- Revoking or expiring the link removes the principal and closes its
  channels.
- **Control sees** that the link id was opened, never the key or the
  content. Fragments are never sent in requests.
- **Link previews:** an unfurler that ran the page's script would see the
  fragment. The spike's share page reports what a script-running fetcher saw:

  | Previewer | Fetched the page | Ran its script (saw the fragment) |
  |---|---|---|
  | Slack | pending | pending |
  | iMessage | pending | pending |

  Whatever the results, the share page also refuses to connect until it has
  focus and a user gesture ("Open the live view"). A headless previewer
  never clicks.

## Block channels

M50 (#150, from S27): a port or editor block opened on any joined machine
shows in control's page. Control serves each block an origin of its own,
`https://b-<key>.<block domain>`, whose service worker carries the
block's requests over a Noise channel to the daemon. That origin holds no
device key, so it opens the channel with a grant
(`crates/e2e/src/block.rs`):

- Control's page makes a one-off X25519 key for one block and signs, with
  its device key:

  ```text
  arugula block grant v1
  daemon <daemon device id>
  block <block id>
  key <the one-off key's public half, hex>
  expires <ms since the epoch, by the daemon's clock>
  by <signing device id>
  ```

  It hands key and grant to the block's frame by `postMessage`.
- The frame does the handshake with the one-off key as its static key and
  puts `{"grant": …}` in message 1's payload. The daemon checks it before
  answering: this daemon, the handshake's key, a device it trusts to open
  channels, the signature, not expired by its own clock and good for at
  most ten minutes more, a block it has. Message 2's payload is
  `{"ok":true}` or `{"ok":false,"refused":"<code>","message":"…"}`,
  and a refused channel closes. A client reads message 2 before it treats
  the close as a failed handshake; `expired` means "ask for a new grant".
- A replayed message 1 gets an answer nobody can read: message 2 and the
  channel need the one-off key's private half.
- The expiry comes from the daemon's clock, which the page learns over its
  own device channel (message 2's payload there is `{"now": <ms>}`), never
  from the frame, whose code could stretch its own grant. Expiry gates new channels; an open one lasts until it, the
  block, or the device's trust ends.
- Once open, the channel carries S27's frames (`arugula_e2e::block::wire`):
  HTTP requests the daemon answers from its block proxy (`sites.rs`), and
  the page's WebSockets. The daemon's side is `crates/daemon/src/block_channel.rs`.

### Block origins

Control's side is `crates/control/src/blocks.rs`, on when
`ARUGULA_CONTROL_BLOCK_DOMAIN` is set (docs/control-ops.md);
`/control.json` then says `"block_domain"`. A block's origin is
`https://b-<site_key>.<block_domain>`, with `site_key` from the block's
state (the daemon's `Site::site_key`). There control serves only:

| Path | What |
|---|---|
| any navigation the worker didn't answer | the bootstrap page: `<meta name=arugula-control content="<control's origins>">` and `/.arugula/boot.js`, under `default-src 'none'; script-src 'self'; …; frame-ancestors <control's origins>` |
| `/.arugula/boot.js`, `/.arugula/sw.js`, `/.arugula/shim.js` | `web/src/block/`'s scripts; the worker gets `Service-Worker-Allowed: /` and no CSP |
| any request with `Service-Worker: script` | `self.ARUGULA_APP_SW="<path>";importScripts("/.arugula/sw.js")`: the app's own worker, run inside the block's |
| `/.arugula/relay?d=<daemon id>` | the block's channel through the relay, for a socket whose `Origin` is the block's own |
| anything else | 503: the worker wasn't in the way |

Control's page, cookies and API never answer on a block's origin, and
nothing under the block domain that isn't a block's origin answers at all.
The relay can't read the grant, so it relays ciphertext to the named
daemon for anyone, with limits instead (docs/control-ops.md); an unknown
daemon and one that isn't connected get the same 404.

### The frame's contract with control's page

The bootstrap page, the worker and the shim (`web/src/block/`) talk to the
page that framed them with `postMessage`. Every message carries
`arugula: "arugula-block"` and `v: 1`.

- **The frame asks** `{arugula, v, ask: "hello"}` on its first load and
  `{arugula, v, ask: "renew"}` when its grant was refused as `expired`.
  The bootstrap page sends it to each of control's origins from its
  `<meta name=arugula-control>`, so only the real parent receives it; later
  asks go to the origin that answered.
- **Control's page answers** the frame only when `event.source` is the
  `contentWindow` of a frame it made, and `event.origin` is the origin it
  gave that frame. It answers for that frame's block only, whatever the
  message says, and posts to that origin only:

  ```ts
  frame.contentWindow.postMessage({ arugula: "arugula-block", v: 1, creds }, blockOrigin);
  // or, when it can't: { arugula: "arugula-block", v: 1, refused: "<a sentence>" }
  interface Creds {
    daemon: { id: string; noise: string }; // device id, Noise key (hex) from a checked cert
    key: { pkcs8: ArrayBuffer; pub: string }; // a new X25519 key: PKCS#8, public half hex
    grant: Grant; // arugula_e2e::block::Grant for key.pub, expires by the daemon's clock
    route: "relay" | string; // "relay", or a wss://…/e2e URL straight to the daemon
  }
  ```

  A new key and grant each time; the grant at most ten minutes ahead of
  the daemon's clock.
- **The frame accepts** an answer only from `window.parent`, at one of
  control's origins, and keeps `{creds, parent: event.origin}` in its
  origin's IndexedDB (`arugula-block`). Its worker takes them from the page
  over a `MessageChannel` (`{arugula, v, held}`), and asks its pages for a
  renewal with `{arugula, v, ask: "renew"}`.
- `route: "relay"` connects to `wss://<the block's host>/.arugula/relay?d=<daemon id>`.
  S27 kept WebKit on the relay until the worker's direct path is measured
  in real Safari.

## Push

RFC 8291 encryption needs only the subscription's public parts
(`p256dh`, `auth`). Decrypting needs the browser's private key, which never
leaves it. So:

- **Subscribing:**
  - devices subscribe once, with control's VAPID public key as the
    `applicationServerKey`;
  - the device signs `{endpoint, p256dh, auth}` with its `sign` key, and
    control stores and distributes that.
  - The signature matters: otherwise control could swap in a `p256dh` of
    its own and read notifications.
- **Sending:**
  - the daemon checks the signature and encrypts the payload (today's
    `push::encrypt`);
  - it hands control `{endpoint, ciphertext, ttl, urgency}` through the
    dial-out socket;
  - control signs the VAPID JWT and posts it.
- **What control and the push service see:** endpoint, size and timing.
  Control logs "daemon X notified device Y".
- **Control's own notices:** a new device waiting for the account, or
  someone asking to join a team (to its owners). Control encrypts these
  itself. They say only that something waits ("A new browser wants into
  your account", "Ada asks to join Acme"); approving still happens on the
  page, with the fingerprint.

This is confirmed by construction: the daemon's existing RFC 8291 encrypt
plus a separate VAPID signer is exactly what the RFC allows. The push work adds the
handoff.

## Identity

Signing in proves who the account is. It doesn't make a device trusted:
only an approval does.

- **First:** GitHub OAuth, and passkeys as a first-class login (the same
  passkey can supply PRF).
- **Later:** Google OAuth, and email magic links for invitees.
- Control's session cookie is for control's API only (directory, approvals,
  invites). Daemons never accept it.

## What control stores

| Stored | Never stored |
|---|---|
| accounts and their display names, OAuth ids, passkey public keys | Noise or signing private keys of anything |
| device and daemon certificates, revocations, signed grants | terminal bytes, snapshots, logs, history |
| directory: daemon ids, names, URLs, last seen, presence | link keys (the fragment) |
| connection metadata: who, which daemon, when, byte counts | push payloads in the clear |
| signed push subscriptions, VAPID key pair | provider tokens for your own machines |

Logs follow the same rule: nothing a daemon sends inside a stream is
logged.

## Open items

These are pending the phone runs (see the spike README):

- PRF on real devices (the go/no-go table above);
- the cellular round trip through the relay, against the relay's 30 ms budget;
- link-preview behaviour in Slack and iMessage.
