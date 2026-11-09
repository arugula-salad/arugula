---
section: Removed
title: The Sprites adapter (wisp) is removed
line: loam, named by `--loam-url`, is the only sandbox provider for VM panes; `--wisp-url` and `--wisp-token-file` are gone.
---
The daemon's Sprites adapter, which ran VM panes on a wispd on the machine or
on Fly's sprites, is removed with the older machine model. loam is the only
sandbox provider: `--loam-url` (or `LOAM_API_URL`) names it, with its key in
`--loam-key-file`, and without it VM panes are off. The `sandbox-providers`
flag gates it as before, on the running daemon.

- `--wisp-url` and `--wisp-token-file` (and `ARUGULA_WISP_URL`,
  `ARUGULA_WISP_TOKEN_FILE`) are gone: a service file or script that still
  passes them won't start, so take them out before updating.
- A wisp token left on disk (`~/.local/share/wisp/token`) is never read; the
  daemon says so in its log at startup, and leaves the file alone.
- A layout saved with a machine from wisp loads whole: the daemon logs one
  line per such machine and keeps it with its pane, which says its machine
  is gone (or that VM panes aren't set up) until it's used again.
