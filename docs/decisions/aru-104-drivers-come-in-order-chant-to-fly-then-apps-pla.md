---
schema: 1
id: "aru-104"
title: "Drivers come in order: chant to Fly, then apps-platform; planter in step 8"
state: "decided"
area: "Build and release"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "Which deploy drivers does step 7 build, and in what order?"
options:
  - id: "a"
    label: "chant to Fly first, apps-platform second, planter in step 8"
    how: "The chant to Fly driver runs chant's release operation and approves its gate as the approver's identity, writes chant's ledger, and holds a Fly deploy token scoped to one org. Arugula's own release (held in #1121) ships through it, approved from Jake's phone: that's the step's gate. The apps-platform driver commits app.yaml and the pointer to home-cloud's apps-platform repo with a token that can write only that repo, and Flux applies it. planter's Prod and Go live move with planter's absorption in step 8, and so does the gate's criterion that a planter app reaches *.apps.arugula.io."
    tradeoff: "planter keeps its own approval until step 8, so deploys live in two places a while longer."
  - id: "b"
    label: "planter first"
    how: "Build the planter driver before the others."
    tradeoff: "planter's absorption is step 8, so its driver would be built against an app that's about to move."
  - id: "c"
    label: "All three in step 7"
    how: "chant to Fly, apps-platform and planter together."
    tradeoff: "Step 7 would wait on planter's absorption."
choice:
  option: "a"
  reason: "Arugula's own release is the gate, and it's a chant release on Fly (Jake, 2026-10-21, platform step 7)."
rejected:
  - option: "b"
    why: "planter moves in step 8."
  - option: "c"
    why: "Keeps step 7 from waiting on step 8's work."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "#1121"
    url: "https://github.com/arugula-salad/arugula/pull/1121"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src"
---

# Drivers come in order: chant to Fly, then apps-platform; planter in step 8

Step 7 builds two deploy drivers, in this order (Jake, 2026-10-21, platform
step 7, #1317).

1. **chant to Fly.** Apply runs chant's release operation, then approves its
   gate as the approver's identity. It writes chant's ledger, and the run
   links to the entry ("A run is a platform object on a hosted workspace").
   The Fly token is a deploy token scoped to one org, sealed in the project.
   Roll back goes through chant and Fly. Arugula's own release (the chant
   operation held in #1121) ships this way, approved from Jake's phone. That's
   step 7's gate.
2. **apps-platform.** Apply commits `app.yaml` and the pointer to home-cloud's
   apps-platform repo, with a token that can write only that repo, and Flux
   applies it. Roll back is a revert commit.

**planter waits for step 8,** with planter's absorption. So does the gate's
criterion that a planter app reaches `*.apps.arugula.io`. Until then,
planter's Prod and Go live keep their own approval.
Where: the drivers (Arugula #16, #17 and #18, M220–M222, native).
From: #1317 (platform step 7), the plan in #1290, #1121.
