# Arugula app 0.26.4

- **Developer settings in the Daemon menu** (#665). It opens this
  machine's Labs flags on the daemon's page, with a switch for each one.
- **The rename's bridges are gone** (#685). Every install is on 0.26 or
  newer now, so the app no longer handles `illogical://` links, an
  `illogicald` or the old app's launch agent and files. If an `illogicald`
  is still running somewhere, update it first: `curl -fsSL
  https://arugula.io/install.sh | sh`.

It carries arugulad 0.32.0.
