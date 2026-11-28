---
section: Fixed
title: A board says when its lead isn't reachable
line: A board says when its lead isn't reachable, in board_read and on its page.
headline: no
---
A board whose lead is offline, or whose lead's session was lost, now says so after the lead's name, in `board_read` and on the board's page, instead of naming it as if it were live. A lost lead says how to take the lead again, with `board_write` `lead_board`.
