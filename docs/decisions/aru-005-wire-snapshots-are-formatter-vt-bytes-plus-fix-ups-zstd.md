---
schema: 1
id: "aru-005"
title: "Wire snapshots are formatter VT bytes plus fix-ups, zstd-compressed on request"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "Wire snapshots are formatter VT bytes plus fix-ups, zstd-compressed on request"
options:
  - id: "a"
    label: "Wire snapshots are formatter VT bytes plus fix-ups, zstd-compressed on request"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S1/S5 follow-ups"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named"
  - title: "#1"
    url: "https://github.com/arugula-salad/arugula/issues/1"
  - title: "#49"
    url: "https://github.com/arugula-salad/arugula/issues/49"
  - title: "#53"
    url: "https://github.com/arugula-salad/arugula/issues/53"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/vt/src/ghostty/wire.rs"
  - "path:crates/proto/src/lib.rs"
  - "path:crates/mux/src/pane.rs"
---

# Wire snapshots are formatter VT bytes plus fix-ups, zstd-compressed on request

On attach or resync a client gets VT bytes that rebuild the pane in a fresh
terminal, because xterm.js needs VT bytes. libghostty's formatter writes most
of them; `wire.rs` fixes what it gets wrong (cursor, saved cursor, blank-cell
colours, hyperlinks, Kitty keyboard flags, alt-screen order). A client that
asks gets them as `SnapshotZstd`, with scrollback capped at what it keeps
(`history`). After a resync a client asks again with `history: 0`: the screen
alone, with a visible "output skipped" rule, so a flood can't keep it
resyncing. Visible-first attach was measured (S10) and is not built.
Where: `crates/vt/src/ghostty/wire.rs`, `crates/proto/src/lib.rs` (`AttachPane`), `crates/mux/src/pane.rs`.
From: [S1/S5 follow-ups](docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named), #1, #49, #53.
