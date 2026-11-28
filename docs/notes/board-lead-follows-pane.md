---
section: Fixed
title: A board's thread reaches its lead after a rename
line: A board's thread reaches its lead after the lead's session is renamed
headline: no
---
A post on a board's thread now goes to the pane its lead leads from, not only to the session's name. Rename the lead's session, or restart it under another name, and the board's thread still reaches it. A pane that's gone falls back to the name, as before. The event catalog gains `board.lead.lost` (a board's lead can't be reached), which control emits.
