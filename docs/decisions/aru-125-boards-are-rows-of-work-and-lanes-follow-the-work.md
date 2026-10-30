---
schema: 1
id: "aru-125"
title: "Boards are rows of work, and lanes follow the work"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "How does a board stay true to the work without anyone tending it?"
options:
  - id: "a"
    label: "One row is one piece of work; its lane is read from live state, and proof is a mark"
    how: "A board is rows. A row's item is an issue or a PR; a PR, an agent block or a run is attached to it as work, a run or a link as proof (board_attachments). A read answers four fixed lanes (Next, Doing, Review, Done) worked out from the items' state and never stored: Done when the issue is closed or the PR merged or closed, Review when a PR is open, Doing when an agent block isn't closed or a run is running, otherwise Next. A row with no issue or PR is a note and stays in Next. A Done row is proven by CI (a PR merged with every check completed and none red) or by evidence (a proof attachment), or not proven yet. Nobody moves a row, people and agents alike; zones, moves and statuses (but needs_input) are refused. Adding a PR that names the issue of a row attaches it to that row; an item already on the board isn't added again. Boards from before move in place, once."
    tradeoff: "A board no longer says what a person wants it to say: an item in the wrong lane is fixed by fixing the work. A lane is worked out for readers who can't read an item too, so Review shows for a private PR whose title they never see."
  - id: "b"
    label: "Keep zones that people and agents place rows in"
    how: "Zones and statuses stay as they are, with lanes as a default set of zones."
    tradeoff: "A board is only as current as the last hand that tended it, which is what made them go stale."
  - id: "c"
    label: "Keep the old view beside the new"
    how: "Boards made with zones keep their zones; new boards get lanes."
    tradeoff: "Two models in every reader and writer, for a feature still behind a Labs flag."
choice:
  option: "a"
  reason: "Decided by alecraso, 2026-10-30, on the proposal \"Boards, made opinionated\": boards are the connective tissue between issues, PRs, agents and runs, so they must follow them. Fixed lanes on every board; nobody moves rows; proof is a mark on a Done row, not a zone; a PR may be a row's item, but the tool attaches rather than duplicating; migrate in place with no old view kept."
rejected:
  - option: "b"
    why: "Hand-placed rows are what go stale; the work already knows where it is."
  - option: "c"
    why: "Boards are behind a Labs flag, so migrating in place costs less than carrying two models."
supersedes: []
evidence:
  - title: "Arugula native #365 (B1: the boards model)"
    url: "https://app.arugula.io/issues"
decided_by: "alecraso"
decided_on: "2026-10-30"
reviews: []
constrains:
  - "aru-123"
  - "aru-122"
  - "path:crates/platform/src/boards.rs"
  - "path:crates/platform/src/boards/lanes.rs"
  - "path:crates/platform/src/db/boards_lanes.rs"
  - "path:crates/platform/migrations/postgres/0072_board_lanes.sql"
---

# Boards are rows of work, and lanes follow the work

Boards are the connective tissue between a project's issues, PRs, agents and
runs (aru-122, aru-123). This makes them follow those, not a hand.

- **A row is one piece of work.** Its item is an issue (native, GitHub or
  Plane) or a PR. A row with no item, or a link, is a note: never counted,
  never moved by state.
- **Attachments** (`board_attachments`): `work` is a PR, an agent block or a
  run; `proof` is a run or a link. They are read through the same checks as a
  row: what a reader can't read says nothing in its chip, though the lane
  may still move.
- **Lanes** are read, not stored, in the order Done, Review, Doing, Next
  (`crate::boards::lanes`). A read answers them as four zones, `lane_next`,
  `lane_doing`, `lane_review`, `lane_done`.
- **Proof** is a mark on a Done row: by CI, by evidence, or not proven yet.
  Progress counts the rows with an item, those in Done, and those proven.
- **Nobody moves rows.** A move, a zone or a status (but `needs_input`, until
  asks replace it) is refused: "lanes follow the work".
- **No duplicates.** A PR that names the issue of a row (`Arugula #N`,
  `Closes #N`, `(#N)`, a branch `feat/N-…`, `IDENT-N` for Plane) is attached to
  that row; an issue that PR rows name takes them in; an item already on the
  board is a no-op that says where.
- **Migration in place:** zones go, and PR, block, run and proof rows that
  reference an issue row become its attachments; the rest stay as rows.
