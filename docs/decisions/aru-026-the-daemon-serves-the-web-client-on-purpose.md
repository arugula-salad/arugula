---
schema: 1
id: "aru-026"
title: "The daemon serves the web client, on purpose"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "The daemon serves the web client, on purpose"
options:
  - id: "a"
    label: "The daemon serves the web client, on purpose"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Cargo workspace"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#cargo-workspace"
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/server.rs"
  - "path:web"
---

# The daemon serves the web client, on purpose

`web/dist` is embedded in the daemon (`rust-embed`), so any browser works
against any machine and there is one artifact to install. The price is that a
UI change needs a daemon release. This is not on the list to change; don't
split the web client out by accident.
Where: `crates/daemon/src/server.rs`, `web/`.
From: [Cargo workspace](docs/plan-archive.md#cargo-workspace), #387 (point 4).
