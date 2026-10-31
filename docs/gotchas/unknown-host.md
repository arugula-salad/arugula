---
title: "The page says unknown host \"…\" (421)"
description: "You reached the daemon by a name it wasn't told about: a LAN address, your own DNS name, a proxy."
area: "Web and desktop"
---

# The page says unknown host "…" (421)

## You see

A page, or a client's error, with only:

```
unknown host "box.lan:7681"
```

The status is 421 Misdirected Request.

## Why

The daemon only answers to host names it knows: `127.0.0.1`,
`localhost` and `[::1]` on its port, its tailnet (MagicDNS) name, which it
finds by itself, and names it's given (`crates/daemon/src/access.rs`,
`Access::new` and `check_host`). That stops another
site in your browser from reaching it under a name of its own (DNS
rebinding). A LAN address, a name in your own DNS, or a reverse proxy
that passes its own `Host` aren't on the list.

## Do

Tell the daemon the name, and keep it in the service:

- Reached on the daemon's own port (`box.lan:7681`):
  `arugulad install -- --direct-url http://box.lan:7681`
  (`ARUGULA_DIRECT_URL`). It accepts that host with the port, and lists
  the address in control's directory, so your other devices try it
  directly.
- Behind a proxy that serves it at a name of its own, without a port:
  `arugulad install -- --public-host box.example.com`.

Remember that `arugulad install -- …` replaces the arguments the service
had: give the ones you want to keep as well.
