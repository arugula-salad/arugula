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

`arugula login [URL]` prints a link to control's page. Open it in a browser
that's signed in and click Allow: the browser hands the CLI a one-time grant
on a loopback port, and the CLI swaps it, with a verifier only it holds, for
a session of your account (`cli-session.json` in the config dir), the way
the desktop app signs in (control's `/auth/app`). If the browser is on
another computer (the CLI over ssh), it can't reach the CLI's port: paste the
address it lands on into the CLI instead. `arugula logout` ends the session.

There's no key and no certificate (aru-141). `--host NAME` finds the machine
in control's directory and opens a channel to it with a machine token from
control (`POST /api/machines/{id}/token`) and a key made for that channel
alone; the daemon checks the token. A CLI logged in before this is one of
the account's devices: it asks for tokens the same way, and uses its key only
when control gives none (a control without an issuer key) or the machine's
daemon is older than tokens. `arugula login --device` (or `--account
FINGERPRINT`) still makes such a device, until the key code goes (#1007).
