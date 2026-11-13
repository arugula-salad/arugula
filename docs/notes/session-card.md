---
section: Added
title: A session carries its project and card
line: A session can be on a project's card, set by link, run or start_agent or found from a terminal's feat/N-… branch, and its panes list that project and card.
headline: no
---
A session's work is a project and, maybe, a native issue in it (its card). Its terminals, and blocks without an issue of their own, show the card in `on_issue`, read when asked, so a card set after a pane opened shows on it too. MCP's `list` has `project` and `card` columns; `run` takes `project` and `issue`; `start_agent` takes `project`; `link`'s `card` sets the caller's session's; and `board_write attach` of an agent block puts the block on that row's issue. A terminal on a `feat/N-…` branch puts its session on `#N` when a board of its project has a row for it, never over a card set by hand. The current project of an agent's calls comes from its session's card, after its own issue.
