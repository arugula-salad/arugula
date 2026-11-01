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
to update Arugula. It acts only where `ARUGULA_PANE` is set, and it reaches
the daemon through the `arugula` on PATH, which finds it through
`ARUGULA_SOCK` (set in every Arugula pane and agent block; tmux windows lack
it) or the default socket (the named pipe on Windows). Outside an Arugula
pane it does nothing. Where it's been tested is below.

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

### Where it was tested (#448)

These runs used the module as it was before it joined this plugin (the
separate `arugula-live` plugin, #1942): the same four events, the same
signals. Loading it now means this plugin with `live` on.

| Surface | Claude Code | start | step | usage | compact | Right pane |
| --- | --- | --- | --- | --- | --- | --- |
| Terminal, `claude` and `claude -p` (#1942) | 2.1.316 | yes | yes | yes | yes | proven |
| Agent block on a hosted box (arugula-dev-6), ACP adapter 0.85.0, loaded through `CLAUDE_CODE_PLUGIN_DIRS` | 2.1.286 (the SDK's) | yes | yes | yes | yes (`/compact`) | proven: the block's own id |
| VS Code extension's bundled Claude Code, started as the extension starts it (`stream-json`, `CLAUDE_CODE_ENTRYPOINT=claude-vscode`) | 2.1.314 | yes | yes (effort too) | yes | yes | proven for the pane in `ARUGULA_PANE` |
| VS Code's UI | | | | | | not run: needs a GUI |
| Desktop app, Windows 11 (Claude 4.4212, in a VM) | 2.1.309 (the app's) | yes | yes | yes | yes (`/compact`) | proven for the pane it was started from; status shown under the message box |
| Desktop app, macOS 26.6 (Claude 4.4212, in a tart VM) | 2.1.309 (the app's) | yes | yes | yes | yes (`/compact`) | proven for the pane it was started from; status shown (`arugula %6 ● live · 8% of 1M · 1 compacted`) |

Started from the Start menu or the Dock instead, the desktop app sent nothing,
as it should.

- **Hosted agent block.** The box's daemon and CLI were 0.64.0, older than
  `session-signal`, so the run used a throwaway daemon built from main on the
  box. An agent block's environment is the daemon's plus `ARUGULA_PANE` (the
  block's id), `ARUGULA_SOCK` and the daemon's folder first on PATH, and the
  ACP adapter hands all of it to Claude Code: the block's Claude Code reached
  that daemon and that block, never the box's own daemon. The adapter has no
  status row, so the status line goes nowhere there.
- **VS Code.** The extension starts Claude Code with its own environment plus
  `claudeCode.environmentVariables`, so `CLAUDE_CODE_PLUGIN_DIRS` there loads
  a plugin folder. The status line came back to the host as `ui_status`
  messages (`arugula %1 ● live · 4% of 1M · v0.66.5`), which the extension's
  mod surface draws.

What doesn't reach a pane, for now:

- **A hosted session's agent** (M255, a session from control running as
  `sesN`) starts under `env -i`: no `ARUGULA_PANE`, no `ARUGULA_SOCK`, no
  plugin folder, and the daemon's socket is closed to it, so it stays silent.
  Tabs from `cloud open_tab` run as the box's user and are covered above. A
  box's blocks load the plugin only if their daemon's environment (its spec,
  or the platform) names it.
- **VS Code and the desktop app** have no pane of their own. Claude Code there
  inherits the app's environment, which has `ARUGULA_PANE` only when the app
  itself was started from an Arugula pane (VS Code only when that start opened
  its first window: `code .` into a running VS Code keeps the old
  environment), and then every session in the app reports to that one pane.
  Arugula's VS Code editor block drops `ARUGULA_PANE` on purpose, so Claude
  Code inside it is silent.
- **Windows:** the module runs `arugula` with no shell, so `arugula.exe` must
  be on the PATH the app started with (a `.cmd` or `.ps1` shim won't do).

### Desktop app checklist (macOS, then Windows 11)

The kit is a zip of this folder with `.claude-plugin/plugin.json` and
`hooks/` at its root, the shape Settings > Plugins takes.

1. Install an `arugula` with `session-signal` (main after #1942, or the first
   release with it). `arugula session-signal </dev/null` answers JSON
   (`{"error":"not in an Arugula pane"}` outside a pane), not
   `unrecognized subcommand`.
2. Start a dev daemon (`just dev`, or `arugulad --state-dir <dir>`) and open a
   terminal pane in it. `echo $ARUGULA_PANE $ARUGULA_SOCK` shows the pane and
   the dev socket. On Windows: `$env:ARUGULA_PANE`, and `where.exe arugula`
   finds `arugula.exe`.
3. Quit the desktop app completely (macOS: Cmd-Q; Windows: quit from the
   tray), then start it from that pane so it inherits the pane's environment.
   - **macOS:** not `Claude &`. Started that way, the pane's shell exits about
     a second later (Claude's own shell takes the pane's terminal), and Claude
     is left pointing at a pane that's gone. Start it in a session of its own
     instead: `perl -MPOSIX -e 'exit 0 if fork; POSIX::setsid() or die; exec @ARGV' /Applications/Claude.app/Contents/MacOS/Claude`.
     If the app was downloaded with Safari and never opened from Finder, clear
     its quarantine first (`xattr -dr com.apple.quarantine /Applications/Claude.app`):
     otherwise macOS runs a translocated copy for the sign-in callback, and
     that copy doesn't have the pane's environment.
   - **Windows:** `Start-Process claude-desktop.exe` from the pane's
     PowerShell (the app is an MSIX package with that command alias).
4. Settings > Plugins > upload the zip. It lists as `arugula`. The upload is
   account-wide: it shows up in your other Claude desktop apps too, so remove
   it there when you're done testing.
5. Settings > Plugins > arugula > Settings: turn on **Live session signals**.
   If there's no Settings tab right after the upload, restart the app (from
   the pane again) and it appears. Saving it writes
   `pluginConfigs."arugula@inline".options.live` to `~/.claude/settings.json`.
   The command hooks (attention, `arugula hook`, `arugula ask`, `arugula
   inbox`) and the MCP server come along with the plugin too: that's
   expected, and they talk to the dev daemon the same way.
6. In the Code tab, start a local session in any folder. Send `Reply with
   just ok`, then `/compact`.
7. Look for the status text `arugula %N ● live · …% of … · v…` (N is the
   pane from step 2), with `1 compacted` after the `/compact`, and in the dev
   daemon's log `session signal pane=N` lines with `event=start`,
   `event=step` (model, effort), `event=usage` (tokens, window, cost) and
   `event=compact`.
8. Negative check: quit, start the app from the Dock or Start menu, open a
   session. No status text, no signal lines, no error.

Record which events arrived and whether the status showed. A session that
starts before the app's window attaches (Podium's note) may send `start`
before there's a status row to draw it; the daemon logs it all the same.
