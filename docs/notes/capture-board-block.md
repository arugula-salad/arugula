---
section: Changed
title: Capture reads a board block
line: `arugula capture` of a board block prints the board's text, as board_read does, so an agent can check what a person is looking at.
headline: no
needs: control
---
A board block's capture is its lanes, then Steps and Chain. If control can't be asked (offline, or no grant), it is one line saying why. A Doc block's capture is its Markdown.
