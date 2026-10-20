---
schema: 1
id: "aru-047"
title: "Agents on a machine drive its owner's Cloud workspaces"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Agents on a machine drive its owner's Cloud workspaces"
options:
  - id: "a"
    label: "Agents on a machine drive its owner's Cloud workspaces"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#1094"
    url: "https://github.com/arugula-salad/arugula/issues/1094"
decided_by: "jhgaylor"
decided_on: "2026-10-19"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/cloud"
  - "path:crates/daemon/src/mcp/cloud.rs"
  - "path:crates/daemon/src/control.rs"
---

# Agents on a machine drive its owner's Cloud workspaces

Every agent on a machine joined to control can drive the machine owner's
Cloud workspaces through MCP (`list` kinds `cloud_*`, `read_cloud`,
`cloud`, `wait` on a tab), acting as the owner's account. The daemon calls
control's Cloud workspace and tab routes with its own signature, and
control scopes a daemon's signature to its certificate's account there,
as it would a session. So a teammate with run rights on a machine, or a
guest's agent on it, drives the owner's Cloud workspaces; that is the
point, not a leak. A daemon reaches no other account's, and not what stays
a person's: settings, deleting, secrets, the agent library, past tabs.
Where: `crates/control/src/auth.rs` (`CloudDriver`), the handlers in
`crates/control/src/cloud/`, `crates/daemon/src/mcp/cloud.rs`,
`crates/daemon/src/control.rs` (`Control::cloud`),
[docs/control.md](docs/control.md), [managed cloud agents](docs/managoat.md).
From: #1094, Jake, 2026-10-19 (full drive, for any agent on the machine).

Since step 1 of the platform roadmap (#1122, Jake, 2026-10-20) the power
stays and is recorded as grants ("Grants, not impersonation"): each machine
holds a `cloud.drive` grant scoped to its owner's account, backfilled for
every joined machine and given to each new one, and control checks it on
these routes. Behaviour doesn't change, but the owner can revoke it machine
by machine, and a machine without a live grant is refused. It's per machine
because the daemon signs every MCP call as itself; narrowing it to the
owner's own agents is a later daemon change. From: M130 (#1126).
