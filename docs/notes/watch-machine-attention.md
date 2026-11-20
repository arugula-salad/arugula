---
section: Added
title: Agents watch a machine's agents ask and go idle
line: "An agent anywhere, a cloud one included, can be woken when an agent block on one of your machines asks you something or finishes its turn."
---
`watch` with `event` set to `session.needs_input` (with `kind`: ask or permission) or `session.idle`, matched by `session` (`<machine>/<pane>`), `project`, `machine` or `pane`. They come from the state your machines already report for boards, so they need the project's boards.read grant, and they say only which block, its kind and when: never what it asks, a command or its output.
