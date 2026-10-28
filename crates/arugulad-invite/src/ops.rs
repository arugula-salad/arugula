//! `invite` and `team-pins`: sharing a session with someone and telling them
//! (`arugula invite`, the People page), and the teams the owner's browser
//! pinned for naming people (#233). Both are the owner's (`serve` checks
//! that before the handler runs); an agent on the owner's CLI is refused
//! inside, as it was. The logic stays in `invite`.

use arugula_proto::{
    api::{Empty, InviteRequest, Invited, TeamPins, TeamPinsRequest},
    op::ops::{InviteSend, TeamPinsGet, TeamPinsSet},
};

use super::{Cx, Handle, OpError};
use crate::invite::{AGENT_ASKS, add_pins, run, team_pins};

impl Handle for InviteSend {
    async fn handle(cx: &Cx<'_>, _: (), req: InviteRequest) -> Result<Invited, OpError> {
        if cx.agent_or_guest() {
            return Err(OpError::Forbidden(AGENT_ASKS.into()));
        }
        run(cx.app, req, None).await.map_err(|(code, why)| OpError::Status(code, why))
    }
}

impl Handle for TeamPinsGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<TeamPins, OpError> {
        Ok(team_pins(cx.app))
    }
}

impl Handle for TeamPinsSet {
    async fn handle(cx: &Cx<'_>, _: (), req: TeamPinsRequest) -> Result<TeamPins, OpError> {
        add_pins(cx.app, cx.agent_or_guest(), req).await.map_err(|(code, why)| OpError::Status(code, why))
    }
}
