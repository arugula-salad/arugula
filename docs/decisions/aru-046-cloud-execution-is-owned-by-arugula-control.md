---
schema: 1
id: "aru-046"
title: "Cloud execution is owned by Arugula control"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Cloud execution is owned by Arugula control"
options:
  - id: "a"
    label: "Cloud execution is owned by Arugula control"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#1039"
    url: "https://github.com/arugula-salad/arugula/issues/1039"
decided_by: "jhgaylor"
decided_on: "2026-10-16"
reviews: []
constrains:
  - "path:crates/control/src/cloud"
  - "path:crates/control/src/db/cloud.rs"
  - "path:web/src/ui/cloud.tsx"
  - "path:web/src/ui/cloud-workspaces.tsx"
  - "path:web/src/ui/cloud-launch.tsx"
  - "path:spikes/s40-cloud-workspaces/PLAN.md"
---

# Cloud execution is owned by Arugula control


Arugula owns the backend account and inference configuration. Users run
cloud agents through their Arugula account, choose curated or custom agents,
and provide only the application secrets those agents need. Control
persists their lifecycle, approvals and transcripts across browser
disconnects and process restarts. The beta is account-allowlisted.

Agents run in Cloud workspaces (#1039, S40): a session backed by a Managoat
workspace, one machine per Cloud workspace with its own environment and
vault, whose tabs are conversations sharing its disk. Control sets no
limits on them. One-shot cloud runs (a fresh checkout per run, with daily,
concurrent and duration caps) were retired for them (S40 decision 10,
#1076): nothing starts, cancels or answers a run, past runs stay readable
with their downloads, a deploy stops a leftover run (one in recovery stays
for an operator), and the cap variables still parse and say they do
nothing. Jake, 2026-10-18 and 2026-10-19.

This replaced the daemon's direct-account workflows (#942): its Managoat
login, account streams, remote catalogs, and recipe execution were retired,
and then removed ("Daemon-side Managoat is removed", below).
Old saved agent blocks remain readable without reconnecting. Definitions
can be imported explicitly into account-scoped cloud agents; old remote
conversations are not transferred. Local agents and their permission rules
remain machine-owned. Cloud processing is separate from the encrypted
terminal relay.

Where: `crates/control/src/cloud/`, `crates/control/src/db/cloud.rs`,
`web/src/ui/cloud.tsx`, `web/src/ui/cloud-workspaces.tsx`,
`web/src/ui/cloud-launch.tsx`, [managed cloud agents](docs/managoat.md),
`spikes/s40-cloud-workspaces/PLAN.md`.
From: Jake's managed-platform integration decisions, 2026-10-16; #1039.
