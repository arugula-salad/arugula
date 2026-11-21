---
section: Added
title: watch a pane's output for a pattern
line: An agent can watch, or wait on, a pane's new output for a regex and be woken with the matching line, with no account and no script.
headline: no
---
`watch` (or `wait`) with `event` set to `pane.output.matched` and `match` naming the pane and a regex (`{"pane": "4", "pattern": "ready|ERROR"}`) fires when a new line of the pane's output matches, ANSI stripped as `wait`'s `until: match` strips it. Lines already on screen or in scrollback when the watch is made never fire it, so a `log stream` that echoes its own filter doesn't set it off. Its message quotes the matching line. It has the same rules as the other machine watches: it fires once, expires after 24 hours unless told (at most 7 days), survives a restart, and a bad regex is refused when it's made.
