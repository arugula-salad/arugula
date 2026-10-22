---
schema: 1
id: "aru-102"
title: "A driver is a run with a credential"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "How does an approved deploy reach its target, and how are the target's credentials held?"
options:
  - id: "a"
    label: "An approved deploy enqueues a run on the project's hosted workspace, and the driver holds the environment's credential for that run only"
    how: "A driver plans, applies, verifies, and rolls back where its target can. An environment's secrets are sealed per project (the project's own data key, wrapped by the platform's master key), injected into one run, and never written to the workspace's disk or a pane's scrollback. Short-lived tokens are preferred (installation tokens, Fly deploy tokens scoped to one org), and a kubeconfig is never stored. A checked-in list beside tables.toml names every environment whose secrets exist. A driver failing mid-apply marks the deploy failed and pages the operator with the run's log."
    tradeoff: "A deploy needs the hosted workspace awake, and every target needs a driver."
  - id: "b"
    label: "The platform applies deploys itself"
    how: "The platform process holds the credentials and calls each target's API."
    tradeoff: "Every estate's credentials sit in one long-running process, and the platform grows a client per target."
  - id: "c"
    label: "Store kubeconfigs and long-lived tokens"
    how: "Keep whatever credential each target hands out, for as long as it lasts."
    tradeoff: "A leaked kubeconfig is cluster admin until someone notices."
  - id: "d"
    label: "Secrets written to the workspace"
    how: "The driver reads its credentials from a file on the workspace."
    tradeoff: "They'd outlive the run, and go wherever the workspace's files go."
choice:
  option: "a"
  reason: "One way to execute, runs, for CI and deploys alike, with a credential that exists only inside the run that needs it (Jake, 2026-10-21, platform step 7)."
rejected:
  - option: "b"
    why: "The platform would hold every estate's keys and execute against all of them."
  - option: "c"
    why: "Credentials are short-lived where the target allows, and never a kubeconfig."
  - option: "d"
    why: "A deploy secret is never written to a workspace's disk or a pane."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "#1121"
    url: "https://github.com/arugula-salad/arugula/pull/1121"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/cloud"
  - "path:crates/control/tables.toml"
---

# A driver is a run with a credential

A deploy is applied by a run, and the run is what holds the credential
(Jake, 2026-10-21, platform step 7, #1317).

- **Applying an approved deploy enqueues a run** on the project's hosted
  workspace ("A run is a platform object on a hosted workspace").
- **A driver** plans, applies, verifies, and rolls back where its target can.
  A driver failing mid-apply marks the deploy failed, and pages the operator
  with the run's log.
- **Credentials:**
  - sealed per project, with the project's own data key wrapped by the
    platform's master key;
  - injected for one run, then gone: never on the workspace's disk, never in
    a pane's scrollback;
  - short-lived where the target allows (installation tokens, Fly deploy
    tokens scoped to one org);
  - never a kubeconfig.
- **A checked-in list,** beside `crates/control/tables.toml`, names every
  environment whose secrets exist.

The drivers and their order are "Drivers come in order: chant to Fly, then
apps-platform; planter in step 8".
Where: per-project envelope keys and drivers (Arugula #5 and #15, M209 and M219, native), `crates/platform/src/cloud/` (today's sealing).
From: #1317 (platform step 7), the plan in #1290, #1121.
