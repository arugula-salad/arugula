---
schema: 1
id: "aru-049"
title: "A machine reaches another's agents as its account, through the relay"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A machine reaches another's agents as its account, through the relay"
options:
  - id: "a"
    label: "A machine reaches another's agents as its account, through the relay"
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
  - title: "#399"
    url: "https://github.com/arugula-salad/arugula/issues/399"
  - title: "#856"
    url: "https://github.com/arugula-salad/arugula/issues/856"
decided_by: "jhgaylor"
decided_on: "2026-10-09"
reviews: []
constrains:
  - "path:crates/daemon/src/peer.rs"
  - "path:crates/daemon/src/e2e.rs"
  - "path:crates/daemon/src/control.rs"
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/teams.rs"
---

# A machine reaches another's agents as its account, through the relay

The team's catalog (M77) is read by the daemon itself, not by a page that
happens to be open, so its agents can use it. A daemon signs its relay
socket and its directory read with its own key, and control routes it
exactly as it would route its person. On the far end a daemon certificate
gets in as `account:<id>`, never as the owner (on a team box every owner is
`Owner`, and a daemon reaching for agents gets the agents and nothing
more). Its channel answers `/api/a2a/` only: no mux, no panes, no hands.
Another account's machine is trusted through the root a checked team roster
names for that account, not through first sight, because a teammate's
roster is already verified. A teammate's own machine is reachable only while it
offers agents (the A2A-only grant). It names those accounts in a separate
`agents` list. Control routes their daemons with it and lists it to them,
but never to their browsers, so a person's fleet doesn't gain machines it
can't open. A daemon keeps one standing channel to each machine it reaches
(#856), every request on it numbered, and closes it once it's idle a few
minutes.
Where: `crates/daemon/src/peer.rs`, `crates/daemon/src/e2e.rs` (`serve_agent_caller`, `agents_only`), `crates/daemon/src/control.rs` (`agent_callers`, `known_root`), `crates/control/src/auth.rs` (`Reacher`), `crates/control/src/teams.rs` (`daemon_may_reach`).
From: [S34](https://github.com/arugula-salad/arugula/issues/397) q3, q7; M77 (#399); #856.
