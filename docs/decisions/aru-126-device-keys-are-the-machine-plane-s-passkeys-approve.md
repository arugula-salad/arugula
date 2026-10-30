---
schema: 1
id: "aru-126"
title: "Device keys are the machine plane's; the platform's approvals are passkeys"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "What proves that a person meant a platform action (a deploy approval, a standing run grant, a team owner change), and what are device keys for?"
options:
  - id: "a"
    label: "A passkey over the action's bytes; device keys only decide who reaches a machine"
    how: "Every platform-plane action that needs a person (a deploy approval or deny, a standing run grant or its revoke, adding or removing a team owner) is one WebAuthn assertion, user verified, whose challenge is the SHA-256 of the action's bytes, from a passkey registered to the account. It needs no device-key signature, and a device's kind or the devices that certified it play no part. The hub publishes each account's passkey public keys as an append-only, hash-chained log, so the platform, a box or anyone else can check a stored assertion without asking the hub. The device chain (the root pinned at join, certificates, Noise channels, join codes, recovery codes) keeps one job: which keys may reach a machine."
    tradeoff: "Whoever holds a synced passkey can approve from anywhere it syncs (aru-061's cost, now for approvals too). Against whoever runs the platform, an approval is tamper-evident, not tamper-proof: the platform holds the environment's secrets and enrolls the fresh box that applies a deploy, so it could act without one, and the log and the published keys are what make that visible."
  - id: "b"
    label: "Keep both proofs: a browser device chained by browsers, and a passkey"
    how: "As built in M218, M211 and #1343: the device key signs, it must be a browser's certified by browsers back to the root (no CLI, no app window, old app windows grandfathered), the owner role must have been given by such a browser, and a passkey assertion goes with it."
    tradeoff: "Two ceremonies' worth of failures, refusals that arrive after the passkey prompt, and a chain rule that locked the account's own owner out four times (#1386, #1394, #1424, #1827). Against the operator it adds little: the platform reads the root and certificates from the hub, and a fresh deploy box pins what control sends it at enrollment."
  - id: "c"
    label: "Certify passkeys into the device chain, and apply deploys on a long-lived pinned box"
    how: "A device signs each passkey's public key into the account's chain; deploys apply on a per-project box that pinned the root long before, which checks the assertion itself."
    tradeoff: "The only option that resists the operator, at the cost of a long-lived box holding deploy credentials, a passkey certificate step for every account, and a second chain rule to keep alive."
choice:
  option: "a"
  reason: "Device keys were built so a machine, not control, decides which keys may read its terminals, and that job stays. Asked to prove a person, a key's ancestry grew rules that each fixed the last; a user-verified passkey over the bytes is the proof the person was there, and agents can't make one. Against the operator, approvals on the platform plane can be a record, not a lock, as aru-071 has it for that plane (Jake, 2026-10-30, device-keys board, Arugula native #383)."
rejected:
  - option: "b"
    why: "The ancestry rules cost more than they keep out once a passkey is required anyway."
  - option: "c"
    why: "Tamper-evident is enough for the platform plane; deploys keep a fresh box per deploy."
supersedes: []
evidence:
  - title: "Arugula native #383 (DK2: decisions)"
    url: "https://app.arugula.io/issues/iss_e81b07faf1d3361ef8590a5de0de14d0"
  - title: "Arugula native #382 (DK1: S46, a passkey as the platform's approver)"
    url: "https://app.arugula.io/issues/iss_77c31cb41c18dcfdec936df832e84e8e"
  - title: "Arugula native #384 (DK3: the hub publishes passkey public keys)"
    url: "https://app.arugula.io/issues/iss_96c5a22e5d567ee2884dbf952eaaf271"
decided_by: "jhgaylor"
decided_on: "2026-10-30"
reviews: []
constrains:
  - "aru-056"
  - "aru-061"
  - "aru-071"
  - "aru-078"
  - "aru-099"
  - "aru-101"
  - "path:crates/platform/src/approvals.rs"
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/e2e/src/deploy.rs"
  - "path:crates/control/src/passkey.rs"
---

# Device keys are the machine plane's; the platform's approvals are passkeys

Device keys and passkeys answer different questions (Jake, 2026-10-30, the
device-keys board, Arugula native #382 to #390).

- **Device keys decide who reaches a machine.** The root pinned at join,
  the chain of certificates back to it, Noise channels, join codes and
  recovery codes stay as aru-055 and aru-056 have them. A device's kind
  matters here only for who may connect and who may approve a new device.
- **A passkey says a person meant an action.** On the platform plane, each
  of these is one WebAuthn assertion, user verified, whose challenge is the
  SHA-256 of the action's bytes, from a passkey registered to the account:
  - a deploy's approval or deny (aru-101);
  - a standing run grant, and its revoke (aru-099);
  - adding or removing a team owner.

  No device-key signature goes with it. Which kind of device the person
  used, and which devices certified it, play no part: the ancestry rules
  ("browsers all the way", app windows matched by name, grandfathered
  windows, an owner made by a browser) go.
- **Anyone can check one.** The hub publishes each account's passkey public
  keys as an append-only, hash-chained log of keys added and removed (DK3,
  #384). The platform, a box and anyone else check a stored assertion
  against it, with no call to the hub.
- **Against the operator, tamper-evident.** The platform holds an
  environment's secrets and enrolls the fresh box that applies a deploy,
  so whoever runs it could act without an approval. The hash-chained log
  (aru-103) and the published keys make that visible; nothing claims it's
  prevented. This is aru-071's platform plane: a record, not a lock.
- **Agents still never approve.** An agent holds no passkey and can't make
  a user-verified assertion, on its owner's machine too.
- **Migration.** Grants signed with device keys before the switch stay
  valid until they expire (90 days at most); new grants and approvals are
  passkey-signed. Approvals already in a deploy's log keep verifying as
  they were made.
- **Where it's approved.** The RP ID that lets one passkey approve on both
  control's page and the platform's (related origins, or `arugula.io`)
  follows from S46 (#382) and amends aru-078 then.

Where: `crates/platform/src/approvals.rs`, `crates/e2e/src/deploy.rs`,
`crates/e2e/src/cert.rs`, `crates/control/src/passkey.rs`.
From: the Docs "Deploy approvals" (15c8d24bb14a1963) and "Device keys"
(6ea44bfc557de4c6), Arugula native #382 to #390.
