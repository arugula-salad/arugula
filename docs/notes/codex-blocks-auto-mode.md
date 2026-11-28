---
section: Fixed
title: Codex agent blocks run in auto mode
line: A Codex agent block started in auto mode runs in Codex's own auto-review mode instead of stopping on every action, and a finished turn reads as done, not as waiting on you.
---
Arugula's `auto` permission mode is Codex's `agent` mode, which asks only about what Codex's reviewer finds unsafe and keeps Codex's sandbox. It never lifts a restriction of Codex's own. If an agent doesn't offer the mode asked for, its block says so instead of starting in a different one. Agents still can't start another agent with checks turned off, under Codex's name for that mode too.
