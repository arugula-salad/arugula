---
section: Added
title: Rename a machine
line: A machine can have a shorter name than its hostname, so `@mini/<session>` reaches its agents, and the old name keeps working in mentions.
try: {"route": "/settings"}
---
Rename a machine from its settings, the switcher's machine menu, or `arugula machine rename <name>`. The new name shows everywhere straight away and survives a reconnect. A name that another of your machines, your team's machines or a teammate's machine already uses is refused. Old names still reach the machine in `@` mentions, unless another machine now uses that name.
