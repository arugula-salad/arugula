# arugula.nvim

Your nvim in your [Arugula](https://arugula.io) swarm. Core nvim (0.10 or later), no dependencies; nvim-dap's debugger stops show too if it's installed.

Install it like any plugin from this directory, e.g. with lazy.nvim:

```lua
{ dir = "~/src/arugula/editors/nvim" }
```

or copy `editors/nvim` onto your `runtimepath`.

In an Arugula pane, nvim is already in the swarm: the pane shows its file, cursor line, unsaved buffers and diagnostic counts with or without this plugin (the daemon reads them over nvim's own server socket, nvim 0.10 or later). With the plugin, it connects by itself, sends `pane` (from `$ARUGULA_PANE`) in its hello, and the daemon puts what it sends on that pane rather than making an entry of its own: following the pane follows your buffer live, and a debugger stop or a merge conflict is a reason on the pane. Outside an Arugula pane it joins as an entry of its own, as below.

- `:ArugulaJoin` shows the current folder in the swarm and remembers it: nvim joins by itself next time it starts there.
- `:ArugulaLeave` takes it out at once, and forgets it. In a pane, the pane stays, read without the plugin.
- `:ArugulaStatus` says whether it's in, and how many follow it. `require("arugula").status()` is the same for a statusline.

It talks to the Arugula daemon on the same machine (`$ARUGULA_SOCK`, else `~/.local/state/arugula/sock`). Over ssh, nvim runs on the remote machine, so that's the daemon there. Set `vim.g.arugula = { socket = "…" }` before the plugin loads to use another socket.
