---
section: Fixed
title: A watch wakes its session once, and obeys mute
line: A machine back after days away shows each fired watch once, and turning agent mentions off, or silencing a pane, now holds for watches too.
headline: no
---
A watch's message is remembered by its watch for up to a week, across restarts, so one sent again reaches its session once. One dropped from a pane's queue before its session took it is sent again. With agent mentions off on the machine, a watch's message is refused, and a pane silenced for mentions doesn't take one (control sends it again once it isn't silenced); `watch` refuses to make a watch that could never reach its session.
