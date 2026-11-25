---
section: Added
title: A lead starts agents on your other machines
line: start_agent with a board starts its lead or an issue agent on another of your machines, under the board's grant, checked by control and by that machine.
headline: no
---
`start_agent {board, issue?, to?}` asks control for a start ticket within the board's grant, sends it over the machine-to-machine channel, and the target machine checks it, opens the agent in a worktree of its own clone, and attaches it to the issue's row or makes it the board's lead (aru-143). The grant itself comes with "Run this board"; until then control refuses every ticket.
