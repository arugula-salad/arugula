---
schema: 1
id: "aru-090"
title: "Threads hang off any object"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1264"
question: "Where does the conversation about an issue, a PR or a workspace live, and who can read it?"
options:
  - id: "a"
    label: "A chat room of kind object, scoped to the object's project and targeted at its global id"
    how: "chat_rooms gains kind 'object': scope 'project:<prj_id>', project_id that project, no machine, and the object's global id as target (iss_ a native issue, fi_ a mirrored GitHub PR or issue, cw_ a workspace, later runs and deploys). One room per target, made lazily on its first message, as pane and session threads are. Whoever can see the object sees its thread; for a mirrored GitHub object the viewer must also pass aru-086's own-access check. Numbering, read marks, mentions, search, push and the live stream are chat's. An issue's comments are its room's messages."
    tradeoff: "Visibility gets a third rule, project membership, beside roster role and machine reach, and mirrored objects need the GitHub check on top."
  - id: "b"
    label: "A comments table per kind of object"
    how: "issue_comments, and the like for PRs and workspaces, each with its own numbering and search."
    tradeoff: "Every object re-implements what chat already does, and search and the inbox split in two."
  - id: "c"
    label: "Threads only on teams, panes and sessions"
    how: "As before step 6: talk about an object in a team channel."
    tradeoff: "What people say about a thing isn't kept with the thing."
choice:
  option: "a"
  reason: "Chat already has the shape a thread needs, so a thread on any object is one more room kind, and an issue's comments get mentions, search and push from chat (Jake, 2026-10-21, #1263)."
rejected:
  - option: "b"
    why: "A second comments store duplicates chat and splits search, mentions and the inbox."
  - option: "c"
    why: "Conversations wouldn't hang off what they're about, which is what step 6 is for."
supersedes: []
evidence:
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
  - title: "#1264"
    url: "https://github.com/arugula-salad/arugula/issues/1264"
  - title: "#1267"
    url: "https://github.com/arugula-salad/arugula/issues/1267"
  - title: "#1272"
    url: "https://github.com/arugula-salad/arugula/issues/1272"
  - title: "#1273"
    url: "https://github.com/arugula-salad/arugula/issues/1273"
  - title: "#1305"
    url: "https://github.com/arugula-salad/arugula/pull/1305"
  - title: "#1318"
    url: "https://github.com/arugula-salad/arugula/pull/1318"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/chat.rs"
  - "path:crates/platform/src/db/chat.rs"
  - "path:crates/control-wire/src/chat.rs"
  - "path:crates/platform/migrations"
---

# Threads hang off any object

Any platform object can have a thread (Jake, 2026-10-21, #1263): a native
issue, a mirrored GitHub PR or issue, a hosted workspace, and later a run or
a deploy.

- **One room kind, `object`.** A thread is a `chat_rooms` row of kind
  `object`, scope `project:<id>`, with `project_id` set to the object's
  project, no machine, and the object's global id as its target: `iss_…` a
  native issue, `fi_…` a mirrored GitHub item, `hw_…` a hosted workspace. There is
  one room per target, made on its first message, as pane and session
  threads are (`chat_room_for`).
- **Whoever can see the object sees its thread.** That's a third visibility
  rule, project membership, beside roster role and machine reach ("Chat
  lives in control, in plain text; terminal content doesn't"). For a
  mirrored GitHub object the viewer must also pass their own GitHub access
  check ("Private items are shown per viewer, by their own GitHub access"),
  so a thread never shows more than its item would.
- **An issue's comments are its object thread,** not a second comments
  table. The issue's `issue_events` say what happened to it; its thread holds
  what people and agents said ("A project owns numbered issues").
- **The rest is chat's:** per-room numbering, read marks, mentions, search,
  push and the live stream.
- **Pane and session threads don't change.** They're about machines and
  people, and belong to no project.

Agents read and post in object threads under grants ("Agents are in
conversations").

**Hosted workspaces' threads target `hw_…` ids** (step 5's M179, #1305;
their threads in #1318), not `cw_…`. A `cw_…` is an old Cloud workspace: it
stays visible to its account only, so its thread answers `not_yet`, and
those workspaces are retired in M189.
Where: `crates/platform/src/chat.rs`, `crates/platform/src/db/chat.rs` (`chat_room_for`), `crates/control-wire/src/chat.rs`, `crates/platform/migrations/` (M196).
From: #1263 (platform roadmap, step 6), M193 (#1264), M196 (#1267, #1318), M201 (#1272), M202 (#1273), M179 (#1305).
