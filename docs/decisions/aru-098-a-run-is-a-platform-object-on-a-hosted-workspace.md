---
schema: 1
id: "aru-098"
title: "A run is a platform object on a hosted workspace"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "What is a run, where does it execute, and who keeps its record?"
options:
  - id: "a"
    label: "A platform object that runs on the project's hosted workspace; the platform writes its record in S37's states, and the chant driver also writes chant's ledger, linked from the run"
    how: "A runs row (run_ id) in a project: its trigger (a schedule, a GitHub event or a person), its spec as an S37 operation and that operation's hash, its state in S37's eight states, who or what started it, its times and exit code. State changes are append-only run_events, each signed by the daemon that reported it. The project's hosted arugulad pulls the operation, writes its own ledger before starting anything (at most once), and runs it in an ordinary end-to-end pane. For a release chant manages, the chant driver writes chant's ledger as chant always does, and the run row links to that entry."
    tradeoff: "A chant release has two records, the platform's run and chant's ledger entry, joined by a link."
  - id: "b"
    label: "chant's ledger is the run record"
    how: "Every run is a chant operation, and the platform reads chant's ledger to show it."
    tradeoff: "Only what chant manages would have a record. A scheduled agent run or a test command has no ledger, and the platform's own state would be in another tool's format."
  - id: "c"
    label: "Only the platform's record; the chant driver skips chant's ledger"
    how: "The chant driver runs chant's steps without writing its ledger, and the run row is the only record."
    tradeoff: "chant's status, gates and history would miss every release the platform ran."
  - id: "d"
    label: "Runs on any machine in the project, personal ones included"
    how: "A run targets any machine, as S37's work requests do."
    tradeoff: "A schedule would wait on a laptop that's asleep, and a run's log would have to stay on it or leave it."
choice:
  option: "a"
  reason: "The platform writes run records using S37's states, so every run has one, chant's or not, and chant keeps writing its ledger for the releases it manages, with the platform linking to the entry (Jake, 2026-10-21, platform step 7)."
rejected:
  - option: "b"
    why: "Most runs aren't chant operations, and the platform's state shouldn't live in another tool's format."
  - option: "c"
    why: "chant's own view of an estate would go wrong."
  - option: "d"
    why: "Out of step 7. Runs need a machine that's awake and a log the platform may read. If runs on personal machines come later, they keep S37's 64 KB result tail."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "S37"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s37-team-permissions/README.md"
  - title: "#971"
    url: "https://github.com/arugula-salad/arugula/issues/971"
  - title: "#1002"
    url: "https://github.com/arugula-salad/arugula/discussions/1002"
  - title: "#1121"
    url: "https://github.com/arugula-salad/arugula/pull/1121"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src"
  - "path:crates/platform/migrations"
  - "path:crates/daemon/src/main.rs"
---

# A run is a platform object on a hosted workspace

A run is something a project has, like an issue (Jake, 2026-10-21, platform
step 7, #1317). It runs on the project's hosted workspace ("A hosted machine
is arugulad in a rented sandbox").

- **Three triggers:** a schedule, a GitHub event (on step 4's ingester), or a
  person (the page, the CLI or MCP). Scheduled and event runs start under the
  project's standing grant ("One standing grant per project starts runs").
- **The platform writes the run record.** A `runs` row with a `run_` id holds
  the trigger, the spec as an S37 operation and its hash, the state, who or
  what started it, the times and the exit code.
- **S37's states** (q4): pending, approved, denied, canceled, expired,
  running, succeeded and failed. A run under the standing grant goes from
  pending to running, and records the grant it ran under.
- **State changes are append-only** `run_events`, each signed by the daemon
  that reported it. A row is never updated or deleted in place.
- **At most once.** Hosted `arugulad` writes its own ledger
  (`<state>/work/ID.json`) before it starts anything, so a run sent twice
  runs once, and a crash can lose a run but never double it.
- **The live view is the ordinary end-to-end pane.** The durable log is "Run
  logs on hosted workspaces are readable by the platform".
- **chant's ledger too.** For a release chant manages, the chant driver also
  writes chant's ledger, as chant always does, and the run row links to that
  entry. chant's status, gates and history keep seeing every release, and the
  platform doesn't copy chant's format. Alex is told, because it
  touches chant.

Runs on personal machines are out of this step. If they come later, they
keep S37's 64 KB result tail, and their output stays end to end.
Where: the platform's runs tables and hosted `arugulad`'s execution contract (Arugula #6 and #8, M210 and M212, native).
From: #1317 (platform step 7; tracked natively as Arugula #1), the plan in #1290, S37 (q3, q4), M89 (#971, parked in #1002), #1121.
