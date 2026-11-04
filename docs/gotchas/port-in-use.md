---
title: "arugulad won't start: \"can't listen on 127.0.0.1:7681\""
description: "Another daemon, or another program, already has the port."
area: "Install and updates"
---

# arugulad won't start: "can't listen on 127.0.0.1:7681"

## You see

The daemon stops as soon as it starts. Its log (or the terminal you ran it
in) says:

```
Error: can't listen on 127.0.0.1:7681: Address already in use (os error 98)
```

## Why

The daemon binds its listen address before anything else, so a taken port
fails at once (`crates/daemon/src/lib.rs`, `run`). The default is
`127.0.0.1:7681` (`--listen`, `crates/daemon/src/args.rs`). Something
already holds it: most often a second daemon (a dev build, an older
install still running, `arugulad` started by hand beside the service), or
another program that likes 7681.

## Do

- Find what holds it: `lsof -iTCP:7681 -sTCP:LISTEN` (macOS, Linux) or
  `ss -ltnp 'sport = :7681'` (Linux). If it's another `arugulad`, stop the
  one you don't want.
- To run this one elsewhere, give it another address and keep it in the
  service: `arugulad install -- --listen 127.0.0.1:7690`. Port 0 picks a
  free one, written to `listen` in the state directory. `ARUGULA_LISTEN`
  does the same as `--listen`.
- For a second daemon on the same machine, give it its own
  `ARUGULA_STATE_DIR` too.
