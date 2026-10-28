---
schema: 1
id: "aru-122"
title: "Boards manage a project's issues, wherever they live; an epic is a board"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1479"
question: "Where is a project's work managed once its issues can live in Arugula, on GitHub or in another tracker, and what is an epic?"
options:
  - id: "a"
    label: "Boards are where a project's work is managed and an epic is a board; other trackers' issues are mirrored into control, their source stays the record"
    how: "An issue is the unit of work and lives in a source: native (iss_), a forge (fi_) or a tracker (Plane first). A board is a view over issues and everything else a row points at. An epic is a board with an optional goal and done note, its progress counted from its issue rows. Issues still get no parent field: an issue is in an epic by being a row on its board, and may be on several boards. The Issues section becomes the untriaged inbox: a project's issues, from every source it's connected to, that are on no board. A tracker is connected to a project, as a repo is linked, with read and write together; its credentials stay in control, its items are kept in the one forge item mirror, and writes to it (create, comment, close, edit) are explicit actions sent through control, by a person or by an agent under a machine grant. Moving a row never writes to the source, and a source's closed state shows on the row's card without moving it."
    tradeoff: "A board does two jobs (tracker and epic) and the Issues page stops being the list of everything; trackers' items share the forge mirror's shape, so that shape has to fit work items as well as PRs and issues."
  - id: "b"
    label: "An epic is its own object, beside boards"
    how: "An epic (epc_) groups issues and has a goal, a done note and progress; a board may show epics as rows."
    tradeoff: "Two concepts that both mean \"a group of issues\", which compete and have to be kept consistent."
  - id: "c"
    label: "Arugula imports other trackers' issues and becomes their record"
    how: "Connecting a tracker copies its issues into native issues, and Arugula is where they change from then on."
    tradeoff: "Teams keep two trackers in step by hand, or give theirs up; a tracker's own people and automations stop seeing the work."
  - id: "d"
    label: "A machine talks to each tracker with its own credentials"
    how: "The daemon reads and writes Plane or GitHub with keys on the machine, and boards show what machines report."
    tradeoff: "Keys leave control, which the trust line forbids, and the board sees a tracker only while some machine is awake."
choice:
  option: "a"
  reason: "Decided by Jake, 2026-10-27, on the design note \"Boards manage issues, wherever they live\" (Doc %502, sent in a DM), with Aaron's choices: an epic is a board, external trackers are read and write from the start, and GitHub issues come first, then Plane. Jake left the open questions to the author's leans, which this records: one mirror for trackers, a tracker connects to a project, and a source's closed state shows on the card without moving the row. It extends aru-116's third ruling (the board is the tracker, so issues get no parent field) and replaces none of its rulings."
rejected:
  - option: "b"
    why: "Two ways to group the same work. If one board for each epic turns out to be awkward, epics can be split out later."
  - option: "c"
    why: "The source stays the record: teams keep the tracker their people and automations already use, and Arugula is where they see and manage it."
  - option: "d"
    why: "Keys, money and gates stay in the control plane, and a board has to be live while machines sleep (aru-116)."
supersedes: []
evidence:
  - title: "#1479 (live status board, AR-5)"
    url: "https://github.com/arugula-salad/arugula/issues/1479"
  - title: "Arugula #191 (boards step 5)"
    url: "https://app.arugula.io/issues/iss_cb1d02776d75e7a393094c5bc2f1b4f4"
decided_by: "jhgaylor"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "aru-116"
  - "aru-073"
  - "path:crates/platform/src/boards.rs"
  - "path:crates/platform/src/db/boards.rs"
  - "path:crates/platform/src/forge"
  - "path:crates/control-wire/src/boards.rs"
  - "path:web/src/platform/pages/boards.tsx"
  - "path:web/src/shared/boards"
---

# Boards manage a project's issues, wherever they live; an epic is a board

Boards become where a project's work is managed, and issues are what goes
on them (Jake, 2026-10-27). This extends the status board's design
(aru-116, Arugula #187 to #191), whose third ruling already says a board is
the project's tracker, so issues get no parent field. Three rulings:

1. **Boards are where a project's work is managed, and an epic is a board.**
   - A board gets an optional **goal** and a **done note** (the "what a
     person can now do" note an epic closes with). An epic's progress is
     counted from its issue rows, done out of total.
   - Issues still get no parent field. Belonging to an epic is being a row
     on its board, and an issue may be on several boards: rows are
     pointers, not copies.
   - The **Issues** section becomes the **untriaged inbox**: the project's
     issues, from every source it's connected to, that are on no board,
     each one addable to a board from there. An issue's own page stays.
   - Native issues stay, for teams with no tracker and for Arugula's own
     work.
2. **An external tracker's issues are mirrored into control, and their
   source stays the record.**
   - The source owns the issue (title, body, state, assignees, labels,
     comments); the board owns where it sits (zone, order, the row's note
     and status).
   - A tracker is **connected to a project**, as a repo is linked.
     Connecting it grants read and write together. Its credentials (an API
     key, the GitHub App's token) stay in control; a machine or an agent
     never sees them.
   - Its items are kept in **the one forge item mirror**, as a GitHub issue
     is, with a provider and a source id, so boards keep one path for
     ingest, live updates and resolving rows. Whether a tracker's item keeps
     the `fi_` prefix is settled with its provider.
   - Writes (create, comment, close, edit) are **explicit actions** sent
     through control, by a person or by an agent under a machine grant (as
     `board_write` is under `boards.write`). Moving a row to Done never
     closes the issue, and a source's closed state **shows on the row's
     card without moving the row**: zones are left to people, and later to
     rules.
3. **The first providers are GitHub issues, then Plane.** Each is an
   interface in control with a fake. Others (Linear, Jira, Asana) wait
   until someone needs them.

All of it stays behind the `boards` Labs flag until it has been used for
real.

## Phases

1. **Bring together what's there:** one list of a project's issues across
   native and GitHub; a row picker that searches them (and PRs and runs)
   instead of taking a typed id; the untriaged inbox, with Issues folded
   into it in the navigation; issue rows as cards (source, state,
   assignee); a native issue made from a board, in a zone; and the board
   grants in a machine's settings.
2. **Epics as boards:** a goal, a done note and progress from issue rows;
   open epics on the project home; closing an epic writes its done note.
3. **Plane:** a provider in control (an API key held there, work items by
   webhook into the mirror), Plane rows on boards, and create, comment,
   close and edit through control.

Step 5 of the status board (#191) leaves Labs only after phase 1, so boards
don't leave Labs just before their UI is reshaped.
