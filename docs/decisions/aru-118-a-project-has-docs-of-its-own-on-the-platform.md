---
schema: 1
id: "aru-118"
title: "A project has Docs of its own, on the platform"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Where does a Doc live when a project's people, or a session's agent on its Cloud workspace, make it for the project: on a machine, as Docs are, or on the platform with the project's issues?"
options:
  - id: "a"
    label: "Project Docs on the platform: plaintext, in the project, read by its members"
    how: "A project Doc (pdc_) is a platform object, as a native issue is: a title and an ordered list of sections, each a heading and its Markdown (the machine Doc's shape), kept in control's database in plain text. Its project's members read it on the platform page (/p/<project>/docs/<id>), drawn by the Doc block's renderer; owners and editors make, write and delete; viewers read. An agent session makes one with its asker's rights, as itself, and writes only its own. A section is at most 64 KB, a Doc 40 sections and 1 MiB, as a machine Doc's."
    tradeoff: "A third place a Doc can be, beside a machine's library and its share link, and control reads this one, as it reads issues and chat."
  - id: "b"
    label: "Keep Docs on machines: a session's Doc in its box's library"
    how: "The box's daemon makes the Doc in its own library, in a block beside the session's agent block, after the platform says the asker may; the project's members open it on the box through control."
    tradeoff: "A Doc lives on one box, goes with a rebuild unless the box keeps its state, can't be read from another of the project's boxes, and its link opens a block, not a page."
  - id: "c"
    label: "No Docs for sessions: the agent opens an issue instead"
    how: "A session's agent answers \"copy this to a native doc\" with a native issue holding the text."
    tradeoff: "An issue is something to do, not a document to read; the ask stays unanswered."
choice:
  option: "a"
  reason: "The session Docs ask (Jake, 2026-10-27): in #general Jake asked a session's agent to copy something over to a native doc, and it had nowhere to put one. A project's write-ups belong with its issues, readable with every machine asleep, and the session is the authority for its agent's writes as it is for its issues (aru-115)."
rejected:
  - option: "b"
    why: "A project's Doc would be one box's, gone with it, and reached only through it."
  - option: "c"
    why: "It doesn't do what was asked."
supersedes: []
evidence:
  - title: "PR #1584"
    url: "https://github.com/arugula-salad/arugula/pull/1584"
decided_by: "jhgaylor"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "aru-115"
  - "aru-088"
  - "path:crates/control-wire/src/project_docs.rs"
  - "path:crates/platform/src/project_docs.rs"
  - "path:crates/platform/src/db/project_docs.rs"
  - "path:crates/platform/migrations"
  - "path:crates/daemon/src/mcp/session.rs"
  - "path:web/src/platform/pages/docs.tsx"
---

# A project has Docs of its own, on the platform

The session Docs ask (Jake, 2026-10-27). A Doc so far was a machine's: an
agent builds it with the `doc` tool in its machine's library, and sharing
it makes an encrypted link control can't read (#1064). A project had no
place for a Doc of its own, so a session's agent on a Cloud workspace,
asked in a thread to "copy this over to a native doc", had nowhere to put
one.

- **What it is.** A project Doc (`pdc_` and 32 hex characters) is a
  platform object in its project, like a native issue ("A project owns
  numbered issues", aru-088): a title and an ordered list of sections,
  each a heading and its Markdown, pending with an intent until written,
  which is a machine Doc's shape. It's plain text in control's database,
  as issues and chat are, and control reads it. It has no pictures of its
  own.
- **Who.** Its project's members read it (a personal project's account,
  a team's roster). A team's owners and editors make, write and delete
  Docs; viewers read. Anyone else gets 404, and a Doc is named with its
  project in every path, so one in another project is no Doc at all.
  Nothing shares one outside the project: there is no link but the page's.
  Native Docs come with chat, as native issues do.
- **An agent session** (aru-115) makes a Doc with its asker's rights, as
  itself: no maker, the session where an agent's machine would be, shown
  as `Agent session <id>`. It writes only the Docs it made (people who may
  write may write those too), reads any of the project's, and never
  deletes. A review session reads only. The tools are on its own MCP
  listener and go through the session route, which names the session and
  never a project.
- **Its page** is on the platform page, `/p/<project>/docs/<id>`, drawn by
  the Doc block's renderer (charts and drawings included); the project's
  home lists its latest Docs. A write pushes an object change on the Doc's
  thread, so an open page reads it again.
- **Size.** A section is at most 64 KB, a Doc 40 sections and 1 MiB, as a
  machine Doc's; a project has at most 1,000.

What doesn't change: a machine's Docs stay in its library, its share links
stay end-to-end encrypted, and nothing copies one into a project yet.

Where: `crates/control-wire/src/project_docs.rs` (the wire),
`crates/platform/src/project_docs.rs` (who may do what, the routes),
`crates/platform/src/db/project_docs.rs` and migration `0064_project_docs`
(storage and the caps), `crates/platform/src/sessions/issues.rs` and
`crates/daemon/src/mcp/session.rs` (a session's tools), and
`web/src/platform/pages/docs.tsx` (the page).
