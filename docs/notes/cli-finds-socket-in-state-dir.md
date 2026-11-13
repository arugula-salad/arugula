---
section: Fixed
title: The CLI finds a daemon with its own state directory
line: Outside a pane, `arugula` looks for the daemon's socket in `ARUGULA_STATE_DIR` before the default place.
headline: no
---
A daemon started with `ARUGULA_STATE_DIR` keeps its socket there. In a shell
of your own, `arugula` used to look only in the default directory, so it said
the daemon wasn't running. It now reads `ARUGULA_STATE_DIR` too. `arugulad
flags --help` also shows a flag that exists today as its example.
