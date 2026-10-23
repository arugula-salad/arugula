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
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1250"
    url: "https://github.com/arugula-salad/arugula/issues/1250"
  - title: "#1251"
    url: "https://github.com/arugula-salad/arugula/issues/1251"
decided_by: "jhgaylor"
decided_on: "2026-10-16"
reviews: []
constrains:
  - "path:crates/platform/src/cloud"
  - "path:crates/platform/src/hosted"
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
terminal relay: Cloud workspaces, their transcripts and cloud secrets are
on the platform plane ("Two planes, two trust rules", #1122), listed in
`crates/control/tables.toml`, and each belongs to a project ("The project
is the unit of platform sharing"), its owner's personal one until people
can choose. Within control, Cloud is the platform process's ("Control is two
processes: a hub and a platform", #1150): the worker, its leases, Managoat
and cloud secrets move there (M137), and the hub's gateway keeps the Cloud
paths on `control.arugula.io`.

Where (until 2026-10-23): `crates/platform/src/cloud/`,
`crates/platform/src/db/cloud.rs`, `web/src/platform/cloud/`,
`spikes/s40-cloud-workspaces/PLAN.md`; in git before #1250's PR.
From: Jake's managed-platform integration decisions, 2026-10-16; #1039.

**Step 5 (#1235, Jake, 2026-10-21): a Cloud workspace becomes a hosted
`arugulad`.** It runs in a rented sandbox attached to a project ("A hosted
machine is arugulad in a rented sandbox", "The substrate is built to E2B's
API"), with terminals, agent blocks and previews through the daemon, and
pauses only between turns. Managoat stops being the Cloud workspace
backend: beta workspaces are recreated on E2B, and the Managoat path is
deleted 14 days after the notice (M189). The M20 hosted-sandbox product
(`arugulad` in a Sprites VM) is retired with it (M190). Control still owns
the account and inference configuration ("Hosted inference: strangers bring
their own key"). From: M175 (#1236).

**Managoat removed (Jake, 2026-10-23).** Jake was the beta's only user, so
instead of moving and archiving he chose to delete it all. Managoat's
resources went first (`managoat-retire --execute`: 2 workspaces, 2 vaults,
2 environments), then the code (the `/api/cloud/…` routes, the worker, the
move, `managoat-retire`, the agent library, cloud secrets and past runs)
and its 11 `cloud_*` tables (platform migration 0030), with nothing
archived. Control's Cloud is hosted workspaces only now
(`crates/platform/src/hosted/`). What the Managoat era left in
`crates/platform/src/cloud/` is the `cloud.drive` check and the master key's
and beta allowlist's flags. Where it went: [docs/managoat.md](docs/managoat.md).
From: M189 (#1250), M190 (#1251).
