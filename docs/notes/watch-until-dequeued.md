---
section: Added
title: Watches hear a PR leave the merge queue
line: A watch on a pull request can wait until it is dequeued, and event watches can match github.merge_queue.added and github.merge_queue.removed.
headline: no
needs: control
---
A pull request that leaves a linked repo's merge queue without merging now wakes a watch on it, with GitHub's reason (`failed_checks`, `merge_conflict`, `manual` and so on). Watch a PR `until: "dequeued"`, or watch the events `github.merge_queue.added` and `github.merge_queue.removed`, matching on `pr`, `repo` and `reason`. A `checks_failed` watch on a queued PR also fires when its merge-group run fails.
