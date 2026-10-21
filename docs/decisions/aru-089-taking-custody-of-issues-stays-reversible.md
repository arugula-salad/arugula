---
schema: 1
id: "aru-089"
title: "Taking custody of issues stays reversible"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1264"
question: "Once a project's issues live on the platform, how do people take them back out, and how do native issues and GitHub's relate?"
options:
  - id: "a"
    label: "Export from the first day, and an explicit, per-issue Open on GitHub; never a continuous sync"
    how: "JSON and Markdown export of a project's issues, events and threads ships in the same PR as create. Open on GitHub is a person's action on one issue, sent through aru-085's send path (a one-use user token, else their machine). It sets the issue's GitHub origin and step 4's mirror links the pair. Comments flow both ways only for exported pairs: a person's native comment is sent as them, and an agent's becomes a draft for its owner. Nothing else syncs."
    tradeoff: "An exported issue has two copies whose titles and states can drift; only their comments stay joined."
  - id: "b"
    label: "A two-way reconciler"
    how: "Keep native issues and a linked repo's GitHub issues in step continuously: creates, edits, state, labels and comments, both ways."
    tradeoff: "Two sources of truth that conflict, a standing write path to GitHub, and agents' writes reaching GitHub with no person sending them."
  - id: "c"
    label: "Create first, export later"
    how: "Ship native issues, and add export after the dogfood."
    tradeoff: "Custody is a one-way door until export lands."
choice:
  option: "a"
  reason: "People can always leave with their issues, and GitHub gets only what a person chose to send, one issue at a time (Jake, 2026-10-21, #1263)."
rejected:
  - option: "b"
    why: "A reconciler means two sources of truth, and it would send agents' writes to GitHub without a person; GitHub writes stay drafts a person sends (aru-041)."
  - option: "c"
    why: "Taking custody with no way out is the trap the roadmap names; export ships with create."
supersedes: []
evidence:
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
  - title: "#1264"
    url: "https://github.com/arugula-salad/arugula/issues/1264"
  - title: "#1266"
    url: "https://github.com/arugula-salad/arugula/issues/1266"
  - title: "#1270"
    url: "https://github.com/arugula-salad/arugula/issues/1270"
  - title: "#1275"
    url: "https://github.com/arugula-salad/arugula/issues/1275"
  - title: "#1276"
    url: "https://github.com/arugula-salad/arugula/issues/1276"
  - title: "#1277"
    url: "https://github.com/arugula-salad/arugula/issues/1277"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/forge"
  - "path:crates/control-wire/src"
  - "path:crates/cli/src/cmd/issue.rs"
---

# Taking custody of issues stays reversible

Native issues put a project's issues in Arugula's keeping, so leaving has to
stay easy (Jake, 2026-10-21, #1263).

- **Export ships with create.** A project's issues, their events and their
  threads export as JSON and as Markdown from the API, in the same PR that
  lets anyone create one (M195). An export reads back with every issue,
  event and comment.
- **"Open on GitHub" is explicit and per issue.** A person picks one issue
  and sends it through "Sending as a person uses a one-use user token,
  falling back to their machine", so it appears on GitHub as theirs. That
  sets the issue's GitHub origin, the mirror links the two ("The platform
  mirrors linked GitHub repos: metadata, with content opt-in"), and the
  timeline gets an `exported` event. It's built after the step 7 dogfood
  (M204), and dropped if the dogfood keeps sending people back to GitHub.
- **Comments flow both ways only for exported pairs.** A person's native
  comment on an exported issue is sent to GitHub as them, and a GitHub
  comment reaches the native thread through the mirror. An agent's native
  comment becomes a draft for its owner, and stays native until a person
  sends it ("Forge blocks use the person's own CLI, and agents draft while
  people send").
- **There is never a continuous two-way sync.** No reconciler keeps native
  issues and a repo's GitHub issues in step, and nothing reaches GitHub that
  a person didn't send.
Where: the platform's issue routes (export, M195), `crates/platform/src/forge/` (Open on GitHub, M205), `crates/cli/src/cmd/issue.rs` (`arugula issue export`, M199).
From: #1263 (platform roadmap, step 6), M193 (#1264), M195 (#1266), M199 (#1270), M204 (#1275), M205 (#1276), M206 (#1277).
