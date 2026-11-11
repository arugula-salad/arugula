---
section: Changed
title: A board row shows its agent session
line: An agent session started on an issue now moves that issue's board rows to Doing while it works, and each row shows the session, its state and a link to its thread.
needs: control
---
A session started on an issue (`session_start` with `issue`, or @arugula on the issue) counts as work under way on every row of that issue, on every board of the project. It is Doing while the session is queued, working or waiting on its person, and while a finished one's PR hasn't been sent; failed and stopped sessions don't count. An open PR still puts the row in Review, a closed issue is Done, and a live session keeps the row from being marked stalled. The row's card and drawer show the session as a chip, and `board_read` names it under the row.
