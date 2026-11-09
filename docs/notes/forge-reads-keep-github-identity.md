---
section: Fixed
title: A slow hub no longer stalls PR and board reads
headline: no
line: Pull request blocks, board pages and object threads keep your GitHub identity for a few seconds, so a slow control hub doesn't hold each read up.
---
The platform used to ask the hub for your GitHub identity on every read of a linked repo's items. It now keeps the answer for at most ten seconds. Linking or unlinking GitHub still shows within a second, because the hub's change feed says so.
