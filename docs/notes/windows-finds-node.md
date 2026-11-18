---
section: Fixed
title: Windows finds Node for agent adapters
line: On Windows, arugula status and arugula setup claude find Node, npm and Claude Code on PATH instead of saying Node 20+ is missing.
headline: no
---
The daemon looked for programs the Unix way: it split PATH on `:` and looked for `node` rather than `node.exe`, so on Windows it never found Node, npm or the adapter. It now splits PATH on `;` there and tries `PATHEXT`'s extensions (`node.exe`, `npm.cmd`, npm's `.cmd` shims), and an *Install* pane's command is written for PowerShell.
