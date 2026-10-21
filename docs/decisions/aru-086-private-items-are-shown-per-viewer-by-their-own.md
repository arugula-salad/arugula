---
schema: 1
id: "aru-086"
title: "Private items are shown per viewer, by their own GitHub access"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1205"
question: "Who may see a mirrored PR or issue from a private repo linked to a project?"
options:
  - id: "a"
    label: "Every read checks the viewer's own GitHub access; project membership alone never shows a private item"
    how: "Lists, items, checks, events, content, drafts, the inbox, pushes and search all go through one viewer check. A public repo's items show to any project member. A private repo's show only when GitHub says the viewer has access: may(), by numeric GitHub id, kept 10 minutes and dropped on member, membership, organization and repository events. A member with no linked GitHub account sees no private items."
    tradeoff: "A GitHub call per viewer and repo every 10 minutes, and every read path has to go through the one check."
  - id: "b"
    label: "Project membership shows every item of the project's linked repos"
    how: "Whoever linked the repo vouches for the project; members see what it mirrors."
    tradeoff: "Someone GitHub would refuse sees private code review because they're on a project."
  - id: "c"
    label: "Public repos only at first"
    how: "Link only public repos until a viewer check exists."
    tradeoff: "Most teams' work is in private repos, so the mirror would be no use to them."
choice:
  option: "a"
  reason: "GitHub stays the authority over who reads a private repo, so linking a repo never widens who sees it, and private repos are in from the start (Jake, 2026-10-21, #1204)."
rejected:
  - option: "b"
    why: "It turns a project into a way round GitHub's access, and a removed collaborator would keep reading."
  - option: "c"
    why: "Leaves out the repos teams use; the viewer check makes private repos safe from day one."
supersedes: []
evidence:
  - title: "#1204"
    url: "https://github.com/arugula-salad/arugula/issues/1204"
  - title: "#1205"
    url: "https://github.com/arugula-salad/arugula/issues/1205"
  - title: "#1211"
    url: "https://github.com/arugula-salad/arugula/issues/1211"
  - title: "#1217"
    url: "https://github.com/arugula-salad/arugula/issues/1217"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/control/src/forge.rs"
  - "path:crates/platform"
---

# Private items are shown per viewer, by their own GitHub access

Every read of a mirrored item checks the viewer, never just the project
(Jake, 2026-10-21, #1204). That covers lists, single items, checks, events,
content, drafts, the inbox, pushes and search, through one check.
- **Public repos:** any member of the project the repo is linked to sees the
  items.
- **Private repos:** a viewer sees an item only if GitHub says they have
  access. That's `may()`, by numeric GitHub id, with answers kept 10
  minutes. The cache is dropped on `member`, `membership`, `organization`
  and `repository` events, so a removed collaborator is cut off at the next
  event, not after 10 minutes.
- **No linked GitHub account,** no private items.

Project membership alone never shows a private item. A project is the unit
of platform sharing ("The project is the unit of platform sharing"), but
GitHub stays the authority over who reads a private repo, so linking a repo
to a project never widens who sees it. Private repos are in from the start,
behind this check.

An inbox entry or a push never carries a private item's title to someone the
check refuses; pushes go through the hub's ("Push stays on the hub's app; a
push opens the platform page"). Nothing is readable through the platform
until the check is in (M165), and leak tests (across projects, after a
collaborator is removed, with no linked account, after an unlink, and in
search) keep it so.
Where: `crates/control/src/forge.rs` (`may()`, until M161 moves it to `crates/platform/`).
From: #1204 (platform roadmap, step 4), M159 (#1205), M165 (#1211), M171 (#1217).
