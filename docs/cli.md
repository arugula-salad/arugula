# The CLI

This page moved to **[docs.arugula.io/cli](https://docs.arugula.io/cli/)**. MCP is
on [docs.arugula.io/mcp](https://docs.arugula.io/mcp/), and Claude Code's hooks on
[docs.arugula.io/agents](https://docs.arugula.io/agents/#claude-code-in-a-pane).

`arugula --help` lists its commands in the page's groups (`GROUPS` in
`crates/cli/src/main.rs`): Panes, Blocks, Agents, Sharing, Machines and
control, Cloud and projects, Setup. What Claude Code's hooks and the clients
run (`ask`, `hook`, `inbox`, `session-signal`, `bridge`, `attention STATE`) is left out of it
and listed by `arugula help plumbing`. `arugula inbox` is the follow-up hook
(hidden); what wants you on linked repos is `arugula forge inbox`.

## Signing in to Arugula cloud

`arugula login [URL]` makes this CLI one of the account's devices: it shows
a code to approve from a device you already have (`--account FINGERPRINT`
checks the account too, for an older control). `--host NAME` then asks
control for a machine token first (`POST /api/machines/{id}/token`) and opens
a channel with it and a key made for that channel alone; it uses the device
key only when control gives none (a control without an issuer key) or the
machine's daemon is older than tokens.

`arugula login --browser [URL]` signs in with no key (aru-141): it prints a
link to control's page; click Allow in a browser that's signed in, and the
browser hands the CLI a one-time grant on a loopback port, which the CLI
swaps, with a verifier only it holds, for a session of your account
(`cli-session.json` in the config dir), the way the desktop app signs in
(control's `/auth/app`). If the browser is on another computer (the CLI over
ssh), paste the address it lands on into the CLI. It's opt-in for now: the
session is a page's as far as control can tell, and any agent on this machine
can read the file, so it stays off by default until control gives a CLI a
session kind that page-only routes refuse. `arugula logout` ends the session.
