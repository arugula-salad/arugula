---
schema: 1
id: "aru-013"
title: "Panes outlive the daemon"
state: "decided"
area: "State and durability"
source:
  via: "cli"
question: "Panes outlive the daemon"
options:
  - id: "a"
    label: "Panes outlive the daemon"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M2b"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m2b-in-place-daemon-upgrade-start-of-daily-use"
  - title: "M58"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m58-panes-survive-daemon-restarts-on-windows-221"
  - title: "#35"
    url: "https://github.com/arugula-salad/arugula/issues/35"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/shim.rs"
  - "path:crates/mux/src/holder.rs"
  - "path:crates/mux/src/sys.rs"
  - "path:crates/mux/src/conpty.rs"
  - "path:crates/daemon/src/install.rs"
---

# Panes outlive the daemon

Each pane's program runs under a shim, so the daemon can be restarted or
upgraded without ending shells. On Linux with systemd the PTY master waits in
the FD store and each pane has its own transient scope; elsewhere the shim
holds the master and lends it to whichever daemon connects, ending the pane
after a grace period if none does. Stopping the daemon still ends its panes.
The shim writes the exit status because a restarted daemon is not the
program's parent. On Windows the shim is a pty host (ConPTY).
Where: `crates/mux/src/shim.rs`, `crates/mux/src/holder.rs`, `crates/mux/src/sys.rs`, `crates/mux/src/conpty.rs`, `crates/daemon/src/install.rs`.
From: [M2b](docs/plan-archive.md#m2b-in-place-daemon-upgrade-start-of-daily-use), S3, #35, [M58](docs/plan-archive.md#m58-panes-survive-daemon-restarts-on-windows-221).
