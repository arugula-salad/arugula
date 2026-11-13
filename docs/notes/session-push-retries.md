---
section: Fixed
title: A failed session push is tried again
line: When a session's push to GitHub fails (a full disk, say), its box tries again after 1, 5 and 15 minutes, then every 30, and the error clears once a push gets there.
headline: no
needs: control
---
The session keeps showing why its last push failed until a push succeeds. A new turn stops the retries: its own finish step pushes.
