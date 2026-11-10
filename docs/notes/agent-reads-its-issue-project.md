---
section: Fixed
title: An agent started on an issue reads it
line: An agent started on an issue looks up "#N" in that issue's project, even from a clone that isn't linked to it, and a missing issue says which project was searched.
headline: no
needs: control
---
`start_agent` with `issue` keeps the issue's project on the agent's block, and
that is the agent's current project: its `forge_read` of `"#N"` and its
`issue_write`, `board_read` and `link` calls go there, after any `project` it
names and before the project its clone's repo is linked to. Its environment
has `ARUGULA_PROJECT`, so the `arugula issue` it runs looks there too.

A `#N` that isn't there now names the project searched, why that one, and that
`project` picks another.
