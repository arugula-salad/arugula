---
section: Changed
title: Every session has a project
line: A session you start on a project's machine page starts in that project, and one nothing else places goes in your personal project instead of none, never a team's.
needs: control
---
A new session made from a machine's page in the app starts in the page's project, set by hand, so a linked repo never moves it. A session that nothing places (no pane in a clone of a linked repo, nothing set by hand) goes in your personal project, even on a team's machine, so it stays private until you move it. That counts as inferred, so a later pane in a linked clone still puts it in that repo's project. Sessions from before that have no project get your personal project the next time the machine reaches Arugula cloud; one you took out of its project by hand stays out.
