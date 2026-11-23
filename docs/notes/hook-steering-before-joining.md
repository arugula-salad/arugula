---
section: Changed
title: Claude Code's hook skips issues before you join
line: On a machine that hasn't joined Arugula cloud, a Claude Code with Arugula's hooks is no longer told to file Arugula issues it can't reach.
headline: no
---
`arugula hook` names Arugula's own issues, at a session's start and before a
`gh issue create`, only once the machine has joined Arugula cloud, as the MCP
server's instructions already do. The Doc sentence, and how a teammate's
message arrives, are said either way.
