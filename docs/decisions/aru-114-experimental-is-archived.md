---
schema: 1
id: "aru-114"
title: "experimental is archived"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "What happens to experimental, its staging app and its unbuilt ideas?"
options:
  - id: "a"
    label: "Archive the repo, delete its staging Fly app, and post E7–E11 as Ideas discussions"
    how: "experimental has no users and needs no migration. Its staging Fly app is deleted, with Jake asked first because it deletes its volume. The repo is archived, and its unbuilt epics E7–E11 become Ideas discussions on arugula-salad/arugula, where unscheduled ideas are kept."
    tradeoff: "experimental's plan stops being a plan; its ideas wait until arugula schedules them."
  - id: "b"
    label: "Keep it as a sandbox"
    how: "Leave the repo and the staging app as they are."
    tradeoff: "An unused control plane with its own sign-in and secrets stays deployed."
choice:
  option: "a"
  reason: "It has no users, and its control plane is retired (Jake, 2026-10-22, platform step 8, deciding Aaron's part on Aaron's behalf, pending Aaron's review)."
rejected:
  - option: "b"
    why: "arugula is the platform's home."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #36"
    url: "https://app.arugula.io/issues/iss_6a0160a6c5771d3952327254fa646ba6"
  - title: "experimental"
    url: "https://github.com/arugula-salad/experimental"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:docs/platform-features.md"
---

# experimental is archived

experimental is archived (Jake, 2026-10-22, platform step 8, #1337).

- **No migration.** It has no users.
- **Its staging Fly app is deleted,** with Jake asked first: deleting it
  deletes its volume.
- **The repo is archived.**
- **E7–E11 become Ideas discussions** on arugula-salad/arugula, where
  unscheduled ideas are kept.

experimental is Aaron's repo. **Jake decided this on Aaron's behalf,**
pending Aaron's review of this record. **The fallback:** if Aaron wants to
keep experimental's control plane, step 8 stops, and "arugula is the
platform's home; experimental's D3 is retired" is settled first.
Where: experimental (Arugula #36, M238, native), `docs/platform-features.md` (which still links experimental's docs).
From: #1337 (platform step 8, tracked natively in Arugula #22).
