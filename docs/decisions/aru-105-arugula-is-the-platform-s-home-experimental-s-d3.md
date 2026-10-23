---
schema: 1
id: "aru-105"
title: "arugula is the platform's home; experimental's D3 is retired"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "Where do accounts, projects and their secrets live, now that experimental planned a control plane of its own?"
options:
  - id: "a"
    label: "arugula: the hub and the platform; experimental's D3 is retired, and product.md's vocabulary is adopted where it doesn't contradict \"Project\""
    how: "arugula's hub (identity) and platform (projects, grants, secrets, runs, deploys) are the source of truth for accounts, projects and project secrets. experimental's D3, a new control plane in that repo on Fly as the source of truth for accounts, apps and secrets, is retired before it's built on. product.md's words come over wherever they don't contradict aru-074: the thing a person builds is still a Project, not an App."
    tradeoff: "experimental's plan loses its control plane, and its unbuilt epics wait as ideas until arugula builds them."
  - id: "b"
    label: "experimental's control plane (D3)"
    how: "Accounts, apps and secrets live in experimental's control plane on Fly; arugula control stays the device and relay layer under it, linked by GitHub identity."
    tradeoff: "A second account system beside the hub's, with no users yet, and six steps of the platform's projects, grants, runs and deploys moved or built twice."
  - id: "c"
    label: "Both, side by side"
    how: "Each keeps its own accounts and secrets, kept in step by a sync."
    tradeoff: "Two homes are two rows per person: the problem step 8 exists to end."
choice:
  option: "a"
  reason: "arugula already holds the people, machines, projects, grants, runs and deploys, and experimental's control plane has no users (Jake, 2026-10-22, platform step 8, deciding Aaron's part on Aaron's behalf, pending Aaron's review)."
rejected:
  - option: "b"
    why: "experimental's control plane has no users, and the platform already does what D3 planned."
  - option: "c"
    why: "One account per person."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "experimental D3"
    url: "https://github.com/arugula-salad/experimental/blob/main/docs/PLAN.md"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/platform/src"
  - "path:crates/control/src/gateway.rs"
  - "path:docs/platform-features.md"
---

# arugula is the platform's home; experimental's D3 is retired

Accounts, projects and project secrets have one home: arugula's hub and
platform (Jake, 2026-10-22, platform step 8, #1337).

- **experimental's D3 is retired.** D3 made a new control plane in
  experimental, on Fly, the source of truth for accounts, apps and secrets,
  with arugula control as the device and relay layer under it. Nothing builds
  on it: the hub keeps identity ("The hub is an issuer: one identity for
  every product") and the platform keeps projects and their secrets ("The
  platform is the one store for users' project secrets").
- **product.md's vocabulary is adopted** where it doesn't contradict "One
  vocabulary: Project, with prj_ ids". The thing a person builds is still a
  Project, not an App; product.md's other words come over as they are.
- **experimental is archived** ("experimental is archived").

**Jake decided this on Aaron's behalf** (experimental is Aaron's repo),
pending Aaron's review of this record. **The fallback:** if Aaron wants to
keep experimental's control plane, step 8 stops. This record is then
contested, and it's settled first, before anything else in step 8 builds on
it.
Where: the platform (`crates/platform/src`) behind the hub's gateway (`crates/control/src/gateway.rs`), `docs/platform-features.md`, experimental (Arugula #36, M238, native).
From: #1337 (platform step 8, tracked natively in Arugula #22), experimental's PLAN.md (D3).
