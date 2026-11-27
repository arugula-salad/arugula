---
section: Changed
title: A board's lead hears when work it waits on moves
line: A board's lead hears when work it waits on, from another board, moves.
headline: no
---
A session that leads a board (`lead_board`, or by making it with `create_board`) gets a standing watch on the new `board.wait.changed` event for that board: a blocker held by another board had its PR merged, go red or leave the merge queue. `lead_board` says so, and `release` cancels the watch. A session with no hooks gets none, and is told.
