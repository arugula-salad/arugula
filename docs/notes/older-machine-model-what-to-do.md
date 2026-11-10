---
section: Removed
title: After the older machine model: what to do
line: A host that dialed out needs moving to control before updating, `arugula hosts token`, `revoke` and `invite` are gone, and a `synced/` directory can be deleted.
headline: no
---
v0.68.0 removed synced history, the dial-out tunnel with its host tokens and
invites, and remote blocks, but didn't say what to do about them.

- **A host that dials out** (`arugulad --peer`, `--token`, `--join`) doesn't
  start on v0.68.0 or later: the daemon refuses the flags, and nothing can
  reach it from outside once it's gone. Move it to Arugula control first:
  `arugulad join` on that machine, then update it.
- **`arugula hosts token`, `arugula hosts revoke` and `arugula hosts invite`**
  are gone; `arugula hosts` is `add` and `rm`.
- **A `synced/` directory** in the daemon's state directory holds history
  other hosts synced there, sealed, and nothing reads it now; the daemon
  says so in its log at startup. `rm -r ~/.local/state/arugula/synced`
  removes it.
