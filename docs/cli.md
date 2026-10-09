# The CLI

This page moved to **[docs.arugula.io/cli](https://docs.arugula.io/cli/)**. MCP is
on [docs.arugula.io/mcp](https://docs.arugula.io/mcp/), and Claude Code's hooks on
[docs.arugula.io/agents](https://docs.arugula.io/agents/#claude-code-in-a-pane).

## Claude Code in a pane

`arugula hooks install` puts Arugula's hooks in Claude Code's settings.json and,
beside them, `permissions.allow` rules (`--project DIR` writes
DIR/.claude/settings.json, `--dry-run` prints the result). It only adds; a second
run changes nothing.

- **Reading, by default:** `Bash(arugula ls:*)` and the same for `describe`,
  `tail`, `wait`, `history`, `log`, `search`, `capture`, `process`, `status`, `fs`
  and `hooks status`; `Bash(arugula hosts)` and `Bash(arugula shares)` with no
  arguments (their other forms change things); and the MCP tools that only read.
- **Acting, with `--allow-acting`:** `agent`, `run`, `send`, `keys`, `close`,
  `call`, `open`, `edit`, `view`, `diff`, `pr`, `issue`, `cd`, `rerun`,
  `upload`, `mouse`, `share` and `invite`, and every other MCP tool. An agent
  can then start other agents and type into panes without asking, so nothing
  adds them without the flag.

`arugula hooks status` says whether each tier is present, partial or missing.

Run each `arugula ...` command on its own, not in a pipeline or an `&&` chain:
Claude Code matches a rule against the whole command, so
`cd dir && arugula agent ... | grep x` falls through to its safety check even
with `Bash(arugula agent:*)` allowed.
