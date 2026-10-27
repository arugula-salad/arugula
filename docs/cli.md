# The CLI

This page moved to **[docs.arugula.io/cli](https://docs.arugula.io/cli/)**. MCP is
on [docs.arugula.io/mcp](https://docs.arugula.io/mcp/), and Claude Code's hooks on
[docs.arugula.io/agents](https://docs.arugula.io/agents/#claude-code-in-a-pane).

`arugula --help` lists its commands in the page's groups (`GROUPS` in
`crates/cli/src/main.rs`): Panes, Blocks, Agents, Sharing, Machines and
control, Cloud and projects, Setup. What Claude Code's hooks and the clients
run (`ask`, `hook`, `inbox`, `bridge`, `attention STATE`) is left out of it
and listed by `arugula help plumbing`. `arugula inbox` is the follow-up hook;
what wants you on linked repos is `arugula forge queue` (`forge inbox` still
works).
