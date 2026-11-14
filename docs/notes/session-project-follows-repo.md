---
section: Changed
title: A session's project follows its repo
line: When a repo moves to another project, the sessions whose project came from that repo move with it; a project you set by hand stays where you put it.
headline: no
---
A session takes its project from the linked repo of its first pane. The machine now remembers which repo that was. When the repo is linked to another project, those sessions take the new project. A machine checks each such repo whenever it connects to Arugula, so one that was offline catches up when it's back. Nothing restarts: only the session's project changes. A project set from the session's menu, or taken away there, never moves. Sessions whose project was inferred before this release didn't record a repo, so they stay where they are.
