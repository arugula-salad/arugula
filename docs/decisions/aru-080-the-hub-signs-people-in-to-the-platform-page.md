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
    how: "A visitor without a session goes to control.arugula.io/signin/platform?return=…; the hub uses its session or signs them in as usual, then redirects back with a code that is short-lived, single use, bound to the return URL and a PKCE verifier. The platform swaps it for a host-only session cookie. Signing out or ending sessions on the hub reaches the platform through the change feed."
    tradeoff: "One redirect on first visit, and the platform keeps session rows of its own that the hub's feed has to end."
  - id: "b"
    label: "One cookie for .arugula.io"
    how: "The hub sets its session cookie on the parent domain so both hosts read it."
    tradeoff: "Every arugula.io host, the site and block origins included, would carry the session; a bug on any of them leaks it."
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
  - "path:crates/platform"
---

# The hub signs people in to the platform page

The hub signs people in to `app.arugula.io` (Jake, 2026-10-20, #1171):
1. A visitor without a session is sent to
`control.arugula.io/signin/platform?return=…`.
2. The hub uses its own session, or signs the person in with a passkey or
GitHub as usual.
3. The hub redirects back with a one-time code: short-lived, single use,
bound to the return URL and a PKCE verifier.
4. The platform swaps the code for a host-only session cookie on
`app.arugula.io`.

Someone already signed in on the hub sees no prompt. Passkeys stay bound to
`control.arugula.io` ("The hub stays the identity provider; passkeys are
bound to control.arugula.io"). Signing out on the hub, and "End all
sessions", reach the platform through the change feed, and its sessions end
within a feed tick. A replayed code or a mismatched return URL fails.
Where: `crates/control/src/auth.rs`, `crates/platform/`.
From: #1171, M146 (#1172), M152 (#1178).
