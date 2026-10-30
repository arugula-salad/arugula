---
schema: 1
id: "aru-124"
title: "Plane is a source of a project's issues, mirrored beside GitHub's"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "How does a Plane project's work come onto a project's boards, and who may write back to it?"
options:
  - id: "a"
    label: "One mirror, a project's sealed key, agents' writes set per connection"
    how: "A project's owner or editor connects a Plane project (tracker_connections) with an API key, sealed in project_secrets and never read back; no machine or agent sees it. Plane's work items are rows of the one mirror, forge_items, with a provider and the connection (repo_id is the connection's negative mirror_id), so a board row on one is a forge_item pointer like a GitHub issue's. Any member of the project reads them, because the key is the project's, not a person's. A completed or cancelled state is closed. People's writes land at once; an agent's need trackers.write, and the connection's agent_writes says whether each waits as a card a person approves (approve, the default) or lands at once (allow). Plane's epics can be imported as boards."
    tradeoff: "Plane sees every write as the key's user, so each write says who it was for in a footer; and a project's members read what the key reads, whoever gave it."
  - id: "b"
    label: "A mirror of its own, and a person's own Plane sign-in"
    how: "Plane items go in tables of their own, read with each person's own Plane token, as GitHub issues are read with each person's gh login."
    tradeoff: "Every reader needs a Plane account and a token, the board code needs a second pointer kind and a second card path, and Plane's API keys are a workspace user's, not a per-person OAuth grant."
  - id: "c"
    label: "Agents write drafts only, as on GitHub"
    how: "An agent's Plane write is always a draft a person sends, as aru-041 has it for GitHub."
    tradeoff: "A board an agent runs on Plane stalls on a person for every comment, where the person may have chosen to trust that connection."
choice:
  option: "a"
  reason: "Decided by alecraso, 2026-10-29, on the phase 3 design (Arugula native #356 to #361). The key belongs to the project, so the project's members share what it reads; one mirror keeps boards, pointers and threads unchanged. Whether agents' writes wait is the connection owner's call, approve by default. GitHub stays draft-then-send (aru-041)."
rejected:
  - option: "b"
    why: "Plane has no per-person grant to read with, and a second mirror doubles the board's code for no gain."
  - option: "c"
    why: "Draft-only is right for GitHub, where writes go out as the person; on a Plane connection the people who own it can choose."
supersedes: []
evidence:
  - title: "Arugula native #356 (P3-1: Connect a Plane project)"
    url: "https://app.arugula.io/issues/iss_8716becc14a585f917cd92f646c82d70"
  - title: "Arugula native #361 (P3-6: Docs and decision aru-124)"
    url: "https://app.arugula.io/issues/iss_c4d0f0853146928f37b332c803de3487"
decided_by: "alecraso"
decided_on: "2026-10-29"
reviews: []
constrains:
  - "aru-122"
  - "aru-041"
  - "path:crates/platform/src/trackers"
  - "path:crates/platform/src/db/trackers.rs"
  - "path:crates/platform/migrations/postgres/0071_tracker_connections.sql"
---

# Plane is a source of a project's issues, mirrored beside GitHub's

aru-122 has boards manage a project's issues wherever they live. This is how a
Plane project's live there (phase 3).

- **Connecting:** a project's owner or editor gives a Plane project and an API
  key in project settings. The key (and a webhook secret) are sealed in
  `project_secrets` and never read back; no machine or agent route touches
  them. A Plane project is connected to one Arugula project at a time.
- **One mirror:** Plane's work items are `forge_items` rows with
  `provider = 'plane'` and their connection; `repo_id` is the connection's
  negative `mirror_id`, so nothing that joins `linked_repos` sees them. A row
  on one is a `forge_item` pointer, so boards, cards and threads don't change.
- **Reading:** any member of the project, since the key is the project's. A
  completed or cancelled state is closed, and counts as done.
- **Writing:** a person's write lands at once. An agent's needs `trackers.write`
  over the project, and the connection's `agent_writes` says whether it waits
  as a card a person approves (`approve`, the default) or lands at once
  (`allow`). Each write ends with a "via Arugula" line naming who it was for.
  GitHub stays draft-then-send (aru-041).
- **Epics:** a Plane epic can be imported as a board, its children as rows.
