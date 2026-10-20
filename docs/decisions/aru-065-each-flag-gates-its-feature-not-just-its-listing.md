---
schema: 1
id: "aru-065"
title: "Each flag gates its feature, not just its listing"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "Each flag gates its feature, not just its listing"
options:
  - id: "a"
    label: "Each flag gates its feature, not just its listing"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#347"
    url: "https://github.com/arugula-salad/arugula/issues/347"
  - title: "#665"
    url: "https://github.com/arugula-salad/arugula/issues/665"
decided_by: "jhgaylor"
decided_on: "2026-10-11"
reviews: []
constrains:
  - "path:crates/mux/src/labs/mod.rs"
  - "path:crates/daemon/src/ops/mod.rs"
  - "path:crates/daemon/src/mcp/ops.rs"
  - "path:crates/proto/src/op.rs"
  - "path:crates/daemon/src/labs/mod.rs"
  - "path:crates/mux/src/mux/machines.rs"
  - "path:crates/daemon/tests/integration/flag_gates.rs"
---

# Each flag gates its feature, not just its listing

A flag off used to hide its feature and leave it working for anyone who
called the route, named the MCP tool or typed the hidden command. Now
everything about the feature follows the flag (#347):
- **Operations** name their flag (`Op::FLAG`), and `ops::flagged` refuses
  one by every way in, HTTP and MCP by name alike, as a 404 that says how to
  turn it on. That is the route list: threads (chat, until M125); machines and
  sandboxes (vms); studio; ssh guests (guest-ssh); Fountain's agents and
  sign-in until #942's F5 (since removed). The
  A2A routes (agents) and the forge webhooks already did; an invite from a
  thread asked `chat` too, until M125 took it out.
- **Block types** a flag owns (the agent catalog, app, workspace) are
  wrapped (`labs::Flagged`) so a new one is refused. (Fountain's block,
  and an agent block that was a Fountain agent, checked `fountain` until
  #942's F5; both are gone now.) `POST
  /api/blocks` asks `labs::block_flag` first, so an app block doesn't ask
  the studio before it's refused.
- **The mux** checks `vms` before it makes or borrows a machine or shares
  one with a tab.
- **What was made with the flag on stays.** A block restored at start-up
  isn't asked (as a Forgejo block wasn't), old threads stay on disk (until
  control's chat imports them), a VM pane in a saved layout comes back. Turning a flag off
  stops new use; it doesn't destroy anything, so on again finds it all.
- **One sentence, from the daemon.** `labs::flag` / `flag_off` in the mux
  say "X is a Labs feature that's off on this machine: its owner turns it
  on with `arugulad flags NAME on` there, or in Developer settings". The CLI
  prints the daemon's error, so a hidden command run with `--host` or
  `--ssh` against a machine without the flag says the same about that
  machine; the CLI keeps no list of its own for this.
- **A build without Labs is unchanged.** `labs::flag` is `Ok` there, and
  each feature's twin says it isn't built, as before.
Where: `crates/mux/src/labs/mod.rs` (`flag`, `flag_off`), `crates/daemon/src/ops/mod.rs` (`flagged`), `crates/daemon/src/mcp/ops.rs`, `crates/proto/src/op.rs` (`FLAG`), `crates/daemon/src/labs/mod.rs` (`Flagged`, `block_flag`), `crates/mux/src/mux/machines.rs`, `crates/daemon/tests/integration/flag_gates.rs`.
From: #347, #665.
