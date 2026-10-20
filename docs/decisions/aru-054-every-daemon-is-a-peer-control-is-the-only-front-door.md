---
schema: 1
id: "aru-054"
title: "Every daemon is a peer; control is the only front door"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Every daemon is a peer; control is the only front door"
options:
  - id: "a"
    label: "Every daemon is a peer; control is the only front door"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M4"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox"
  - title: "No special machines track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#no-special-machines-track-s27-m49m50-added-2026-10-04"
  - title: "M49"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m49-the-cli-and-the-daemon-page-without-a-hub-149"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/hosts.rs"
  - "path:crates/daemon/src/control.rs"
  - "path:crates/control/src/api.rs"
---

# Every daemon is a peer; control is the only front door

A daemon owns its terminals and serves the page, the protocol and the CLI.
Control's page lists every joined machine, direct or relayed; a daemon's own
page shows that daemon, plus its tailnet `hosts.json` for setups without
control. No daemon is a hub: a joined daemon doesn't list or relay for the
account's other machines. Terminal bytes go between a client and the daemon
that owns the terminal.
That is the machine plane ("Two planes, two trust rules", #1122). Control's
front door also serves the platform plane, the plaintext objects that have
to exist while every machine is asleep (team chat, Cloud workspaces), which
hang off a project ("The project is the unit of platform sharing").
Where: `crates/daemon/src/hosts.rs`, `crates/daemon/src/control.rs`, `crates/control/src/api.rs`.
From: [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), [No special machines track](docs/plan-archive.md#no-special-machines-track-s27-m49m50-added-2026-10-04), [M49](docs/plan-archive.md#m49-the-cli-and-the-daemon-page-without-a-hub-149).
