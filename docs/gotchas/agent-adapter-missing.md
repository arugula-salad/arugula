---
title: "An agent block won't start: \"agent blocks run it through it\""
description: "The agent's ACP adapter, or Node, isn't installed on the machine the block runs on."
area: "Agents and MCP"
---

# An agent block won't start: "agent blocks run it through it"

## You see

`arugula agent claude` (or opening an agent block) fails with the reason,
then one of:

```
…: agent blocks run it through it.
`arugula setup claude` installs it (or: npm install -g …); then run this again.
```

```
….
Install Node (`mise use -g node@22`, or nodejs.org), then `arugula setup claude` (or: npm install -g …).
```

## Why

An agent block doesn't run the agent's terminal program: it talks to the
agent through its ACP adapter, an npm package, on the machine the block
runs on (`crates/cli/src/cmd/agent.rs`, `adapter_fix`). That machine
needs the adapter, and Node to run it. Having `claude` itself isn't
enough.

## Do

On the machine the block runs on: `arugula setup claude` (or
`arugula setup codex`), after installing Node if it says so. Then start
the block again.
