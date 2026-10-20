---
schema: 1
id: "aru-024"
title: "A guest types on your machine only by trust, or in a VM"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "A guest types on your machine only by trust, or in a VM"
options:
  - id: "a"
    label: "A guest types on your machine only by trust, or in a VM"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M14"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m14-safe-write-access"
  - title: "M70"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m70-images-into-terminal-panes-249"
  - title: "#959"
    url: "https://github.com/arugula-salad/arugula/issues/959"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/authz.rs"
  - "path:crates/mux/src/mux/who_may.rs"
  - "path:crates/daemon/src/api.rs"
  - "path:crates/daemon/src/upload.rs"
---

# A guest types on your machine only by trust, or in a VM

Write access to a local shell is code execution as your uid. A guest's new
panes run in a VM (or join the tab's), and driving one of the owner's local
panes needs a per-pane, time-limited trust grant. Anything that types into a
pane, including uploads, checks both the role and `MayDrive`; a new route that
types must be added to that check by path.
Instructing an agent and approving its tools are the machine owner's unless
granted (Jake, 2026-10-17, #959): prompting a pane (`prompt`), `cd`, and
answering for it (an approval, a question, its dialog, a command run again,
an edit, a debugger) check `MayDrive` too. A
block or a VM pane is no exception: only a VM the guest started (and so
the agent block they started on it) is theirs to drive. A team's editors on
a team's machine drive by their role, as before. Dismissing a card, and
answering what isn't a shell or an agent here (a workspace's or a studio
box's gate, decision or question, a forge's review or checks, a page's
question), don't drive.
Where: `crates/daemon/src/authz.rs` (`Api::MayDrive`), `crates/mux/src/mux/who_may.rs` (`may_drive_here`), `crates/daemon/src/api.rs` (`may_drive`, `is_review`), `crates/daemon/src/upload.rs`.
From: [M14](docs/plan-archive.md#m14-safe-write-access), [M70](docs/plan-archive.md#m70-images-into-terminal-panes-249), #959.
