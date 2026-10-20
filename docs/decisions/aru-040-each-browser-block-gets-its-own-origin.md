---
schema: 1
id: "aru-040"
title: "Each browser block gets its own origin"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Each browser block gets its own origin"
options:
  - id: "a"
    label: "Each browser block gets its own origin"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M6a"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m6a-browser-blocks"
  - title: "M50"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m50-blocks-through-control-150-after-s27"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/sites.rs"
  - "path:crates/daemon/src/browser.rs"
---

# Each browser block gets its own origin

A dev server in a machine runs code an agent wrote. Block 42 is served at
`b-42.<domain>`, at `/`, so blocks can't read each other or the app, and dev
servers need no base path. Through control the same origin comes from control
with no DNS or certificate on the machine.
Where: `crates/daemon/src/sites.rs`, `crates/daemon/src/browser.rs`.
From: [M6a](docs/plan-archive.md#m6a-browser-blocks), [M50](docs/plan-archive.md#m50-blocks-through-control-150-after-s27).
