---
schema: 1
id: "aru-006"
title: "Flow control: acks, a window, and a log that absorbs slow clients"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "Flow control: acks, a window, and a log that absorbs slow clients"
options:
  - id: "a"
    label: "Flow control: acks, a window, and a log that absorbs slow clients"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Protocol, flow control"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#protocol-one-protocol-two-transports"
  - title: "#52"
    url: "https://github.com/arugula-salad/arugula/issues/52"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/pane.rs"
---

# Flow control: acks, a window, and a log that absorbs slow clients

A client that attaches with `acks` sends `ack{pane, offset}` about every 64 KB
and is held to a 512 KB unacked window; past it, it is held back and catches
up from the log (or gets `resync` if the log no longer has the gap). A slow
client never pauses the program: the log absorbs its output. The program is
paused only when the daemon itself falls behind, through a bounded queue (64
chunks), and client requests are served before that queue, so Ctrl-C stops a
flood at once. Clients that don't ack (`attach`, the share viewer) get `resync` on a full queue.
Where: `crates/mux/src/pane.rs` (`ACK_WINDOW`, `PROGRAM_QUEUE`).
From: [Protocol, flow control](docs/plan-archive.md#protocol-one-protocol-two-transports), #52.
