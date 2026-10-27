# Features

Most of this page moved to **[docs.arugula.io](https://docs.arugula.io/)**, split by
topic: [panes and sessions](https://docs.arugula.io/panes/),
[blocks](https://docs.arugula.io/blocks/),
[pull requests and issues](https://docs.arugula.io/forges/),
[agents](https://docs.arugula.io/agents/),
[attention and history](https://docs.arugula.io/attention/) and
[sharing](https://docs.arugula.io/sharing/). Labs features (VMs and the
rest) and chat and threads (control's, on by team) are described for the
team in [labs.md](labs.md),
and hosted (Cloud) workspaces in [control.md](control.md). The workspace,
below, is here until the
site's panes page has it.

## The workspace (0.46)

On a computer, the web client and the desktop app draw one workspace
around your panes. A phone keeps its own header and sheet.

- **The rail**, down the left: Arugula's mark (this machine's sessions at a
  glance), *Sessions* (shows or hides the sidebar), *Panes* (back to the
  session's last tab and pane), *Swarm* (every pane, on all machines),
  *Chat* and *Agents* where you have them, and *Settings*.
- **The session sidebar:** the sessions on the machine you're looking at,
  grouped into *Needs you*, *Working* and *Idle* from what their panes are
  doing, with their tab and pane counts, and the repository and branch
  when every pane agrees on one. Search filters it, `+` makes a session,
  and each row's `…` renames, shares, sets notifications or closes it.
  Its width and whether it's open are kept on this device. Below 1100
  pixels wide it folds into the rail and opens over the panes.
- **The header** says where you are: the machine and how it's reached
  (*Direct*, *Relayed*, *Reconnecting* or *Offline*), the session (its
  overview; ▾ for its menu) and the tab. Then command search, the bell
  (what needs you, on every machine), *Share*, who else is here, and your
  account.
- **The tabs**, then **the pane toolbar:** the pane count (a switcher),
  the layouts *Columns*, *Grid* and *Main + stack*, *Focus* (one pane fills
  the tab; again to restore it), and *+ Pane* (a shell, agent, chat, editor,
  changes or a port, on which machine, in which directory, to the right or
  below). The drawer previews the split before creation. Layouts and focus
  are the machine's, so everyone sees the same.
- **Each pane's header**, including a single or focused desktop pane: its number (the
  Ctrl+Shift+digit one), what it is, its status, where it runs, its
  directory and branch. Click it to type there, drag it to move the pane,
  double-click to focus, `…` for its menu. Headers reserve their own space;
  the cyan outline encloses the focused pane without covering terminal rows.
- **The pane menu** (right-click a pane, or `…` on its header): Copy, Paste,
  Select all and Clear scrollback, then Split right and down, Copy working
  directory and Attach file…, and Close pane. The rest is in submenus that
  open under their row (→ and ← on the keyboard): *Open beside* (an agent, a
  web page, a port, a pull request, the editor, …), *Move*, *Share* (links,
  ssh invites, who drives), *Thread* and *Pane settings* (restart policy,
  shell integration, *Forget history*, which asks for a second click). The
  command palette lists the same actions.
- **The activity drawer** under the panes (the session's commands,
  agent turns, and panes opening, exiting and closing and their machines,
  kept across a reload, a closed pane's too), and **the footer**:
  the machine, the focused pane's repository and branch, agents running,
  what needs input, and the keys.

**Cloud workspaces** are hosted workspaces: `arugulad` in a rented
sandbox, attached to a project ([control.md](control.md), "Hosted
workspaces"). A project's are listed on the platform page
(`app.arugula.io/p/:project/workspaces`, [control-ops.md](control-ops.md));
`arugula cloud create` makes one, small (2 vCPU, 2 GiB) or large (8 vCPU,
8 GiB, `--size large`). Each is a machine like any other: its
sessions and agent blocks open on control's page, and an agent on one of
your own machines drives it over MCP (`list kind cloud_workspaces`, `cloud`,
`read_cloud`). The Managoat-backed Cloud workspaces that came first, their
tabs, past runs and the Cloud workspace block were removed in M189 and
M190 ([managoat.md](managoat.md)).

While the connection is down the workspace says so: it shows what it last
heard, claims nothing is working or running, and turns off what would
change something (with why, on the button). A viewer sees the session
without the buttons they can't use, or with them off and the reason.

Appearance (in Settings) picks the palette: Slate until you pick one, or
*Match system* for Slate or Latte with the system. Every palette's text reads at WCAG AA (4.5:1) on
its backgrounds, and with *reduce motion* on nothing animates and the
cursor doesn't blink. New workspaces use soft pane corners; a saved corner
choice is kept. Local editor blocks follow the selected palette unless you
choose a different editor theme or custom editor colors. The editor palette
is shared per block: when clients choose different palettes, the latest update
wins. New embedded editors start with the Explorer
collapsed and compact chrome; Ctrl+B (Cmd+B on macOS) reopens the Explorer,
and subsequent layout choices are kept. VM editors use the bundled Slate theme.

### Chat blocks

A chat block is a plain conversation with Claude, as in Claude's desktop
app or ChatGPT: no repository, no shell, and none of a coding agent's
chrome. Open one from *+ Pane* (*Chat*), a pane's menu (*Start a chat*),
the phone's sheet (*New chat*), `arugula agent --chat`, or an agent's
`start_agent {chat: true}`. It runs Claude Code on the machine's own login
(through `claude-agent-acp`, as an agent block does) with every tool off,
none of your settings (so no hooks or `CLAUDE.md`), no MCP servers (Arugula's
neither, nor the account's claude.ai connectors), and a system prompt for a
conversation, in a folder of the block's own in the daemon's state
directory. No API key, and no other model client.

Its bar picks the model from the ones Claude Code offers. Until Claude Code
titles the conversation (after the first answer) it's called by its first
message. Messages are Markdown; images and files go in by paste, drop or
📎, as in an agent block. It belongs to the machine it's opened on: that
daemon keeps the conversation and brings it back after a restart, and
`capture`, history and search read it as they read an agent's transcript.
Codex isn't offered: codex-acp has no way to turn every tool off (the
shell goes with a feature flag, but its patch tool, code mode's `exec` and
its subagents stay).

### Keys

None of these takes a key a terminal program would get on its own: each
takes it only when it can act, and otherwise the key goes to the pane.

| Keys | What they do |
|---|---|
| Ctrl+Shift+P (⌘⇧P on a Mac) | Command search: every command, and this machine's sessions, tabs and panes |
| Ctrl+Shift+G | Go to a directory |
| Ctrl+Shift+1–9 | Go to pane 1–9 of this tab (the numbers on the pane headers) |
| Ctrl+Shift+Enter | Focus the active pane, or restore the layout |
| Ctrl+Shift+E | + Pane |
| Ctrl+Alt+Shift+arrows | Grow (→ ↓) or shrink (← ↑) the active pane (GNOME keeps these for itself) |
| Ctrl+Shift+C, Ctrl+Shift+V | Copy and paste in a terminal |
| Escape | Close the drawer, dialog or menu that's open |
| ⌘W, ⌘T, ⌘N, ⌘U | In the desktop app on a Mac: close the pane, new tab, new window, attach files |

From the keyboard alone: command search's *Move the keyboard to …* puts
it on the rail, the sessions' search, the tabs, the pane toolbar or the
header, and Tab goes on from there. On the tabs, ← → Home and End move
between them, Enter shows one, F2 renames it and the menu key opens its
menu. In the sessions' search, ↑ ↓ pick a session, Enter shows it and
Escape clears the search, then gives the keys back to the pane.
