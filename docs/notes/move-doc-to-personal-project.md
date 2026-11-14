---
section: Changed
title: A machine's Doc can move into your personal project
line: `doc move --project` takes your own personal project as well as a team's, and Move to project… moves your machines' Docs into a personal project too.
needs: control
---
A Doc goes where its project goes, a team's or your own. `arugula doc move
--project <your personal project>` and an agent's `doc move` move a Doc from
this machine into it, leaving the usual pointer, so its pane, `doc open` of
its old id and old links follow it there. Someone else's personal project is
refused. Without `--project`, `doc move` still moves to the current project
only when it's a team's.

On Move to project…, with a personal target, Docs on your own machines that
the page reaches can be picked and are moved after the rest, as for a team's.
