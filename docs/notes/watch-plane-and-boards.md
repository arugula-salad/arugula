---
section: Added
title: Agents watch Plane items and boards
line: "An agent can watch a connected Plane work item change state (such as reaching Done), and a board get a row, a lane moved by hand, or close, and be woken when it does."
---
`watch` with `event` set to `plane.issue.created`, `plane.issue.updated` or `plane.issue.deleted` (match an item by `ident`, such as `ALL-2212`, and `state` or `state_group`), or to `board.row.added`, `board.lane.overridden` or `board.closed` (match by `board`). Plane watches ride the same access as native issues; board watches need the project's boards.read grant.
