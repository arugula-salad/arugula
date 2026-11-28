---
section: Fixed
title: Chat's Saved and Threads badges count what their views show
line: In a project's chat, the Saved and Threads badges no longer count rooms their views leave out.
headline: no
---
In a project's chat, the Saved badge counted every saved message you had, and the Threads badge every unread reply, while their views list only that project's rooms (and no DMs). Each badge now counts what its view shows in the same scope: Personal's, a project's, or every room's where the page has no scope. The Threads count needs a server that sends it by room (`threads_by_room`); with an older server the badge shows the totals as before.
