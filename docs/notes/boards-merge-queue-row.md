---
section: Added
title: A board row says where its PR is in the merge queue
line: A board's Review row says how long its PR has been queued, or that it left the merge queue and why (checks timed out, checks failed, a conflict); a repo with no merge queue sees nothing new.
headline: no
---
On a project whose repo has a merge queue, a row's PR chip reads `queued 12m` while the PR is in the queue, and `left queue: checks timed out` once GitHub took it out, with GitHub's reason in words. A PR that left the queue puts its row in "Needs you", as failed checks do. `board_read` says the same. This needs the platform's GitHub webhook to include merge queue events; a queue's position isn't shown.
