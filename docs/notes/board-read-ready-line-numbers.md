---
section: Fixed
title: Board text names ready rows by number
line: board_read's Ready line says #N for each row that can start, not an internal pointer.
headline: no
---
The `Ready (n):` line in a board's text view used the rows' internal pointers
(`iss_740b34d7`); it now prints the same `#N` the rows below it do.
