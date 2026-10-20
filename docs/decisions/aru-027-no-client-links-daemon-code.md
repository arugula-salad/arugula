---
schema: 1
id: "aru-027"
title: "No client links daemon code"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "No client links daemon code"
options:
  - id: "a"
    label: "No client links daemon code"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:web"
  - "path:Cargo.toml"
---

# No client links daemon code

`cli`, `desktop` and `web/` speak the wire through
`illogical-proto`. `core` has no internal dependencies, `proto` depends only
on `core`, `vt` and `e2e` stand alone, and `control` shares only `e2e`.
Where: `Cargo.toml`, `crates/*/Cargo.toml`.
From: #387.
