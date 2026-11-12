---
section: Fixed
title: A cloud rebuild checks every branch and stash first
line: `arugula cloud rebuild` now stops for commits on any branch no remote has, stashes, and changed or untracked files in every git repo on the machine, session users' too, and names them.
---
Before, it looked only at the checked-out branch of the repos the workspace's setup
cloned, so other branches, stashes and other repos were lost with the old machine.
`--download FILE` saves every repo with work to a tarball; `--force` rebuilds anyway.
On a machine made before this release, the CLI looks itself, in a pane it opens
there and closes.
