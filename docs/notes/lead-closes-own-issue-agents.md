---
section: Fixed
title: A lead closes its issue agents on its own machine
line: A board's lead now closes the issue agents its board started on its own machine, which frees their slots, instead of being refused as "in another tab".
headline: no
---
A lead's `close {pane}` closes a block that a start for its board opened on the same machine, and tells Arugula cloud at once so the board's grant has the slot back. Before, only `close {pane, to}` on another machine worked, and a person had to close the agents here by hand. Any other pane in another tab is still refused, and so is an issue agent closing its sibling.
