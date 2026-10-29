---
schema: 1
id: "aru-110"
title: "hud is an in-app client: platform assertions, a project roster"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "How does hud know who may open a box, and where do its reviews go?"
options:
  - id: "a"
    label: "hud accepts the platform's assertions, takes its roster from project members, and posts reviews as object threads"
    how: "hud verifies the hub's assertions in its existing Ed25519 style. Who may open a box is the box's project's members, not a list per box. Comment-mode reviews post as threads on the object they review (step 6)."
    tradeoff: "hud depends on the hub to sign people in and on the platform to answer membership."
  - id: "b"
    label: "Sign in through the hub, keep a roster per box"
    how: "hud trusts the hub's assertions but keeps inviting people box by box."
    tradeoff: "A project's members still need an invite to every box."
  - id: "c"
    label: "Keep reviews in hud"
    how: "Review comments stay in hud's own store."
    tradeoff: "A review never reaches the thread where the project talks about the object."
choice:
  option: "a"
  reason: "A project member opens a hud box without a separate invite, and a review shows where the project already talks (Jake, 2026-10-22, platform step 8, deciding Aaron's part on Aaron's behalf, pending Aaron's review)."
rejected:
  - option: "b"
    why: "The roster is the project's members."
  - option: "c"
    why: "Threads hang off any object, reviews included."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #35"
    url: "https://app.arugula.io/issues/iss_5582e2b38e73726017ad382110827919"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/platform/src/projects.rs"
  - "path:crates/platform-chat/src/chat.rs"
---

# hud is an in-app client: platform assertions, a project roster

hud becomes an in-app client of the platform (Jake, 2026-10-22, platform
step 8, #1337):
- **Platform assertions.** hud accepts the hub's assertions in its existing
  Ed25519 style ("The hub is an issuer: one identity for every product").
- **Its roster comes from project members,** not a list per box, so a
  project member opens a box without a separate invite.
- **Comment-mode reviews post as object threads** ("Threads hang off any
  object"), and show on the platform page.

hud is Aaron's repo. **Jake decided this on Aaron's behalf,** pending Aaron's
review of this record. The code there is proposed as PRs that Aaron reviews
and merges. If Aaron pushes back in review, step 8's hud work stops and goes
to Jake as a choice.
Where: hud (Arugula #35, M237, native), `crates/platform/src/projects.rs` (members), `crates/platform-chat/src/chat.rs` (object threads).
From: #1337 (platform step 8, tracked natively in Arugula #22).
