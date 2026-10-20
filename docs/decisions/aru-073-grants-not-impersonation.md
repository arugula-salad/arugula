---
schema: 1
id: "aru-073"
title: "Grants, not impersonation"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1123"
question: "How does a machine or an agent act on platform objects that belong to a person?"
options:
  - id: "a"
    label: "Through a revocable grant row: a subject, a power and a scope"
    how: "A grants table (subject kind and id, power, scope, granted_by, created_at, revoked_at). aru-047's power is a cloud.drive grant per machine, backfilled for every joined machine and given to each new one; control checks it on the Cloud paths. Per machine, because the daemon signs every MCP call as itself; per agent needs a daemon release."
    tradeoff: "One more check on the cloud paths, and the grant has to be backfilled so nothing changes for anyone."
  - id: "b"
    label: "Act as the owner"
    how: "As aru-047 did: a daemon's signature counts as its owner's account on the Cloud routes."
    tradeoff: "No way to take the power from one machine without taking the machine out."
  - id: "c"
    label: "Per-agent grants now"
    how: "Each agent on a machine gets its own credential."
    tradeoff: "Needs a daemon release and a new signing path."
choice:
  option: "a"
  reason: "Keeps aru-047's behaviour while making it revocable machine by machine, with no daemon release (Jake, 2026-10-20, #1122)."
rejected:
  - option: "b"
    why: "Impersonation can't be narrowed or revoked short of removing the machine."
  - option: "c"
    why: "Waits on a daemon change; per machine is what the daemon's signature can tell apart today."
supersedes: []
evidence:
  - title: "#1094"
    url: "https://github.com/arugula-salad/arugula/issues/1094"
  - title: "#1122"
    url: "https://github.com/arugula-salad/arugula/issues/1122"
  - title: "#1126"
    url: "https://github.com/arugula-salad/arugula/issues/1126"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/platform/src/cloud"
  - "path:crates/control/src/db"
---

# Grants, not impersonation

A machine or an agent acts on platform objects through a **grant**, not as
the person who owns them (Jake, 2026-10-20, #1122). A grant is a row in
control's `grants` table: a subject (a kind and an id; a daemon for now), a
power (`cloud.drive` first), a scope (a project, or an owner account), who
granted it, when, and when it was revoked. An intent that touches platform
objects checks for a live grant, as "One function says what an intent
needs" checks a role on the machine plane.

"Agents on a machine drive its owner's Cloud workspaces" stays, recorded as
grants: every machine joined to an account gets a `cloud.drive` grant scoped
to that account, backfilled for the machines already joined and given to
each one that joins, so nothing changes for anyone. Control checks the grant
on the Cloud workspace routes that took any of the owner's daemons; with no
live grant, the call is refused with a message naming the machine and how to
restore it. The owner lists and revokes grants through control's API and the
CLI.

Grants are per machine because the daemon signs every MCP call as itself:
control can tell machines apart, not the agents on one. Narrowing a grant to
the owner's own agents needs a daemon release and is a later step.
Where: `crates/control/src/db/` (`grants`), `crates/control/src/auth.rs`, `crates/platform/src/cloud/`.
From: #1094, #1122, M130 (#1126).
