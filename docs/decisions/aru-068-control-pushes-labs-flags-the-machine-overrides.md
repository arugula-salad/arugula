---
schema: 1
id: "aru-068"
title: "Control pushes Labs flags; the machine overrides"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "Control pushes Labs flags; the machine overrides"
options:
  - id: "a"
    label: "Control pushes Labs flags; the machine overrides"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#347"
    url: "https://github.com/arugula-salad/arugula/issues/347"
  - title: "#828"
    url: "https://github.com/arugula-salad/arugula/issues/828"
decided_by: "jhgaylor"
decided_on: "2026-10-12"
reviews: []
constrains:
  - "path:crates/proto/src/flags.rs"
  - "path:crates/control-wire/src/team.rs"
  - "path:crates/control/src/labs_flags.rs"
  - "path:crates/control/src/api.rs"
  - "path:crates/daemon/src/control.rs"
  - "path:web/src/shell/ui/control.tsx"
  - "path:web/src/shell/ui/developer.tsx"
---

# Control pushes Labs flags; the machine overrides

An account sets Labs flags for every machine it owns, and a team's owners
for the team's machines, on control's page (Teams…). Control sends both to
each joined machine with its certificates, and nudges the machines a change
reaches. The machine's own word still wins, so a flag can be forced on or
off locally:
- **Precedence, first match wins:** the machine's `flags.json` (the flag by
  name, then `labs` there, then the old marker); control's word for the
  machine's account; control's word for its team; the flag's default.
  Account before team because it's the narrower say: the account owns the
  machine, and a team reaches only the machines added to it.
- **Team flags reach the team's machines only** (`daemons.team`), not the
  members' own machines: a team owner doesn't set what runs on someone
  else's laptop.
- **`guest-ssh` and `agents` are local-only** (`Flag::local_only`): only the
  machine's `flags.json` turns them on or off. They open a way in, or act
  for the machine, so its owner decides there. The daemon ignores control's
  word on them, control refuses to store them (`wire::LOCAL_ONLY_FLAGS`,
  checked against the registry by a daemon test), and its page doesn't
  offer them.
- **Strangers never see Labs on control's page.** The flag lists show only
  for someone with a machine that has Developer settings: a daemon whose
  state dir is unlocked adds `dev-settings` to the features it sends
  control, and `GET /api/flags` answers `dev`.
- **Tri-state locally.** A flag `flags.json` doesn't name follows control;
  *Follow control* in Developer settings (`PUT /api/flags/{name}` with
  `{"on": null}`, `arugulad flags NAME unset`) takes the machine's word
  back. Where `labs` spoke for it, the rest are written out first, so they
  stay on.
- **Kept beside, not in, `flags.json`:** the daemon writes control's answer
  to `flags.control.json` in the state dir and removes it when the machine
  isn't in control, so a machine that leaves (or never joined) is as before.
  A control from before this sends no `flags`, and the daemon keeps what it
  had. Reads stay uncached: `flags::get` reads both files.
- **Control doesn't know the list.** It stores any short lowercase name
  (`labs_flags`, owner `account:ID` or `team:ID`), and a daemon ignores one
  it doesn't have; the page offers the list it was built with (`FLAGS` in
  `proto.gen.ts`). Developer settings says where each flag comes from.
Where: `crates/proto/src/flags.rs` (`explain`, `write_control`, `unset`), `crates/control-wire/src/team.rs` (`TrustAnswer::flags`), `crates/control/src/labs_flags.rs`, `crates/control/src/api.rs` (`daemon_trust`), `crates/daemon/src/control.rs` (`refresh`), `web/src/shell/ui/control.tsx` (`LabsFlagList`), `web/src/shell/ui/developer.tsx`.
From: #347 (decided by Jake, 2026-10-12; who sees the lists and the local-only flags on PR #828, the same day).
