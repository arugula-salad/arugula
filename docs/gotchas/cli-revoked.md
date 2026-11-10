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

`arugula login` makes the CLI a device of your control account, with keys
of its own, and machines only talk to devices the account trusts. Before
opening a channel the CLI checks that it's still one
(`crates/cli/src/control.rs`, `open`). It isn't once it has been removed
from the account's devices.

## Do

`arugula logout`, then `arugula login`, and approve the new code as the
CLI asks.
