---
title: "\"this CLI isn't one of the account's devices any more (revoked?)\""
description: "The CLI's device was removed from your account; log out and log in again."
area: "Control and sign-in"
---

# "this CLI isn't one of the account's devices any more (revoked?)"

## You see

`arugula --host NAME …` (or anything that reaches a machine through
control) stops with:

```
arugula: this CLI isn't one of the account's devices any more (revoked?): `arugula logout`, then log in again
```

## Why

A CLI logged in with `arugula login` (without `--browser`)
is a device of your control account, with keys of its own. It reaches a
machine with a machine token from control, and with its key only when
control gives no tokens or the machine's daemon is older than them; before
using its key it checks that it's still one of the account's devices
(`crates/cli/src/control.rs`, `open_device`). It isn't once it has been
removed from them.

## Do

`arugula logout`, then `arugula login` for a new code to approve from a
device you have (or `arugula login --browser` to sign in from a browser with
no key).
