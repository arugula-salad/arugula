---
section: Added
title: Projects from the CLI
line: `arugula project list` lists your projects by owner, `project new` makes one, and `project use` puts the session you're in in one.
needs: control
---
`arugula project list` shows Personal's projects, then each team's, and marks
the current one; `--archived` shows archived ones too. `arugula project new
NAME` makes a project, Personal's unless `--team` names a team you own.
`arugula project use PROJECT` sets the project of the session it runs in, by
id, `personal`, or name.

`--project` and `$ARUGULA_PROJECT` now choose the current project in the same
order as Arugula's MCP tools: what you name, then the session's project, then
the project this clone's GitHub repo is linked to, then your personal one.
`arugula project ls` is now `arugula project repos`; `ls` still works.
