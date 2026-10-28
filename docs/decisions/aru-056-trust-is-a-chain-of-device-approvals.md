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
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1239"
    url: "https://github.com/arugula-salad/arugula/issues/1239"
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

**Project delegation** (step 5, #1235, Jake, 2026-10-21). An unattended
hosted machine has no device to approve it, so an owner's device signs once
per project that control may enroll hosted machines into it ("Control
enrolls hosted machines under a project delegation"). Those machines get
project-scoped certificates: clients accept them only for that project,
never for an account's personal trust, and show them as "Hosted: the
operator can read". Revoking the delegation stops new enrollments. From:
M175 (#1236), M178 (#1239).

**Account reset** (lex00, Jake, 2026-10-27, #1582). When no device can
approve and no recovery code is left, a fresh GitHub sign-in can reset the
account's devices after a wait any device can cancel ("An account can reset
its devices from a fresh GitHub sign-in, after a wait"). The new chain has a
new root; daemons keep the old pin and need joining again on the machine,
so control still can't add a reader to a joined machine.
