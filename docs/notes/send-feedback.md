---
section: Added
title: Send feedback from an agent, the terminal or the app
line: Your agents can tell the Arugula team what broke or is missing, and so can you, with `arugula feedback "…"` or *Send feedback…* in the app.
needs: control
---
Agents get a `feedback` tool: they say what they were trying to do and what
happened, and it goes with Arugula's version, your OS and their last failed
Arugula tool call (its name and error, never its arguments). Nothing else is
attached: no code, files or terminal contents. `arugula feedback "TEXT"
[--kind bug|missing|idea|praise] [--title …]` sends your own. If Arugula cloud
can't be reached, it's queued and sent later.

In the web client and the desktop app, *Send feedback…* is in the account menu
and the command palette: pick Bug, Missing, Idea or Love it, write a line or
more, and send. Each one taken answers with an id such as `F-7K2Q9X`.

Telemetry's answer doesn't matter. To stop agents sending it, turn off
*Agents may send feedback to Arugula* in the machine's sharing settings, run
`arugulad sharing agent-feedback off`, or set `ARUGULA_FEEDBACK=off`; you can
still send your own.
