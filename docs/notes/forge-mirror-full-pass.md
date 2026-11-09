---
section: Fixed
title: Deleted GitHub items leave
line: An issue or PR deleted or moved on GitHub leaves your lists and inbox within a day, even when its webhook was lost.
headline: no
---
The platform's copy of a linked repo now gets a slow full pass about once a
day. It lists what's open on GitHub and reads again each open item it no
longer lists, so an issue deleted or transferred while its webhook was lost
goes. It also reads again the items it read longest ago, with their text for a
project that mirrors it, so a comment deleted or edited on GitHub catches up.

A deleted item's text goes with it. A read that GitHub refuses for one item or
commit no longer stops the repo's sync. A webhook whose rows fail to write
isn't counted as taken, so a redelivery of it is written; GitHub doesn't
redeliver on its own.
