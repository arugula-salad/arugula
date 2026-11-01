# Arugula's Claude Code plugin

One install sets Claude Code up for Arugula:

- **MCP:** `arugula mcp`, as `claude mcp add arugula -- arugula mcp` would add it.
- **Hooks:** what `arugula hooks install` writes: questions (`arugula ask`),
  permission prompts (`arugula hook`), follow-ups (`arugula inbox`) and
  attention, as cards on the web, the phone and the swarm.
- **A skill** (`arugula`): when and how to use the tools.
- **Live session signals, opt in** (off by default): a Claude Code mod that
  tells the pane its session's context window fill and size, model and
  effort, and compactions as they happen, with a status line. See below.

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

## Live session signals (opt in)

`hooks/live.ts` is a Claude Code mod (an in-process hooks module, listed
under `modules` in `hooks/hooks.json`) that tells the pane's daemon about
the session as it runs: the window's fill and size, the model and effort,
the cost, and each compaction, through `arugula session-signal`. Claude
Code's status line shows it: `arugula %1 ● live · 5% of 1M · v0.66.5`.

It's the plugin's `live` option, **off by default**. Off, the module loads
and hooks nothing. To turn it on:

- when you install the plugin, on its options screen;
- in Claude Code, `/config`, then "Live session signals";
- or from a shell:
  `echo '{"live":"true"}' | claude plugin configure arugula@arugula --values-stdin`.

`arugula hooks status` says whether it's on.

**Where it works.** It needs Claude Code 2.1.286 or later (mods), and an
`arugula` CLI with `session-signal`; with an older CLI the status line says
to update Arugula. It's verified **in a terminal** (Linux). The desktop app,
VS Code and hosted (cloud) sessions aren't verified yet (#448). Outside an
Arugula pane it does nothing.

**Older Claude Code.** One `hooks.json` holds both the command hooks and the
module (#449): a Claude Code without mods ignores `modules` and runs the
hooks, and a module that can't load never stops them.

**Early access.** The mod API changes between Claude Code releases. The
module hooks four events, pinned in `LIVE_EVENTS` in
`crates/cli/src/hooks.rs` (a test holds them): `session.start`,
`turn.step`, `session.measure` (once `session.usage`) and
`session.compact`. Claude Code refuses the whole module for an event name it
doesn't know, so a rename means no live signals, never broken hooks, and an
event that arrives in a shape the module doesn't read is passed on and named
on the status line (`session.measure changed in Claude Code 2.1.x`).

**Versions.** Like the rest of the plugin it carries no version of its own
and follows main (aru-039); `MOD_VERSION` in `live.ts`, sent with each
signal, moves when the signals do.

To work on it: `claude --plugin-dir plugins/claude-code --settings
'{"pluginConfigs":{"arugula@inline":{"options":{"live":true}}}}'` in a pane
of a dev daemon. Why it sits beside the hooks and how far it goes:
`docs/decisions/aru-128-*.md`.
