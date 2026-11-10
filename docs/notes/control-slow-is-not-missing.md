---
section: Fixed
title: A slow control no longer reads as a missing grant
line: When Arugula control is slow to answer, agents are told to try again shortly, not to ask for a grant they already have, and a long wait on a session keeps waiting.
headline: no
---
`capabilities` used to say a tool was `not_granted`, with the
`arugula grants add` command to fix it, whenever control didn't answer the
grants read in time, right after a daemon restart, say. Now it says
`unreachable` and to try again shortly. Only an answer from control without
the grant says `not_granted`.

`wait` on agent sessions or on a run asks control again and again until its
timeout. One ask that went unanswered (a dropped connection, a timeout) used
to end the whole wait with "can't reach Arugula control". Now it asks again,
and a wait that runs out says "still running", as it does when nothing went
wrong. When control doesn't answer, the daemon's log names the cause.
