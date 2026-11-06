---
section: Changed
title: Cloud workspace help says machine
line: The help for `arugula cloud` calls what runs a Cloud workspace its machine, not its box.
headline: no
---
`arugula cloud create`, `edit`, `secret`, `rebuild`, `delete` and `key` said
"box" for a Cloud workspace's machine, a word that also means hud and studio
app boxes. Their help says "machine" now, and so do the line `cloud secret
set` prints and `arugula access`' line for a deploy's or a snapshot's
machine. `arugula grants`' help said boards.write never makes or deletes a
zone, which was wrong: an agent makes groups. It now says what's true: the
agent never sets a lane, and deletes or repoints only the rows its machine
added. Nothing else changes.
