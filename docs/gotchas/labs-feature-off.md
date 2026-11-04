---
title: "\"… is a Labs feature that's off on this machine\""
description: "A Labs flag is off on the machine that answered, or the flag you set was retired."
area: "Labs"
---

# "… is a Labs feature that's off on this machine"

## You see

```
Boards is a Labs feature that's off on this machine: its owner turns it on with `arugulad flags boards on` there, or in Developer settings
```

or, turning on a flag from an older guide:

```
chat isn't a flag any more: Chat is no longer in Labs: it's on wherever a team's owners turn it on in control (Teams)
```

or `… isn't in this build (built without labs)`.

## Why

Labs features are off until a machine's owner turns them on, one flag
each, and a feature refuses as well as hides while it's off
(`crates/mux/src/labs/mod.rs`, `flag_off`). The flag that counts is the
one on the machine that answered: with `arugula --host` or `--ssh`, that's
the other machine. Some flags have left Labs (`chat`, `docs`,
`swarm-themes`, `guest-ssh`) or were removed (`fountain`, `huddles`), and
setting them says why (`crates/proto/src/flags.rs`, `RETIRED`). A build
without the `labs` feature has none of them.

## Do

- On the machine the message names: `arugulad flags NAME on`, or
  *Developer settings…* in the web client. `arugulad flags` lists them.
  It works with the daemon stopped.
- For a retired flag, do what its message says: chat is a team setting
  in control, and Guest SSH is `arugulad sharing guest-ssh on`.
