---
schema: 1
id: "aru-111"
title: "A hosted MCP endpoint serves agents with no machine"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "How does an agent with no machine of its own, like redone's, work on a project?"
options:
  - id: "a"
    label: "An MCP endpoint on the platform, behind the hub's gateway, authenticated by a bearer agent-grant token"
    how: "The token is a grant, scoped to projects and powers, that the owner sees and revokes on the page and in the CLI. Its tools are typed project operations: native issues and threads, runs (start, read, wait), deploy plans and approval requests, and project secrets, set by name. Tool names match the daemon's MCP where they overlap. No tool approves a deploy."
    tradeoff: "A bearer token is a secret held by whatever runs the agent, and anyone holding it acts with its powers until it's revoked."
  - id: "b"
    label: "Machines only"
    how: "An agent needs arugulad on a machine to reach the platform, as today."
    tradeoff: "redone's agents, and any agent running somewhere without arugulad, can't work on a project."
  - id: "c"
    label: "A token that acts as its owner"
    how: "The token carries the owner's identity and every power they have."
    tradeoff: "An agent with its owner's reach over every project, which grants exist to avoid."
  - id: "d"
    label: "Tokens that can approve a deploy"
    how: "A deploy power on the token lets the agent approve."
    tradeoff: "An agent could plan and approve its own change."
choice:
  option: "a"
  reason: "redone's agents have no machine and still file issues, start runs and plan deploys; a grant keeps them scoped and revocable, and approvals stay with devices (Jake, 2026-10-22, platform step 8, kept from the roadmap)."
rejected:
  - option: "b"
    why: "Agents without a machine are what redone's users are."
  - option: "c"
    why: "Grants, not impersonation."
  - option: "d"
    why: "Agents never approve a deploy."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #28"
    url: "https://app.arugula.io/issues/iss_8945932a56d01ff92612fe6de6bf2bd1"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/platform/src/grants.rs"
  - "path:crates/control/src/gateway.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
---

# A hosted MCP endpoint serves agents with no machine

Agents with no machine reach the platform through a hosted MCP endpoint
(Jake, 2026-10-22, platform step 8, #1337, kept from the roadmap). It amends
"MCP is served by the daemon, with the API's own checks", which still holds
for agents on a machine.

- **On the platform, behind the hub's gateway,** like every platform path.
- **A bearer agent-grant token.** It's a grant ("Grants, not
  impersonation"), scoped to projects and powers, never the owner's identity.
  The owner sees and revokes it on the page and in the CLI.
- **Typed project operations:** native issues and threads, as step 6 gives a
  machine's agents; runs (start, read, wait); deploy plans and approval
  requests; project secrets, set by name.
- **Never an approval.** No tool or token approves a deploy ("A deploy needs
  an owner device's approval, and agents never approve").
- **The daemon's tool names,** where they overlap, so an agent's prompts work
  on both.

redone's `rdn_` tokens become these grants when redone is absorbed.
Where: the endpoint (Arugula #28, M230, native), `crates/platform/src/grants.rs`, `crates/control/src/gateway.rs`, `crates/daemon/src/mcp/tools.rs` (the names it shares).
From: #1337 (platform step 8, tracked natively in Arugula #22).
