---
section: Fixed
title: Cloud runs and sessions end
line: A cloud workspace's runs and agent sessions now end, saying why, when the workspace fails or its machine is removed, and a run that goes past 6 hours fails.
headline: no
---
A run on a cloud workspace stayed "running", and an agent session kept waking its box, when the workspace failed or an owner removed its machine at Arugula control. Now:

- A workspace whose machine was removed fails within a minute, saying so, and its box is paused until a rebuild or a delete.
- A run on a failed workspace fails, and so does a run still going 6 hours after it started.
- A session on a failed workspace stops. One that's done and waiting for nothing stays, so a rebuild still moves it to the new box.
- A session a rebuild didn't move to the new box is moved again, and fails if it still can't move after 15 minutes.
- A failed run's inbox entry and push say why in words, and so do a stopped session's card and its history.
