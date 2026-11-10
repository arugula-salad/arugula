---
section: Fixed
title: Thread readers follow the machine
line: A pane's or session's thread is read by whoever its machine says now, including when someone is taken off the list or the pane has closed.
headline: no
---
A machine now tells Arugula which panes and sessions it still has, and every
list that changed, so people it takes off a list stop reading the thread. A
list that couldn't be sent goes again on its own, whole, and a list that
arrives late doesn't undo a newer one.

A closed pane's or session's thread keeps the people who could read it when
it closed, less anyone who loses access afterwards: a share taken away, or
someone leaving the team. Nobody new gains it. Someone shared the session
or joining the team after it closed doesn't read it, and nor does someone
who lost access and was given it again.

Once a machine is on this version, only its owner reads the threads of
panes and sessions that closed before the update. The same goes for the
oldest closed ones once there are more than the machine remembers: 2,000
panes and 500 sessions.
