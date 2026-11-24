---
section: Fixed
title: Agents' build folders are cleaned up
line: An agent's build folder is removed when its work ends, and old leftovers are swept after 3 days, so disks stop filling up.
---
Agents build in a folder of their own under `~/.cache`. Closing a worktree now
removes its folder, a hosted session's end removes its build folders (sccache stays),
and the daemon sweeps the rest every 6 hours: folders no open worktree owns, that
nothing has touched for 3 days and that no build is using.
