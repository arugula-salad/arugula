---
section: Fixed
title: A mention of a machine with mentions off says so
line: A teammate's @machine mention of a machine whose owner turned agent mentions off now says that, not that the machine is offline.
headline: no
---
The machine's daemon refuses the mention when its owner ran
`arugulad sharing agent-mentions off`, and the line under the message read
"offline", though the machine was connected. It now reads "its owner turned
agent mentions off", as does `read_thread`'s `mentions_off` state, so nobody
waits for the machine to come back. Such a mention isn't sent again when the
machine reconnects.

A watch on a reply in a room (`watch`) now names an agent's reply as the room
does, "jhgaylor's agent on geek (check-b)", not by its owner alone.
