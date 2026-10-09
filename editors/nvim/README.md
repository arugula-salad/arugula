# arugula.nvim

Your nvim in your [Arugula](https://arugula.io) swarm. Core nvim (0.10 or later), no dependencies; nvim-dap's debugger stops show too if it's installed.

Install it like any plugin from this directory, e.g. with lazy.nvim:

```lua
{ dir = "~/src/arugula/editors/nvim" }
```

or copy `editors/nvim` onto your `runtimepath`.

- `:ArugulaJoin` shows the current folder in the swarm and remembers it: nvim joins by itself next time it starts there.
- `:ArugulaLeave` takes it out at once, and forgets it.
- `:ArugulaStatus` says whether it's in, and how many follow it. `require("arugula").status()` is the same for a statusline.

It talks to the Arugula daemon on the same machine (`$ARUGULA_SOCK`, else `~/.local/state/arugula/sock`). Over ssh, nvim runs on the remote machine, so that's the daemon there. Set `vim.g.arugula = { socket = "…" }` before the plugin loads to use another socket.
