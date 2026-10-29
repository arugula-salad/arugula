# arugula-platform-agents

The platform's agents (#314's first feature crate), on the platform's base
(`arugula-platform-base`, [crates/platform](../platform/README.md)):

- `src/agent_mcp.rs` and `src/agent_tokens.rs`: the MCP endpoint for
  agents with no machine (M230, `POST /api/mcp`), and the bearer agent
  grant tokens that open it, which only it reads. Each tool calls the
  route a machine's agents call, with the token in the machine's place.
  `src/agent_mcp_redone.rs` is redone's tools on it (M235).
- `src/redone.rs`: `arugula-platform import redone` (M235), redone's
  tenants, tokens and apps as projects, agent grant tokens and
  environments. Its fixture backup is `fixtures/redone-backup`.
- `src/infisical.rs`: the Infisical projection of apps' secrets (M235),
  set by the platform binary's flags and kept in `Platform::slots`.
- `src/clients.rs`: registered clients such as studio (M236), and
  `src/links.rs`, the platform page's "Linked products" (M229).

`src/lib.rs` registers its routes and background work (`features::Feature`).
The files name the base's modules as they did inside it (`crate::db`,
`crate::grants`, through `use arugula_platform::*`). Its tests use the
base's `testing` harness: `scripts/server-cargo nextest run -p
arugula-platform-agents`.
