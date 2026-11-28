---
section: Changed
title: A pr watch takes a head sha
line: watch create takes head (a sha) with pr, so only that commit's checks count.
headline: no
---
`watch` kind `create` takes `head` with one `pr`: a commit's sha, or its first 7 or more hex digits. Only that commit's checks count. Without it, a PR whose head just moved waits for the new head. A `pr` can already be several PRs in one watch.
