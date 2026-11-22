# Arugula

Any agent, on any machine. Run Claude Code, Codex or any
[ACP](https://agentclientprotocol.com) agent in panes that keep running on
your own machines. Watch them from the web or your phone, and take a
session over.

Arugula is a terminal multiplexer for coding agents. A daemon owns your
terminals; the browser, the desktop app and your phone draw them. Claude
Code, Codex and plain shells sit side by side as panes, on every machine
you've connected.

![A recording: Claude Code asks to run cargo test, the request appears in Needs you on the Overview in the browser and on a phone, and it's allowed from the phone](https://arugula.io/img/arugula-loop-10s.gif)

- **Any agent, on any machine.** On app.arugula.io, the Overview is
  where a project opens: each board in one row, with the tickets in
  flight and the agents on them. Click a board to zoom to its tickets, and a ticket to see its
  agents' panes, live. Detected requests for input, and failed commands
  it recognizes, wait in *Needs you*. *Swarm*, a view of the Overview,
  shows every pane on every machine at once. Claude Code, Codex or any
  ACP agent also runs as a block beside your terminals (macOS and
  Linux): tool calls with their output, approvals and questions as cards.
- **Take a session over.** Every pane keeps running on its machine
  while you watch it from the browser, the desktop app or your phone.
  *Hand off to me* sends your typing to it; *Give it back* returns it.
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
  from a browser or phone without a VPN. Control decides who reaches a
  joined machine: it can read the terminal traffic it relays (it's built
  to store none of it), and it can open a shell on any machine your
  account reaches, so you trust it with them. Each machine logs who came
  in, and can turn typing through control off
  ([what control can do](docs/control-e2e.md#what-control-can-do-to-your-machines)).
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
- **Your editor beside your agents.** VS Code, Cursor or nvim (over Remote-SSH
  too) shows up beside your panes once you ask it to. Follow its cursor
  from your phone; a debugger stopping, or Claude Code wanting to edit a
  file, is a card you answer from anywhere. The extension is
  `arugula.arugula-editor` on the Visual Studio Marketplace and Open VSX
  (`code --install-extension arugula.arugula-editor`), or `arugula editors
  install` puts in the daemon's own copy.

[![A tour of the Overview: a project's three boards, one row each, then the Launch v1 board with an agent on each ticket in flight, then ticket #14's agent and shell, live, and back out](https://arugula.io/img/dive.gif)](https://arugula.io)

Linux (x86_64, arm64), macOS (Apple silicon and Intel) and Windows 10 and 11
(x86_64).

## For your team

Start alone. When your team joins, through
[Arugula control](https://docs.arugula.io/control/):

- **Boards that follow the work.** A board is a goal and the tickets
  that reach it. A row's lane follows its work: an agent starting on the
  ticket moves it to *Doing*, opening a pull request moves it to
  *Review*, and the merge moves it to *Done*. When an agent needs a
  person to decide something, it asks on the row.
- **Watch a session, or answer its agent.** Share a session or a whole
  machine, and choose who watches and who drives. A teammate who drives
  can answer
  your agent's card from the Overview or their phone.
- **Everyone sees who did what.** An answered card names who answered,
  a board keeps who answered each ask, and an issue's timeline says
  which person or whose agent did each thing.
- **Talk beside the work.** Team channels, DMs, and a thread on each
  board that its lead agent reads.

[![The Launch v1 board: tickets in Next, Doing, Review and Done, with the agents working on them](https://arugula.io/img/board.png)](https://docs.arugula.io/boards/)

See [boards](https://docs.arugula.io/boards/),
[teams](https://docs.arugula.io/teams/) and
[sharing](https://docs.arugula.io/sharing/).

## Install

The desktop app (macOS, Linux, Windows) is at [arugula.io](https://arugula.io). On a server, or
a machine without a screen:

```
curl -fsSL https://arugula.io/install.sh | sh
```

On Windows, in PowerShell: `irm https://arugula.io/install.ps1 | iex`. With Homebrew:
`brew tap arugula-salad/tap && brew trust arugula-salad/tap && brew install arugula && arugulad install`.

The Mac app is signed and notarized. The Windows installer isn't signed yet:
SmartScreen says *Windows protected your PC*, and *More info*, then *Run anyway*
gets past it. Every download has a SHA-256 in its release's `SHA256SUMS`, and the
installers have a minisign signature: [Check a download](https://docs.arugula.io/install/#check-a-download).

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

The plugin also has **live session signals**, off by default: Claude Code tells
Arugula its context window's fill and size, its model and effort, its cost and
each compaction as it happens, and shows them in its status line. Arugula keeps
them by session: the conversations list shows each session's figures, from a
terminal, VS Code, the editor block or the desktop app, and a pane shows the
session running in it. Turn them on at install, or later in `/config` ("Live
session signals"). They need Claude Code 2.1.286 or later, and on Windows the
real `arugula.exe` on PATH. They're verified in a terminal, in an agent block,
with VS Code's Claude Code and in the desktop app on macOS and Windows; the
plugin's README says where each one shows.

## Docs

**[docs.arugula.io](https://docs.arugula.io/)** has the docs for using Arugula: [panes](https://docs.arugula.io/panes/),
[blocks](https://docs.arugula.io/blocks/), [agents](https://docs.arugula.io/agents/), [control](https://docs.arugula.io/control/),
[teams](https://docs.arugula.io/teams/), [the CLI](https://docs.arugula.io/cli/) and [MCP](https://docs.arugula.io/mcp/). They're built from
[arugula-salad/site](https://github.com/arugula-salad/site).

When something goes wrong, or before you trust it with your machine:

- [status.arugula.io](https://status.arugula.io): whether hosted Arugula (control, the
  platform, installs and downloads) is up right now, and any incident we're working on.
- [docs/gotchas/](docs/gotchas/README.md): sharp edges people have hit, one page each, found
  by what you see on screen.
- [docs/what-leaves-your-machine.md](docs/what-leaves-your-machine.md): what the daemon sends to
  Arugula control, to Cloud workspaces and elsewhere, what it never sends, and the switch for
  each.

For working on Arugula itself:

- [docs/development.md](docs/development.md): building from source, testing, releasing, and the
  code's layout.
- [docs/testing.md](docs/testing.md): the test suites.
- [docs/control-e2e.md](docs/control-e2e.md): what control can do to your machines, and the
  design of its keys, channels and relay.
- [DECISIONS.md](DECISIONS.md): the decisions that still hold, an index of the records in
  [docs/decisions/](docs/decisions/).
- [docs/plan-archive.md](docs/plan-archive.md): the original plan and every milestone.

## What's open

This repository is the open part of Arugula: the daemon (`arugulad`), the
CLI, the web client, the desktop app, the iOS app and its Rust core, the
editor extensions, and the wire contract they and control speak
([contract/](contract/README.md)).

Arugula control and the platform, the hosted service behind
control.arugula.io and app.arugula.io, are closed source and live in a
private repository. Their code was published here under MIT OR Apache-2.0
until it moved out, and that code stays under those licences: it's in this
repository's history (`git log -- crates/control crates/platform`).

## License

MIT OR Apache-2.0, at your option, for everything in this repository.
Terminal emulation by [libghostty](https://ghostty.org); see
[THIRD_PARTY.md](THIRD_PARTY.md).
