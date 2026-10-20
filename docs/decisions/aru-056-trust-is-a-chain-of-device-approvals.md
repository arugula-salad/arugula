---
schema: 1
id: "aru-056"
title: "Trust is a chain of device approvals"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Trust is a chain of device approvals"
options:
  - id: "a"
    label: "Trust is a chain of device approvals"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M17"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m17-illogical-control-accounts-devices-enrollment-directory"
  - title: "M19"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m19-teams-sharing-roles-team-daemons-invites"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/e2e/src/team.rs"
---

# Trust is a chain of device approvals

A new device must be approved by a device the account already trusts (the
first is trusted on enrollment). Daemons verify the chain themselves back to
the account's pinned root, so control can't add a reader; team rosters are
signed by an owner's device of the previous version.
Where: `crates/e2e/src/cert.rs`, `crates/e2e/src/team.rs`.
From: [M17](docs/plan-archive.md#m17-illogical-control-accounts-devices-enrollment-directory), [M19](docs/plan-archive.md#m19-teams-sharing-roles-team-daemons-invites), S15.
