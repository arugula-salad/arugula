---
schema: 1
id: "aru-074"
title: "One vocabulary: Project, with prj_ ids"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1123"
question: "What do we call the platform's nouns, in tables, code, ids and URLs?"
options:
  - id: "a"
    label: "\"Project\", with ids prj_…"
    how: "Org (a team's billing and identity home, later), team (people with a signed roster), project (what platform objects hang off), Cloud workspace (cw_…), machine, session, pane. Tables, code, ids and later URLs say project."
    tradeoff: "experimental's product.md says \"App\" for the same thing, so it and Aaron have to change their word."
  - id: "b"
    label: "\"App\", as experimental's product.md has it"
    how: "apps table, app_ ids."
    tradeoff: "Reads as a deployable program, which is one thing a project may hold, not the container of chat, workspaces and secrets."
choice:
  option: "a"
  reason: "A project holds chat, workspaces, secrets and later issues and deploys, more than an app; decided now so the schema and ids don't churn (Jake, 2026-10-20, #1122). This overrides experimental's product.md."
rejected:
  - option: "b"
    why: "Too narrow for what the noun holds."
supersedes: []
evidence:
  - title: "#1122"
    url: "https://github.com/arugula-salad/arugula/issues/1122"
  - title: "#1123"
    url: "https://github.com/arugula-salad/arugula/issues/1123"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/db"
---

# One vocabulary: Project, with prj_ ids

The platform's nouns, in tables, code, ids and later URLs (Jake, 2026-10-20,
#1122):
- **Org:** where a group's billing and identity will live (later steps).
- **Team:** people with a signed roster.
- **Project:** what platform objects hang off ("The project is the unit of
  platform sharing"). Ids are `prj_` and random characters, beside `cw_`
  for Cloud workspaces.
- **Cloud workspace:** a session backed by a cloud machine (`cw_…`).
- **Machine, session, pane:** the machine plane's nouns, unchanged.
- **Grant:** a subject's power over a scope ("Grants, not impersonation").

The word is **Project**, not "App". experimental's `product.md` calls the
same thing an App; this record decides otherwise, and that document and its
readers follow it.
Where: `crates/control/src/db/`.
From: #1122, M127 (#1123).
