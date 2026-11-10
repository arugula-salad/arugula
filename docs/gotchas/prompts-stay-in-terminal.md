---
title: "Claude Code's questions stay in its terminal, not as cards"
description: "The hooks only act inside an Arugula pane, with the daemon answering."
area: "Agents and MCP"
---

# Claude Code's questions stay in its terminal, not as cards

## You see

Claude Code asks for a permission, or a question, in its own terminal, and
no card shows up in Arugula (on your phone, in the bell). Sometimes, after
a minute, the terminal shows:

```
arugula hook: the daemon isn't answering; answer in the terminal
```

## Why

Arugula's hooks for Claude Code turn its prompts into cards, but only for
a Claude Code running in an Arugula pane: outside one (no `ARUGULA_PANE`
in its environment) they do nothing, on purpose and without a word
(`crates/cli/src/hook.rs`, `stdin_hook`, and `crates/cli/src/util.rs`,
`env_pane`). In a pane, a daemon that doesn't answer for 60 seconds leaves
the question to the terminal, so a session never waits on Arugula. And
without the hooks installed, there's nothing to ask Arugula at all.

## Do

- Run Claude Code in an Arugula pane (or as an agent block).
- `arugula hooks status` says what's installed and what's missing;
  `arugula hooks install` (or the Claude Code plugin) installs them.
- If the daemon isn't answering, see
  [arugula says "can't reach arugulad"](daemon-not-running.md).
