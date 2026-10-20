---
schema: 1
id: "aru-003"
title: "Some names outlive the rename to Arugula"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "Some names outlive the rename to Arugula"
options:
  - id: "a"
    label: "Some names outlive the rename to Arugula"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#504"
    url: "https://github.com/arugula-salad/arugula/issues/504"
  - title: "#534"
    url: "https://github.com/arugula-salad/arugula/issues/534"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/core/src/rename.rs"
  - "path:crates/e2e/src/frozen.rs"
---

# Some names outlive the rename to Arugula

The product was renamed (#509). Names that something stored or running
depends on stay "illogical" for good: the domains signed or hashed into
certificates, rosters, proofs and push and call tokens, the Noise prologue,
the sync key's HKDF info, the key file header, the checkpoint magic, agent
blocks' MCP token MAC (`illogical block N`), control's hash of a deleted
machine (`illogical gone daemon`), the
browser's IndexedDB `illogical-device`, and the names a restarted daemon or
another version finds running panes and sandboxes by (holder socket, Windows'
pane pipe, systemd scopes, `illogical-eph-`, the resident service). The
state, config, data and cache directories made under the old name are used
where they are. Names that only cross between versions (headers,
`ILLOGICAL_*` variables, service and binary names, download names,
`window.__illogicalApp`) were accepted under both names from the bridge
release (0.25) until every install was on 0.26 or later; since #534 they
are Arugula's alone.
Where: `crates/core/src/rename.rs`, `crates/e2e/src/frozen.rs` (pins the signed domains as hex), "Frozen (#504)" comments.
From: #504, #534.
