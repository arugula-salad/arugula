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
  the box's `Purpose`, and the project's `InTeam` when a team owns it.
  Readers ask `team()`, `roster()`, `hosted()`, `is_hosted()` and
  `purpose()`.
- **A purpose only closes.** `Purpose` is people, deploy, snapshot, or
  both: each of the file's `deploy` and `builder` flags, once set, stays
  set (`Purpose::or`), as the code before kept them. A deploy's box lets
  nobody in and takes one deploy; a snapshot's takes no runs and wipes
  itself after its setup; one told both does all of that. A refresh can
  close a box further and never open it.
- **The file keeps its flat shape.** `control.json` is written as the code
  before wrote it (`OnDisk`): the team's fields at the top level, `hosted`
  with its `deploy` and `builder` flags, both set for a box told both. A
  daemon from before this reads a file this one saved and decides the
  same, so a downgrade keeps the machine joined and a hosted box enrolled.
- **What no kind can be doesn't load, and nothing touches it.** A file
  with a roster, members' certificates or a lock and no team pinned; a
  team pinned that isn't the hosted delegation's owner, or none where the
  delegation names one. Reading it fails with a line that says why and
  what to do; the daemon runs as not joined, letting nobody in through
  control, and leaves the file and the key as they are. A hosted box with
  such a file enrolls nothing: its claim is spent, and the workspace is
  rebuilt. `arugulad leave` forgets such a file, telling the control it
  names, so a machine can join again. No release from 0.43 on has
  written such a file: a join pins the team before anything of it, a move
  out clears all of it, and a hosted enrollment is checked whole
  (`hosted::trust::saved_from`).
- **No trust decision changed.** Who a certificate must chain to, who
  gets in and as whom, who drives a hosted machine and calls its agents,
  who may sign for its project, which other hosted machines it accepts,
  and what only ever accumulates (removals, rosters that only move
  forward, pinned passkey logs, a box's purpose) are as they were:
  `aru-056` and `aru-095` stand. Files the code before wrote, one per kind
  and purpose, are in `crates/daemon/tests/fixtures/control-json/`, each
  with what that code decided from it beside it (`<name>.who.json`,
  `<name>.accept.json`), and the README there says how they were made; a
  test loads each, writes it back the same, and decides the same.

**Amended by aru-141, "Machines trust control: the hub's tokens replace device keys"** (Jake, 2026-11-13). What a machine pins changes: no root, roster, members' certificates or hosted certificate, only the hub's public keys and its own identity. `MachineKind` keeps Personal, Team and Hosted for what a machine is; the trust it carried goes with the key code.
