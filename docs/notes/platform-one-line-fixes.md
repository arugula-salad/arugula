---
section: Fixed
title: Small platform fixes
line: Board groups keep their epic's rows, folded notes stay on one line, and a deleted workspace's schedules and event rules turn off.
headline: no
---
- Removing a board group that holds rows its Plane epic adds is refused, as removing one of those rows is.
- A row's old status from before lanes no longer reaches clients: it reads as none unless the row has an open ask.
- When two rows of one piece of work fold together, their notes join on one line.
- A question to the agent on a thread is matched to the thread's own session or its picked try, never a review or an unpicked try.
- A machine sending an issue's export from its block is refused once the issue is on GitHub, as a send from the page is. The machine still sends the copy it has, not the issue's latest title and body.
- Deleting an account takes everyone's inbox entries on the runs that went with it.
- Deleting a cloud workspace turns off the schedules and event rules of its run specs. Runs already going aren't stopped. A project that chose it for its agents chooses none. Moving a spec to another workspace turns its schedules back on, but its event rules need turning on again by hand on the Runs page.
