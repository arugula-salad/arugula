---
schema: 1
id: "aru-042"
title: "Uploads land in a per-pane folder on the pane's host"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Uploads land in a per-pane folder on the pane's host"
options:
  - id: "a"
    label: "Uploads land in a per-pane folder on the pane's host"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Images track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#images-track-s32-m70m72-added-2026-10-05"
  - title: "M70"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m70-images-into-terminal-panes-249"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/upload.rs"
  - "path:web/src/upload.ts"
---

# Uploads land in a per-pane folder on the pane's host

A file from any client goes to `illogical-uploads/<pane>/` under
`$XDG_RUNTIME_DIR` (else `$TMPDIR`, else `/tmp/illogical-<uid>`), mode 0700,
files `O_EXCL` 0600, named by the daemon, swept after 24 h, capped at 20 MB a
file and 200 MB a host. The daemon pastes the path into the pane. The state
dir is never used: it holds secrets and a VM pane can't see it.
Where: `crates/daemon/src/upload.rs`, `web/src/upload.ts`.
From: [Images track](docs/plan-archive.md#images-track-s32-m70m72-added-2026-10-05), [M70](docs/plan-archive.md#m70-images-into-terminal-panes-249).
