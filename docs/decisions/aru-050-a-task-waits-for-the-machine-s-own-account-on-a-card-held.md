---
schema: 1
id: "aru-050"
title: "A task waits for the machine's own account, on a card held by a block"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A task waits for the machine's own account, on a card held by a block"
options:
  - id: "a"
    label: "A task waits for the machine's own account, on a card held by a block"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S34"
    url: "https://github.com/arugula-salad/arugula/issues/397"
  - title: "#400"
    url: "https://github.com/arugula-salad/arugula/issues/400"
  - title: "#401"
    url: "https://github.com/arugula-salad/arugula/issues/401"
decided_by: "jhgaylor"
decided_on: "2026-10-09"
reviews: []
constrains:
  - "path:crates/daemon/src/labs/agents/tasks.rs"
  - "path:crates/daemon/src/api.rs"
---

# A task waits for the machine's own account, on a card held by a block

Tasks from another account's agent (M78) wait until the receiving machine's
own account allows them (M79). The check is by account, not principal:
every owner of a team is `Owner` on its machines, so on a team box an
owner from another account would otherwise allow tasks for agents that
aren't theirs. The card can't be on the task's agent block (the mux refuses
cards there), so the Team agents block holds them, one at a time. One is
opened in session `a2a` when none is open, so a card always has a place,
gets attention and is pushed. Answering it goes through the routes that see
the channel's caller (`act`, a block's `answer`), and those refuse a device
of another account. *Allow for an hour* is a grant for that account and
agent, kept on disk, listed and revocable. The caller polls rather than
streams, and a channel request answers within seconds; tasks take minutes.
Where: `crates/daemon/src/labs/agents/tasks.rs`, `block.rs` (the hub), `delegate.rs`, `crates/daemon/src/api.rs` (`foreign`, `is_agents`).
From: [S34](https://github.com/arugula-salad/arugula/issues/397) q4, q6; M78 (#400), M79 (#401).
