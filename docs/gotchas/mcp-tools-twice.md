---
title: "Claude Code lists Arugula's MCP tools twice"
description: "The plugin's server and one added by hand both run; remove the hand-added one."
area: "Agents and MCP"
---

# Claude Code lists Arugula's MCP tools twice

## You see

Claude Code's `/mcp` shows two `arugula` servers, or each Arugula tool
twice. `arugula hooks status` says:

```
/home/you/.claude.json has an MCP server `arugula` of its own beside the plugin's: `claude mcp remove arugula` leaves one copy of each tool.
```

## Why

The Claude Code plugin brings its own `arugula` MCP server. If you set
Arugula up by hand before (`claude mcp add arugula -- arugula mcp`), that
server is still in Claude Code's settings, and both run
(`crates/cli/src/hooks.rs`, `manual_mcp_beside_plugin`). Hooks don't have
this problem: a plugin hook that settings.json has too stands aside.

## Do

`claude mcp remove arugula`, which removes the one you added by hand and
keeps the plugin's.
