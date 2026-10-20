---
schema: 1
id: "aru-011"
title: "Layout is a file written atomically"
state: "decided"
area: "State and durability"
source:
  via: "cli"
question: "Layout is a file written atomically"
options:
  - id: "a"
    label: "Layout is a file written atomically"
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
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/store.rs"
  - "path:crates/mux/src/acl.rs"
---

# Layout is a file written atomically

`layout.json` holds sessions, tabs, splits, machines and every block's config.
It is written to a temp file, renamed, and the directory fsynced, debounced
to 250 ms, with a schema version. Grants (`acl.json`) are written the same way.
Where: `crates/mux/src/store.rs`, `crates/mux/src/acl.rs`.
From: [M2](docs/plan-archive.md#m2-durability), M12.
