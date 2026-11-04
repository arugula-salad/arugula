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
  - "path:crates/mux/src/mux/mod.rs"
---

# Layout belongs to a host; a remote block references a pane elsewhere

Each daemon owns its own tree. A tab can hold a `remote` block, `{host,
pane}` and nothing else, whose terminal, size, policy and history stay on the
other daemon. The client asks the other host for the pane and then records
the reference here; the daemon never talks to the other one on a client's
behalf, and closing the reference closes only the reference.
Where: `crates/mux/src/mux/mod.rs` (`restore`).
From: [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), #17.

Amended 2026-11-04: remote blocks were removed with the rest of the older
machine model (the trace in Doc `2cb871d19a9fb508`; Jake signed every
removal off). The first half stands: each daemon owns its own tree, and a
daemon never talks to another on a client's behalf. The second half is
gone: no block type places a pane from another daemon in this layout
(`remote.rs`, the CLI's `run --home` and `close`'s reach to the host, the
web's `Split right on HOST` and `New tab on HOST`). `BlockType::Remote`
stays a readable word so a stored `layout.json` still parses, and a daemon
restoring one drops that leaf with a log line and keeps the rest of the
layout (`restore` in `crates/mux/src/mux/mod.rs`, tested in
`crates/arugulad/tests/integration/remote_layout.rs`). The direction this
once pointed at is native #503, views across tabs and machines, which
needs none of it.
