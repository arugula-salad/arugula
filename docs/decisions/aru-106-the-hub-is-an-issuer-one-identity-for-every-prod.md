---
schema: 1
id: "aru-106"
title: "The hub is an issuer: one identity for every product"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "How do studio, hud and the platform's other clients sign a person in, once there's one account?"
options:
  - id: "a"
    label: "The hub issues: OIDC-shaped endpoints and hud's Ed25519 assertions, short-lived, with keys at a JWKS URL, rotated"
    how: "The hub serves authorize, token and userinfo endpoints shaped like OIDC on control.arugula.io, with clients registered as rows (studio, hud, the platform page), and signs hud's Ed25519 assertion style from the same keys. Assertions are short-lived and carry the account, its numeric GitHub id and an expiry; projects and roles are read from the platform, never put in a token. Keys are published at /.well-known/jwks.json and rotated with one overlapping the next, and docs/control-ops.md has the incident runbook. studio's passkeys stay as a fallback for a quarter."
    tradeoff: "Every product trusts the hub's keys, so a leaked key reaches all of them until it's rotated, and the hub has to be up for anyone to sign in anywhere."
  - id: "b"
    label: "A separate identity provider"
    how: "Move sessions, passkeys and GitHub sign-in to a hosted IdP or a new service, which the hub and every product trust."
    tradeoff: "Passkeys are bound to control.arugula.io, so everyone enrolls again, and it's another service to run for what the hub already does."
  - id: "c"
    label: "Each product keeps its sign-in, and they share a link table"
    how: "Products sign people in as they do today and only record which rows are the same person."
    tradeoff: "Seven sign-ins stay seven."
  - id: "d"
    label: "Long-lived tokens carrying projects and roles"
    how: "A token names the person's projects and roles, so clients needn't ask the platform."
    tradeoff: "A token outlives a revoke, and its claims go stale when membership changes."
choice:
  option: "a"
  reason: "Identity already lives in the hub, with passkeys that can't move, so the hub issues to the other products, kept small (Jake, 2026-10-22, platform step 8, including studio's and hud's parts on Alex's and Aaron's behalf, pending their review)."
rejected:
  - option: "b"
    why: "Re-enrolls every passkey and adds a service. Identity stays in the hub."
  - option: "c"
    why: "One account per person."
  - option: "d"
    why: "Assertions are short-lived and carry no project or role claims."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #26"
    url: "https://app.arugula.io/issues/iss_73564eac80f26e7b9ca5c7b192d0fe2e"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/passkey.rs"
  - "path:crates/control/src/app_signin.rs"
  - "path:docs/control-ops.md"
---

# The hub is an issuer: one identity for every product

A person has one account, and the hub signs them in to every product (Jake,
2026-10-22, platform step 8, #1337). The hub serves:
- **OIDC-shaped endpoints:** authorize, token and userinfo, on
  `control.arugula.io`. Clients are registered as rows: studio, hud and the
  platform page.
- **hud's Ed25519 assertion style,** as a second format signed by the same
  keys.

It's kept small:
- **Assertions are short-lived.** They carry the account, its numeric GitHub
  id and an expiry. Projects and roles are read from the platform with the
  token and never put in it, so a member who's removed loses access on the
  next read.
- **Keys are published at a JWKS URL** (`/.well-known/jwks.json`) and
  rotated, one key overlapping the next. Where they're kept, and the first
  rotation, are Jake's to say (M228).
- **An incident runbook** in `docs/control-ops.md`: what to do if a key
  leaks, and how clients pick up a new one.
- **studio's own passkeys stay as a fallback for a quarter.** WebAuthn binds
  a passkey to one origin, so they can't move to the hub.

This is the "own record" that "The hub stays the identity provider; passkeys
are bound to control.arugula.io" (amended) left for a separate identity
provider, and it decides against one: identity stays in the hub, which now
issues to other products. Passkeys stay bound to `control.arugula.io`.

**Jake decided this,** including the parts studio (Alex's) and hud (Aaron's)
take on, on their behalf, pending their review of this record.
Where: the issuer (Arugula #26, M228, native), `crates/control/src/auth.rs`, `crates/control/src/passkey.rs`, `crates/control/src/app_signin.rs`, `docs/control-ops.md` (the runbook).
From: #1337 (platform step 8, tracked natively in Arugula #22).
