---
section: Added
title: A Steps view on boards
line: A board has a Steps view beside its lanes, where each phase shows its open work by step with the agents on it live, and hovering a row draws its links.
try: {"route": "/projects"}
needs: control
---
Switch a board between Lanes and Steps in its header; Steps is the default
on a phone. Each of the board's groups is a phase, with its steps as columns
(sections on a phone). Done rows are counted in the phase's header and not
drawn, and step 1 folds after eight rows behind "+M more". A row with a
running agent shows its state live on its bar.

Hovering a row (tapping it, on a phone) draws arrows for its links, each kind
its own way: a thin arrow for `before`, a heavier warm one for `blocks`, a
dotted line for `relates`, a double line for `duplicate_of` and a dashed arrow
for `discovered_from`. The drawer lists the same links in words.

Drag a row up or down inside its step to reorder it (↑ and ↓ in the drawer on
a phone). Only the row's place in its group changes; the step is still worked
out from the links. A reorder made on an order someone else has since changed
isn't made: the row says it was moved first by someone else.
