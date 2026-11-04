---
title: "Over Tailscale: \"This machine's owner is …, and nothing on it is shared with you\""
description: "The daemon's owner isn't the Tailscale login you're using, or no owner is set."
area: "Web and desktop"
---

# Over Tailscale: "This machine's owner is …, and nothing on it is shared with you"

## You see

Opening the machine at its tailnet name (`https://box.tail1234.ts.net`),
one of:

```
You're signed in to Tailscale as ada@example.com. This machine's owner is ada@github, and nothing on it is shared with you.

If it's yours, run this on it:

arugulad install -- --owner ada@example.com
```

```
You're signed in to Tailscale as ada@example.com, and this machine has no owner set, so nobody from the tailnet gets in.
```

or, from a tagged device, `tagged tailnet nodes have no user identity`.

## Why

Over the tailnet the daemon knows you by your Tailscale login and lets the
owner in, and others only to what's been shared with them
(`crates/daemon/src/access.rs`, `must_be_owner` and `not_yours`). The
owner is `--owner` (`ARUGULA_OWNER`), else the login that owns the node.
So it refuses you when the login your device uses isn't that one (two
Tailscale identities, say, or a machine set up under someone else's), and
a tagged device, whose requests carry no user at all.

## Do

If the machine is yours, run the command the page gives, on the machine:
`arugulad install -- --owner LOGIN`. It keeps the owner in the service, so
it survives restarts. If it isn't yours, ask its owner to share a session
with you.
