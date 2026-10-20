---
schema: 1
id: "aru-019"
title: "Layout belongs to a host; a remote block references a pane elsewhere"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "Layout belongs to a host; a remote block references a pane elsewhere"
options:
  - id: "a"
    label: "Layout belongs to a host; a remote block references a pane elsewhere"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M4"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox"
  - title: "#17"
    url: "https://github.com/arugula-salad/arugula/issues/17"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/remote.rs"
---

# Layout belongs to a host; a remote block references a pane elsewhere

Each daemon owns its own tree. A tab can hold a `remote` block, `{host,
pane}` and nothing else, whose terminal, size, policy and history stay on the
other daemon. The client asks the other host for the pane and then records
the reference here; the daemon never talks to the other one on a client's
behalf, and closing the reference closes only the reference.
Where: `crates/daemon/src/remote.rs`.
From: [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), #17.
