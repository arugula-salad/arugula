---
section: Added
title: Queued PRs say so
line: A PR block shows Queued, not Open, while the PR is in its branch's merge queue, and offers no Merge until it leaves the queue.
headline: no
---
While a PR is in GitHub's merge queue, its block's state reads Queued, and the strip says it merges when its group passes. If the PR leaves the queue, for example because its group failed, the block shows Open again with Merge. A block on a repo that isn't linked asks GitHub on each poll. On a linked repo, Queued shows once Arugula control tracks the queue.
