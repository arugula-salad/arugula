//! The platform's MCP endpoint for agents with no machine (M230, platform
//! step 8), and the agent grant tokens its callers show it: their paths, as
//! the platform registers them and the CLI calls them.

/// The MCP endpoint (Streamable HTTP: JSON-RPC over `POST`), behind an
/// agent grant token (`Authorization: Bearer aru_agent_…`).
pub const MCP: &str = "/api/mcp";

/// An account's agent grant tokens: `GET` lists them, `POST` mints one.
pub const AGENT_TOKENS: &str = "/api/agent-tokens";

/// `POST`: revoke a token, and its grants.
pub const AGENT_TOKEN_REVOKE: &str = "/api/agent-tokens/{id}/revoke";

/// `GET`: the calls a token's agent made, newest first.
pub const AGENT_TOKEN_CALLS: &str = "/api/agent-tokens/{id}/calls";

pub fn agent_token_revoke(id: &str) -> String {
    AGENT_TOKEN_REVOKE.replace("{id}", id)
}

pub fn agent_token_calls(id: &str) -> String {
    AGENT_TOKEN_CALLS.replace("{id}", id)
}
