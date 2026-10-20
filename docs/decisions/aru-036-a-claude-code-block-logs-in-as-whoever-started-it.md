---
schema: 1
id: "aru-036"
title: "A Claude Code block logs in as whoever started it"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A Claude Code block logs in as whoever started it"
options:
  - id: "a"
    label: "A Claude Code block logs in as whoever started it"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#379"
    url: "https://github.com/arugula-salad/arugula/issues/379"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/agent/defs.rs"
  - "path:crates/daemon/src/agent/mod.rs"
  - "path:crates/mux/src/mux/blocks.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
  - "path:crates/cli/src/cmd/agent.rs"
  - "path:crates/cli/src/mcp.rs"
---

# A Claude Code block logs in as whoever started it

A block's adapter used to get the daemon's environment, so under launchd it
read the default login whatever login its caller had, and a stale one failed
every first turn with nothing to say which. A Claude Code block's config now
has `claude_config_dir`, which its local adapter gets as `CLAUDE_CONFIG_DIR`
over the daemon's and the login shell's. `illogical agent` sends the CLI's
(`""` for none: the default login). `illogical mcp` sends its client's in an
`Illogical-Claude-Config-Dir` header, on the local socket only, and
`start_agent` puts it on the block. With neither, the block takes it from what
it's opened beside: an agent block's own `claude_config_dir` (a lead's
subagents share its login), or a terminal's foreground program's environment,
else its shell's (`procinfo::env_var`, that one variable from
`/proc/PID/environ`). macOS 26 shows no other process's environment, so there
*Start an agent…* beside a terminal falls back to the daemon's login. Opened
conversations (M33) keep the daemon's directory, where the index found them;
a VM's Claude Code keeps its token file. An `acp` block takes it too (it may be
the adapter run by hand); Codex refuses it.

The directory is a path, so it's kept in `layout.json` and a restarted daemon
starts the adapter with it again. Keys and tokens in a caller's environment
(`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`) are
not passed on: they'd have to be kept on disk to survive a restart. There's no
daemon-wide setting for agents' directory: `CLAUDE_CONFIG_DIR` in the
service's or the login shell's environment already is one, and a second would
hide whose login a block used.

Each local start notes which login the adapter got (`{"e": "login"}` in the
block's log, so a rebuilt block knows too), naming any key or token variable
by name only. A turn or session that fails with ACP's `authRequired` (-32000),
or a message about authenticating, says in the transcript and the card's
error which login it used and how to log in to that one
(`CLAUDE_CONFIG_DIR=… claude`, or `env -u CLAUDE_CONFIG_DIR claude`, then
`/login`); a VM block names its token or key file.
Where: `crates/daemon/src/agent/defs.rs` (`claude_config_dir`, `login_hint`), `crates/daemon/src/agent/mod.rs` (`local_login`, `auth_failed`), `crates/mux/src/mux/blocks.rs` (`agent_login`), `crates/daemon/src/mcp/tools.rs` (`start_agent`), `crates/cli/src/cmd/agent.rs`, `crates/cli/src/mcp.rs`.
From: #379.
