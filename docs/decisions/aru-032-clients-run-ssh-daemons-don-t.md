---
schema: 1
id: "aru-032"
title: "Clients run ssh; daemons don't"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "Clients run ssh; daemons don't"
options:
  - id: "a"
    label: "Clients run ssh; daemons don't"
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
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/cli/src/ssh.rs"
  - "path:crates/cli/src/hosts.rs"
---

# Clients run ssh; daemons don't

The CLI and the desktop app run the system `ssh` themselves, where
the user's agent and prompts are. A daemon never relays ssh (that would make
it a hub and put the user's ssh credentials in a background service). ssh is
used to install and join a box to control; control reaches it after that.
Only the target `user@box` is stored.
Where: `crates/cli/src/ssh.rs`, `crates/cli/src/hosts.rs`.
From: [SSH track](docs/plan-archive.md#ssh-track-s28-m51m53-added-2026-10-04), M51, M52.
