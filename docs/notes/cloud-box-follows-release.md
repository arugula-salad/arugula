---
section: Fixed
title: Cloud workspaces keep up with Arugula's releases
line: A Cloud workspace's box updates its daemon to the release control runs when it starts or wakes, so its sessions get the new tools without a rebuild.
---
Before it connects, a box compares its daemon with the one control was
released with and, when control's is newer, downloads it (checked against the
release's signed checksums) and restarts onto it, keeping its terminals and
the work on its disk. When it can't, `capabilities` tells a session on the
box why and how to try again.
