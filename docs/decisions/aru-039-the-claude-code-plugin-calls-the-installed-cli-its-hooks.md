---
schema: 1
id: "aru-039"
title: "The Claude Code plugin calls the installed CLI; its hooks stand aside for settings ones"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "The Claude Code plugin calls the installed CLI; its hooks stand aside for settings ones"
options:
  - id: "a"
    label: "The Claude Code plugin calls the installed CLI; its hooks stand aside for settings ones"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#229"
    url: "https://github.com/arugula-salad/arugula/issues/229"
decided_by: "jhgaylor"
decided_on: "2026-10-12"
reviews: []
constrains:
  - "path:plugins/claude-code"
  - "path:.claude-plugin/marketplace.json"
  - "path:crates/cli/src/hooks.rs"
  - "path:crates/cli/src/hook.rs"
  - "path:crates/cli/src/ask.rs"
  - "path:crates/cli/src/cmd/attention.rs"
---

# The Claude Code plugin calls the installed CLI; its hooks stand aside for settings ones

`plugins/claude-code` (with `.claude-plugin/marketplace.json` at the root, so
the repo is its marketplace) bundles `arugula mcp`, the hooks `arugula hooks
install` writes and a short skill. It ships no binary and isn't in any
release artifact: every command is the user's `arugula` on PATH, so the
plugin follows main and the CLI follows releases. Its hooks are
`HOOKS_SNIPPET` exactly (a test holds them to it), plus one SessionStart check
that says once when `arugula` isn't on PATH. Claude Code runs a plugin's hook
beside an identical settings.json one (it dedupes only across settings
files), so each `arugula` hook command, run with `CLAUDE_PLUGIN_ROOT` pointing
at this plugin, does nothing when a settings file has the same hook: the
settings one, which predates the plugin, is the one that runs. `hooks install`
adds no hooks while the plugin is enabled, and `hooks status` names it. For
the MCP server, Claude Code counts a plugin server with the same command as a
configured one (`claude mcp add arugula -- arugula mcp`) as a duplicate and
keeps the configured one; one registered by path (the setup page's Add to
Claude Code) isn't, and `hooks status` says to remove it. The plugin's tools
are `mcp__plugin_arugula_arugula__*`, so with the plugin enabled `hooks
install` adds its permission rules under that name too.
Where: `plugins/claude-code/`, `crates/cli/src/hooks.rs` (`plugin_copy_stands_aside`, `PLUGIN`), `crates/cli/src/hook.rs`, `crates/cli/src/ask.rs`, `crates/cli/src/cmd/attention.rs`.
From: #229.
