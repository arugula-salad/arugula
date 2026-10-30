# arugula.nvim

Your nvim in your [Arugula](https://arugula.io) swarm. Core nvim (0.10 or later), no dependencies; nvim-dap's debugger stops show too if it's installed.

In an Arugula pane, Arugula offers to add it: one click installs it into nvim's own package directory (`stdpath("data")/site/pack/arugula/start/arugula.nvim`, with an `ARUGULA_INSTALLED` marker file) and loads it into the running nvim, with no restart and nothing written to your config. The daemon refreshes that copy when it's upgraded; Settings > Editor > Remove arugula.nvim deletes it. A copy your plugin manager installed is never offered over or touched.

Or install it with your plugin manager. Arugula serves the plugin as a git repository at `https://arugula.io/git/nvim`, with one commit for each release and a tag (`v0.66.1` and so on) on it. With lazy.nvim:

```lua
{ url = "https://arugula.io/git/nvim", name = "arugula.nvim" }
```

With vim-plug:

```vim
Plug 'https://arugula.io/git/nvim', { 'as': 'arugula.nvim' }
```

With packer.nvim:

```lua
use { "https://arugula.io/git/nvim", as = "arugula.nvim" }
```

Without a plugin manager, clone it into a package directory of your own:

```sh
git clone --depth 1 https://arugula.io/git/nvim ~/.local/share/nvim/site/pack/plugins/start/arugula.nvim
```

`git pull` in that directory updates it, and `:helptags ALL` (after `:packadd`, or on the next start) makes `:help arugula` work; the plugin managers do that themselves. The `name` and `as` options only keep the folder from being called `nvim`.

You get the newest release by default. To stay on one, name its tag: `tag = "v0.66.1"` in the lazy.nvim spec, `{ 'as': 'arugula.nvim', 'tag': 'v0.66.1' }` as vim-plug's options, `tag = "v0.66.1"` in packer's, or `git clone --branch v0.66.1 --depth 1 ...` by hand.

A copy from a plugin manager or from `git clone` is yours. Before the one-click install writes anything, it looks for arugula.nvim on nvim's runtimepath, in every package directory and in lazy.nvim's plugin list; if it finds a copy it didn't install, it doesn't offer, and it never changes that copy. The copy it does manage is the one directory holding the `ARUGULA_INSTALLED` marker, `site/pack/arugula/start/arugula.nvim`, and a directory at that path without the marker is left alone too. That is why the clone above goes under `pack/plugins/` rather than `pack/arugula/`. Use one or the other: if the one-click copy is already there and you add another, nvim has two, the first on the runtimepath loads and the second does nothing, and the daemon keeps refreshing its own. Remove it first with Settings > Editor > Remove arugula.nvim.

In an Arugula pane, nvim is already in the swarm: the pane shows its file, cursor line, unsaved buffers and diagnostic counts with or without this plugin (the daemon reads them over nvim's own server socket, nvim 0.10 or later). With the plugin, it connects by itself, sends `pane` (from `$ARUGULA_PANE`) in its hello, and the daemon puts what it sends on that pane rather than making an entry of its own: following the pane follows your buffer live, and a debugger stop or a merge conflict is a reason on the pane. Outside an Arugula pane it joins as an entry of its own, as below.

When Claude Code runs in an Arugula pane in the same session (the same tab first), each edit it proposes opens in this nvim as a vimdiff tab: the file as it is against the proposed text, with a header naming the agent and the file. `<leader>aa` or `:ArugulaAccept` accepts it (edit the proposed side first to send your text instead), `<leader>ar` or `:ArugulaReject` rejects it. The diff card on the pane and the rail shows the same edit and says "Also open in nvim (pane %N)"; answering in either place, or in Claude Code's terminal, closes both. Two edits in a row open a tab each, in order. Change the keys with `keys = { accept = …, reject = … }` (false for none). See `:help arugula-diff`.

An nvim editor block (Add a pane > Editor > nvim, or Open in editor with nothing open when your Editor preference is nvim) is a pane running nvim with this plugin loaded from the daemon's own copy, ahead of any other, even if you never installed it; your config still loads. The block also takes the page's colors: the plugin applies them as the colorscheme `arugula` and again whenever the page's palette changes. `vim.g.arugula = { theme = false }` keeps your colorscheme instead, and picking another with `:colorscheme` keeps that one for the rest of that nvim.

- `:ArugulaJoin` shows the current folder in the swarm and remembers it: nvim joins by itself next time it starts there.
- `:ArugulaLeave` takes it out at once, and forgets it. In a pane, the pane stays, read without the plugin.
- `:ArugulaStatus` says whether it's in, and how many follow it. `require("arugula").status()` is the same for a statusline.
- `:checkhealth arugula` checks the daemon's socket, says which daemon answers on it, and names the pane this nvim is tied to.

It talks to the Arugula daemon on the same machine (`$ARUGULA_SOCK`, else `~/.local/state/arugula/sock`). Over ssh, nvim runs on the remote machine, so that's the daemon there. Set `vim.g.arugula = { socket = "…" }` before the plugin loads to use another socket.
