# Arugula's Claude Code plugin

One install sets Claude Code up for Arugula:

- **MCP:** `arugula mcp`, as `claude mcp add arugula -- arugula mcp` would add it.
- **Hooks:** what `arugula hooks install` writes: questions (`arugula ask`),
  permission prompts (`arugula hook`), follow-ups (`arugula inbox`) and
  attention, as cards on the web, the phone and the swarm.
- **A skill** (`arugula`): when and how to use the tools.

It calls the `arugula` CLI you installed, so install Arugula first
(<https://arugula.io>) and make sure `arugula` is on your PATH. Then, in
Claude Code:

```
/plugin marketplace add arugula-salad/arugula
/plugin install arugula@arugula
```

(From a shell: `claude plugin marketplace add arugula-salad/arugula` and
`claude plugin install arugula@arugula`.) A new session picks it up.

Without `arugula` on PATH, the plugin says so once when a session starts; the
hooks fail with "not found" until it's there. Outside an Arugula pane the hooks
do nothing. With the daemon stopped they behave as the settings.json hooks do:
a question or permission prompt waits up to a minute for it, then stays in the
terminal, and the MCP server answers each call with the error.

**Already ran `arugula hooks install`?** Nothing to undo: where settings.json
has a hook, the plugin's copy of it does nothing, so each runs once.
`arugula hooks status` names the plugin, and `arugula hooks install` adds only
permission rules while it's enabled. If you added the MCP server by path (the
setup page's **Add to Claude Code**), `hooks status` says to remove it, or
Claude Code lists every tool twice.

The plugin's MCP tools are named `mcp__plugin_arugula_arugula__*`. With the
plugin enabled, `arugula hooks install` adds the read-only permission rules
for those names as well.

## Working on it

`hooks/hooks.json` is `HOOKS_SNIPPET` in `crates/cli/src/hooks.rs` plus the
PATH check; `cargo test -p arugula hooks::tests::plugin_hooks_are_the_snippet`
fails when they differ. Check the plugin and the marketplace with
`claude plugin validate plugins/claude-code` and `claude plugin validate .`.
