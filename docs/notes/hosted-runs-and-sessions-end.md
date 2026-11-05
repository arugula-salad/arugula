---
section: Fixed
title: Cloud runs and sessions end
line: A run on a cloud workspace that failed now fails too, saying why, and a session there stops waking it and ends if no rebuild comes within a day.
headline: no
---
A run on a cloud workspace stayed "running", and an agent session kept waking its box, when the workspace failed. Now:

- A run on a failed workspace fails, saying so.
- A run past 6 hours goes on while its box is busy, and fails once its box has been idle or offline for 10 minutes.
- A session on a failed workspace waits without waking it, whatever its state, and a rebuild moves it to the new box. With no rebuild within a day, it stops, saying so.
- A session a rebuild didn't move to the new box is moved again, and fails if it still can't move after 15 minutes.
- A failed run's inbox entry and push say why in words, and so do a stopped session's card and its history.
