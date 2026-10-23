---
schema: 1
id: "aru-038"
title: "MCP is served by the daemon, with the API's own checks"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "MCP is served by the daemon, with the API's own checks"
options:
  - id: "a"
    label: "MCP is served by the daemon, with the API's own checks"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M16"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m16-an-mcp-server-illogical-as-tools-for-any-agent"
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/mcp/mod.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
  - "path:crates/daemon/src/mcp/tokens.rs"
---

# MCP is served by the daemon, with the API's own checks

`/mcp` (Streamable HTTP) is part of the owner's API: the owner over the
socket (`illogical mcp` bridges stdio), loopback or the tailnet, or per-client
bearer tokens that can be revoked. Tools cover everything the CLI does; the
MCP client's own tool permissions are the guard, so tool annotations matter.
Agent blocks get it injected, scoped to their tab. Every tool answers by name
whether or not it is listed.

**A hosted MCP endpoint** (step 8, #1337). Agents with no machine, like
redone's, reach the platform through an MCP endpoint of its own, behind the
hub's gateway, with a bearer agent-grant token scoped to projects and powers
that the owner can revoke ("A hosted MCP endpoint serves agents with no
machine"). Its tools are typed project operations and share the daemon's
names where they overlap. The daemon's `/mcp` stays as it is for agents on
a machine.
Where: `crates/daemon/src/mcp/mod.rs`, `crates/daemon/src/mcp/tools.rs`, `crates/daemon/src/mcp/tokens.rs`.
From: [M16](docs/plan-archive.md#m16-an-mcp-server-illogical-as-tools-for-any-agent), S14, #1337 (platform step 8).
