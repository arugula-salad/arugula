# Arugula app 0.26.3

- **An older daemon gets its update offered** (#661). When the daemon
  that answers is older than the one the app carries, or is an illogicald
  from before the rename, the app says so and offers *Update arugulad*,
  with *Not now* to keep using it this time. illogicald is replaced by the
  app's own arugulad, which takes over its service and keeps your panes.
- **`arugulad` on PATH** (#552): the Mac app links `arugulad` and
  `arugula` into `~/.local/bin` when none is there, so `arugulad join`
  and `arugulad update` work from a terminal. Its daemon keeps the flags
  `arugulad install -- FLAGS` set (#550).
- **Fixes**: a file picker after an in-place update no longer crashes the
  app (#557); panics go to a log file, since Finder drops stderr (#560);
  on Linux an `arugula://pane/%N` link opens that pane (#299).

It carries arugulad 0.28.0.

Not notarized yet (#177): from a browser download, open it once with
*Open Anyway* in System Settings › Privacy & Security.
