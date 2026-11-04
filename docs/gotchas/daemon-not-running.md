---
title: "arugula says \"can't reach arugulad … (is it running?)\""
description: "The CLI can't find the daemon's socket: it's stopped, or it keeps its state somewhere else."
area: "Install and updates"
---

# arugula says "can't reach arugulad … (is it running?)"

## You see

Any `arugula` command, from a terminal outside Arugula:

```
arugula: can't reach arugulad at /home/you/.local/state/arugula/sock (is it running?): Connection refused (os error 111)
```

On Windows it names the daemon's pipe instead of a socket.

## Why

The CLI talks to the daemon on this machine through a socket (on Windows
a named pipe) in the daemon's state directory
(`crates/cli/src/http.rs`, `Target::connect`). There are two common causes:

- **The daemon isn't running.** It was never installed as a service, or
  the service stopped.
- **It runs with another state directory.** A daemon started with
  `ARUGULA_STATE_DIR` puts its socket there. Inside its panes the CLI is
  told where (`ARUGULA_SOCK`), but in a shell of your own the CLI only
  reads `ARUGULA_SOCK`, else the default place
  (`crates/cli/src/main.rs`, `default_socket`), so it looks in the wrong
  directory.

## Do

- `arugula status` says whether the service is there and running, and
  gives the fix on its `daemon` line: `systemctl --user start …`, the
  desktop app's *Daemon > Start*, or `` `arugulad install` sets it up as a
  service and starts it `` when there's none.
- If you moved the state directory, point the CLI at the socket:
  `ARUGULA_SOCK=$ARUGULA_STATE_DIR/sock arugula …`, or `--socket PATH`.
