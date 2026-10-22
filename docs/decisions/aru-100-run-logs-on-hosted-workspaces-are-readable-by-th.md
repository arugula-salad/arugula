---
schema: 1
id: "aru-100"
title: "Run logs on hosted workspaces are readable by the platform"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "Where do a run's full log and artifacts live, and may the platform read them?"
options:
  - id: "a"
    label: "In a bucket the platform can read, for runs on hosted workspaces only, kept 30 days"
    how: "Hosted arugulad uploads the log every few MB and at the end, and artifacts with their digests, through URLs the platform signs for one run's prefix. The bucket deletes them after 30 days. The live view stays the end-to-end pane. The daemon refuses to upload from a personal machine. tables.toml, control-e2e.md and the privacy page say the platform reads them."
    tradeoff: "The platform reads a run's output, which it never can on a personal machine: a stated exception to \"Control can refuse service but can't read\"."
  - id: "b"
    label: "S37's 64 KB result tail only"
    how: "As S37 has it: the exit status, the duration and a 64 KB tail kept on the request, with the full transcript in the pane."
    tradeoff: "A failed run can't be read while its workspace is paused, and artifacts have nowhere to go."
  - id: "c"
    label: "Logs sealed end to end to the project's members"
    how: "The daemon uploads the log encrypted to members' devices, so the platform stores bytes it can't read."
    tradeoff: "A key scheme for every member and device, and the platform couldn't attach a log when it pages someone about a failed deploy."
  - id: "d"
    label: "Personal machines upload too"
    how: "Any machine running a run uploads its log."
    tradeoff: "A laptop's terminal output would reach the platform, which \"Two planes, two trust rules\" keeps on the machine plane."
choice:
  option: "a"
  reason: "A hosted workspace is the project's, rented by the platform, so its runs' output can be read with every device asleep; personal machines stay end to end, and 30 days keeps the bucket small (Jake, 2026-10-21, platform step 7)."
rejected:
  - option: "b"
    why: "Too little to debug a failed run or ship an artifact. It stays the rule for personal machines if runs reach them later."
  - option: "c"
    why: "Much more machinery for output the project's own rented box produced, and the platform needs the log to report failures."
  - option: "d"
    why: "Out: terminal content from a personal machine stays end to end."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "S37"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s37-team-permissions/README.md"
  - title: "#971"
    url: "https://github.com/arugula-salad/arugula/issues/971"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/control/tables.toml"
  - "path:docs/control-e2e.md"
  - "path:crates/platform/src"
---

# Run logs on hosted workspaces are readable by the platform

A run's durable log and artifacts are readable by the platform, for runs on
hosted workspaces only, kept 30 days (Jake, 2026-10-21, platform step 7,
#1317). This is a stated exception to "Control can refuse service but can't
read", amended.

- **Signed uploads.** Hosted `arugulad` uploads the log every few MB and at
  the end, and artifacts with their digests, through URLs the platform signs
  for one run's prefix.
- **30 days,** set on the bucket.
- **The live view stays the end-to-end pane.** Only the durable copy is
  readable.
- **Personal machines are out.** The daemon refuses to upload from one, and
  their terminal content stays end to end ("Two planes, two trust rules").
- **Said where people look:** `crates/control/tables.toml`,
  `docs/control-e2e.md` and the privacy page name the logs and artifacts.

Why it's fine for hosted workspaces: the box is the project's, rented by the
platform, and a failed run or deploy has to be readable with every device
asleep. Which bucket, and its provider, is Jake's call when it's created.
Where: durable run logs and artifacts (Arugula #9, M213, native), `crates/control/tables.toml`, `docs/control-e2e.md`.
From: #1317 (platform step 7), the plan in #1290, S37 (q4's 64 KB tail), M89 (#971).
