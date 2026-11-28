---
section: Fixed
title: A renamed lead still hears its board
line: A board's lead gets every post in its board's channel after its session is renamed: it's found by its conversation, not its name.
needs: control
headline: no
---
A Claude Code session that took a board's lead and was renamed afterwards (with `/rename`, say)
was told of the board's posts by its old name, which matched no session, so they waited unread
in its machine's inbox. A lead now goes by its Claude Code conversation: its posts reach the pane
that holds it, whatever it's called, and a lead whose conversation no pane holds any more is let go.
