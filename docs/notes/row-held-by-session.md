---
section: Added
title: A row says which session holds it
line: A board row shows the session that last started an agent on its issue, and another session's start_agent on it is refused unless it passes take.
headline: no
needs: control
---
The session that starts an agent on an issue holds its row until the issue closes or it lets go with `board_write` `release_row`. The web row and `board_read` say who holds it. A start from another session, on this machine or another, is refused naming the holder; with `take` the row moves and the old holder is told.
