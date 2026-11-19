---
section: Added
title: Send feedback from an agent or the terminal
line: Your agents can tell the Arugula team what broke or is missing, and so can you, with `arugula feedback "…"`.
---
Agents get a `feedback` tool: they say what they were trying to do and what
happened, and it goes with Arugula's version, your OS and their last failed
Arugula tool call (its name and error, never its arguments). Nothing else is
attached: no code, files or terminal contents. `arugula feedback "TEXT"
[--kind bug|missing|idea|praise] [--title …]` sends your own. If Arugula cloud
can't be reached, it's queued and sent later.

Telemetry's answer doesn't matter. To stop agents sending it, turn off
*Agents may send feedback to Arugula* in the machine's sharing settings, run
`arugulad sharing agent-feedback off`, or set `ARUGULA_FEEDBACK=off`; you can
still send your own.
