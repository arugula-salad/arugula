---
title: "arugulad install keeps flags you meant to drop"
description: "A plain install reuses the daemon arguments the last install wrote."
area: "Install and updates"
---

# arugulad install keeps flags you meant to drop

## You see

After `arugulad install` (or an update, which runs it), the daemon still
listens where it used to, or keeps an `--owner` or `--block-listen` you
thought was gone. The install said so:

```
keeping the daemon arguments from the last install: --listen 127.0.0.1:9000 (--reset-args drops them)
```

## Why

Daemon arguments go after `--` (`arugulad install -- --listen …`). With
none given, an install reads back the ones the last install wrote into the
service and keeps them, so that updating doesn't drop them
(`crates/daemon/src/install.rs`, `args_to_install`). That's also why
`arugulad update` and *Update* in the web client keep them.

## Do

- To change them, give the whole new set: `arugulad install -- --listen
  127.0.0.1:7681`. Arguments you give replace the old ones, not add to
  them.
- To go back to none: `arugulad install --reset-args`.
