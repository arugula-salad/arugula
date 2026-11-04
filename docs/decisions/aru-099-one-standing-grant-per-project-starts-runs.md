---
schema: 1
id: "aru-099"
title: "One standing grant per project starts runs"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "What lets a scheduled or event-triggered run start while every owner device is asleep?"
options:
  - id: "a"
    label: "One standing grant per project, signed once by an owner's device and revocable"
    how: "S37's standing grant (arugula exec grant v1), signed by a project owner's device, with runs.start and runs.instruct over the project's hosted workspaces. It's offered when the project gets its first hosted workspace, and can be signed again from the phone. The platform stores and relays it but can't make or widen it, and hosted arugulad checks it against the project's delegation. A revoke stops new runs; running ones finish, or are stopped if the owner says so. Deploys never run under it."
    tradeoff: "One signature lets the platform start runs in the project until it's revoked, so a platform that's been broken into can queue runs the grant covers."
  - id: "b"
    label: "An approval for every run"
    how: "Each run waits for an owner device's S37 work approval."
    tradeoff: "Schedules and events stop while the owner sleeps, and a week of nightly runs is seven approvals."
  - id: "c"
    label: "The platform signs runs itself"
    how: "The platform holds a key hosted daemons trust, and signs each run."
    tradeoff: "The platform could start anything on any project's workspaces, with no owner's signature behind it."
  - id: "d"
    label: "A grant per run spec or schedule"
    how: "Each schedule or run spec gets its own signed grant."
    tradeoff: "Every new spec needs an owner's device, for a scope the project's hosted workspaces already bound."
choice:
  option: "a"
  reason: "One grant per project, so scheduled runs start with every device asleep and a revoke stops new ones; deploys still need an approval every time (Jake, 2026-10-21, platform step 7). Amended (Jake, 2026-11-04): agent sessions run under it too, the box checking it, and a revoke ends them."
rejected:
  - option: "b"
    why: "Scheduled and event runs are the point of the step, and they'd stop whenever the owner is away."
  - option: "c"
    why: "The platform never signs for an owner."
  - option: "d"
    why: "More signing for no narrower reach: the grant already covers only the project's hosted workspaces."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "S37"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s37-team-permissions/README.md"
  - title: "#970"
    url: "https://github.com/arugula-salad/arugula/issues/970"
  - title: "#1002"
    url: "https://github.com/arugula-salad/arugula/discussions/1002"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/grants.rs"
  - "path:crates/platform-api/src/auth.rs"
  - "path:crates/platform/src/run_grant.rs"
  - "path:crates/platform-chat/src/sessions/grant.rs"
  - "path:crates/daemon/src/hosted/runs.rs"
---

# One standing grant per project starts runs

A project has one standing grant for runs (Jake, 2026-10-21, platform step
7, #1317).

- **What it is:** S37's standing grant (q3, `arugula exec grant v1`), with
  `runs.start` and `runs.instruct` over the project's hosted workspaces. It's
  a grant in the sense of "Grants, not impersonation", amended.
- **An owner's device signs it once.** It's offered when the project gets
  its first hosted workspace, and can be signed again from the phone.
- **The platform never signs.** It stores and relays the grant, but can't
  make or widen it. Hosted `arugulad` checks the signature against the
  project's delegation ("Control enrolls hosted machines under a project
  delegation").
- **Scheduled and event runs start under it** with every owner device
  asleep.
- **Revoking it stops new runs.** Running ones finish, or are stopped if the
  owner says so.
- **Deploys never run under it.** Each deploy is approved by an owner's device
  ("A deploy needs an owner device's approval, and agents never approve").

The cost: while the grant stands, a platform that's been broken into can
queue runs it covers, on that project's hosted workspaces only.
**Amended: signed with a passkey** (Jake, 2026-10-30, aru-126, Arugula
native #387). The grant, and its revoke, is a user-verified passkey
assertion over its bytes from a project owner's passkey, not an owner
device's signature, and hosted `arugulad` checks it against the account's
published passkey keys. Grants signed by a device before the switch stay
valid until they expire; renewing one makes a passkey-signed grant.
**Amended: agent sessions run under it, and a revoke ends them** (Jake,
2026-11-04). A thread's agent session ("A thread's agent session runs on the
project's hosted box", aru-115) is instructions to the box as much as an
agent run is, so it runs under this grant's `runs.instruct`, and an agent
run needs `runs.instruct` as well as `runs.start`.

- **The box checks it.** A session's work carries the project's live grant,
  on every pull, and hosted `arugulad` checks it against what it pinned
  before it takes the session and before each turn.
- **With none,** `@arugula` starts nothing, and the thread says an owner
  gives the project a grant, and how. A grant that expires or goes away with
  no revoke stops new turns, not the session: the turn under way finishes,
  and a follow-up is refused until an owner gives one.
- **A revoke ends live sessions** and refuses follow-ups and new ones.
  `running stop` ends them at once, mid-turn too. `running finish` lets a
  turn under way finish, and its session ends with it; one with no turn
  under way ends at once, and a stop ends a push under way too. The platform
  ends them as the revoke arrives, and the box ends them itself as it hears
  the revoke, without waiting on control. A revoke of a grant another has
  replaced ends nothing that runs under the other.
- **The newest grant counts.** A box that has seen a newer grant refuses an
  older one, by the time each signs; the platform takes a new one only past
  every grant the project has had.

The cost: every project that wants `@arugula` needs an owner's passkey first,
so a project with no grant loses it until one is given.

Where: the standing project grant (Arugula #7, M211, native), `crates/platform/src/grants.rs`.
From: #1317 (platform step 7), the plan in #1290, S37 (q3), M88 (#970, parked in #1002).
