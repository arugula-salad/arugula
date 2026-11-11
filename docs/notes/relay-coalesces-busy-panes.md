---
section: Fixed
title: Busy panes over control's relay cost less
line: A busy pane reached through Arugula control sends its output at most every 16 ms as one message, and control keeps relaying while it writes to disk, so a few busy panes no longer slow it for everyone.
headline: no
---
A busy pane used to send each small read of its output as a message of its
own, thousands a second, and each one cost control's relay two WebSocket
frames (#856). Through the relay, a machine now sends what a pane wrote in
the last 16 ms as one message; output after a quiet spell, such as a
keystroke's echo, still goes at once. Control also moves relayed bytes on
threads of their own, apart from the ones that wait on its database (#857).
