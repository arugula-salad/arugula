---
section: Added
title: wait takes an event, as watch does
line: An agent's wait can block on an event from watch's catalog (issue.closed, agent.idle) with a match, and answers with the message that watch would send.
headline: no
---
`wait {event, match, timeout}` is a short-lived watch: it blocks until a matching event happens, answers at once if it already has, and at its timeout says "still running" and cancels the watch. This machine's own types (`agent.idle`, `pane.exited` and the rest) need no account; the cloud's need the machine joined.
