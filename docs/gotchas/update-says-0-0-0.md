---
title: "arugulad --version says \"arugulad-base 0.0.0\""
description: "0.64.0's daemon misreported its version, so updating to it failed; 0.64.1 fixed both."
area: "Install and updates"
---

# arugulad --version says "arugulad-base 0.0.0"

## You see

On a machine running 0.64.0:

```
$ arugulad --version
arugulad-base 0.0.0
```

Or, updating a machine on 0.63.x to 0.64.0, the update stopped with:

```
…/arugulad says it's 0.0.0, not 0.64.0
```

## Why

When the daemon became a thin binary over the `arugulad-base` library,
`--version` took the library's name and version instead of the release's.
`arugulad update` checks the downloaded build's `--version` before
installing it (`crates/daemon/src/selfupdate.rs`, `fetch`), so it refused
0.64.0. The daemon's own API reported the right version all along, so
`arugula status` and the web client looked fine. Fixed in 0.64.1 (native
issue #279).

## Do

`arugulad update`. Machines on 0.63.x or 0.64.0 both go straight to the
latest release, whose `--version` is right. If you script around
`arugulad --version`, don't trust a 0.64.0 daemon's answer: ask
`arugula status`.
