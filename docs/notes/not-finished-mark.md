---
section: Added
title: Mark a ticket not finished
line: A ticket, a native issue or a Plane item, can be marked Not finished with what is left, so a merged PR doesn't close it; clearing the mark closes it if a PR already merged.
needs: control
---
Some tickets need a second PR, or a check after the first merges. Mark the
ticket from its row's drawer (**Mark not finished…**), or let an agent do it
with `board_write`'s `hold` (and `release`). While the mark is on, a merged PR
that names the ticket still attaches to its row but doesn't close the issue or
move the Plane item to Done, and the row stays in Review with a **Not
finished** chip that carries the reason. **Clear the mark** lets it finish:
if a PR that would have closed it has already merged, it closes then, saying
which PR.

This replaces the "merged, not closed" pill. A row whose PR merged while its
issue is still open and that nobody marked shows the same chip, as "Not
finished · no reason given".
