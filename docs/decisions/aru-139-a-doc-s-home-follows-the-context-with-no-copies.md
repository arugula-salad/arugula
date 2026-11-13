---
schema: 1
id: "aru-139"
title: "A Doc's home follows the context, with no copies"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Where does a Doc live, and what does sharing it with a team do?"
options:
  - id: "a"
    label: "The home follows the context, and nobody sets it"
    how: "Personal (your own machine, with or without an account or the relay): the Doc lives on the machine, only there. In a team (a team pane, a session or a team machine): the Doc lives in the team's project on Arugula's cloud. Joining a team is the one hand-off, with a plain notice: \"Docs you share with the team are stored on Arugula's cloud.\" Moving a personal Doc in, by onboarding's checklist or Move to team, takes it off the machine and leaves a hidden pointer, so old links and panes follow it. A Doc has one home, with no copies."
    tradeoff: "Anyone in a team has their team Docs on Arugula's cloud, and the notice says so once. Anyone who refuses that is handled if they show up."
  - id: "b"
    label: "A personal cloud copy of the machine's Docs (copy-up, aru-135)"
    how: "A machine copies each Doc up to its owner's personal project, read-only there, so the web reads it with the machine asleep."
    tradeoff: "Two lists and two counts for one library (34 and 50), and a team's machine never copied."
  - id: "c"
    label: "A team snapshot made from a copy, with Update copy (#418)"
    how: "Share with… makes a team Doc from the copy as it was, and Update copy replaces it."
    tradeoff: "Two versions of one Doc that drift."
  - id: "d"
    label: "A setting for where Docs live"
    how: "Each person or team picks machine or cloud."
    tradeoff: "A choice nobody should have to make."
choice:
  option: "a"
  reason: "The sell is that everything, Docs included, works on your own machine with no account. The team step is the only transition, and keeping one path removes the caveats, warnings and hedging about copies (Aaron, 2026-11-10)."
rejected:
  - option: "b"
    why: "It gave two counts and two lists, 34 against 50, and a team's machine never copied."
  - option: "c"
    why: "Two versions of one Doc drift apart."
  - option: "d"
    why: "It is a choice nobody should have to make."
supersedes:
  - decision: "aru-135"
evidence:
  - title: "Arugula #832 (the spike: One Docs view, and where a Doc lives)"
  - title: "Arugula #861 (this record)"
decided_by: "alecraso"
decided_on: "2026-11-10"
reviews: []
constrains:
  - "path:crates/arugulad-docs/src/docs/moving.rs"
  - "path:crates/platform/src/project_docs.rs"
  - "path:crates/platform/src/project_docs_machine.rs"
  - "path:web/src/shared/docs/move-docs.ts"
  - "path:web/src/platform/pages/doc-actions.tsx"
---

# A Doc's home follows the context, with no copies

A Doc used to live on its machine and be copied up to the platform
(aru-135), and Share with… made a team Doc from a copy (#418). A person had
two lists for one library, and a Doc could exist in two versions. Decided
(Aaron, 2026-11-10), from the spike (#832) and #861:

- **Personal: the Doc lives on the machine, only there.** That holds with
  or without an account or the relay. Everything, Docs included, works on
  your own machine with no account.
- **In a team: the Doc lives in the team's project on Arugula's cloud.**
  A team pane, a session and a team machine all make team Docs there.
- **Nobody sets it.** The context decides. There is no setting and no
  option on `doc new`.
- **Joining a team is the one hand-off.** It says once: "Docs you share
  with the team are stored on Arugula's cloud." Moving a personal Doc in,
  by onboarding's checklist or Move to team, takes it off the machine. It
  leaves a hidden pointer, so old links and open panes follow the Doc.
- **A Doc has one home, with no copies.** The personal cloud copy, Mine
  copies, Share with… as a snapshot and Update copy went with #865, and
  aru-135 with them: it is superseded. A personal project's Docs on the
  platform are offered Move to a team, or Download.

Anyone who refuses cloud Docs in a team is handled if they show up.

Where it changes, today: `crates/arugulad-docs/src/docs/moving.rs`,
`crates/platform/src/project_docs.rs`,
`crates/platform/src/project_docs_machine.rs`,
`web/src/shared/docs/move-docs.ts` and
`web/src/platform/pages/doc-actions.tsx`.
