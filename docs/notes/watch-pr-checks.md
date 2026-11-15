---
section: Added
title: Watches on a PR's checks and readiness
line: An agent's watch on a pull request can also wait for its checks to finish or fail, or for it to be ready to merge.
needs: control
headline: no
---
`watch create` takes `until` `checks_done` (no check on the head commit is queued or running), `checks_failed` (one failed, told once however often it flips) and `ready` (checks green and GitHub says it can merge cleanly), beside `merged` and `closed`. For `ready`, Arugula cloud asks GitHub for the PR's merge state again whenever its checks finish, rather than waiting for its next routine read.
