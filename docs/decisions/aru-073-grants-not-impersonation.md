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
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
  - title: "#1264"
    url: "https://github.com/arugula-salad/arugula/issues/1264"
  - title: "#1268"
    url: "https://github.com/arugula-salad/arugula/issues/1268"
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "Arugula #683"
    url: "https://app.arugula.io/issues/iss_e180f5bf116b44cf72ece7553d962f71"
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

**Thread and issue powers** (step 6, #1263). Three powers join
`cloud.drive`, for agents in conversations ("Agents are in conversations"):
`threads.read` (read rooms and issues in scope), `threads.post` (post
messages, issue comments included) and `issues.write` (create, edit, close,
label and assign native issues). Their scope is a project (a team's channels
are its default project's) or the owner's account (their DMs). Unlike
`cloud.drive` they're off by default and nothing is backfilled: only the
machine's owner grants them, per scope, and only over scopes the owner can
read. A call without one is refused with a message saying how to grant it,
and a revoke takes effect on the next call. They're still per machine, so
what an agent writes is labelled with its machine, not the agent.
**Hosted machines** (step 5, #1235). A hosted workspace's machine is
reached through the same grants: a machine's `cloud.drive` (and the step 6
powers) cover hosted workspaces in the scopes they name, and the hosted
machine itself is only trusted for its project ("Control enrolls hosted
machines under a project delegation").

**The standing project grant** (step 7, #1317). `runs.start` and
`runs.instruct`, scoped to a project, over its hosted workspaces ("One
standing grant per project starts runs"). Unlike the other powers it's
signed by a project owner's device, as S37's standing grant: the platform
stores it but can't make or widen it, and hosted `arugulad` checks the
signature. There's one per project. Revoking it stops new runs, and it never
covers a deploy.

**Session powers** (amended, Jake, 2026-10-27, Arugula #265, aru-121).
`sessions.start` and `sessions.reply`, over a project, for a machine's
agents: with them its agents start an agent session as the owner (who stays
its asker and its payer, the card saying it was their agent on that
machine) and answer or prompt one in its thread. Off by default, given and
revoked by the owner like the step 6 powers, never to an agent grant token,
and never an approval: a session's permission waits on a person.
**Owners revoke, and a viewer's write is refused** (amended, Jake,
2026-11-03, Arugula #683, from the access design's Option B). A grant is
revoked by its giver, or the account it's over, as above, and also by any
owner of the project it's over, whoever gave it, a machine's or a token's,
as they already could a token's whole reach: the owner's reach is the
project's grants only, never one over an account or another project, and
the grant records who revoked it and when (its giver, the owner, the
platform when a token's giver left the team, or an account since deleted).
A revoke isn't a ban: the grant is the machine owner's, as above, so the
giver is told and can give it again; an owner who means it takes them out
of the team. A revoke stops the next call `agent_may` sees; a run, an
agent session or a deploy approval already under way carries on as it
was. A write power that does nothing at all for a viewer's agents
(`issues.write`, `trackers.write`, `runs.start`, `runs.command`, a token's
`secrets.write`) is refused to a viewer's machine at `give`, and left out
of a viewer's token at its mint, project by project, since every write it
tried would be refused by the route's own role check; one that still does
something for a viewer (`deploys.plan` reads deploys, `boards.write` reads
boards, `sessions.start` follows sessions and stops the owner's own) is
given, and lists with what it does. The one list is `WRITE_POWERS`. A
grant from before stays and lists with its limit, so a role change brings
the rest of it back. Both only narrow what's possible.
Where: `crates/platform/src/db/projects.rs` (`grants`), `crates/platform/src/grants.rs`, `crates/platform/src/cloud/` (`CloudDriver`).
From: #1094, #1122, M130 (#1126), #1263 (step 6), M193 (#1264), M197 (#1268), M175 (#1236), #1317 (step 7, planned in #1290).
