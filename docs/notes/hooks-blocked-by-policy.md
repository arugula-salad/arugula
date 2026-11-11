---
section: Changed
title: Says when Claude Code's policy blocks Arugula's hooks
line: Where an organization's Claude Code policy (allowManagedHooksOnly) or disableAllHooks stops Arugula's hooks, Getting started, `arugula hooks status` and setup now say so, instead of showing them as working.
headline: no
---
Claude Code runs no user or plugin hooks when its managed settings set
`allowManagedHooksOnly` (or `disableAllHooks`), so a terminal Claude's
questions and permission prompts can't come to cards. Arugula reads those
files (`/etc/claude-code`, `/Library/Application Support/ClaudeCode`, and
`C:\Program Files\ClaudeCode` or `C:\ProgramData\ClaudeCode`, with their
`managed-settings.d`) and says which one blocks them. A pane that waits for
you is still marked from its screen, and agent blocks ask on cards without
hooks.
