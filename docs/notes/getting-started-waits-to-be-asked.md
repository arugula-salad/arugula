---
section: Changed
title: The first screen asks nothing; hooks offered at a prompt
line: Getting started no longer opens by itself, and a terminal where Claude Code waits at a permission prompt offers its hooks, so you can answer it from anywhere.
---
Getting started stays in the command palette (Help), Settings and the phone's sheet, now without its Phone step: Tailscale is in Settings › This machine. A first visit no longer stacks the usage-reports card, What's new or the agent adapter's banner on top of your work; the card asks once you've continued a conversation, or on your next visit.

When Claude Code waits at a permission prompt in a terminal and Arugula's hooks aren't installed, that pane's header says *Answer these from anywhere*. **Turn on** runs `arugula hooks install`, nothing more, and says which file it changed (`~/.claude/settings.json`). Claude Code picks them up the next time it starts. Turned on or dismissed, the offer doesn't come back.
