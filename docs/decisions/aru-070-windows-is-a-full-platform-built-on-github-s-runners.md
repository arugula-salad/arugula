---
schema: 1
id: "aru-070"
title: "Windows is a full platform, built on GitHub's runners"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "Windows is a full platform, built on GitHub's runners"
options:
  - id: "a"
    label: "Windows is a full platform, built on GitHub's runners"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Windows track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#windows-track-s29-m54m60-added-2026-10-05"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/conpty.rs"
  - "path:crates/daemon/src/pipe.rs"
  - "path:.github/workflows/windows.yml"
---

# Windows is a full platform, built on GitHub's runners

The daemon runs panes on Windows (ConPTY, named pipes, a logon task), not just
drives them. Unix-only code stays behind `cfg` gates, and Windows CI runs
clippy and tests on every change (`illogical-control` is excluded).
Where: `crates/mux/src/conpty.rs`, `crates/daemon/src/pipe.rs`, `.github/workflows/windows.yml`.
From: [Windows track](docs/plan-archive.md#windows-track-s29-m54m60-added-2026-10-05), M55-M60.
