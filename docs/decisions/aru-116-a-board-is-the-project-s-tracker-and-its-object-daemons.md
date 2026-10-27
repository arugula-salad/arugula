---
schema: 1
id: "aru-116"
title: "A board is the project's tracker, a platform object daemons report agent blocks to"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1479"
question: "Where does a live status board live, what may a machine tell control for it, and how does it relate to issues?"
options:
  - id: "a"
    label: "A board is a project-scoped platform object; daemons report their own agent blocks' state under a boards.write grant; the board is the tracker, so issues get no parent field"
    how: "A board (brd_) belongs to a project, with ordered zones of rows, each row a pointer (a forge item, a native issue, a run, a Cloud workspace or an agent block) with a one-line note and a status. Control keeps it, so the phone and other machines see it live while machines sleep. A daemon may tell control only about its own agent blocks: the state word, the branch, the short HEAD, whether it is pushed, the number of uncommitted files and the diffstat line. It sends no message text, file names or transcripts, and only under a boards.write machine grant its owner gives. Issues stay flat; the board groups them."
    tradeoff: "Control now holds a little of what an agent block is doing, a step past aru-051, and a project's structure lives in boards and not in issues."
  - id: "b"
    label: "A board is a daemon block"
    how: "The board is a block on one machine, as a terminal or a browser is, and other clients reach it through that machine."
    tradeoff: "The phone and teammates see it only while that machine is awake and reachable, which is when they need it least."
  - id: "c"
    label: "Daemons report their agent blocks in full"
    how: "A daemon sends an agent block's messages, file names and transcript so the board can show them."
    tradeoff: "Control would hold what aru-051 keeps out of it: the content of an agent's work, on every machine that reports."
  - id: "d"
    label: "Issues get a parent field"
    how: "A native issue names a parent issue, and a project's structure is a tree of issues."
    tradeoff: "A second way to group the same work, beside the board, and a tree to keep consistent with it."
choice:
  option: "a"
  reason: "The owner approved the board design: a status board is useful when machines sleep, so it is the platform's, and what a daemon says of an agent block is facts about its state, never its content."
rejected:
  - option: "b"
    why: "A board that disappears when its machine sleeps can't be the shared, live view that phones and other machines need."
  - option: "c"
    why: "Messages, file names and transcripts stay on the machine (aru-051); the board needs only the state."
  - option: "d"
    why: "The board is the project's tracker: it groups issues and everything else a row can point at, so issues need no parent field for now."
supersedes: []
evidence:
  - title: "#1479"
    url: "https://github.com/arugula-salad/arugula/issues/1479"
  - title: "Arugula #187"
    url: "https://app.arugula.io/issues/iss_2eb7c661b2f79e84f07fa7c719949fe1"
decided_by: "jhgaylor"
decided_on: "2026-10-25"
reviews: []
constrains:
  - "aru-051"
  - "aru-090"
  - "path:crates/platform/src/boards.rs"
  - "path:crates/platform/src/db/boards.rs"
  - "path:crates/platform/migrations"
  - "path:crates/control-wire/src/boards.rs"
  - "path:crates/platform/src/chat/objects.rs"
  - "path:crates/daemon/src/board_block.rs"
  - "path:crates/daemon/src/board_state.rs"
---

# A board is the project's tracker, a platform object daemons report agent blocks to

A live status board is a **platform object**, not a daemon block. This is
the approved design for the board (Arugula #187, step 1 of 5), three rulings:

- **A board is a project's object (`brd_`).** It has ordered zones, each
  with ordered rows; a row points at a forge item (`fi_`), a native issue
  (`iss_`), a run (`run_`), a Cloud workspace (`hw_`) or an agent block
  (`{machine, pane}`), or at nothing, with a one-line note and a status
  (open, working, needs input, done, failed, or none). A zone of kind
  `proofs` is kept last. Control stores zones and rows as entries, each with
  a `rev` that a write names, so a stale write is refused (409) and the
  client reads again, as a Doc's `ifRev` works; a status change and a move
  leave the rev alone, so they never conflict with a note. Because control
  holds it, the phone and other machines see it live while machines sleep
  ("Two planes, two trust rules"). A board has a thread like an issue does
  ("Threads hang off any object"). It is behind the `boards` Labs flag, and
  with it off the routes answer 404.
- **A daemon may tell control only about its own agent blocks:** the state
  word, the branch, the short HEAD, whether it is pushed, the number of
  uncommitted files and the diffstat line. It may not send message text, file
  names or transcripts. It does so only under a `boards.write` machine grant
  its owner gives. This is a deliberate step past "Chat lives in control, in
  plain text; terminal content doesn't": control learns what an agent block
  is doing, never what it said or touched.
- **A board serves as the project's tracker,** so issues get no parent field
  for now. A board groups issues with everything else a row can point at.

Where: the tables (migration `0058_boards`) and their rules (`crates/platform/src/db/boards.rs`), the routes (`crates/platform/src/boards.rs`, paths in `crates/control-wire/src/boards.rs`), boards as objects (`crates/platform/src/chat/objects.rs`). A row may also point at a link (`https://` only, at most 2048 bytes, no userinfo; migration `0065`, the URL kept in `target`). A read adds each `fi_`, `iss_` and `run_` row's state now as `live`, from the forge mirror, the issues table and the runs table (a run only when the reader may read it, by the runs routes' own project-membership check), and a row in the `proofs` zone counts as proven, derived on read and never stored over its `status`, when it points at a link, a run that succeeded or a `fi_` that is merged (the page and the 72-column text mark it); and a change to a board (or to an item a row points at) pushes an object change on the board's thread. The page (`web/src/platform/pages/boards.tsx`, view in `web/src/shared/boards/`) and the board pin block (`crates/daemon/src/board_block.rs`, config `{board}`, drawn only when shown, opened by a pane's *Pin a board…* or by an agent's `show {kind: "board"}`) are step 2 (Arugula #188); the block keeps only the board's id, and the page reads and writes as the person. The MCP tools (`board_read`, `board_write`, `crates/daemon/src/mcp/boards.rs`) and the `boards.read` and `boards.write` machine grants are step 3, with the agent routes in `crates/platform/src/boards/agent.rs`. The pushes are step 4 (Arugula #190): the daemon (`crates/daemon/src/board_state.rs`) sends only `BlockState` (`crates/control-wire/src/boards.rs`: the state word, branch, short HEAD, pushed, uncommitted count and the diffstat as three numbers, a type with no field for text, and a body with another field is refused) for its own agent blocks, from the mux's events and, at idle, the turn's work summary, debounced to one request per pane every few seconds and silent for ten minutes after control refuses it. Control takes it on `PUT /api/daemon/block-state` (`boards::agent::put_block_state`) for the machine that signed the request, never one named in the body, and only when that machine holds `boards.write` over a project of its owner's and the flag is on for the account; it keeps it per (machine, pane) in `board_block_states` (migration `0063`). A read shows it as the row's `block` only on a board of a project over which the block's machine holds `boards.write` now, so revoking the grant hides it, and a report pushes an object change for the boards whose rows point at the block. Nothing adds rows for a block: a person or a lead does, with `board_write` and an agent block's pointer.
From: Arugula #187 (native, step 1 of the status board).
