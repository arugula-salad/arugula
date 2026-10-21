---
schema: 1
id: "aru-088"
title: "A project owns numbered issues"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1264"
question: "Where do a project's own issues live, and how are they numbered?"
options:
  - id: "a"
    label: "Platform rows owned by the project, numbered per project, the counter bumped in the insert's transaction"
    how: "An issues table (iss_ ids) with project, number, title, body, state, author, labels, assignees and an optional GitHub origin, and issue_events for the timeline. projects.last_issue is bumped in the same transaction as the insert, as chat_rooms.last_seq numbers a room's messages. Full-text search on title and body as chat does it: FTS5 on SQLite, a tsvector column on Postgres. Plain text on the platform plane, listed in tables.toml."
    tradeoff: "The platform holds issue text in plain text, and a project linked to GitHub now has two places an issue can live."
  - id: "b"
    label: "Numbered per team"
    how: "One counter per team, across all of its projects."
    tradeoff: "A team's projects share one sequence, an account's personal project has no team, and moving a project would clash numbers."
  - id: "c"
    label: "GitHub stays the only issue home"
    how: "Issues are GitHub issues, mirrored by step 4 and read through blocks."
    tradeoff: "A project with no repo, or a person with no GitHub account, has no issues, and agents can only draft them."
choice:
  option: "a"
  reason: "The project is the unit of platform sharing, so numbers follow it, and bumping the counter in the insert's transaction gives numbers with no gaps and no duplicates, as chat's messages have (Jake, 2026-10-21, #1263)."
rejected:
  - option: "b"
    why: "A team isn't the unit of sharing (aru-072): its projects' numbers would interleave, and a personal project has none."
  - option: "c"
    why: "Issues stay GitHub-only and tied to a person's token or a machine; step 6 is where the platform owns them."
supersedes: []
evidence:
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
  - title: "#1264"
    url: "https://github.com/arugula-salad/arugula/issues/1264"
  - title: "#1265"
    url: "https://github.com/arugula-salad/arugula/issues/1265"
  - title: "#1266"
    url: "https://github.com/arugula-salad/arugula/issues/1266"
  - title: "#1270"
    url: "https://github.com/arugula-salad/arugula/issues/1270"
  - title: "#1272"
    url: "https://github.com/arugula-salad/arugula/issues/1272"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/migrations"
  - "path:crates/platform/src/db/projects.rs"
  - "path:crates/control/tables.toml"
---

# A project owns numbered issues

A project has issues of its own, and the platform owns them (Jake,
2026-10-21, #1263). Until now an issue was a GitHub issue, read through a
block on a machine, or since step 4 mirrored ("The platform mirrors linked
GitHub repos: metadata, with content opt-in").

- **Numbered per project, never per team.** `#12` is the twelfth issue of one
  project. The project's `last_issue` counter is bumped in the same
  transaction as the insert, the way chat numbers a room's messages with
  `last_seq`, so numbers have no gaps and no duplicates however many are
  filed at once.
- **Platform rows, in plain text.** An issue has to exist while every machine
  sleeps, so it's platform-plane data ("Two planes, two trust rules"),
  listed in `tables.toml`. Its id is `iss_` and 32 hex characters, in the
  vocabulary's style ("One vocabulary: Project, with prj_ ids").
- **What an issue holds:** its project, number, title, body, state (open, or
  closed with a reason), author, labels, assignees, an optional GitHub
  origin, and when it was created, updated and closed. Its timeline is
  `issue_events`: opened, edited, closed, reopened, labelled, assigned,
  exported. Comments aren't events; they're its object thread ("Threads
  hang off any object").
- **Search** is full text on title and body, as chat's is: FTS5 on SQLite, a
  `tsvector` column on Postgres.
- **Roles come from the project** ("The project is the unit of platform
  sharing"). A personal project's account is its owner. On a team project,
  owners and editors create, edit, close and reopen issues, and viewers read
  and comment. Nobody deletes a single issue: deleting the project takes its
  issues.

**A project's issue home** is native or a linked GitHub repo, and the CLI,
MCP and the page read it to decide where a new issue goes. A project linked
to GitHub keeps GitHub as its home until someone switches it, so nothing
changes for anyone (the lead's call in #1263, which Jake may overturn). The
page says so when a project has issues in both places.

Agents write native issues directly, under a grant ("Agents are in
conversations"). Writes to GitHub stay drafts a person sends ("Forge blocks
use the person's own CLI, and agents draft while people send").
Where: `crates/platform/migrations/` (M194), `crates/platform/src/db/` (`issues`, `projects.last_issue`), `crates/control/tables.toml`.
From: #1263 (platform roadmap, step 6), M193 (#1264), M194 (#1265), M195 (#1266), M199 (#1270), M201 (#1272).
