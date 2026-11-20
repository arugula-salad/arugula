---
section: Added
title: Agents watch a linked repo's releases
line: "An agent can watch a linked GitHub repo for a release being published, by its tag, and be woken when it is."
---
`watch` with `event: "github.release.published"` and `match` naming the `repo` (and, if it likes, the `tag`, `prerelease` or `by`) fires once when a release, a published draft's included, comes in through Arugula's GitHub App. The event carries the release's tag, whether it's a pre-release and who published it; never its name or notes.
