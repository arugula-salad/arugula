# Arugula control: keys, channels and the relay

Written 2026-10-01, from the control spike ([spikes/s15-control](../spikes/s15-control/README.md)).
This is the design the control track builds: accounts and devices, the
end-to-end relay, teams and push. It turns the control track's promise
(plan-archive.md, "Control track") into mechanisms: **control can refuse service,
but it can't read.** That promise has limits, set out in
[What holds against control](#what-holds-against-control): the client code
control serves, and the first account a machine joins, are trusted.
And it covers the machine plane only: the platform plane, what has to exist
while every machine is asleep (chat, Cloud workspaces, Shared Docs,
billing), is held in control, much of it in plain text. [What control
stores](#what-control-stores) lists every table of both.

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
    itself, and they say only that;
  - chat's pushes (M122) are control's own too: who, where, and the
    first 200 characters of the message.
- **Chat:** held by control in plain text (M122): channels, DMs and the
  threads about panes, with the lines people choose to quote from a pane.
  The rest of a pane never reaches control.

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
{ v: 1, account, device: <id>, name, kind: browser|cli|daemon|recovery|app,
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
- **Who signs what** (`Kind::certifies`, the page's `certifies`): a
  browser signs any kind. A recovery code signs people's devices, not
  machines. A CLI signs CLIs and machines, and the desktop app (`app`)
  signs apps, CLIs and machines: their keys sit where a program on their
  computer can read them, so neither makes a browser, nor a recovery code
  (which would make one). The app is never an account's first device.
  Daemons and pages drop a certificate that breaks this, wherever it is in
  the chain.
- **What only a browser all the way back does** (`Trust::browser_chained`:
  the device and every device that certified it, the root included, are
  `browser`): make a team's owner (a roster that adds or promotes anyone
  but its signer to owner, or moves its signer's own entry to another
  root, and an owner's Ask me first invite) and remove a browser, a phone
  or a recovery code. A team's editor (a roster, a presigned or Ask me
  first invite) comes from that or the desktop app's device, never a CLI
  (`team::raise_ok`; Jake, 2026-10-22). Daemons and pages ignore such a
  paper from anything else. A CLI or the app still adds viewers, removes
  machines, CLIs and apps, and moves machines.
- The app's devices from before it had a kind are `browser` certificates
  it named ("Arugula app on …"). Control holds them to `Cert::is_browser`
  on every link, the root included: they, and browsers they certified,
  make no new browsers, recovery codes, owners or editors, and remove no
  browsers. What they did before stands. The deploy approval (M218) asks
  the same of every link.
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

- **Derivation** (`adoptPrfKeys` in `web/src/shell/e2e/keys.ts`): every passkey
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
- **Hosted machines** (step 5) join under a project's delegation instead
  ([below](#hosted-machines-a-projects-delegation)).

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
  (`deviceMac` in `web/src/shell/e2e/cert.ts`), where the root is the one the
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
- **Allow makes the app a device, an `app` one.** The app's own sign-in
  page (served by the app, not control) makes the window's device keys and
  sends their public half with the ask. Control shows them to the browser,
  and a browser that's one of the account's devices signs an `app`
  certificate for them when the person presses Allow. The certificate and the root that browser
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
- **Why `app`, not `browser`:** the grant and the certificate go to
  whatever listens on `127.0.0.1:<port>`, which on that computer could be
  another program, an agent's included. So the window's device reaches the
  account's machines and approves machines, CLIs and other apps, but never
  a browser, a phone, recovery codes or a deploy. The prompt says so. A
  session the hand-over made (`sessions.via = app`) enrolls an `app`
  device only. Daemons and CLIs from before `app` can't read one, so
  control leaves `app` certificates out of what it answers a client that
  doesn't send `x-arugula-features: app-devices` (`compat.rs`): such a
  daemon doesn't let the app's window in until it's updated.
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

### Hosted machines: a project's delegation

A hosted machine (a Cloud workspace, step 5) is `arugulad` in a box the
operator rents: nobody's device is there to approve its join. So an owner
approves once per project instead ("Control enrolls hosted machines under
a project delegation", aru-095; `arugula_e2e::hosted`,
`crates/control/src/hosted.rs`). This changes what control can do, inside
that project only.

- **The delegation.** An owner's own device (a browser or the CLI: not a
  machine, not a recovery code) signs `arugula project delegation v1`: the
  project, its owner (`team:<id>` or `account:<id>`), control's hosted key
  (Ed25519, in `/control.json` as `hosted_key`), when, and which device.
  For a team's project the signer must be an owner in the roster; for a
  personal project, a device of that account. Devices and machines… has
  *Hosted machines*: *Allow* signs it, *Stop* revokes it. Control keeps it
  beside the roster (`project_delegations`, in the hub, which holds the
  rosters and device chains it's checked against).
- **The key.** The hub makes its hosted key on first start and keeps it in
  `settings`, as it keeps its VAPID key. It signs hosted machines'
  certificates and nothing else. A new key ends every delegation (they
  name the old one), and owners sign again.
- **The enrollment.** When the platform makes a box, it asks the hub for
  a one-time enrollment (`HubPort::hosted_enrollment`), bound to the
  project and the workspace (`hw_…`), under the project's newest live
  delegation. Two kinds, both once only:
  - **a ticket**, made by the hub (32 random bytes), for the box's env
    (`ARUGULA_HOSTED_TICKET`), good for 15 minutes: boxes started fresh,
    loam, and testnet;
  - **the box's sandbox id**, which the platform passes when it creates
    the box, good for 2 minutes: a box started from a running snapshot
    (E2B) can't read the env it was made with, but it can read its own
    sandbox id (`/run/e2b/.E2B_SANDBOX_ID`). The box makes its key when it
    first sees a new sandbox id. The sandbox id is a bearer secret for those
    2 minutes: never logged, never in a URL. One enrollment per sandbox id,
    ever.

  The hub keeps only a hash of either (`hosted_enrollments`).
- **Enrolling.** The box sends `POST /api/hosted/enroll` once: the ticket
  or the sandbox id, its new public keys, and its signature over
  `arugula hosted enroll v1` (the hash, its keys, the time), so it holds
  the key it enrolls. The hub uses the enrollment up, checks that the
  delegation is still live, and signs an `arugula hosted machine v1`
  certificate with the hosted key: the project, its owner, the workspace,
  the delegation, the machine's keys and name, when. **Fail closed:** a
  second try is refused (409), even with the right key, and the machine
  that used the enrollment is removed with it, since control can't tell
  which of the two is the box; the platform hears it was taken
  (`HubPort::hosted_enrolled`) and deletes the box. The platform recording
  the same sandbox id for the same workspace again (a lost answer) gets the
  same enrollment back; for another workspace, it's taken. A key is one machine's,
  ever, and a workspace has one machine: enrolling a new box for it
  removes the old one.
- **What the box pins.** The enrollment's answer has the certificate, the
  delegation and its revocations, and who's in the project: for a team's,
  the team pin, the roster and members' certificates (it serves people as a
  team machine does); for a personal one, the account's root and
  certificates. `GET /api/hosted/trust` gives it again.
- **Clients accept it for its project only** (`hosted::accept`,
  `acceptHosted` in `web/src/shell/e2e/hosted.ts`). The directory lists a
  hosted machine only to its project's people, with its certificate,
  delegation and revocations; the page checks them against what it holds
  itself (its team's roster and members' certificates, or its own
  account's chain), not control's word for the machine:
  - the delegation is signed by a device of one of the project's owners,
    trusted now (an owner who leaves the owners, or a signing device that's
    removed, takes the delegation with them: another owner signs again);
  - the certificate is signed by the key the delegation names, for that
    project and delegation, made after the delegation and before any
    revocation of it;
  - this account is in the project. A device outside it refuses the
    machine even if it's shown one.

  It's never one of an account's devices: its certificate is its own
  domain, so `Trust::evaluate` never counts it, it approves nothing, and
  its keys can't stand for a daemon or a browser of anyone's. The shell
  shows it as **Hosted: the operator can read** wherever a machine's place
  is shown.
- **Routing.** Control routes to a hosted machine only its project's people
  (a locked team's: its owners), never by a share, never as anyone's own
  machine. Its signature opens only its own routes and the relay: every
  route a daemon's signature opens refuses it. It isn't in `daemons`, so
  nothing that reads a daemon's account (moves, shares, grants, Labs
  flags) sees it.
- **Their machines, and agents** (M184). A project person's own machine
  reaches a hosted machine too, for its agents to drive it over MCP
  (`run`, `start_agent`, `read_output`, `show` there): control routes it
  only while it holds a live `cloud.drive` grant ("Grants, not
  impersonation"), and the hosted machine lets it in as it lets in that
  person's browsers. An owner's or editor's machine has full use; a
  viewer's reaches only its agents (`/api/a2a/`). When an agent block on a
  hosted machine closes, the machine sends its transcript
  (`POST /api/hosted/transcripts`, signed as itself), and the platform
  keeps it read only under the project (`hosted_transcripts`), in the
  clear, for the project's people to read after the box is gone. Control
  can read those, as it can read the box.
- **Revoking.** Any owner's device signs `arugula project delegation
  revoke v1`. Nothing more enrolls under it, even an enrollment recorded
  before. The machines enrolled before stay, as a revoked device's earlier
  approvals do, until they're removed: by a project owner
  (`DELETE /api/hosted/machines/{id}`) or when the platform deletes or
  rebuilds the workspace (`HubPort::hosted_gone`, which also drops an
  enrollment nobody used). A removed machine is
  hung up on, and its key signs nothing here again.

**What control can do with a delegation:** within that project, add
machines the project's people will reach and type into, and read what runs
in them (it runs the box). That's what the label says, and it's true of any
VM: whoever runs it can read its memory. So can E2B, which runs the boxes
for control: it processes what's in them, their disks and memory and the
secrets in its broker ([Who else sees it](#who-else-sees-it)). **What it can't:** add a hosted
machine to a project nobody delegated, or after the delegation is revoked,
without the project's people's pages refusing it; make a hosted machine
count as anyone's device or approve one; reach any other machine or
terminal of the project's people (their own machines check devices against
their own chains, which the hosted key isn't in). **Accepted risks:**
control writes the certificate's time, so a malicious control could
backdate one past a revocation (it runs the boxes anyway); and the page's
team roster is the one control serves, as for team machines.

### Runs on hosted machines: the standing project grant

Scheduled and event runs start on a project's hosted workspaces with every
owner's device asleep (platform step 7). An owner's device signs, per
project, that the platform may ("One standing grant per project starts
runs", aru-099; `arugula_e2e::project_grant`, `crates/platform/src/run_grant.rs`,
`crates/daemon/src/hosted/runs.rs`). It's S37's standing grant with a
project as its resource.

- **What it allows.** The platform may start runs, and instruct them, on
  every hosted workspace of the project, running whatever command the
  project's runs start, until the grant expires or is revoked. It never
  covers a deploy.
- **The grant.** An owner's browser or phone, one that's a browser all
  the way back to its account's root (`Owners::browser_signed`; recovery
  codes count on the way), signs `arugula project grant v1`: its id, the project and its
  owner, the project's live delegation, `resource hosted`, `to platform`,
  the powers (`runs.instruct`, `runs.start`), when, when it expires, and
  which device. The page and the CLI give 90 days unless the owner picks
  (at most 366); the platform refuses one with no expiry or a longer one.
  Devices and machines… has *Runs on hosted workspaces* under *Hosted
  machines*: *Allow runs* signs it, on a computer or the phone, and
  *Renew* signs a revoke and a new grant when it's about to expire (or
  *Sign again* when the project's delegation changed). Not the CLI or the
  desktop app: their keys may be a program's on their computer, so
  `arugula runs grant sign` and `revoke` send you to the page, and the
  app's window has those buttons off. The platform's project settings
  show it, and offer it once the project has a hosted workspace.
- **The platform keeps it, and can't make or widen it.** It stores the
  signed bytes and the signature (`run_grants`), one live grant a project
  (not revoked, expired or superseded), and reads the grant back from
  those bytes only. It takes a grant only if it names the project's live
  delegation (the hub's word), and only if the hub says a browser of one
  of the project's owners signed it, all the way back
  (`HubPort::owner_browser_signed`, with `Cert::is_browser` on every link),
  and the same of a revoke. One that expired, or names a delegation that
  isn't live any more, is superseded by the next. It holds no device keys
  itself; the hosted machines check again.
- **Each start carries it.** The platform asks the workspace's machine
  over the relay (`runs.start`, a typed op), with the grant and the
  newest revoke of the project's grants. The machine checks, with what it
  pinned at enrollment (the team's roster and members' certificates, or
  the account's chain): the grant is signed by a browser of one of the
  project's owners, trusted now and a browser all the way back; it's for the machine's own project and
  owner and names the delegation it was enrolled under; that delegation
  isn't revoked; it covers what's asked; it hasn't expired; no owner's
  revoke covers it. If any check fails, nothing starts and the refusal's
  code comes back.
- **Revoking.** Any owner's browser (all the way back, as for the grant)
  signs `arugula project grant revoke v2`
  (the project, a delegation, `running finish` or `running stop`, when, by).
  It names no grant: it takes back every grant of the project under that
  delegation made at or before its time, so whoever relays it can't point
  it at another grant, and a grant signed after it is a new one. The
  platform stops starting at once, and relays it to every hosted machine
  of the project (`runs.revoke`), and again with every later start. A
  machine checks a revoke's signature once, when it first hears it, and
  keeps the newest (`<state>/run-grant-revokes.json`): after that it counts
  without being checked again, so the platform can't bring a grant back by
  leaving the revoke out, or by dropping its signer's certificate from what
  it sends. The kept file fails closed: once one was kept, a missing or
  unreadable file refuses every start. It's in arugulad's state directory,
  which runs must never be able to write (M212). `running stop` also stops
  the runs going: each machine stops everything they started, and they
  fail, saying the grant was revoked.
- **It gates only what the platform starts.** People's own use of a hosted
  machine (its owners and editors) doesn't go through it.

**What control can do with a grant:** start and instruct runs on that
project's hosted workspaces until it expires or is revoked, at times it
chooses. A machine that heard a revoke keeps it; one that never heard it
(control dropped it) honours the grant until it expires, which is why it
expires. **What it can't:** make a grant, widen one (another power, a
later expiry, another project or delegation), point a revoke at another
grant, or bring back one a machine has heard revoked.

**Running one** (M212; `crates/platform/src/work.rs`,
`crates/daemon/src/work.rs`). A run is S37's operation (`arugula op v1`,
`arugula_e2e::run::Op`): a command and its arguments, or an instruction for
an agent, and a directory, for one hosted machine.

- **Only to a hosted machine.** The run's job checks that the machine its
  operation names is its hosted workspace's box. A run for any other
  machine (a person's) fails, saying so, and never reaches it.
- **The machine pulls it.** A hosted machine asks for its work every few
  seconds, signed as itself (`GET /api/hosted/work`; only a hosted
  machine's signature opens it), and at once when a start names a run.
  Control answers every run that's ready for it and hasn't started, again
  and again until it starts, and the project's revokes every time. A
  paused box is woken for its run. One run goes at a time on a machine.
  A run nobody asks for in 15 minutes fails.
- **Checked again on the machine.** The operation's bytes must hash to the
  run's hash; it must be for this machine, project and workspace; its
  directory must be under the box user's home and not hidden; and the
  grant must pass the checks above. Otherwise the run fails, with the
  check's name, and nothing starts.
- **At most once.** Before it starts anything, the machine makes the run's
  ledger entry (`<state>/work/ID.json`, made once, fsynced), saying
  `running`. It never starts an id with an entry again, or one whose entry
  can't be read, whatever control sends; it only sends again the reports
  control hasn't taken. A run it finds `running` after a restart with no
  pane fails ("interrupted by a restart; not run again").
- **As another user, in its own copy.** The command runs as `runner`
  (`sudo -n -u runner`, an empty environment, a pty of its own), which:
  - has no sudo, and can't read arugulad's state directory (the machine's
    keys, its enrollment, the revokes it kept) or write anything of the
    box user's;
  - reads the box user's repos through their group, and works only in its
    own copy (a clone, under `~runner/work/ID`, which is also its `HOME`);
  - can't reach the daemon's ports, the provider's agent or the privileged
    ports on loopback (iptables rules the daemon makes before the first
    run; until they're in, nothing runs).
  When a run ends, or is stopped, everything `runner` runs is killed.
- **In a pane.** A new pane in the `runs` session, an ordinary end-to-end
  pane its project's people watch as any. Its first lines and its tab say
  which run it is, the operation's hash and the exact command, with
  anything that wouldn't print as itself escaped.
- **Agent runs** are Claude Code, headless (`claude -p`), with the
  instruction, as `runner` in the run's copy. Its login reaches it on its
  standard input, never on a command line, and its exit is the run's.
- **Reported, signed.** `running` (with the pane and the operation's
  hash), then `succeeded` or `failed` (the exit code), each signed by the
  machine (`arugula run event v1`) and kept with its signature
  (`POST /api/hosted/work/report`).

**What control can do with runs:** under a standing grant, nothing signed
by a person binds the command, so control chooses it, on that project's
hosted workspaces. Each run's events say, signed by the machine, which
operation it ran, and its pane shows it. **What it can't:** run anything on
a person's machine, run a run twice, or report a run's state for the
machine.

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
| enrolls a **hosted machine** into a project an owner delegated | can read what runs in it: it runs the box (labelled "Hosted: the operator can read"). Only that project's people reach it, and nobody accepts it after the delegation is revoked |
| enrolls a hosted machine into a project nobody delegated | refused by every client: no owner's device signed for that project |
| starts runs on a project's hosted workspaces | only under an owner's signed standing grant, which each machine checks; refused once a machine has heard it revoked, and by every machine once it expires |
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

The platform page is apart from it (step 3, "The platform page is on
app.arugula.io, not on the keys' origin"). Projects, chat and Cloud
workspaces are on `app.arugula.io`, an origin of its own that the hub serves
from the platform's binary. It holds no device key, and its build provably
loads none of the code that does: CI refuses any import of the key code,
the daemon client, control's trust client or the fleet from it, and checks
its build's modules (`web/scripts/check-imports.mjs`). A modified platform
page could read what it shows, which is the platform plane's plaintext that
control holds anyway (chat, Cloud workspaces), but it can't use a device key
or reach a terminal. Its session is its own host's, made by the hub from
yours and ended with it; passkeys stay on control's page.

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
| `/.arugula/boot.js`, `/.arugula/sw.js`, `/.arugula/shim.js` | `web/src/shell/block/`'s scripts; the worker gets `Service-Worker-Allowed: /` and no CSP |
| any request with `Service-Worker: script` | `self.ARUGULA_APP_SW="<path>";importScripts("/.arugula/sw.js")`: the app's own worker, run inside the block's |
| `/.arugula/relay?d=<daemon id>` | the block's channel through the relay, for a socket whose `Origin` is the block's own |
| anything else | 503: the worker wasn't in the way |

Control's page, cookies and API never answer on a block's origin, and
nothing under the block domain that isn't a block's origin answers at all.
The relay can't read the grant, so it relays ciphertext to the named
daemon for anyone, with limits instead (docs/control-ops.md); an unknown
daemon and one that isn't connected get the same 404.

### The frame's contract with control's page

The bootstrap page, the worker and the shim (`web/src/shell/block/`) talk to the
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
- **The frame says when it can't load** the block (no service worker in a
  cross-site frame, the parent refused, the worker never answering):
  `{arugula, v, failed: "<a sentence>"}`, to each of control's origins.
  Control's page, checking source and origin as above, then shows the
  daemon's own block site where it has one (`--block-listen`), with a line
  saying why, or says there's none.
- `route: "relay"` connects to `wss://<the block's host>/.arugula/relay?d=<daemon id>`.
  A `wss://…/e2e` route that doesn't connect is tried once more through
  the relay. S27 kept WebKit on the relay until the worker's direct path
  is measured in real Safari.
- **Chrome's Local Network Access** (#849): a tailnet's 100.64.0.0/10 and
  a LAN are "local", and Chrome checks WebSockets since 147. Control's
  page frames a block with `allow="local-network; local-network-access"`,
  so a page in the frame may take the direct route, asked on control's
  page's behalf (which has the permission when its own channel went
  direct). A service worker is checked against its own origin's
  permission, which a block's origin never has and a worker can't ask
  for, so in Chrome 147 and later the worker doesn't try the direct route
  (`workerDirectBlocked`): the block's requests go through the relay, its
  pages' WebSockets direct.
- **The frame says which way it connected**, since that fallback asks
  no one: `{arugula, v, route: "direct" | "relayed", why?, ws?}`, to the
  origin in `Held.parent` (#150, PR 5). `why` says why not direct when
  the route was (the error and how long it took, or why it wasn't
  tried), and `ws: true` marks the pages' own channel (their WebSockets)
  rather than the worker's. The worker tells its pages each time it
  opens a channel, and a page that loaded after the channel opened asks
  it (`{arugula, v, ask: "route"}`, page to worker); the page's shim
  passes it on, and tells its own when it opens one. Both log to their
  console too. Control's page hears it with the same checks as an ask
  (source, origin, that frame's block, tag and version, and a route it
  knows) and shows it on the block and in the host menu. It's the
  block's word, and the block's own pages could say anything, so it's
  for showing only; nothing is decided by it.

### Control's page, the parent

`web/src/shell/blocks/through-control.ts` and `block-frame.tsx` (#150, PR 4):

- **When:** a port or editor block whose state has a `site_key`, on a
  machine this page reaches through a control whose `/control.json` says
  `block_domain`: a hosted machine's too (M186). The frame is `<page's scheme>//b-<site_key>.<block
  domain>[:<page's port>]` plus the daemon's `src` made a path.
- **The editor's `src`:** code-server opens a file from VS Code's
  `payload` (`[["openFile","vscode-remote://<host>/path"]]`), where the
  host must be the page's own. The daemon builds `src` for its own block
  site, or as a path with an empty host when it has none; the parent puts
  the block origin's host into every `vscode-remote://` of `payload`
  (`blockSrc`). The page that frames a block picks its origin, and the
  same block can be open on the daemon's own page and on control's at
  once, so the daemon doesn't learn control's block domain for this.
- **The grant:** a new X25519 key (extractable, handed over as PKCS#8) and
  a grant signed with the device's Ed25519 key, for that frame's block
  only. Expiry is the daemon's now plus five minutes; the daemon's now is
  its `{"now"}` from the device channel's message 2, plus the time since on
  `performance.now()`, so this device's wall clock never counts. No clock
  (a daemon too old to send one, or not connected): the frame gets
  `refused`.
- **The route:** the daemon's `wss://…/e2e` when this page's own channel
  went there directly, except in WebKit (Safari, every iOS browser,
  WebKitGTK); else the relay.
- **The device channel** (`web/src/shell/e2e/channel.ts`) reads message 2 before
  it treats a close as a failed handshake, so a refusal with the close
  reads as one.
- **Which way it went** (`web/src/shell/blocks/block-route.ts`): each block
  through control shows "direct" or "relayed" from its frame's `route`,
  or "own site" once it fell back to the machine's own block site, in a
  corner of its frame. That's the worker's channel (the block's
  requests); its tooltip adds why not direct, and which way the pages'
  WebSockets went (`data-block-route-why`, `data-block-ws-route`), and
  control's page logs each to its console. The host menu's *This machine* section sums them
  up ("Blocks: relayed", "Blocks: 1 direct, 1 relayed"), in the words
  the host button uses for the machine's own channel. A block on the
  daemon's own page says nothing.

### Hosted machines' previews

M186 (#1247, aru-097): a dev server in a hosted workspace shows in a
browser block for the project's people, the same way as any machine's:
on a block origin, through the hosted machine's relay socket.

- **The hosted daemon** reads `block_domain` and control's URLs from
  `/control.json` on each refresh, as other daemons do, so its port and
  editor blocks get sites reached by block channels (it never has a block
  listener).
- **Control relays** a block's socket to a hosted machine it knows and
  that's connected, metered against its project, as the machine's own
  socket is. It doesn't wake a paused box for a block socket: the page's
  own channel, which has a session, does that (M182). A removed machine
  is unknown.
- **Members only.** The relay has no session on a block origin, so the
  daemon decides, as for every block: a grant counts only when it's signed
  by a device it lets open blocks. On a hosted machine that's a device of
  one of the project's owners or editors, from the roster or the account it
  pinned (a locked team's: its owners). A viewer's, an outsider's, or a
  removed device's grant is refused (`untrusted`), and a page with no
  device signed in has nothing to sign one with.
- **Never the provider's preview URLs.** E2B's are public by default and
  need a header browsers can't send. A test (`no_provider_preview_urls`
  in `crates/control/src/blocks.rs`) keeps their hosts out of the tree.
- **Public previews (M187)** are a separate path (below): a port a member
  makes public is served through the relay on a registrable domain of its
  own, never under the block domain, since every block origin shares a
  site with every other.

### Public previews

M187 (#1248, aru-097): a port of a hosted workspace that one of its
project's owners or editors makes public is open to anyone, signed out,
at `https://<id>.<preview domain>` (`ARUGULA_PREVIEW_DOMAIN`,
`arugula.site` for the hosted one).

- **Not end to end.** A stranger has no device key to sign a block grant
  with, so control opens the channel itself. Control terminates HTTPS for
  the preview domain and brings each request to the hosted machine over
  its relay socket, on a block channel whose message 1 carries a public
  grant (`arugula_e2e::preview`) instead of a block grant. **Control sees
  a public preview's traffic in plain text.** That's acceptable because the
  page is public: anyone can load it. Members' previews (above) stay end
  to end on block origins, and nothing about them changes.
- **The grant** is signed with control's hosted key, the key the project's
  delegation names and the machine already trusts for its own
  certificate, under a first line of its own (`arugula public preview
  grant v1`). It names the machine, one port, the preview's id, the
  one-off key doing the handshake and an expiry (at most ten minutes from
  the machine's now, as block grants). It names no block.
- **The machine decides too.** It serves a public grant only when it's a
  hosted machine, the grant checks out, the port isn't one of the daemon's
  own, and control, asked at most every 20 seconds (`GET
  /api/hosted/previews`, signed as the machine; the platform answers),
  says that port is public under that id now. If control can't be asked,
  nothing is served. An open channel (a WebSocket) is checked again every
  20 seconds and closed once the port isn't public. So control can't open
  any other port of the machine, and a preview turned off stops within a
  minute even while control's own cache (15 seconds) lags.
- **The id** is 32 random hex characters, made fresh each time a port is
  made public and kept while it's renewed: never the workspace's id, and a
  link that was turned off stays dead. It's in `hosted_ports.public_id`
  (platform migration 0024), with who made it public or last renewed it
  (`public_by`) and until when (`public_until`, 7 days later). Nothing has
  to run when it passes.
- **What crosses.** The request's headers, less hop-by-hop ones and what
  says who or where the visitor is (`Fly-*`, `X-Forwarded-*`,
  `Forwarded`, `Via`, `CF-*`). Its cookies are the preview's own: nothing
  of control's is ever sent to this domain, which shares no site with
  `arugula.io` or `arugula.run`. An answer's `Set-Cookie` loses any
  `Domain`, so one preview can't set a cookie for another under the
  shared domain. The machine passes requests from the preview's own pages
  as local (`Origin` and `Referer` rewritten, as for blocks), but leaves a
  request from another site with its own `Origin`, so the dev server's own
  checks still see it; and it adds no frame rules of ours.
- **Asleep.** A paused box isn't woken for a stranger: control answers
  503 with a short page that says it's asleep. A member opening the
  workspace wakes it (M182).
- **On control's side** (`crates/control/src/previews.rs`): the preview
  host reaches nothing of control's (no page, cookie, session or API). It
  takes 20,000 requests an hour per client address and 64 at once per
  preview, and its traffic is metered against the project, as block
  sockets are.
- **Who.** A project's members list its workspaces' ports; its owners and
  editors make one public, renew it and turn it off (`arugula cloud
  preview`). The operator lists every public preview and takes any down
  at once (docs/control-ops.md, "Public previews").

### Machines with no block flags

Without `--block-listen` the daemon still keeps a block site per port and
editor block (`sites::Scheme::Channel`): no listener and no name of its
own, reached only by block channels. Each refresh reads `block_domain` from
control's `/control.json`. `GET /api/host`'s features say `blocks` (its own
block sites, which its own page can show) and `block_channels` (joined to a
control with a block domain). Control's page offers ports and editors when
either is on; the daemon's own page only with `blocks`. A new port or
editor block is refused when neither is on; one restored at startup keeps
its site, since control's answer may not be in yet.

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
- **Chat's pushes (M122):** control sends these itself, encrypted to each
  subscription, since it holds the chat anyway (S41 decision 5): who, where
  and the first 200 characters ("jake mentioned you in #general", "jake
  wrote to you"), tagged `chat-<room>` with the room for the tap. A mention
  pushes, and anything in a DM; not your own posts, a room you have open or
  muted, or a room you were pushed for in the last minute.

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

### Page-only actions need a browser and a passkey

A CLI's login signs its requests with a key file on its machine, where
an agent can read it. So it's a session only where routes.toml's
`clients` lists `cli` (`clients::gate`, 401 `page_only` elsewhere). What
only the page does takes the session cookie alone (`auth::PageSession`):

- allowing the desktop app's sign-in (a CLI could otherwise allow a
  sign-in it started itself and redeem it for a cookie);
- enrolling and approving devices, and machines' joins;
- recovery codes;
- adding a passkey.

**Adding a passkey** (`passkey.rs`) takes more than a session, since
attestation is `none` and a passkey made in software can claim to verify
anyone. Never from the desktop app's window (`sessions.via = app`), and:

- an account with a passkey confirms the new one with one of its own,
  with user verification, over a challenge that hashes the account and the
  new passkey's id (`arugula add passkey v1`), and with a browser or a
  phone, one of them (a browser all the way back) also signs the account,
  the new passkey's id and the registration's challenge
  (`arugula add passkey device v1`);
- an account with none: a browser or a phone of its signs, as above, or
  the session is a GitHub sign-in from the last ten minutes. The page says
  which device it is as it starts; when that one can't sign (no browser,
  or browsers an app window approved before #1343, which don't count), the
  page sends the person through GitHub
  again first, with `prompt=select_account` so GitHub asks rather than
  redirecting by itself. A code that comes back to a browser that didn't
  start that sign-in is still exchanged, and its token revoked, so it
  signs nobody in anywhere;
- every device of the account gets a push ("A passkey was added to your
  account") naming the device that signed for it, or else how the session
  signed in and its User-Agent (what a session says its device is is a
  claim), and the passkey keeps both.

Sessions made before `via` that look like the app's window (they said
they're one of its devices, or their User-Agent names it) are marked
`app` when the column comes.

**What old app windows approved is grandfathered.** Before #1343 the
desktop app's window enrolled as a `browser` device it named ("Arugula app
on …", "illogical app"), and it approved browsers. A browser or a phone
such a window approved counts again wherever control checks "a browser all
the way back" (signing for a passkey, approving a browser or recovery
codes, removing a browser, raising a team's owner or editor, a standing
grant's signature, approving a deploy), when control stored and approved
both rows before #1343 shipped: before `2026-10-22T20:18:00Z`
(`legacy_app::APP_KIND_SHIPPED_MS`, when the v0.59.0 release run's
`control` job finished, rounded down). The window then counts as the link
just above that browser in the chain, and nowhere else: never as the
device that signs, never above what it certifies now or a recovery code it
made. "Before" is control's own record, not the certificates' `created`
(their signer's claim, which a stolen key could backdate):
`devices.created`, control's clock when the row was first stored (kept
when the same device is put again), and `devices.approved_at`, its clock
when the row became approved (so a window that asked before and was
approved after doesn't count). Removing the window or the browser later
doesn't change that, as a removed device's past approvals stand on any
chain; the device that signs must still be one the account trusts now,
and a removed device approves nothing new. Rows approved before `approved_at` came
have none; after the cutoff those were approved under #1343's rule, which
refused a window approving a browser. The platform gets the list with an
account's devices (`DeviceTrust::legacy_app_children`); a hosted box pins
the first list control sends (`Enrollment::legacy_app_children`), and each
later refresh only shrinks it, never adds; a box that has none counts no
window. A box checks a deploy's approval and a standing grant (and its
revokes, M253) against the list it pinned, as the hub does. The page gets it from `GET /api/devices` (`legacy_app_children`)
to show what control will take (Approve, recovery codes, standing grants).
Daemons' and pages' own checks of team papers keep #1343's rule.

Every WebAuthn ceremony is refused from inside another site's frame
(`crossOrigin`, `topOrigin`).

## Chat

Since M122 (S41, #978), chat is control's, in plain text: this is the
exception to "control can't read". A team's channels and DMs, the threads
about a pane or a session, and since step 6 (#1263) a project's native
issues and the threads on objects, live in control's database with their
read marks and a full-text index, so people talk while every machine
sleeps, and control pushes and searches them itself.

- **What enters control:** what people type, and the lines they choose to
  quote from a pane (the page reads the pane end to end, and the person
  picks the lines). Nothing else of the pane does: its output, input and
  scrollback stay on the end-to-end channels above.
- **Who reads a pane's thread:** whom its machine says, kept by control
  (`POST /api/daemon/chat/readers`), and never anyone control wouldn't route
  to the machine anyway. A pane chip opens only for someone who may reach
  the machine; the quote is shown either way.
- **Native issues (M194):** a project's own issues, numbered per project:
  title, body, labels, assignees, state and timeline, in plain text, and
  searched by control. An issue's comments are its thread's messages. The
  project's members read them (a personal project's owner, a team's
  roster); owners and editors open, edit and close them, and viewers read
  and comment. Anyone who reads a project exports its issues as JSON or
  Markdown.
- **Threads on objects (M196):** a room on one object, in the object's
  project: a native issue, or a linked repo's mirrored PR or issue. Those
  who read the object read its thread (for a private repo's item, only
  those GitHub says may read the repo).
- **Text agents write (M197):** an agent reads or writes only under a grant
  on its machine (`threads.read`, `threads.post`, `issues.write`), off
  until the machine's owner gives it over a project or their own DMs, and
  only over what the owner reads. Control checks it on every call, so a
  revoked grant refuses the next one. What an agent writes is stored with
  the owner as its author and the machine named, and is always shown as
  "<owner>'s agent on <machine>". Every room a grant reaches says whose
  agents can read it. Nothing wakes an agent: it reads its mentions over
  MCP, and its machine keeps no copy of the thread.
- **Live:** `GET /api/chat/live`, Server-Sent Events over HTTP with the
  session cookie, not the relay: chat isn't relay traffic, and works with no
  machine up.
- **When a roster changes:** chat is control's platform process (M138), and
  learns who is in which team, which machine is whose, who may reach it and
  which flags are on from the hub, keeping the answers until the hub's
  change feed names them, for as long as it reads the feed. A roster, lock,
  machine, share or flag change reaches who sees what within a feed tick
  plus a cache tick (1 s + 10 s: 11 s at most, about a second in practice,
  and 2 s at most while the feed is read, `arugula_platform::cache`). If
  the feed can't be read for 2 s the platform drops what it kept, and
  until it can, keeps nothing past 10 s; billing's meters, which the feed
  doesn't cover, are kept 10 s at most. A call after a write reads it at
  once: the hub passes each call on with its newest change, and the
  platform reads the feed first when it's behind; and an absence (no
  team, no such machine) is never kept.
  Grants, projects, billing and Cloud read the hub through the same cache,
  so a warm request makes no hub call; a refusal asks the hub again first,
  so a machine that just joined isn't refused. Deleting an account reaches
  it at once.
- **Kept until deleted** (S41 decision 6): an author or an owner deletes a
  message; deleting an account deletes its DMs and its machines' threads
  and blanks its messages elsewhere (issue comments and its agents' posts
  included); a disbanded team's rooms go with it. Nobody deletes a single
  issue: a project's issues and their threads go when the project does
  (with its account, or its disbanded team). The issues an account opened
  in a project that stays keep their text, naming no author.
  Litestream's copies age out with the rest of control's backups.

## What control stores

<!-- Written by `just control-tables` from crates/control/tables.toml. Edit that, not this. -->

Control's database has 110 tables in two planes ("Two planes, two trust rules"):

- **The machine plane** (43 tables) is what control needs to route and check: ids, names, addresses, public keys and signed statements. What moves between a client and a machine stays end to end, and control can't read it.
- **The platform plane** (67 tables) is what has to exist while every machine is asleep: billing and usage, chat and the threads on objects, Shared Docs, Cloud workspaces, linked repos' PRs and issues, and projects' native issues, runs and deploys. Control reads most of it, and says so here.

Every table is listed in `crates/control/tables.toml`, and a test fails when one isn't. **Holds** is what's in it: metadata (ids, names, addresses, times, counts, public keys, signed statements), ciphertext (encrypted to keys control never holds), or plaintext (what people or their agents wrote, or a secret control can read). **Gone with the account** is what deleting an account does to its rows.

### The machine plane

| Table | Holds | What | Gone with the account | Also seen by |
|---|---|---|---|---|
| `accounts` | metadata | An account: its id, its root device, its display name, and its answer about page telemetry. | yes | PostHog |
| `identities` | metadata | A sign-in identity: the provider, its id for the person, and their login. | yes | GitHub |
| `sessions` | metadata | A sign-in session: a hash of its token, the browser's User-Agent, its device, how it signed in (GitHub, a passkey, or the desktop app's hand-over), when it ends. | yes |  |
| `app_sessions` | metadata | A session on the platform page's host (app.arugula.io, M152): a hash of its token, its account, the hub session it was made from (it ends when that one does), when it ends. | yes |  |
| `issuer_clients` | metadata | A product that may ask the hub who someone is (M228): its id, its kind (oidc, or a hud box), its name, the exact addresses a sign-in may go back to, and a hud box's platform project (M237). Registered by an operator. | nothing of one |  |
| `issuer_tokens` | metadata | An access token the issuer handed a product (M228): a hash of it, its account and the client it was issued to (no other client may use it), the hub session it was made from (it ends when that one does), when it ends. | yes |  |
| `issuer_client_secrets` | metadata | A client's secret (M229, M237): a hash of it, which client it is, when it was made. A product calls the link API with it; a bound hud box, its roster and review threads. | nothing of one |  |
| `account_links` | metadata | A product's user linked to a platform account (M229): the client, the product's own id for its user and the name it gives them, the account, the numeric GitHub id both had, when. | yes |  |
| `account_link_events` | metadata | The audit of account links (M229): a link made, renamed, ended (with the consent) or imported from a product's backup by an operator (M232), the client and product user, the account, by the product, the person or an operator, when. Kept a year. | yes |  |
| `issuer_grants` | metadata | A person's yes on the consent screen (M229): the account, the product's client, when. Kept until they unlink the product. | yes |  |
| `account_link_tombstones` | metadata | When an account last unlinked a product (M229): the client, the account, when. An assertion issued until then never links. Kept a day, far past any assertion's life. | yes |  |
| `link_alerts` | metadata | The ops counter of bulk link calls (M229): per product's client and kind (unlinks, lookups), how many went over the alert line, and when last. | nothing of one |  |
| `passkeys` | metadata | A passkey's public key, its sign count, what keeps it, the browser that added it (its User-Agent, the device its sign-in said it is, and that sign-in), whether it gives PRF. | yes |  |
| `devices` | metadata | A device's or machine's certificate (public keys, signed by a device the account trusts), and its approval. | yes |  |
| `turned_down` | metadata | Which device turned a waiting device down, and when. | yes |  |
| `revocations` | metadata | A signed revocation of a device's or machine's key. | yes |  |
| `joins` | metadata | A machine asking to join: its certificate, a hash of its poll token, its addresses, the approval. | yes |  |
| `daemons` | metadata | The directory: a joined machine's id, name, addresses, team, last seen, and the version, OS and arch it runs. | yes | PostHog |
| `daemon_access` | metadata | The accounts a machine says it lets in, for routing (the machine checks for itself). | yes |  |
| `daemon_agent_access` | metadata | The accounts whose machines a machine lets in for its agents, for routing. | yes |  |
| `daemon_offers` | metadata | A machine shared with an account that hasn't said yes yet. | yes |  |
| `daemon_offer_pushes` | metadata | When an account was last pushed about a shared machine. | yes |  |
| `share_answers` | metadata | An account's yes or no to a machine shared with it. | yes |  |
| `daemon_links` | metadata | Until when a machine has a live share link, so control routes link guests to it. | yes |  |
| `project_delegations` | metadata | A project delegation (M178): an owner's device's signed word that control may enroll hosted machines into the project, the account that sent it, and the signed revocation that ended it. | yes: its own project's and its disbanded teams' go, and every one it signed. |  |
| `hosted_enrollments` | metadata | A one-time hosted enrollment (M178): a hash of its ticket or the box's sandbox id, the project, workspace and delegation, when it runs out and when it was used. | yes |  |
| `hosted_machines` | metadata | A hosted machine (M178): its certificate (public keys, project, workspace, delegation, signed with control's hosted key), its addresses, when it was last seen and removed. | yes |  |
| `push_subs` | metadata | A browser's signed push subscription: the push service's endpoint and the keys pushes are encrypted to. | yes |  |
| `settings` | **plaintext** | Control's own VAPID key pair, which signs the pushes it sends, and its hosted key (M178), which signs the hosted machines enrolled under a project's delegation. | nothing of one |  |
| `notices` | metadata | A notice for an account with no push, made from names control holds ("Ada left Acme"). | yes |  |
| `labs_flags` | metadata | The Labs flags an account or a team turned on for its machines. | yes: a team's go when the team does. | PostHog |
| `teams` | metadata | A team: its id, name, founder and founder's root, and whether it's locked. | partly: the teams it founded, or that nobody else is left in, go with it; the rest stay. | PostHog |
| `rosters` | metadata | A team's signed roster, every version: who's in it and their roles. | partly: a disbanded team's go; in a team that stays, it's in the signed roster until an owner signs a new one. |  |
| `invites` | metadata | An Ask me first invite: a hash of its code, the team, the role, its expiry, the owner's signed invite. | yes |  |
| `presigned_invites` | metadata | A one-time invite an owner's device signed, waiting to be used. | yes |  |
| `team_requests` | metadata | Someone asking to join a team: their account, root, name, the role, the invite and its proof. | yes |  |
| `daemon_watches` | metadata | A machine that checks a team's rosters because a session was shared with it. | yes |  |
| `retained_certs` | metadata | A deleted account's device certificates and revocations, to check teams' signed history it signed. | kept: made when the account goes, if a team that stays has its signatures; gone once no such team is left. |  |
| `retained_for` | metadata | Which teams a deleted account's certificates are kept for. | kept: as retained_certs. |  |
| `gone_daemons` | metadata | A hash of each machine of a deleted account, so one asking again is told why. | kept: made when the account goes: a hash of the machine's id and when, nothing of the account. |  |
| `hub_changes` | metadata | The hub's outbox for the platform (#1153): a change to an account's name, a revoked device, a roster or a lock, a machine joining, moving, leaving or being shared, or a Labs flag, by kind, the id it's about and when. Kept a week, and at most the newest 100,000. | yes: every change naming the account, its devices, its machines and the teams it disbands goes, and its deletion is one change that names nobody. |  |
| `maintenance` | metadata | The maintenance switch (M141): one row while it's on, with the seconds Retry-After says, whether the in-process platform's background work is held, and since when. Set by the operator with `arugula-control maintenance`. | nothing of one |  |
| `untold_deletions` | metadata | Accounts the hub deleted that the platform hasn't confirmed erasing yet (M140): the account id, its machines' ids, the teams it disbanded, and when. Written in the deletion's transaction, told again every 30 seconds until the platform takes it. | kept: only until the platform confirms it erased its part, usually at once; then the row goes. |  |

### The platform plane

| Table | Holds | What | Why it exists while machines sleep | Gone with the account | Also seen by |
|---|---|---|---|---|---|
| `billing` | metadata | Who pays: Stripe's customer and subscription ids, the seat item, the status. | A plan holds with every machine off, and Stripe's webhooks come whenever. | yes: a team's go when the team does. An account with a paid plan cancels it before it can be deleted. | PostHog, Stripe |
| `reported` | metadata | How many hosted sandbox minutes have gone to Stripe. | Minutes are reported hourly, whether or not anyone's machine is up. | yes | Stripe |
| `usage` | metadata | Bytes relayed per account per day, for the free plan's monthly allowance. | The month's allowance counts across every machine's sleeps. | yes |  |
| `chat_rooms` | **plaintext** | A chat room: a team's channel and its name, a DM, the thread on a pane or a session, or the thread on an object in its project (M196): a native issue (its comments), or a mirrored PR or issue, named by the object's id. | People talk with every machine asleep. | partly: its DMs, the threads on its machines and its archives go, and the rooms of teams and projects that go with it; it's taken off the rest. |  |
| `chat_members` | metadata | A DM's two people, and an archive's readers. | People talk with every machine asleep. | yes |  |
| `chat_messages` | **plaintext** | A chat message, in plain text (a native issue's comments are its thread's messages), with the lines its author quoted from a pane and who it mentions. One an agent wrote under its machine's threads.post grant (M197) keeps the machine's owner as its author and names the machine, shown as "<owner>'s agent on <machine>"; revoking the grant leaves it labelled. | People talk with every machine asleep. | partly: those in its rooms that go, go; the rest stay blank, with no author and no agent's label, and it's taken out of mentions. |  |
| `chat_fts` | **plaintext** | The full-text index of chat messages and their quotes (SQLite FTS5; on Postgres it's a generated column of chat_messages). | Control searches chat itself, with every machine asleep. | partly: as chat_messages: it's rebuilt from them, and merged when an account goes so blanked words leave it. |  |
| `chat_reads` | metadata | A person's read mark in a room, and whether they muted it. | Unread counts and pushes, with every machine asleep. | yes |  |
| `chat_events` | metadata | Chat's live events, numbered (M144): a room and a message number that changed, a read mark that moved (and whose), or that who sees what may have changed. Never a message's text. Every machine of the platform sends its open pages the ones after the last it sent, and a page that dropped resumes from them. The newest 10,000 are kept. | Open pages hear of messages, edits and read marks through control alone, with every machine asleep. | partly: its read marks' events lose its name (they stay as nameless markers, so the numbers hold); events about rooms name no one and go with the oldest. |  |
| `chat_readers` | metadata | Who may read the thread on a pane or a session, as its machine last said. | A pane's thread stays readable while its machine sleeps. | partly: its machines' rows go, and it's taken off the rest. |  |
| `doc_links` | metadata | A Shared Doc link: its id, the machine that shared it, its version, size and parts, and how often it opened. | A shared Doc opens while the machine that shared it sleeps. | yes |  |
| `doc_link_parts` | ciphertext | A Shared Doc's snapshot, in parts, encrypted to the key in the link's fragment. | A shared Doc opens while the machine that shared it sleeps. | yes |  |
| `projects` | metadata | A project: its id, the team or account it belongs to, its name, whether it's the default one, the last number it gave a native issue, and its issue home (native or GitHub) if someone chose one. | Cloud workspaces, team channels, native issues and secrets belong to one, and they exist with every machine asleep. | partly: its personal project goes, and the default projects of teams that go with it; other teams' stay. |  |
| `team_members` | metadata | Who is in each team as its latest signed roster says: account, role, name. A copy for platform queries, following the hub's change feed; the signed roster stays the authority over machines. | What's shared with a team on the platform (its channels, later its projects) needs its members while every machine is asleep. | yes |  |
| `grants` | metadata | A grant: a subject (a machine, or an agent grant token, M230), a power over a scope (an account or a project), who gave it, when, and when it was revoked. The powers: cloud.drive (its agents drive its owner's Cloud workspaces; given on join), and for its agents in conversations (M197) threads.read, threads.post and issues.write, and over a project's runs (M216) runs.read, runs.start and runs.command, off until its owner gives one over a scope the owner reads. | Control checks it on the Cloud workspace routes and wherever an agent reads or writes threads and issues or starts and reads runs, which work with every other machine asleep. | yes: grants to its machines, over it, or that it gave. |  |
| `agent_tokens` | metadata | An agent grant token (M230): what an agent with no machine shows the platform's MCP endpoint. Its id, the account that minted it (whose agent it is), its name, the SHA-256 of the token (never the token, which is shown once), when it was made, last used and revoked. Its powers are grants (`subject_kind` agent). Minted only from a browser on the platform page: the browser it came from (its User-Agent). | An agent with no machine calls the platform's MCP endpoint, which finds its token here, with every machine asleep. | yes: the account's tokens go, with the grants it gave them. |  |
| `agent_token_calls` | metadata | Every tool call a token's agent made on the platform's MCP endpoint (M230), for its owner: the token, the tool, the project, how it went (ok, refused, failed), a line of what it made or why not, and when. Never an argument's value: a secret's name, never the secret. | Its owner reads what their agent did on the platform page and with the CLI, with every machine asleep. | yes: the account's tokens' calls go with them. |  |
| `planter_imports` | metadata | planter's import (M232): each repo planter's boxes were planted from that the operator's import linked to a project (or found linked there already), by planter's user id (the numeric GitHub id) and the repo's name: GitHub's repo id, the project, the platform account it was imported for, and when. Never updated, so a repo unlinked since is never linked again by the import. | It's the record of a one-off import, read by the import when it's run again, and kept with the repo links it made. | yes: what was imported for the account goes; the repo links stay as linking's own rows do. |  |
| `hub_feed` | metadata | How far the platform has applied the hub's change feed to projects, team members and grants: the last change's number. | A machine's joining or leaving reaches its grant through the feed, whether or not any machine is up. | nothing of one |  |
| `github_webhook_deliveries` | metadata | Each delivery the GitHub App's webhook took: GitHub's delivery id, the event's name and when it came. Kept 7 days, so a redelivery is recognised after a restart. | GitHub's webhooks arrive whether or not any machine is up. | nothing of one | GitHub |
| `github_installations` | metadata | Where the GitHub App is installed: the installation id, the GitHub user or organization (id, login, type), the permissions it was granted, and when it was suspended or deleted. | Linked repos are read through the installation with every machine asleep. | nothing of one | GitHub |
| `linked_repos` | metadata | A GitHub repo linked to a project: GitHub's repo id, its name, whether it's private, the installation, who linked it and when, whether the project mirrors its content, when it was unlinked and when its rows are purged, and its poller's lease, last backfill and reconcile, failures in a row, and where a turn that ran out of pages stopped. | Its PRs and issues are kept current by webhooks and a poller on the platform, and read from it, with every machine asleep. | partly: it stops naming the account as who linked it. Repos linked to its personal project are unlinked, and their rows purged within 24 hours; a team project's stay linked. | GitHub |
| `forge_items` | **plaintext** | A linked repo's PR or issue, without its body: number, kind, state, title, labels, author, assignees, requested reviewers, draft, merge state, head and base, and GitHub's times. | PRs and issues are listed, searched and put in inboxes from here, with every machine asleep. | nothing of one | GitHub |
| `forge_checks` | metadata | A linked repo's check runs and commit statuses per head commit: name, status, conclusion, link and time. | A PR's checks show, and a failure reaches an inbox, with every machine asleep. | nothing of one | GitHub |
| `forge_events` | metadata | What happened to a linked repo's PR or issue, as metadata: reviews and their state, review requests, state changes, with who and when. No text. | Inboxes are computed from them with every machine asleep. | nothing of one | GitHub |
| `forge_content` | **plaintext** | A linked repo's PR and issue bodies, comments, reviews' text and review comments (with their file, line and the comment they answer), with author and times. Only for a repo whose project turned on content mirroring (M166); turning it off deletes them at once, and an unlink or an uninstall purges them within a day. | A project that opted in searches PR text with every machine asleep. | nothing of one | GitHub |
| `drafts` | **plaintext** | A comment, review or new issue for a linked repo, waiting for a person to send it: the project, the person it's for (and the agent and machine that wrote it, if one did), the target, the text, its state, and where it landed once sent. | A person sends a draft from a phone with every machine asleep. | yes: its drafts, and those in projects that go with it. | GitHub |
| `inbox_items` | metadata | A member's inbox: an item (a linked repo's PR or issue, or a native issue), why it's there (a review requested, failed checks, changes requested, a mention, an assignment, merged or approved and green, a draft waiting, a mention in its thread; on a native issue an assignment or a mention, M202), and when it came, was seen and was done. A linked repo's item only for members GitHub lets read it. No text: a mention's words stay in its message. | The inbox is computed and pushed by the platform with every machine asleep, and mentions and assignments reach it as they happen. | yes |  |
| `github_token_mints` | metadata | The audit log of every GitHub token the platform minted: installation or user token, the installation and repo, the permissions, who it was for, what for, and when. Never the token. | Tokens are minted for reads, ingest and sends with every machine asleep. | kept: its mints stay in the audit log, naming it, so a token can be traced after the account goes, for as long as the log keeps a mint: a year, pruned hourly. | GitHub |
| `forge_refetch` | metadata | What the forge mirror's poller reads again from GitHub at its repo's next turn (M164): a linked repo's id and an item's kind and number, or a head commit's SHA, that two answers gave in the same second and disagreed on, and when. | Webhooks arrive and the poller runs with every machine asleep, and a row two answers disagreed on is put right within a turn. | nothing of one | GitHub |
| `forge_access` | metadata | What GitHub said about a person's access to a repository (M165): their numeric GitHub id, the repository's name and owner, why not if not, and when it was asked by the database's clock. Kept 10 minutes, so every machine of the platform gives the same answer, to a read of a private repo's items, a daemon's live updates and a hosted box's token. | Private repos' items are read from the platform with every machine asleep, each read checked against the reader's own GitHub access. | kept: an answer names a GitHub id, not an account: it stops being used after 10 minutes, and goes at the next hourly prune. | GitHub |
| `forge_access_drops` | metadata | When a GitHub event last changed who may read a repo (M165): a repository's name, an owner's login or a person's GitHub id, and when, by the database's clock. Every `forge_access` answer asked before it is dropped. | GitHub's webhooks arrive whether or not any machine is up, and a removed collaborator is cut off at once on every machine. | nothing of one | GitHub |
| `issues` | **plaintext** | A project's native issue, in plain text: its number in the project, title, body, state and close reason, who opened it, labels, assignees (accounts), where it was exported to on GitHub, and its times. One an agent opened under its machine's issues.write grant (M197) keeps the machine's owner as its author and names the machine. Its comments are its thread's chat messages. | A project's issues are read, written, searched and put in inboxes by control with every machine asleep, so control reads their text (aru-088). | partly: the issues of projects that go with it go. The rest stay, with their text, naming no author, and it's taken off assignees. |  |
| `issues_fts` | **plaintext** | The full-text index of native issues' titles and bodies (SQLite FTS5; on Postgres it's a generated column of issues). | Control searches a project's issues itself, with every machine asleep. | partly: as issues: it's rebuilt from them. |  |
| `issue_events` | **plaintext** | A native issue's timeline: opened, edited, closed, reopened, labelled, assigned, exported, with who (an account, and the machine when its agent did it) and when, and a small detail: an edit's old title (the text that makes it plaintext), a close's reason, the label or assignee, where it was exported to. | An issue's timeline shows with every machine asleep. | partly: those of issues that go, go. The rest stay, naming no actor, and it's taken out of assignment events. |  |
| `platform_jobs` | **plaintext** | The platform's leased background work (M208): one job per workspace, run, deploy or schedule, with its kind, what it was asked, how far it got, its state, its failures in a row and the last one's error, when it's next due, and which machine's lease holds it until when. | Runs, deploys and schedules move on with every machine asleep, and a platform machine that dies mid-step leaves its job to another. | partly: a job goes with what it's for (a project's run, deploy or schedule) when that goes. Schedules use it (M214), and runs and deploys will. |  |
| `runs` | **plaintext** | A project's run (M210): the workspace it runs on, what started it (a schedule, a forge event, or someone by hand: an account, and the machine when its agent did), the S37 operation it runs, its hash and the daemon it names (the only one whose reports move the run), its state (one of S37's eight), the standing grant it may run under, its times, exit code, the chant ledger entry a release run wrote, and what a trigger made it for (a schedule's tick or a rule's event, M214). The operation is in plain text: its command, directory and instruction (S37 decision 6). | Scheduled and event runs start, and their state is shown and checked, with every person's machine asleep. | partly: the runs of projects that go with it go. The rest stay, naming no one who started them. |  |
| `run_specs` | **plaintext** | A project's run specs (M214): a name, the hosted workspace it runs on, and what it runs: a command (its arguments) or an instruction to an agent (and the agent's recipe), and the directory; who made and last changed it, and when. The command and instruction are in plain text, as a run's operation is. | Schedules and event rules start a spec's runs with every person's machine asleep. | partly: the specs of projects that go with it go, with their schedules and rules. The rest stay, naming no one who made or changed them. |  |
| `run_schedules` | metadata | A run spec's schedules (M214): a cron expression and its time zone, whether it's on, its next tick, the tick it last fired for, the run that made and why a tick made none, and who made it. Its job on platform_jobs fires it. | The platform schedules and the hosted workspace executes: a schedule ticks with every person's machine asleep. | partly: those of specs that go, go (with their jobs). The rest stay, naming no one who made them. |  |
| `run_rules` | metadata | A run spec's event rules (M214): a linked repo's event (a PR opened, a check failed, a push), the repo and branch it's on or any, whether it's on, whether it takes events from forks and outside the repo, its hourly cap, and who made it. | A GitHub event starts a spec's run with every person's machine asleep. | partly: those of specs that go, go. The rest stay, naming no one who made them. |  |
| `run_events` | **plaintext** | A run's history, append-only (never changed, and never deleted while its run is there): each change of state, numbered in the run, when, a small detail (the daemon's exit code, control's reason for failing it, the approval an approved run was approved by), and, when the run's daemon reported it, that daemon's id and its signature over the change. | A run's history shows, and is checked, with every machine asleep. | partly: those of runs that go, go. The rest stay as written: they name daemons, not people, and a signature names its signer. |  |
| `run_artifacts` | metadata | What a run left behind: each file's name, size, digest and key in the run-log bucket. The files themselves are in the bucket (M213), not here. | A run's artifacts are listed and fetched with every machine asleep. | partly: those of runs that go, go; their files expire from the bucket. Every row goes after 30 days, as the bucket's files do. | The run-log bucket |
| `run_log_chunks` | metadata | Where a hosted run's log is in the run-log bucket (M213): each chunk's number, its offset and size in the pane's output, its SHA-256 and its key. The log itself is in the bucket, which the platform reads. | A run's log is read on the platform page and by the CLI with every machine asleep. | partly: those of runs that go with their projects, go. Every row goes after 30 days, as the bucket's files do. | The run-log bucket |
| `run_upload_urls` | metadata | Each upload URL the platform signed for a hosted run (M213): its key in the run-log bucket, log or artifact, its size, when it was signed and when it expires. They count against the run's caps and the project's daily bytes while they're out. | Hosted runs send their logs while every person's machine is asleep, and the caps hold then too. | partly: those of runs that go with their projects, go, and their files with them. Every row goes a day after its URL expires; the file at a key nobody said they used goes from the bucket then. |  |
| `deploy_approvals` | metadata | A deploy's approvals and denies (M218): S37's work approval over the deploy's operation (its environment, plan's digest, source and planned event's hash), signed by a project owner's device. Which deploy, its event in the log, the decision, the operation's hash, the approver's account, the device that signed and its name then, by when the apply must start, the signature, the passkey assertion the hub checked over the approval (credential id, clientDataJSON, authenticatorData, signature), and when; and when a start used it up, by which run (M219). | An owner approves a deploy from their phone and a driver run checks it, with every machine asleep. | partly: those of projects that go with it go. The rest are kept as signed, the account's id in them included, as its events are: rewriting one would break its signature. |  |
| `deploy_runs` | metadata | The run that applies an approved deploy (M219): which deploy and run, the approval it applies, the hosted workspace it runs on, the environment's driver then, when it was made, when its machine was given the environment's secrets (once), and when the project's owners were told its apply failed (once). No secret: those are sealed in project_secrets and given only to the run's machine, while it runs. | A deploy's run applies it on the project's hosted workspace, and its log takes the deploy's start only from that run, with every owner's device asleep. | partly: those of deploys that go with its projects go. |  |
| `platform_migrations` | metadata | Which of the platform's numbered migrations (M142) its database has: each one's number, name, checksum, whether it's additive (older builds may start past it) and when it ran, or that it was found already in place on a database made before migrations. | The platform checks its tables are the shape its code expects before it serves anything, whether or not any machine is up. | nothing of one |  |
| `hosted_workspaces` | metadata | A hosted workspace (M179): its project, name and spec (clone URLs, packages, its setup script and the names of the env vars it takes), the spec's hash and the one its box was built from, the provider, the template and sandbox id of its box, its state, what was asked of it, the create's journal, when its enrollment runs out, the hosted machine it enrolled as, the digest of its egress rules (M185), the last error, the worker's lease, who made it and when; and how its box's setup goes (M183): its status, its step, the pane it runs in, and a failed setup's reason as a fixed code, never the pane's text. | The platform makes, wakes, pauses and deletes hosted workspaces' boxes itself, and the page lists them, with every machine asleep. | partly: it stops naming the account as its maker. Workspaces in its personal project and its disbanded teams' projects are deleted, boxes and all, by the worker; their rows stay, marked deleted. | E2B |
| `hosted_workspace_secrets` | **plaintext** | A secret a hosted workspace takes, by name, and where its value is: sealed here with its project's key (project_keys), or in the provider's secrets broker. | A hosted workspace's box is made and rebuilt with every machine off. | partly: they go with their workspace, when the worker deletes it. | E2B |
| `project_keys` | **plaintext** | A project's data key (M209), which seals that project's secrets, wrapped by the master key control holds apart from the database; which master key wrapped it (a hash of it), when it was made and when it was last re-wrapped. Rotating the master key re-wraps these and nothing else. | A project's secrets are opened for its workspaces and deploys with every machine off. | partly: a personal project's keys go with it, and so do a disbanded team's; other teams' stay. |  |
| `project_secrets` | **plaintext** | A project's secret that has no table of its own, such as a deploy environment's (M217), by name, sealed with the project's key. And a project's own secrets (`project/<NAME>`, M230), set by name by agents with no machine, each with who last set it and through which agent token. | Deploys run with every machine off. | partly: a personal project's secrets go with it, and so do a disbanded team's; other teams' stay. |  |
| `hosted_ports` | metadata | A port of a hosted workspace (M186), its label, and while it's public (M187) its preview id (32 random hex characters, the label of its host under the preview domain), who made it public or last renewed it, and until when. | Previews are served and expire with every machine asleep. | partly: it stops naming the account as who made a port public. They go with their workspace. |  |
| `hosted_transcripts` | **plaintext** | A closed agent block's transcript from a hosted workspace's box (M184): its project, workspace, machine and block, the agent, its title and working directory, when it ran, and its text (prompts, the agent's messages and tool calls, cut to 768 KiB), kept read only. Transcripts copied from a beta Cloud workspace's tabs when it moved off Managoat (M189) are kept the same way. | A project's people read what its hosted workspaces' agents did after the box is paused, rebuilt or deleted. | partly: those of its personal project and its disbanded teams' projects go. A team's stay with its project: its other people read them. |  |
| `hosted_usage` | metadata | When a hosted workspace's box ran (M188): its workspace, project and box, its size (vCPUs and memory), and each running interval's start and end. | The platform meters boxes from their lifecycle, which runs with every machine asleep, and the operator reads what they cost. | nothing of one: it names no account: a workspace's project and box ids only. It stays when the workspace goes: it's what E2B billed. | E2B |
| `hosted_usage_provider` | metadata | What E2B says a hosted workspace's box ran (M188), from its lifecycle events: how many runs, their milliseconds, vCPU-milliseconds and MiB-milliseconds, and when it was read. | The platform checks its usage against E2B's record with every machine asleep. | nothing of one: it names no account: a workspace's project and box ids only. | E2B |
| `environments` | metadata | A project's environment (M217): its name in the project, the driver that applies its deploys (chant-fly, apps-platform, arugula-release), the names of the secrets a driver run gets (never their values: those are sealed per project, M209), its driver's settings (M220: for chant-fly, the Fly app and where its chant op is; M221: for arugula-release, the bump and the lane; nothing secret), and its times. | Deploys are planned, approved and applied through control with every machine asleep. | partly: those of projects that go with it go. |  |
| `deploys` | metadata | A deploy to an environment (M217): what it deploys (a commit's sha or an artifact's digest, and where), the plan's digest, its state (S37's, with planned and stale), where its plan is in the environment's log, the run that applied it, who planned it (an account, and the machine when its agent did), and its times. | A deploy waits for an owner's approval and is applied by a driver run on the platform, with every machine asleep. | partly: those of projects that go with it go. The rest stay, naming no planner. |  |
| `deploy_events` | metadata | An environment's deploy log (M217): append-only, a hash chain per environment that anyone can verify. Each event says which deploy, what happened (planned, requested, approved, denied, canceled, expired, started, succeeded, failed, staled), who (an account, an agent's owner and machine, a run, or the platform), when, a small detail (the source and plan's digest, the run, the approval), and its hash and the one before. | Approving a deploy checks its environment's log (the stale rule), with every machine asleep. | partly: those of projects that go with it go. The rest are kept as written, the account's id in them included: an event is never rewritten, and rewriting one would break its chain. The id names no one once the account is gone, so they show as a deleted account. |  |
| `project_inference_keys` | **plaintext** | A project's own inference key (M185), one per provider (Anthropic, OpenAI), sealed with the project's key (project_keys) or, until it's re-sealed, the master key control holds apart from the database, who set it and when. Control opens it only to write it into the project's boxes' egress rules, where E2B's proxy sets it on their requests to the provider, and (Anthropic's, M231) to set it on the calls the inference broker makes for the project, or, a personal project's, for its account; it's never answered back. | A hosted workspace's agents run on the project's key with every other machine asleep, and the page shows whether it's set. | partly: it stops naming the account as who set a key. Keys of its personal project and its disbanded teams' projects are dropped by the platform's sweep. | E2B, Anthropic |
| `hosted_turns` | metadata | An agent turn on a hosted workspace's box (M185): the workspace, the provider, whose key paid (the project's or the platform's), the account whose caps a platform turn counts against, and when it started and ended. Kept 30 days. | Caps on the platform's key are checked when a hosted box starts a turn, with every other machine asleep. | partly: it stops naming the account; the turns stay until they're 30 days old. |  |
| `inference_calls` | metadata | A call the inference broker made to Anthropic for a project (M231): the project, the account it was for and the registered client that made it (studio's lobby), whose key paid (the project's, an account's own, which account's, or the platform's), whether it's running, settled or kept at its reservation, the model, the provider's status, the tokens and web searches it reported, what it was reserved at and what it cost at list prices, and when it started and ended. Never a prompt or an answer. Kept 90 days. | The broker checks an account's month on the platform's key against its cap before each call, and a project's members see what its calls cost, with every machine asleep. | partly: it stops naming the account, as whose call and as whose key paid; the calls stay until they're 90 days old. | Anthropic |
| `inference_consents` | metadata | An account letting a registered client of the inference broker act for it (M231): which account, which client, and since when. Off until the account turns it on. | The broker checks it on every call a client makes, with every machine asleep. | yes |  |
| `client_consents` | metadata | An account letting a registered client (studio's lobby) see its projects and its role in each, and keep the client's apps in the ones it edits (M236): which account, which client, what, and since when. Off until the account turns it on; unlinking the product ends it. | The client asks on the account's behalf whenever someone opens or lists its apps, with every machine asleep. | yes |  |
| `client_apps` | metadata | A registered client's app (studio's `app-xxxxxxxx`) and the project it's in (M236), so its owners are the project's members: the client, the app's id and title, the project, and who put it there and when. | The client reads it to list and open its apps, with every machine asleep. | partly: deleting an account forgets it as `added_by`; its personal project's apps go with the project. |  |
| `client_secret_grants` | metadata | A project's own secret a registered client may send for one of its apps, to one host (M236): the project, the client, the secret's name, the app, the host, who asked (an owner, through the client) and when, who let it and when, and how many reads and the last. Never a value. | Studio's egress broker reads the secret on its apps' behalf, with every machine asleep. | partly: deleting an account forgets it as `granted_by` and `requested_by`; its personal project's grants go with the project. |  |
| `client_secret_events` | metadata | What happened to a client's reach into a project's secrets (M236): asked for, granted and revoked by an owner, read by the client for one of its apps and host, ended when the app moved or went, with when. Never a value. Kept 90 days. | Written as the client reads, with every machine asleep, for the project's owners to see. | partly: deleting an account forgets it as the owner who granted or revoked; its personal project's events go with the project. |  |
| `run_grants` | metadata | A project's standing grant for runs (M211): the bytes a project owner's device signed (that the platform may start and instruct runs on the project's hosted workspaces, under its delegation, until it expires) and the signature, its id, the delegation and signing device it names, when it was made and when it expires, who sent it and when, and when a newer grant replaced it; and once revoked (a revoke takes back every grant of the project under its delegation made by its time), the revoke's signed bytes, signature and time, when the platform took it, and whether the runs going were to stop. | Scheduled and event runs start under it with every owner's device asleep, and the platform sends it to the hosted machine with each start. | partly: it stops naming the account as who sent it. The grants of its personal project and its disbanded teams' projects are deleted. A grant its devices signed stays, and hosted machines stop accepting it once those devices aren't an owner's. |  |

### Who else sees it

- **Litestream's bucket:** Litestream copies the whole database to a bucket (Cloudflare R2, for the hosted control), so every table is there too, for as long as control's backups are kept. On Postgres, the platform's tables go there in its nightly dump instead: encrypted, kept 30 days. Secrets are there sealed, and their key isn't.
- **The run-log bucket:** Runs on hosted workspaces (and only those) send their logs, the whole of their pane's output, and the files they leave as artifacts to a bucket in Cloudflare R2 (M213), through URLs the platform signs for one run at a time. The platform reads them and serves them to the project's members, with every machine asleep. The bucket deletes them after 30 days, and the platform forgets their rows then. A person's own machines never send any.
- **E2B:** E2B runs hosted workspaces' boxes, so it processes their contents: it holds each box's disk and memory (the repos, files, terminals and agent conversations in it) while the box lives, paused too, the env and metadata each is made with (the workspace, project, release and template ids), its egress rules (which hold projects' own inference keys, and name the platform's), and the platform's inference keys in its secrets broker. It records each box's runs (when, how long, its size), which it bills by and the platform reads back to check its own usage.
- **Anthropic:** The inference broker (M231) forwards Claude calls to Anthropic for a project's members and for registered clients (studio's lobby): Anthropic gets each call's prompt and answer (they pass through the platform, which keeps neither), and the key that pays (a project's or an account's own, or the platform's), and bills that key's owner.
- **PostHog:** PostHog gets product events, when control has a key for it: account, team and machine ids, a machine's version, OS and arch, plans, Labs flags and counts. No names, logins, addresses or content.
- **GitHub:** GitHub signs people in: it knows the identities control keeps for it. Through the GitHub App, linked repos' PRs, issues, checks and content come from it, it makes every token control mints and sees each one used, every draft sent goes to it, and it sends the webhooks.
- **Stripe:** Stripe bills, when control has a key for it: it holds the customers and subscriptions control keeps ids for, and gets sandbox minutes.

### Never stored

- private keys of any device or machine: Noise, signing, passkeys
- terminal bytes, snapshots, logs, scrollback; the lines of a pane nobody quoted into chat. One exception: a run on a hosted workspace sends its log to the run-log bucket (below), which the platform reads
- link keys (a link's fragment), and Shared Docs' keys
- the push payloads machines send, in the clear
- a person's own credentials: GitHub's OAuth token (used once at sign-in), the one-use user token a send is made with (used for one call, then revoked), forge tokens

<!-- End of what `just control-tables` writes. -->

Logs follow the same rule: nothing a daemon sends inside a stream is
logged, and chat's text isn't logged either.

## Open items

These are pending the phone runs (see the spike README):

- PRF on real devices (the go/no-go table above);
- the cellular round trip through the relay, against the relay's 30 ms budget;
- link-preview behaviour in Slack and iMessage.
