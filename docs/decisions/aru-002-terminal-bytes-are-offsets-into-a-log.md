---
schema: 1
id: "aru-002"
title: "Terminal bytes are offsets into a log"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "Terminal bytes are offsets into a log"
options:
  - id: "a"
    label: "Terminal bytes are offsets into a log"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Decisions table"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#decisions-on-the-briefs-open-questions"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/proto/src/lib.rs"
  - "path:crates/mux/src/pane.rs"
---

# Terminal bytes are offsets into a log

Output frames are `[u8 kind][u32 pane][u64 offset][bytes]`, big-endian. Every
byte has an offset in the pane's log, so a client resumes by offset, and
`tail`, replay and scrollback restore are seeks. A client that is within 1 MB
of the log's end gets a replay; anyone further behind gets a snapshot.
Why: asciicast has no offsets and inflates the data.
Where: `crates/proto/src/lib.rs` (`FrameKind`, `HEADER_LEN`), `crates/mux/src/pane.rs`.
From: [Decisions table](docs/plan-archive.md#decisions-on-the-briefs-open-questions), M1, M2.
