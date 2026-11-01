---
title: "In a Cloud workspace, a session's tool says \"Permission denied\""
description: "The setup ran as user; sessions run as ses1–ses6, who can't enter /home/user."
area: "Hosted workspaces"
---

# In a Cloud workspace, a session's tool says "Permission denied"

## You see

A tool the workspace's setup installed works in the box's own terminal,
but an agent session (or its finish step) fails to run it:

```
bash: /home/user/.local/bin/zig: Permission denied
```

## Why

The setup script runs as `user`, but a session's agent and its finish step
run as a session user, `ses1` to `ses6`, who can't enter `/home/user`. So
anything the setup installed there, or linked into there from
`/usr/local/bin`, is out of their reach (`docs/control.md`, "Session
users' tools"; native issue #215).

## Do

Publish what sessions run in `/opt/arugula/tools`, whose `bin` is first on
every session user's `PATH`, with the template's `arugula-tools`
(`packaging/hosted/tools`), in the setup script:

```
arugula-tools link zig "$(mise which zig)"    # a binary
arugula-tools rustup                          # rustup's proxies
arugula-tools as ses1 zig version             # check it as a session sees it
```

Then rebuild the workspace.
