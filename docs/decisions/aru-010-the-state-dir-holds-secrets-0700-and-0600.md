---
schema: 1
id: "aru-010"
title: "The state dir holds secrets: 0700 and 0600"
state: "decided"
area: "State and durability"
source:
  via: "cli"
question: "The state dir holds secrets: 0700 and 0600"
options:
  - id: "a"
    label: "The state dir holds secrets: 0700 and 0600"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M2"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m2-durability"
  - title: "M4"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/store.rs"
  - "path:crates/mux/src/perm.rs"
  - "path:crates/daemon/src/seal.rs"
---

# The state dir holds secrets: 0700 and 0600

Logs and checkpoints hold what was typed or echoed, tokens included. The state
dir is `0700` and its files `0600`; retention is enforced;
`illogical purge %p` deletes a pane's history. Logs synced from other hosts
are sealed with a key only the home daemon holds.
Where: `crates/mux/src/store.rs`, `crates/mux/src/perm.rs`, `crates/daemon/src/seal.rs`.
From: [M2](docs/plan-archive.md#m2-durability), [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox) (M4c).
