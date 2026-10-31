---
schema: 1
id: "aru-128"
title: "A Claude Code mod reports live session signals beside the hooks, through the CLI"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  harness: "claude-code"
question: "Should Arugula's Claude Code plugin carry a mod (an in-process hooks module) for live session signals, and if so, does it replace the command hooks, and how does it reach the daemon?"
options:
  - id: "a"
    label: "A separate, opt-in mod beside the command hooks: observe only, modern events only, through `arugula session-signal`"
    how: "plugins/claude-code/mod (plugin `arugula-live`) hooks session.start, turn.step, session.measure and session.compact, never classic.*, each with a .catch that passes the event on. It runs `arugula session-signal` with the signal on stdin; the daemon reads it into the same Context #397 reads transcripts into, per pane. It shows a status line: `arugula %1 ● live · 5% of 1M · 1 compacted · v0.66.4`, or the version mismatch, or why it can't reach the daemon. The command hooks stay exactly as they are."
    tradeoff: "One process per signal (a few per turn, not per tool call); a second thing to install until it's folded into the arugula plugin; an early-access API that changes without notice (the brief's `session.usage` event is already `session.measure`), so it has to fail quietly and the transcript stays the fallback."
  - id: "b"
    label: "The mod replaces the command hooks (attention, `arugula hook`, `arugula ask`, `arugula inbox`, SessionStart)"
    how: "turn.complete for Stop, tool.check for permissions, session.send for follow-ups, prompt.compose for the steering context, all in one module."
    tradeoff: "Loses everything that works today without mods: older Claude Code, a Team or Enterprise login whose guard skips a user mod's classic.* hooks (Podium #595, our #395), and any host that doesn't load the plugin. A tool.check hook decides as policy before Claude Code's own dialog, it can't be the person's answer on a card the way PermissionRequest is; Notification has no modern event. A regression for the card flows, for signals we can already add without touching them."
  - id: "c"
    label: "A mod that talks HTTP to the daemon's socket directly (`$.http.fetch` with `socketPath`)"
    how: "The daemon already speaks HTTP/1.1 on its Unix socket, so the mod could POST /api/panes/N/session-signal itself, no process."
    tradeoff: "No process per signal, but no Windows: the daemon there is a named pipe, and the engine refuses a pipe address (Podium #834 had to fall back to a file mailbox). It also has to find the socket itself (ARUGULA_SOCK isn't set in panes; the CLI's default-socket logic would be copied into TypeScript), and drifts from aru-039's rule that the plugin calls the CLI you installed. A later optimisation for macOS and Linux, not the first path."
  - id: "d"
    label: "No mod: keep reading transcripts (#397, PR #1882) and add what's missing there"
    how: "The daemon follows the session's JSONL for model, usage and compact_boundary, and prices tokens itself for #398."
    tradeoff: "Nothing new to install or keep up with, and it works after the fact and across restarts. But it can't know the window (proven below: Sonnet 5.5 and Haiku 5.5 run with a 1M window here, the transcript names them without a marker and carries no 1M beta), it only covers agent blocks (not `claude` in a terminal pane), it has no cost ledger or rate limits, and the transcript format is undocumented and changes too."
choice:
  option: "a"
  reason: "Jake accepted the spike's recommendation: a separate opt-in plugin beside the command hooks, watch-only, via the CLI"
rejected:
  - option: "b"
    why: "It would lose what works without mods (older Claude Code, a Team or Enterprise login's guard, hosts without the plugin), and a mod can't be the person's answer to a permission."
  - option: "c"
    why: "No Windows (the daemon is a named pipe there, which the engine refuses), and it copies the CLI's socket logic; a later optimisation for macOS and Linux."
  - option: "d"
    why: "A transcript can't tell the window, covers only agent blocks, and has no cost ledger or rate limits; it stays the durable fallback, not the only source."
supersedes: []
evidence:
  - title: "#396: Spike: a Claude Code mod in our plugin for live session signals"
    url: "https://github.com/arugula-salad/arugula/issues/396"
  - title: "Podium's reporter, a Claude Code mod (decision 2, gotcha on classic hooks, CHANGELOG #803, #834, #408, #594, #595)"
    url: "https://github.com/eengelking/podium"
decided_by: "jhgaylor"
proposed_by: "claude-code (spike #396)"
decided_on: "2026-10-31"
reviews: []
constrains:
  - "aru-039"
  - "path:plugins/claude-code/mod"
  - "path:crates/arugulad-agent/src/conversations/context.rs"
  - "path:crates/cli/src/hook.rs"
---

# A Claude Code mod reports live session signals beside the hooks, through the CLI

**Decided: go with conditions (option a)**, as the spike recommended. A mod is the only way to
learn the context window as Claude Code has it, the engine's own cost and
rate limits, and per-loop (subagent) usage while the session runs, and it is
the only source at all for `claude` in a terminal pane. It is additive: it
sits beside the command hooks of aru-039 and never replaces them.

## What was proven (Claude Code 2.1.316, Linux, a throwaway dev daemon)

`claude --plugin-dir plugins/claude-code/mod` in a pane of a dev daemon,
interactive (Sonnet 5.5) and then `claude -p` (Haiku 5.5). The daemon logged
`start`, `step` (model, effort), `usage` (fill, window, cost) after each turn,
and a `/compact` as `compact` with its trigger and tokens, 11 ms after Claude
Code wrote the `compact_boundary` line. The status line read
`arugula %1 ● live · 5% of 1M · 1 compacted · v0.66.4`. The run log is in the
PR (#396's).

The window: both models ran with a 1,000,000-token window. Their transcript
entries say `claude-sonnet-5-5` and `claude-haiku-5-5`, with no `[1m]` and no
1M beta in the request, so `context::window_for` (#397) reads 200k: 51k
tokens show as 26% instead of 5%, and an automatic compaction at 190k would
be marked an overflow. The mod's `window` overrides the guess
(`Context::signal`, tested).

## The conditions

1. **Beside, not instead.** Attention, `arugula hook` (permissions),
   `arugula ask`, `arugula inbox` and the SessionStart context stay command
   hooks. They work on every Claude Code that loads the plugin, and a
   Team/Enterprise guard skips a user mod's `classic.*` hooks, not settings
   hooks. A mod can't be the person's answer to a permission (a `tool.check`
   verdict is policy, decided before Claude Code's own dialog), and
   Notification has no modern event.
2. **Observe only, modern events only.** No `classic.*`, every hook passes the
   event on and has a `.catch` that does too, so a mod failure never changes
   what Claude Code does. No hook gates anything.
3. **Through the CLI.** `arugula session-signal` on stdin, as aru-039 has the
   plugin call the installed CLI: the CLI finds the daemon (the named pipe on
   Windows), and its version is what the status line compares. Only turn-level
   events send (a `step` only when the model or effort changes), so it's a
   few processes a turn. HTTP on the socket (option c) can come later for
   macOS and Linux.
4. **Its own plugin until a check says otherwise.** It ships as a second
   marketplace entry (`arugula-live`), opt in, not in `plugins/claude-code`'s
   `hooks.json`: whether a `hooks.json` with both `hooks` and `modules` loads,
   and what a Claude Code without mods does with `modules`, is untested, and
   the arugula plugin must keep loading everywhere it does now.
5. **The transcript stays.** Signals live in the daemon's memory, per pane
   (lost on restart, as a pane's attention is); the transcript is durable and
   covers agent blocks, which the mod doesn't reach until a hosted session
   loads it (`CLAUDE_CODE_PLUGIN_DIRS`). How they combine: one `Context` per
   session; the transcript fills it (history, after a restart, agent blocks),
   the mod's signals override what only the engine knows (window, cost, rate
   limits) and arrive first; neither clears the other.
6. **CI keeps it honest.** `claude plugin validate` and a type check against
   the engine's `claude-code.d.ts` in CI before it's listed, since the API is
   early access.

## The questions #396 asked

- **The API.** In-process TypeScript or JavaScript modules, no Node: the engine
  runs them itself (Podium needs Node only for its TLS helper). Events for
  sessions, turns, steps, tools, compaction, measuring, subagents; `$` for the
  status line, toasts, panes, tools, processes, http, files, settings, env.
  The engine's own types say "EARLY ACCESS: this surface may change between
  releases without notice". Where: the terminal (proven), `claude -p`
  (proven), the desktop app's local sessions on macOS and Windows (its docs
  and Podium #408 say so; untested here), VS Code (a surface the API names),
  SDK hosts through `CLAUDE_CODE_PLUGIN_DIRS` (untested; that's how a hosted
  box's ACP sessions would load it). The first Claude Code version with mods
  isn't stated.
- **The Team/Enterprise guard.** The docs don't name `cc-plugin-sec-default`.
  They say managed plugins sit in the `prepend` tier above `user`, that an
  outer plugin may continue past any inner hook on any event, and that
  `plugin.register` can refuse a user module whole. So nothing documented
  limits a guard to `classic.*`; Podium saw only `classic.*` skipped. #395
  tests it live; a debug-log line `bypassed by` says when it happens.
- **The daemon.** The CLI (above); `$.http.fetch` over the socket is
  macOS/Linux only.
- **Status line.** Proven: pane, live, fill, compactions, and `cli X ≠ daemon
  Y` when they differ. "Hooks are live" is a `$.settings.read()` away (the
  `arugula hook` entries, or the plugin in `enabledPlugins`), not built.
- **#397 and #398.** The mod adds the real window, terminal panes, the
  engine's cost ledger (`cost.usd`, for #398, priced by Claude Code, with
  subagents and compaction calls in it), rate limits, per-subagent usage
  (`agentId` on `turn.step`), and a compaction at the moment it runs. Effort
  is in the transcript too (`effort` on assistant entries since about 2.1.31x).
  Both sources set the fill to `postTokens` after a compaction, which is the
  conversation alone (13k), not the fill the next turn reports (52k): hold the
  fill until the next usage instead.

Where: `plugins/claude-code/mod/` (the mod), `crates/cli/src/hook.rs`
(`session_signal`), `crates/arugulad-agent/src/ops.rs` (`PaneSessionSignal`),
`crates/arugulad-agent/src/conversations/context.rs` (`Context::signal`).
From: #396.
