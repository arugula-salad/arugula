# Arugula

Every agent on every machine in one view. Answer them from your phone.
The workspace comes back after a reboot.

Arugula is a terminal multiplexer for coding agents. A daemon owns your
terminals; the browser, the desktop app and your phone draw them. Claude
Code, Codex and plain shells sit side by side as panes, on every machine
you've connected.

![A recording: Claude Code asks to run cargo test, the request appears on the swarm's Needs you rail in the browser and on a phone, and it's allowed from the phone](https://arugula.io/img/arugula-loop-10s.gif)

- **Every agent on every connected machine.** The swarm is every
  Arugula pane on the machines you've connected, in one live view,
  clustered by project, machine, kind or person. Detected requests for
  input, and failed commands it recognizes, appear on the *Needs you*
  rail. Claude Code, Codex or any [ACP](https://agentclientprotocol.com)
  agent also runs as a block beside your terminals (macOS and Linux):
  tool calls with their output, approvals and questions as cards.
- **Answer them from your phone.** Screen detection flags a waiting
  Claude Code or Codex pane. Claude Code's hooks and agent blocks turn
  requests into cards: large buttons for approvals and choices, or type
  the next instruction. Turn on notifications and a push opens the card
  on your phone. Other agents are detected too; their hooks or the bell
  say when they need you
  ([limitations](https://docs.arugula.io/limitations/)).
- **The workspace comes back after a reboot.** Close the window or
  disconnect: programs keep running on the host. Restart or update the
  daemon: programs keep running. Reboot: processes stop, and Arugula
  restores the layout, working directories and scrollback, then
  restarts each pane per its setting: a shell where it was (the
  default), re-run its command (asking first unless you choose
  otherwise), or resume Claude Code's conversation (`claude --resume`,
  the default for a Claude pane).
- **Open source, on your machines.** MIT or Apache-2.0. Run the daemon
  on your own machines or servers you manage, on Linux, macOS and
  Windows. Reach it over your own [Tailscale](https://tailscale.com)
  tailnet, or through [Arugula control](https://docs.arugula.io/control/)
  from a browser or phone without a VPN. Connections to your machines
  are end-to-end encrypted; control relays them and can't read them.
  Control serves the web client, so you trust that client not to read
  or alter your session
  ([what it can and can't see](https://docs.arugula.io/control/#what-it-can-and-cant-see)).
  Self-hosting control isn't supported yet.
- **Scriptable.** Any MCP client can run commands in panes you watch
  (`arugula mcp`); after installing Arugula, the Claude Code plugin
  configures its MCP tools and hooks; there's a VS Code / Cursor
  extension and a CLI for scripts.
- **Mouse first.** Click, drag and right-click for tabs and splits. No
  chords to learn, no prefix key.
- **VS Code and dev servers beside your terminals** (opt-in). *Open in
  editor* (or `arugula edit src/main.rs:42`) opens VS Code on the pane's
  machine, in its directory, as a block, back with its file after a
  restart; *Open a port…* shows a dev server beside its terminal. Both are
  off until the daemon gets a listener for them (one flag for this
  computer's browser; a domain of yours for the phone), and they're shown
  by a device that can reach the machine directly; not relayed through
  control. See
  [Blocks](https://docs.arugula.io/blocks/#browser-blocks-on-ports).
- **The live diff, from your phone.** *Changes* on a pane (or
  `arugula diff`) shows the live diff in the pane's repository, on its
  machine, with +/−; tap a file for its hunks and a line to see the file
  there, both updating as the edits land. Phone first, and a recognized
  failed command is a *Rerun* tap away.
- **Your editor in the swarm.** VS Code, Cursor or nvim (over Remote-SSH
  too) shows up beside your panes once you ask it to. Follow its cursor
  from your phone; a debugger stopping, or Claude Code wanting to edit a
  file, is a card you answer from anywhere. The extension is
  `arugula.arugula-editor` on the Visual Studio Marketplace and Open VSX
  (`code --install-extension arugula.arugula-editor`), or `arugula editors
  install` puts in the daemon's own copy.

With Arugula control, authorized teammates can share a session or a
whole machine, with roles and presence
([teams](https://docs.arugula.io/teams/),
[sharing](https://docs.arugula.io/sharing/)): anyone allowed can answer
an agent's card, and everyone sees who did.

[![A tour of the swarm: every pane clustered by project, then one project's panes up close, then an agent's request on the Needs You rail, then that agent's session opened](https://arugula.io/img/dive.gif)](https://arugula.io)

Linux (x86_64, arm64), macOS (Apple silicon and Intel) and Windows 10 and 11
(x86_64).

## Install

The desktop app (macOS, Linux, Windows) is at [arugula.io](https://arugula.io). On a server, or
a machine without a screen:

```
curl -fsSL https://arugula.io/install.sh | sh
```

On Windows, in PowerShell: `irm https://arugula.io/install.ps1 | iex`. With Homebrew:
`brew tap arugula-salad/tap && brew trust arugula-salad/tap && brew install arugula && arugulad install`.

[Install](https://docs.arugula.io/install/) has the details, and the [quickstart](https://docs.arugula.io/quickstart/) goes from
the browser to your phone, scripts, agents and MCP.

### Claude Code plugin

Instead of `claude mcp add arugula -- arugula mcp` and `arugula hooks install`,
one plugin brings Arugula's MCP tools, its hooks (questions, permission prompts
and follow-ups as cards) and a short skill. It calls the `arugula` you
installed, so that has to be on your PATH. In Claude Code:

```
/plugin marketplace add arugula-salad/arugula
/plugin install arugula@arugula
```

Already set up the hooks by hand? Each still runs once: the plugin's copy stands
aside. [plugins/claude-code](plugins/claude-code/README.md) has the details.

## Docs

**[docs.arugula.io](https://docs.arugula.io/)** has the docs for using Arugula: [panes](https://docs.arugula.io/panes/),
[blocks](https://docs.arugula.io/blocks/), [agents](https://docs.arugula.io/agents/), [control](https://docs.arugula.io/control/),
[teams](https://docs.arugula.io/teams/), [the CLI](https://docs.arugula.io/cli/) and [MCP](https://docs.arugula.io/mcp/). They're built from
[arugula-salad/site](https://github.com/arugula-salad/site).

For working on Arugula itself:

- [docs/development.md](docs/development.md): building from source, testing, releasing, and the
  code's layout.
- [docs/testing.md](docs/testing.md): the test suites.
- [docs/control-e2e.md](docs/control-e2e.md): how control's end-to-end encryption keeps it out of
  your terminals.
- [DECISIONS.md](DECISIONS.md): the decisions that still hold, an index of the records in
  [docs/decisions/](docs/decisions/).
- [docs/managoat.md](docs/managoat.md): Managoat, the first backend for Cloud workspaces,
  and what became of it (removed, on control's side and the daemon's).
- [docs/platform-features.md](docs/platform-features.md): which agent platform features (ACP, MCP,
  A2A, skills, scheduled tasks) Arugula brings in, which are planned, and the gaps.
- [docs/secrets-inventory.md](docs/secrets-inventory.md): every secret store and key across the
  org, by name and owner only, and where each goes as the org converges on the platform.
- [docs/plan-archive.md](docs/plan-archive.md): the original plan and every milestone.

## License

MIT OR Apache-2.0, at your option. Terminal emulation by
[libghostty](https://ghostty.org); see [THIRD_PARTY.md](THIRD_PARTY.md).
