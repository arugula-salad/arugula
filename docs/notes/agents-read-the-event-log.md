---
section: Added
title: Agents read a project's event log
line: "watch events reads a project's events after a number, oldest first, with where to read on: only those the agent could watch, so an agent woken by a watch can catch up on everything it missed."
headline: no
---
`watch` with kind `events` (`project`, `after`, `types`, `limit`) answers the log's events after `after` in order, as stored (ids and states only), with `next_after` and the log's head. Each one is checked as a watch on it is: its type's grant over the project, and whether its owner may read what it's about now. A hosted session reads its own project's. An `after` older than the 14 days the log keeps says those events are gone. Needs the next server release.
