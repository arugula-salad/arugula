---
schema: 1
id: "aru-129"
title: "A View is the account's layout of references"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "How can someone see panes from different tabs and machines side by side, when a tab owns its panes and a pane lives in exactly one tab?"
options:
  - id: "a"
    label: "A layout of references, kept per account, that the daemons don't know about"
    how: "A View's leaves are `<machine>.<pane>`. It's one account pref (`view:<id>`), drawn by the client over one connection per machine that draws only those panes (`client.only`, split out of remote panes as blocks/pane-links.ts). Built by direct manipulation, starting in the swarm."
    tradeoff: "One more connection per machine with a pane in an open View; a terminal's size stays its tab's until #477, so a leaf can be scaled; 9 leaves at most, to fit a pref's 512 bytes."
  - id: "b"
    label: "Let a pane be in several tabs, in the daemon"
    how: "The mux's split trees reference panes instead of owning them."
    tradeoff: "Every intent that assumes a pane has one tab changes, on one machine only, so it still can't cross machines."
  - id: "c"
    label: "Remote blocks in an ordinary tab"
    how: "Build the layout from `remote` blocks in a home daemon's tab (#17)."
    tradeoff: "It lives on one machine, needs a home daemon (control's directory has none), and closing the block closes the pane."
choice:
  option: "a"
  reason: "Jake and Aaron, 2026-11-01: a tab owning its panes is the whole limit, and it has to work across machines. Closing a View must close nothing, and the daemons need no change."
rejected:
  - option: "b"
    why: "Large, and doesn't cross machines."
  - option: "c"
    why: "Tied to one machine, and closes what it shows."
supersedes: []
evidence:
  - title: "Arugula #503 Views: build a layout from any panes and blocks"
    url: "https://app.arugula.io/issues/iss_75f8616207311d405cd42de64a9c4011"
decided_by: "jhgaylor"
decided_on: "2026-11-01"
reviews: []
constrains:
  - "path:web/src/shell/views/model.ts"
  - "path:web/src/shell/views/surface.tsx"
  - "path:web/src/shell/views/store.ts"
  - "path:web/src/shell/views/links.ts"
  - "path:web/src/shell/blocks/pane-links.ts"
  - "path:docs/design/views.md"
---

A View is a layout an account owns whose leaves point at panes that live
somewhere else. It only shows them: closing a View closes nothing, and the
daemon doesn't know Views exist. Building one is direct manipulation, never a
form or a picker, with no naming step.

Every leaf goes through a connection of its own to its machine, the machine
shown too, because the main client is replaced when you switch machines and has
a selected tab a View mustn't disturb. A terminal leaf follows "Size arbitration"
as any other screen does, except that a page's main client and its Views hand a
tab's size to each other without asking, since they're one screen.

Where: `web/src/shell/views/`, `web/src/shell/blocks/pane-links.ts`,
[docs/design/views.md](../design/views.md).

From: native #503.
