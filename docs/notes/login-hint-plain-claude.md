---
section: Changed
title: A plainer way to log in to Claude Code
line: When an agent block isn't logged in, it now says to run `claude`, then /login.
headline: no
---
An agent block whose turn fails for want of a login used to say to run
`env -u CLAUDE_CONFIG_DIR claude`, which read oddly to anyone who never set
that variable. With Claude Code's default login it now says `claude`, then
`/login`; a block that used another `CLAUDE_CONFIG_DIR` still names it.
