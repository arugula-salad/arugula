---
section: Added
title: Give a machine's agents a team's channels
line: arugula grants add --team gives a machine's agents threads.read or threads.post over a team's channels, and arugula grants lists the grant by the team's name.
needs: control
---
`arugula grants add geek threads.read --team Acme` names the team by its name
or id. A team's grant is for `threads.read` and `threads.post` only; the other
powers are over a project. It takes one scope: `--team`, `--project` or
`--dms`. A control from before team grants refuses one, and the CLI says so.
