---
section: Fixed
title: A merged PR closes only what it says it closes
line: A pull request closes an Arugula issue when it says "Closes #N" or "Arugula #N", or "(#N)" in its title; any other mention links without closing, and the issue and its board row say which PR closed it and by which words.
---
Two ways a merge closed issues it only mentioned are gone:

- In a PR's description, only `Closes #N` (or `Fixes`, `Resolves`) and `Arugula #N` close an issue. Any other `#N`, such as `(#N)` in passing, links the issue to the PR without closing it. `(#N)` in the title still closes.
- A branch named `feat/N-…` no longer closes #N when the title or description says `part of Arugula #N`.

When a merge does close an issue, its timeline and its board row say which pull request closed it and by which words, such as `"Closes #N" in the body` or `the branch feat/N-…`, so a wrong close is easy to spot.
