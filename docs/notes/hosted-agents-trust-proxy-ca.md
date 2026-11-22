---
section: Fixed
title: Cloud workspace agents reach the API again
line: Agents on a cloud workspace no longer fail every turn with "SSL certificate verification failed (UNABLE_TO_VERIFY_LEAF_SIGNATURE)".
headline: no
---
A cloud workspace's outbound TLS goes through its provider's proxy, which signs
with a CA the box's system store has but Node doesn't read. Claude Code and the
ACP adapters are Node, so on some boxes every turn ended in that error. The
workspace's daemon now points `NODE_EXTRA_CA_CERTS` at the system's bundle for
every agent and pane it starts, unless it was given one already. Existing
workspaces get it with the daemon update; new ones also have it in their service.
