---
section: Removed
title: A board's lanes follow the work only
line: A board row moves between Next, Doing, Review and Done only as its work does; there's no dragging it to a lane or moving it from its menu.
try: {"route": "/boards"}
needs: control
---
A row's lane comes from its work: its issue, branch, pull request and agent. Dragging a card onto a lane, the row menu's **Move to**, the lane select in a row's details and the "set by" mark are gone, and so is closing an issue by moving its row to Done. Close an issue from the issue itself.

A row still moves between groups, by dragging it onto one or with **Move to group** in its menu, and Steps still reorders rows.
