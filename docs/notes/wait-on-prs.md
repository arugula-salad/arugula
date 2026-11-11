---
section: Added
title: Agents wait on pull requests
line: An agent's wait can watch several pull requests until checks finish or fail, they're ready to merge, or someone merges or closes one, with each one's failing checks and merge queue state.
needs: control
---
`wait` takes `pr` (`OWNER/REPO#N`, a link or a PR block, one or several) and `until`: `checks_done`, `checks_failed`, `ready`, `merged` or `closed`. It returns when any of them gets there, with a verdict for each: its checks by state, the names of the failing ones, whether it can merge, whether it's in the branch's merge queue, and who merged it. It asks GitHub itself with your own `gh` login every few seconds, so a green PR's `mergeable` is never as old as the mirror's last read, and a pull request you can't see answers as one that doesn't exist.
