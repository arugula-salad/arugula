---
section: Added
title: Steps on a board
line: board_read puts a board's open rows in steps by what they wait on, with the longest open chain, and a loop of blockers no longer hangs a read.
headline: no
---
Every read of a board works out each row's step from its open `blocks` edges
and stacked PRs: 1 plus the highest step of the open rows before it. Nothing is
stored. `board_read` prints a `Steps` block after the lanes, each step's rows in
the order of what they unblock, and `Chain:` with the longest open chain; the
JSON carries `step` on each row. Doing and Review rows are step 1. A blocker on
another board counts as one step, and the row's blocked-by mark names that
board and its lead. Rows in a loop of blockers (Plane can make one) share a
step and are marked `in a cycle`.
