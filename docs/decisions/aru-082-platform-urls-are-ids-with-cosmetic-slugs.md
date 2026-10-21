---
schema: 1
id: "aru-082"
title: "Platform URLs are ids with cosmetic slugs"
state: "decided"
area: "Clients"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1172"
question: "What do the platform page's paths name: ids, names, or both?"
options:
  - id: "a"
    label: "Ids with a cosmetic slug, and only the id is read"
    how: "/p/prj_8f2k-arugula/chat/…: the router reads the id before the first hyphen after the prefix; links are written with the current slug."
    tradeoff: "URLs carry an opaque id."
  - id: "b"
    label: "Names only"
    how: "/p/arugula/chat/general."
    tradeoff: "A rename breaks every link, bookmark and old notification."
  - id: "c"
    label: "Ids only"
    how: "/p/prj_8f2k/chat/…."
    tradeoff: "Links say nothing to a person reading them."
choice:
  option: "a"
  reason: "A rename never breaks a link, and links still read as what they point at (Jake, 2026-10-20, #1171)."
rejected:
  - option: "b"
    why: "Renames break links."
  - option: "c"
    why: "Nothing for a person to read."
supersedes: []
evidence:
  - title: "#1171"
    url: "https://github.com/arugula-salad/arugula/issues/1171"
  - title: "#1172"
    url: "https://github.com/arugula-salad/arugula/issues/1172"
  - title: "#1176"
    url: "https://github.com/arugula-salad/arugula/issues/1176"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:web/src/platform"
---

# Platform URLs are ids with cosmetic slugs

The platform page's paths are ids with cosmetic slugs (Jake, 2026-10-20,
#1171), for example `/p/prj_8f2k-arugula/chat/…`. The router reads only the
id, so a rename never breaks a link; links are written with the current
slug, and a stale slug still lands. The routes are `/`, `/p/:project`,
`/p/:project/chat/:room`, `/dm/:room`, `/p/:project/workspaces/:id`,
`/agents` and `/settings`. Routes for the inbox, issues and changes wait
for steps 4 and 6 rather than shipping empty. Ids keep the prefixes "One
vocabulary: Project, with prj_ ids" sets.
Where: `web/src/platform/`.
From: #1171, M146 (#1172), M150 (#1176).
