---
section: Added
title: Agents watch this machine's agents and panes
line: "An agent can watch another agent on the same machine finish its turn or ask for input, or a pane's command or process end, and be woken when it does, with no Arugula account."
---
`watch` with `event` set to `agent.idle`, `agent.needs_input`, `pane.command.ended` or `pane.exited`, and `match` naming the pane (`{"pane": "4"}`, with `exit` for a code), is kept by the machine's daemon itself, so it works on a machine that never joined Arugula cloud. It has the same rules as other watches: it fires once, expires after 24 hours unless told (at most 7 days), says so if it has already happened, and survives a restart. `watch list` shows these with the cloud's, and `cancel` takes either.
