---
schema: 1
id: "aru-080"
title: "The hub signs people in to the platform page"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1172"
question: "How does someone signed in on control.arugula.io get a session on app.arugula.io?"
options:
  - id: "a"
    label: "A redirect through the hub with a one-time code, swapped for a host-only cookie on app.arugula.io"
    how: "A visitor without a session goes to control.arugula.io/signin/platform?return=…; the hub uses its session or signs them in as usual, then redirects back with a code that is short-lived, single use, bound to the return URL and a PKCE verifier. The page swaps it, at the hub on app.arugula.io, for a host-only session cookie there; the hub keeps that session, made from its own, and ends it when its own ends."
    tradeoff: "One redirect on first visit, and a second kind of session row in the hub."
  - id: "b"
    label: "One cookie for .arugula.io"
    how: "The hub sets its session cookie on the parent domain so both hosts read it."
    tradeoff: "Every arugula.io host, the site and block origins included, would carry the session; a bug on any of them leaks it."
  - id: "d"
    label: "The platform keeps the platform page's sessions, ended through the change feed"
    how: "The platform swaps the code and keeps the session; signing out on the hub reaches it as a change in the feed."
    tradeoff: "The platform would see and check a cookie, which it never does (\"The hub stays the identity provider\"), and a sign-out would take a feed tick to land."
  - id: "c"
    label: "Passkeys on app.arugula.io too"
    how: "A second relying party, or arugula.io as the RP id."
    tradeoff: "Everyone enrolls again, and two sign-ins split one account."
choice:
  option: "a"
  reason: "Passkeys stay bound to control.arugula.io (\"The hub stays the identity provider\"), the session never leaves the host that set it, and someone already signed in on the hub sees no prompt (Jake, 2026-10-20, #1171)."
rejected:
  - option: "b"
    why: "A parent-domain cookie reaches hosts that have no reason to see a session."
  - option: "c"
    why: "Re-enrolls every passkey for nothing the redirect doesn't already give."
  - option: "d"
    why: "Breaks the step-2 rule that the platform trusts only the principal the hub signs, for a slower sign-out (M152, #1178)."
supersedes: []
evidence:
  - title: "#1171"
    url: "https://github.com/arugula-salad/arugula/issues/1171"
  - title: "#1172"
    url: "https://github.com/arugula-salad/arugula/issues/1172"
  - title: "#1178"
    url: "https://github.com/arugula-salad/arugula/issues/1178"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/app_signin.rs"
  - "path:web/src/platform/signin.ts"
---

# The hub signs people in to the platform page

The hub signs people in to `app.arugula.io` (Jake, 2026-10-20, #1171):
1. A visitor without a session is sent to
`control.arugula.io/signin/platform?return=…&challenge=…&state=…`.
2. The hub uses its own session, or signs the person in with a passkey or
GitHub as usual (control's page, `#platform-signin=`, then back).
3. The hub redirects back with a one-time code in the fragment:
short-lived (a minute), single use, bound to the return URL and a PKCE
verifier (S256) only the platform page holds.
4. The page swaps the code, at the hub on `app.arugula.io`
(`POST /api/app/session`), for a host-only session cookie there
(`__Host-arugula_app` over HTTPS).

Someone already signed in on the hub sees no prompt. Passkeys stay bound to
`control.arugula.io` ("The hub stays the identity provider; passkeys are
bound to control.arugula.io").

The hub keeps the platform page's sessions (`app_sessions`), not the
platform: the platform never sees a cookie and trusts only the principal
the hub's gateway signs, on `app.arugula.io` as on control's site (M152).
Each is made from one hub session and ends with it, in the same
transaction: signing out on the hub, "End all sessions", ending that
session in Sign-in and account, deleting the account, and the hub
session's expiry end the platform page's at once, with no feed tick. On
`app.arugula.io` that session is good only from the platform page's own
origin, and the hub's own cookie means nothing there. A replayed code, a
mismatched return URL or the wrong verifier fails.
Where: `crates/control/src/app_signin.rs`, `crates/control/src/platform_host.rs`, `crates/control/src/auth.rs` (`Session`), `web/src/platform/signin.ts`.
From: #1171, M146 (#1172), M152 (#1178).
