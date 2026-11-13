---
section: Fixed
title: Plane relations reach the board
line: A "blocks" relation made in Plane now shows on the board within minutes, instead of waiting for a full re-import of the project.
headline: no
---
Plane doesn't change a work item's update time when a relation is added, so the
board only learned of one at a full backfill. A Plane webhook for an item now reads
its relations with it, and each reconcile reads the relations of a slice of the
project's open items, so a relation made or dropped in Plane reaches the board.
