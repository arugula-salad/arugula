---
schema: 1
id: "aru-007"
title: "Size: a tab has one size, set by its owner"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "Size: a tab has one size, set by its owner"
options:
  - id: "a"
    label: "Size: a tab has one size, set by its owner"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Size arbitration"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#size-arbitration"
  - title: "M1"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m1-multiplexer"
  - title: "M5"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m5-tmux-control-mode--cc-front-end"
  - title: "#333"
    url: "https://github.com/arugula-salad/arugula/issues/333"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/core/src/mux.rs"
  - "path:crates/proto/src/lib.rs"
  - "path:crates/mux/src/mux/clients.rs"
---

# Size: a tab has one size, set by its owner

The client that last claimed a tab sets its cols and rows, and the PTYs
follow. Only editors size a tab. A client claims when it opens or switches to
the tab, when it asks ("use this size", the `sized-elsewhere` button), or when
it types there; a plain view (a window resize, a split) only sizes a tab it
owns or nobody does. Other viewers render at the real size, letterboxed on the
desktop and scaled on the phone. The size is never smaller than the split tree
needs. The plan first said "per pane"; the code is per tab.

Typing takes the size only once the owner has been idle for 3 s
(`SIZE_HOLD`; the owner's typing and taking the size both count). Until then
the newcomer types into the owner's size. Such a claim carries `typed: true`;
an older daemon ignores the field and takes it at once. Focusing a window
doesn't claim, and a remote pane claims its host's tab only when typed
into, not because this window owns its home tab. `illogical attach`'s
resize still claims at once: it is one terminal's own size changing.
Why: one person moving between devices; "smallest wins" would make the desktop
suffer whenever the phone is open. The hold is #333: two editors typing in
turn, or one person with two windows, resized the PTY at every handover, and
each resize is a SIGWINCH and a redraw. tmux's "smallest client wins" as a
pairing mode isn't done; add it per session if pairing asks for it.
Where: `crates/core/src/mux.rs` (`Mux::view`, `SizeHold`), `crates/proto/src/lib.rs` (`ClientMsg::View`), `crates/mux/src/mux/clients.rs`.
From: [Size arbitration](docs/plan-archive.md#size-arbitration), [M1](docs/plan-archive.md#m1-multiplexer), [M5](docs/plan-archive.md#m5-tmux-control-mode--cc-front-end), #333.
