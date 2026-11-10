---
id: "boards"
title: "Boards"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/platform/src/boards.rs", "crates/platform/src/boards/agent.rs", "crates/arugulad-control-tools/src/boards.rs", "web/src/platform/pages/boards.tsx", "web/src/platform/pages/triage.tsx", "web/src/shell/blocks/board.tsx"]
paths: ["crates/platform/src/boards", "crates/platform/src/db/boards.rs", "crates/platform/src/db/boards_asks.rs", "crates/platform/src/db/boards_docs.rs", "crates/platform/src/db/boards_epics.rs", "crates/platform/src/db/boards_lanes.rs", "crates/platform/src/db/boards_relations.rs", "crates/platform/src/db/boards_work.rs", "crates/control-wire/src/boards.rs", "crates/daemon/src/board_block.rs", "crates/daemon/src/board_state.rs", "crates/daemon/src/board_attach.rs", "web/src/shared/boards"]
shared: ["crates/platform/src/db/work_items.rs", "crates/platform/src/forge/ingest.rs", "crates/platform/src/forge/poller.rs", "crates/platform/src/grants.rs", "crates/platform/src/chat.rs", "crates/platform/src/router.rs"]
tests: ["crates/platform/src/boards_tests.rs", "crates/platform/src/boards_docs_tests.rs", "crates/platform/src/boards_asks_tests.rs", "crates/platform/src/boards_relations_tests.rs", "crates/platform/src/forge/lanes_tests.rs", "crates/platform/src/forge/lane_set_tests.rs", "crates/platform/src/forge/auto_attach_tests.rs", "crates/platform-chat/src/boards_tests.rs", "crates/arugulad/tests/integration/board_state.rs", "web/e2e/boards-platform.spec.ts", "web/e2e/board-move-rows.spec.ts", "web/e2e/board-docs.spec.ts"]
decisions: ["aru-116", "aru-122", "aru-123", "aru-124", "aru-125"]
---
# Boards

## What a person can do
A project's board is an ordered list of rows, each one piece of work, shown in four lanes (Next, Doing, Review, Done) worked out from the state of that work. People and agents put rows in named groups, attach work and proof to them, link Docs and ask questions on them. A person can also move a row to another lane by hand.

Boards are behind the `boards` Labs flag, resolved by `flag_on`: the account's own setting, else on if any of its teams turned it on. With it off, every board route answers 404.

## Where it runs
Short paths below are under `crates/platform/src`.

**The platform process owns boards**, with every machine asleep. Its tables are `board_entries`, `board_attachments`, `board_docs`, `board_asks`, `board_settings`, `board_block_states`, and `work_relations` for edges between items. A write such as `PUT /api/boards/{id}/entries` goes `put_route`, `put`, `board_place` (the flag and the role), then `crate::db::boards`. It then calls `changed`, which pushes an object change on the board's thread so open pages read again. No route deletes a board: it is closed with a note and can be reopened.

**Who may:** a personal project's account does everything; a team's owners and editors change; viewers read; anyone else gets 404 (`place`).

**A lane is worked out on read**, by `boards/lanes.rs` (`lane`). In order: Done when the primary issue is completed or canceled, or the primary PR merged or closed. Review when a PR is open, the issue is open and an attached PR merged, or a started Plane state's name says review. Doing when the issue is started, an attached agent block isn't closed, or a run is running. Otherwise Next. A row with no issue or PR is a note and stays in Next. A lane uses facts whoever may read them, so a reader who can't see a private PR still sees the row move, and its chip says nothing (`forge/lanes_tests.rs`).

**A person's move** is `PUT /api/boards/{id}/rows/{row}/lane` (`set_lane`), for owners and editors. No agent has this route.
- Next, Doing and Review store a `LaneSet` that shows until the row's derived lane changes.
- Moving to Done closes a native issue, and moving out of Done reopens it. A GitHub or Plane item must be closed or reopened there (422).

**What attaches:**
- `work` is a PR, an agent block or a run; `proof` is a run or a link (`AttachRole::takes`). Done rows are proven by CI, by evidence or not at all (`lanes::proof`).
- A PR attaches itself. `forge/ingest.rs` and `forge/poller.rs` call `auto_attach_prs` (`boards/auto.rs`), which matches the references in `boards/refs.rs` to rows of the project its repo is linked to. It never takes off an attachment a person or agent made.
- A merged PR closes the native issues it names (`forge/closes.rs`), which then moves the row, and the row says which PR closed it and by which words (`Live::closed_with`, `closed_by`). `part of Arugula #N` and any other `#N` in the body, such as `(#N)` in passing, only link.
- Docs are `board_docs` (`boards/docs.rs`): project Docs of the same project, or a machine's Doc by id. A Doc never moves a lane or counts as proof.
- Asks are `boards/asks.rs`. Only the named person, or an owner or editor for them, answers, and never a machine. The board also raises asks on read, from failed checks and blocks waiting for input.
- Relations are `boards/relations.rs`. `blocks` marks a row as blocked and leaves its lane alone. Plane epics become boards through `boards/epics.rs`, when the connection's `epics_as_boards` is on.

**A machine's agents** use `board_read` and `board_write` (`crates/arugulad-control-tools/src/boards.rs`), which call `/api/daemon/boards/...` signed by the machine (`boards/agent.rs`). Each call asks for `boards.read` or `boards.write` over the project, with the owner's role as the ceiling. An agent can't remove or repoint another hand's row, take off another hand's attachment, set a lane or answer an ask.

**The daemon holds no board.** In `crates/daemon/src`:
- `board_block.rs` is a pin that keeps only the board's id, and the page reads the board as the person looking. Making one needs the flag on the machine.
- `board_state.rs` reports its own agent blocks to `PUT /api/daemon/block-state`: state word, branch, short HEAD, pushed, uncommitted count, diffstat. Message text, file names and transcripts are not in the type. A block's state shows on a board only while its machine holds `boards.write` over that project.
- `board_attach.rs` attaches a block started on an issue to that issue's rows. It and the report need the flag and a machine joined to control.

## Compatibility
- A row pointer from a newer build reads as none and is kept (`boards_tests.rs`, `a_pointer_from_a_newer_build_reads_as_none_and_is_kept`).
- `db/boards_lanes.rs` moves boards made with zones to rows and lanes once, at start, one transaction per board. It is a no-op once none are left.
- A note or pointer write names the row's `rev`. A stale one gets 409 `stale_rev`, and the client reads again.
- `BlockStateReport` denies unknown fields, so control refuses a report with a field it doesn't know.
- `board_state.rs` stops for a while after 403 or 404 from control, so an old or ungranted setup doesn't loop.
- A saved board block restores with the flag off and draws nothing until it's on.
- With a machine asleep, its block keeps the state it last reported.

## Not known
- `docs/labs.md` says the `boards` flag gates nothing on the daemon. The code gates the pin block, block reports, attaching and the MCP tools on it.
- I found no expiry on `board_block_states`. A machine that goes away mid-turn may leave its row in Doing. Is that accepted? I traced this from the code and found no test of it.
- Are boards staying behind a Labs flag? What would a stranger get when they leave it? Only a person can say.
