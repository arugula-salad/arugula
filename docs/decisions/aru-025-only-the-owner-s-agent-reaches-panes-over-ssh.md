---
schema: 1
id: "aru-025"
title: "Only the owner's agent reaches panes over ssh"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "Only the owner's agent reaches panes over ssh"
options:
  - id: "a"
    label: "Only the owner's agent reaches panes over ssh"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "SSH track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#ssh-track-s28-m51m53-added-2026-10-04"
  - title: "M65"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m65-a-pane-for-a-guest-who-has-only-openssh-198-decided-2026-10-04"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/cli/src/ssh.rs"
---

# Only the owner's agent reaches panes over ssh

The fixed `SSH_AUTH_SOCK` path links only to an agent forwarded by the box's
owner. A guest's forwarded agent is never used in the owner's panes, so `git
push` doesn't depend on who attached last.
Where: `crates/cli/src/ssh.rs` (the agent link), `crates/daemon/src/guest_ssh.rs`.
From: [SSH track](docs/plan-archive.md#ssh-track-s28-m51m53-added-2026-10-04), [M65](docs/plan-archive.md#m65-a-pane-for-a-guest-who-has-only-openssh-198-decided-2026-10-04).
