---
schema: 1
id: "aru-078"
title: "The hub stays the identity provider; passkeys are bound to control.arugula.io"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1151"
question: "Who signs people in once control is a hub and a platform?"
options:
  - id: "a"
    label: "The hub, as today: sessions, passkeys and GitHub sign-in stay in the hub, and the platform takes the principal the gateway signs"
    how: "Passkeys keep control.arugula.io as their relying party; the platform never sees a cookie or a passkey and trusts only the hub's HMAC-signed principal header."
    tradeoff: "The platform can't stand alone: every platform call needs the hub up."
  - id: "b"
    label: "A separate identity service, or the platform signing people in"
    how: "Move sessions and passkeys out of the hub, or give the platform its own sign-in."
    tradeoff: "Passkeys are bound to their relying party; moving them means every person enrolls again, and two sign-ins split one account."
choice:
  option: "a"
  reason: "Passkeys can't move off control.arugula.io without everyone enrolling again, and the gateway already puts the hub in front of every platform call; a separate identity provider is a later step if it's ever needed (Jake, 2026-10-20, #1150)."
rejected:
  - option: "b"
    why: "Re-enrolls every passkey and adds a service, for nothing this step needs."
supersedes: []
evidence:
  - title: "#1150"
    url: "https://github.com/arugula-salad/arugula/issues/1150"
  - title: "#1151"
    url: "https://github.com/arugula-salad/arugula/issues/1151"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/passkey.rs"
---

# The hub stays the identity provider; passkeys are bound to control.arugula.io

The hub stays the identity provider for step 2 (Jake, 2026-10-20, #1150).
Sessions, passkeys and GitHub sign-in stay in the hub, and passkeys stay
bound to `control.arugula.io` as their relying party, so nobody enrolls
again. The platform never sees a cookie, a passkey or a daemon signature:
it trusts the principal the hub's gateway signs ("Platform paths stay on
control.arugula.io forever, behind the hub's gateway"). A separate identity
provider, if one is ever needed, is a later step and its own record.
Where: `crates/control/src/auth.rs`, `crates/control/src/passkey.rs` (`rp_id`).
From: #1150, M134 (#1151).
