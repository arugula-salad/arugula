---
schema: 1
id: "aru-136"
title: "A machine's kind is one type; control.json keeps its flat shape"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  harness: "claude-code"
question: "The daemon's `Saved` (`control.json`) spread a machine's kind over `team`, `roster`, the team's fields, and `hosted` with `deploy` and `builder` flags, so it could hold combinations no machine can be, and every reader worked out the kind for itself. Once the kind is one type in memory, what does the file look like?"
options:
  - id: "a"
    label: "One type in memory; the file keeps the shape every release has written"
    how: "`MachineKind` is personal, team (the pin and what was checked against it) or hosted (the delegation, its purpose, and the project's team when a team owns it). Reading maps the old flat shape onto it and refuses what no kind can be; writing spreads it back out, flags and all."
    tradeoff: "A mapping to maintain beside the type, and the file still looks like the old struct."
  - id: "b"
    label: "A tagged `kind` object in the file"
    how: "Serialize the enum as `kind: { personal | team { … } | hosted { … } }` and read the old shape only as a fallback."
    tradeoff: "A daemon downgraded to the release before can't read the file: the machine comes up not joined, and a hosted box re-enrolls with a spent ticket and is deleted."
  - id: "c"
    label: "Leave the struct as it is"
    how: "Keep the optional fields and flags; document which combinations are meaningful."
    tradeoff: "Nothing stops a file or a refresh from holding a roster with no pin, or a box that's both a deploy's and a snapshot's."
choice:
  option: "a"
  reason: "The file is read by every machine, and a downgrade has to keep working during the launch window; the mapping is small and tested against files the release before saved, for each kind and purpose, with what each one let in recorded beside it (Jake, 2026-11-03)."
rejected:
  - option: "b"
    why: "A downgrade that can't read `control.json` is a machine nobody can reach until it's joined again."
  - option: "c"
    why: "It leaves the combinations that shouldn't exist representable, which is what Arugula #672 asked to end."
supersedes: []
evidence:
  - title: "Arugula native #672 (this record)"
    url: "https://app.arugula.io/issues/iss_71e1db88706a64719c8a9ddd35d6595d"
decided_by: "jhgaylor"
decided_on: "2026-11-03"
reviews: []
constrains:
  - "aru-056"
  - "aru-095"
  - "aru-134"
  - "path:crates/daemon/src/control.rs"
  - "path:crates/daemon/src/hosted/trust.rs"
  - "path:crates/daemon/tests/fixtures/control-json"
  - "path:docs/data-model.md"
---

# A machine's kind is one type; control.json keeps its flat shape

`Saved` in `crates/daemon/src/control.rs` is what a machine pinned of
control: its own certificates and removals, and what it is. Before this the
kind was spread over `team: Option<TeamPin>`, `roster`, the team's
certificates, names, former members and lock, and `hosted: Option<HostedPin>`
with `deploy` and `builder` flags. A roster with no pin, a lock on a personal
machine, or a box that was both a deploy's and a snapshot's could all be
written, and each reader (`Enrolled::build`, the drivers, the runs' gate, the
state line) worked out the kind for itself (Arugula #672).

Decided (Jake, 2026-11-03):

- **One type in memory.** `Saved.kind` is `MachineKind`: `Personal`;
  `Team(InTeam)`, the pin a machine joined or moved with and the roster,
  members' certificates, names, former members and lock it checked against
  that pin; or `Hosted(HostedPin)`, the hosted certificate and delegation,
  the box's `Purpose` (people, deploy or snapshot), and the project's
  `InTeam` when a team owns it. A `Purpose` once deploy or snapshot stays
  one. Readers ask `team()`, `roster()`, `hosted()`, `is_hosted()` and
  `is_for(purpose)`.
- **The file keeps its flat shape.** `control.json` is written as every
  release has written it (`OnDisk`): the team's fields at the top level,
  `hosted` with its `deploy` and `builder` flags. A daemon from before this
  reads a file this one saved, so a downgrade keeps the machine joined and
  a hosted box enrolled.
- **What no kind can be doesn't load.** A file with a roster, members'
  certificates or a lock and no team pinned; a hosted machine flagged as
  both a deploy's and a snapshot's; a team pinned that isn't the
  delegation's owner, or none where the delegation names one: reading it
  fails, the daemon logs why and runs as not joined, letting nobody in. No
  release has written such a file: a join pins the team before anything
  of it, a move out clears all of it, and a hosted enrollment is checked
  whole (`hosted::trust::saved_from`).
- **No trust check changed.** Who a certificate must chain to, who gets
  in and as whom, and what only ever accumulates (removals, rosters that
  only move forward, pinned passkey logs) are as they were: `aru-056` and
  `aru-095` stand. The files the release before saved, one per kind and
  purpose, are in `crates/daemon/tests/fixtures/control-json/`, each with
  what that release decided from it beside it (`<name>.who.json`); a test
  loads each, writes it back the same, and decides the same.
