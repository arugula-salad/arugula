---
section: Changed
title: Agents say what comes before what
line: An agent links work items with the new link tool (a new "before" kind is the default, and a cycle of before and blocks links is refused), in place of board_write's relate and unrelate.
headline: no
needs: control
---
`link` adds and removes up to 50 links in one call, between a project's
issues and PRs, with no board needed. Each link lands on its own and the
answer says what became of every one: added, already there, removed, not
there, or refused and why. An item is `#N`, `owner/repo#N`, `IDENT-N` or an
`iss_`/`fi_` id. The default kind, `before`, says the first item comes first;
it shows on both items' rows in a board, like the other kinds, and doesn't
keep a row out of the Ready line (only `blocks` does). An agent takes off only
the links its own machine made. Needs the machine's `issues.write` grant over
the project.

`board_write` no longer has `relate` and `unrelate`: use `link` with kind
`blocks`, `relates`, `duplicate_of` or `discovered_from`.
