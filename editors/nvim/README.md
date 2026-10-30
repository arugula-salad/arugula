# arugula.nvim

Your nvim in your [Arugula](https://arugula.io) swarm. Core nvim (0.10 or later), no dependencies; nvim-dap's debugger stops show too if it's installed.

In an Arugula pane, Arugula offers to add it: one click installs it into nvim's own package directory (`stdpath("data")/site/pack/arugula/start/arugula.nvim`, with an `ARUGULA_INSTALLED` marker file) and loads it into the running nvim, with no restart and nothing written to your config. The daemon refreshes that copy when it's upgraded; Settings > Editor > Remove arugula.nvim deletes it. A copy your plugin manager installed is never offered over or touched.

Or install it like any plugin from this directory, e.g. with lazy.nvim:

```lua
{ dir = "~/src/arugula/editors/nvim" }
```

or copy `editors/nvim` onto your `runtimepath`.

In an Arugula pane, nvim is already in the swarm: the pane shows its file, cursor line, unsaved buffers and diagnostic counts with or without this plugin (the daemon reads them over nvim's own server socket, nvim 0.10 or later). With the plugin, it connects by itself, sends `pane` (from `$ARUGULA_PANE`) in its hello, and the daemon puts what it sends on that pane rather than making an entry of its own: following the pane follows your buffer live, and a debugger stop or a merge conflict is a reason on the pane. Outside an Arugula pane it joins as an entry of its own, as below.

When Claude Code runs in an Arugula pane in the same session (the same tab first), each edit it proposes opens in this nvim as a vimdiff tab: the file as it is against the proposed text, with a header naming the agent and the file. `<leader>aa` or `:ArugulaAccept` accepts it (edit the proposed side first to send your text instead), `<leader>ar` or `:ArugulaReject` rejects it. The diff card on the pane and the rail shows the same edit and says "Also open in nvim (pane %N)"; answering in either place, or in Claude Code's terminal, closes both. Two edits in a row open a tab each, in order. Change the keys with `keys = { accept = …, reject = … }` (false for none). See `:help arugula-diff`.

An nvim editor block (Add a pane > Editor > nvim, or Open in editor with nothing open when your Editor preference is nvim) is a pane running nvim with this plugin loaded from the daemon's own copy, ahead of any other, even if you never installed it; your config still loads. The block also takes the page's colors: the plugin applies them as the colorscheme `arugula` and again whenever the page's palette changes. `vim.g.arugula = { theme = false }` keeps your colorscheme instead, and picking another with `:colorscheme` keeps that one for the rest of that nvim.

If you have no nvim config at all (no `init.lua` or `init.vim` in `stdpath("config")`, no `$VIMINIT`, no `-u`), the block also applies a few starter settings: mouse, system clipboard, line numbers, smart-case search, persistent undo, sensible splits, a netrw file tree on `<leader>e` (space), and Esc to clear search highlight. Built-in options only, and it says so once when nvim starts. Creating any `init.lua` turns them off; `:help arugula-starter` lists them all.

- `:ArugulaJoin` shows the current folder in the swarm and remembers it: nvim joins by itself next time it starts there.
- `:ArugulaLeave` takes it out at once, and forgets it. In a pane, the pane stays, read without the plugin.
- `:ArugulaStatus` says whether it's in, and how many follow it. `require("arugula").status()` is the same for a statusline.
- `:checkhealth arugula` checks the daemon's socket, says which daemon answers on it, and names the pane this nvim is tied to.

It talks to the Arugula daemon on the same machine (`$ARUGULA_SOCK`, else `~/.local/state/arugula/sock`). Over ssh, nvim runs on the remote machine, so that's the daemon there. Set `vim.g.arugula = { socket = "…" }` before the plugin loads to use another socket.
