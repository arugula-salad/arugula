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

`arugula login [URL]` signs in from a browser, with no key (aru-141): it
prints a link to control's page; click Allow in a browser that's signed in,
and the browser hands the CLI a one-time grant on a loopback port, which the
CLI swaps, with a verifier only it holds, for a session of your account
(`cli-session.json` in the config dir), the way the desktop app signs in
(control's `/auth/app`). If the browser is on another computer (the CLI over
ssh), paste the address it lands on into the CLI. `--browser` says the same
and is kept for scripts. `arugula logout` ends the session.

The session is a CLI's, not a browser's (#1018): any agent on this machine
can read its file, so control lets it do what the CLI does (machines,
machine and join tokens, projects, issues, grants, runs) and refuses it what only a
browser's page does: allowing a sign-in, passkeys, approving devices, joins
and deploys, standing run grants, and changing a team's members. Allow
screens say which one asks, the CLI or the desktop app. A control too old to
tell the two apart answers without saying it's a CLI's session; then plain
`arugula login` joins this terminal as one of your devices instead (below),
and `arugula login --browser` stops (#1151).

Anything that runs as you on a computer where the CLI is signed in, agents
you run included, can do what you can on every machine your account
reaches: it can read the session's file, or run `arugula --host NAME`, and
open a shell on any of them. Only the browser's part above is kept from it.

`arugula login --device [URL]` makes this CLI one of the account's devices:
it shows a code to approve from a device you already have (`--account
FINGERPRINT` checks the account too, for an older control). Since #1004
browsers sign in with no device key, so only one that was a device before
can approve it. `--host NAME` then asks control for a machine token first
(`POST /api/machines/{id}/token`) and opens a channel with it and a key made
for that channel alone; it uses the device key only when control gives none
(a control without an issuer key) or the machine's daemon is older than
tokens.

## Join tokens

`arugula join-token` makes a join token (aru-141, Arugula #1152) and prints
it alone, so a script or an image's build joins a machine with no browser:

```sh
arugulad join --token "$(arugula join-token)"
```

The machine joins at once, on control's word. A token is good once, until it
runs out; `--json` prints `{"token", "expires"}` (seconds since the epoch).
`--team ID` makes it for a team you own or edit, and the machine joins the
team; a viewer gets control's refusal. It needs `arugula login`. A project's
settings make the same under Machines: for the project's team, or for you on
a personal project.

## Who reached a machine

`arugulad sharing log`, run on the machine, prints who reached it through
Arugula cloud, newest first: the login, the role, the path (relayed or
direct), when the session opened and how long it lasted, or that it's open
now (Arugula #1094). It reads `remote-sessions.jsonl` in the state directory,
so it works with the daemon stopped. The machine's Settings page shows the
same list to its owner, under Remote sessions.
