---
schema: 1
id: "aru-029"
title: "The TUI and tmux -CC are removed"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "The TUI and tmux -CC are removed"
options:
  - id: "a"
    label: "The TUI and tmux -CC are removed"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence: []
decided_by: "jhgaylor"
decided_on: "2026-10-18"
reviews: []
constrains:
  - "path:crates/cli/src/main.rs"
  - "path:crates/core/src/mux.rs"
  - "path:crates/vt/src/ghostty/paste.rs"
---

# The TUI and tmux -CC are removed

`arugula tui` (M31, M32: tabs, splits and a sidebar in a terminal, with copy
mode) and `arugula tmux -CC` (M5: the CLI as a tmux control-mode server, so
iTerm2 drew the daemon's tabs as native windows) were taken out of Arugula
on 2026-10-18, Jake's decision. The CLI no longer links `arugula-vt`, so it
builds without libghostty and Zig, and ratatui and the crates only it
pulled in left the build. Revive them from git if they're wanted again:
everything is in the tree at 50daf913 (`crates/cli/src/tui/`,
`crates/cli/src/tmux/`, `crates/vt/src/ghostty/copy.rs`, `view.rs` and
`inspect.rs`, `crates/daemon/tests/integration/tmux.rs` with S11's
transcript, `web/e2e/tui*.spec.ts`, `web/e2e/tmux.spec.ts` and
`testnet/macos/iterm2.sh`). What stays, for a fleet whose versions are
mixed:
- **The wire is unchanged.** An older CLI's TUI or tmux front end still
  attaches to a newer daemon. `ClientMsg::Attach`'s `kitty_keys` and
  `/api/panes/{id}/tail`'s `until` stay; so do `Intent::SetOption`,
  `Options` and `OptionScope` (clients' `@` options, which only the tmux
  front end set), in the protocol and in the saved layout, so a layout
  that has them still loads. Nothing reads them now.
- **Telemetry's `tui`** stays in `Surface` and in `telemetry.set`'s `via`:
  an older daemon or CLI reports it. A WebSocket on the local socket now
  counts as `cli` (it is `arugula attach`, or an older TUI).
- **A `tmux` link to the CLI** (what `arugula tmux -CC` asked for, for
  iTerm2 over ssh) now says the front end was removed and exits 1, rather
  than reading tmux's arguments as the CLI's.
Where: `crates/cli/src/main.rs`, `crates/core/src/mux.rs` (`Options`), `crates/vt/src/ghostty/paste.rs` and `screen.rs` (what the mux still uses).
From: Jake, 2026-10-18.
