//! `fountain`: the person's Fountain agents, read with their own login on this
//! host (`arugula fountain agents`). Fountain is Labs', so a build without it
//! answers as a route that isn't there; MCP's `list` kind fountain_agents
//! reads the same agents but stays written out in `mcp/tools.rs`, for its
//! answer is built from the agents themselves, not from this one's rows.

use arugula_proto::{
    api::{FountainAgents, FountainQuery},
    op::ops::FountainAgentsGet,
};

use super::{Cx, Handle, OpError};

impl Handle for FountainAgentsGet {
    async fn handle(cx: &Cx<'_>, _: (), q: FountainQuery) -> Result<FountainAgents, OpError> {
        Ok(crate::labs::fountain_agents(cx.app, q).await?)
    }
}
