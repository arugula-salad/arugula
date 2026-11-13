---
schema: 1
id: "aru-131"
title: "The iPhone app is a browser device, and sees one API"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "What kind of device is a native iPhone app, and how does it reach control without breaking the clients we have?"
options:
  - id: "a"
    label: "A browser-kind device that signs requests, only on routes marked native, behind a flag"
    how: "The app's certificate is `Kind::Browser`, so no wire change. Control accepts a browser-kind signature as a session only on routes whose `routes.toml` row lists `native`, with a flag that's off by default. Every call goes to control's own site, and the hub forwards the platform's."
    tradeoff: "The app can approve new browsers and be an account's first device, as a phone's web page can today, and browser-kind signatures are a new way in that each route has to be reviewed for."
  - id: "b"
    label: "A new `phone` kind"
    how: "A new `Kind` variant with its own rules in `Kind::certifies`, hidden from older clients by a feature name, as `app` certificates are (`crates/control/src/compat.rs`)."
    tradeoff: "A cleaner line, but a daemon that hasn't updated refuses the phone, and one unknown certificate that slips past the filter fails a daemon's whole trust answer."
  - id: "c"
    label: "Reuse the `app` kind"
    how: "The filter for older clients exists already, and an `app` device can't certify a browser."
    tradeoff: "An `app` key may be a program's on that computer, so `app` signatures can't be given a session's reach without widening what an agent beside the desktop app could do."
choice:
  option: "a"
  reason: "It's the only option every daemon in production accepts today, and a phone's web page is already this kind, so the app has no more power than the page it replaces (Jake, 2026-11-02, Arugula native #597)."
rejected:
  - option: "b"
    why: "Production has seen daemons nine versions apart: the phone wouldn't reach them until they updated, for a line the phone's own web page doesn't draw."
  - option: "c"
    why: "It would either widen what the desktop app's key can do or give the iPhone the desktop app's limits for a threat it doesn't have."
supersedes: []
evidence:
  - title: "Arugula native #597 (this record)"
    url: "https://app.arugula.io/issues/iss_a826d13baddfc3b40807ed7f5b5680ee"
  - title: "Arugula native #600 (the hub's change)"
    url: "https://app.arugula.io/issues/iss_fe2a605380681c9cfa14c3688d0a9fb9"
decided_by: "jhgaylor"
decided_on: "2026-11-02"
reviews: []
constrains:
  - "aru-075"
  - "aru-076"
  - "aru-079"
  - "aru-126"
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/clients.rs"
  - "path:crates/control/src/compat.rs"
  - "path:crates/control/routes.toml"
---

# The iPhone app is a browser device, and sees one API

The first native app is the iPhone's, and it joins as a `browser`-kind device
(Jake, 2026-11-02, Arugula native #597). No `Kind` is added.

- **Why no new kind.** One certificate a daemon can't read fails its whole
  answer (its trust, its team's, a join's), and an older CLI reads the list as
  empty and thinks it was removed (`crates/control/src/compat.rs`). Daemons in
  production have been nine versions apart ("Platform paths stay on
  control.arugula.io forever", aru-076).
- **What it may do.** What a phone's web page may do: reach machines, approve
  new browsers, and be an account's first device. A person's approval is still
  a passkey over the action's bytes (aru-126), so holding this key doesn't
  approve a deploy.
- **Its keys.** Device keys stay in the iOS keychain, on this device only and
  never synced. They're X25519 and Ed25519, which the Secure Enclave doesn't
  hold. No agent runs on an iPhone, which is why the reason a CLI's key is kept
  from the page's routes doesn't apply here.
- **Its name.** The certificate's name must not start with "arugula app" or
  "illogical app": `Cert::from_app` reads those as an old desktop app window.
- **How it calls control.** It signs each request as the CLI does. Control takes
  a browser-kind signature as a session only on a route whose `routes.toml` row
  lists `native`, and only with the flag on. A route with no row takes none, and
  what the CLI and the `app` kind may do doesn't change. Browsers' pages keep
  using the cookie.
- **Only a device paired as an app.** A browser's key is in a page, so it signs
  nothing even with the flag on: otherwise a key taken from a page would work
  from anywhere, on top of what it already reaches. Control records which
  devices paired by code as apps (`native_devices`), and that's the only kind of
  browser whose signature counts (`auth::native_cert`).
- **The first scope.** Chat, projects, issues, boards, sessions and approvals.
  Billing and account security stay on the web.
- **One API, one product.** The app sends everything to control's own site, and
  the hub forwards the platform's ("Platform paths stay on control.arugula.io
  forever", aru-076). The hub and the platform stay two processes (aru-075), and
  the two web origins stay as they are ("The platform page is on
  app.arugula.io, not on the keys' origin", aru-079). That rule is about scripts
  on a web page, and the app runs no page with access to its keys. It shows
  machines and projects in one window.

Where: `crates/e2e/src/cert.rs`, `crates/control/src/auth.rs`, `crates/control/src/clients.rs`, `crates/control/routes.toml`.
From: Arugula native #597, #600, the Native iPhone app board.

**Amended by aru-141, "Machines trust control: the hub's tokens replace device keys"** (Jake, 2026-11-13). With no device kinds, the iPhone app is a signed-in client like the web page: it asks the hub for machine tokens and keeps no device keys.
