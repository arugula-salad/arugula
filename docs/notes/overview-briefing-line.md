---
section: Added
title: The Overview says what happened since you last looked
line: "A line under the Overview's stats counts what happened since you last looked (4 merged · 1 went red · 2 wait on your review), beside your chief of staff's latest sentence."
needs: control
headline: no
---
Clicking it opens the chief of staff's tab. The page counts the line from the event log itself, so it works while your chief of staff is paused. It counts three things. Merged is pull requests merged. Went red is a branch whose check failed, when that branch is main or a card's session's. Wait on your review is pull requests that became ready where you're a requested reviewer now. Control keeps the number your Overview last showed you, per project. It moves on when you leave the Overview or hide its window. A person reads the log through `GET /api/projects/{project}/events`, the agents' read checked against that person's own access. Needs the next server release.
