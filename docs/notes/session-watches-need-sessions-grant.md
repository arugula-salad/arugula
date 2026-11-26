---
section: Fixed
title: Session watches ask for the sessions grant
line: Watching a machine's agents ask or go idle needs the sessions grant, not boards.read.
headline: no
---
`watch` with `session.needs_input` or `session.idle` now takes the grant agents already have for sessions (`sessions.start`, `sessions.reply` or `threads.read`) instead of `boards.read`, and the event list says so.
