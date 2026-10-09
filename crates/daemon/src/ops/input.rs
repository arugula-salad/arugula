//! The pane verbs that act on a pane: `send`, `keys`, `mouse`, `followup`
//! (each types into it, so each is `DRIVES`) and `attention` (#574). HTTP
//! only. `authz::check` made the pane's role and trust check before a
//! handler runs.

use arugula_proto::{
    PaneId,
    api::{AttentionRequest, Empty, FollowUpRequest, FollowedUp, KeysRequest, MouseRequest, SendRequest},
    op::ops::{PaneAttention, PaneFollowUp, PaneKeys, PaneMouse, PaneSend},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    api::{ApiError, bad, pane},
    keys,
    mux::{Api, Cmd},
};

impl Handle for PaneSend {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: SendRequest) -> Result<Empty, OpError> {
        pane(cx.app, id).await?.mark_input();
        let mut data = req.text.into_bytes();
        if req.enter {
            data.push(b'\r');
        }
        cx.app.mux.send(Cmd::Input { client: None, pane: id, data });
        Ok(Empty {})
    }
}

impl Handle for PaneKeys {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: KeysRequest) -> Result<Empty, OpError> {
        let p = pane(cx.app, id).await?;
        let modes = p.status().modes;
        let data: Vec<u8> = req.keys.iter().flat_map(|k| keys::key(k, modes)).collect();
        p.mark_input();
        cx.app.mux.send(Cmd::Input { client: None, pane: id, data });
        Ok(Empty {})
    }
}

impl Handle for PaneMouse {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: MouseRequest) -> Result<Empty, OpError> {
        let p = pane(cx.app, id).await?;
        let data = keys::mouse(req.x, req.y, req.button, req.action, p.status().modes)
            .ok_or_else(|| bad("the program in that pane isn't listening to the mouse"))?;
        p.mark_input();
        cx.app.mux.send(Cmd::Input { client: None, pane: id, data });
        Ok(Empty {})
    }
}

impl Handle for PaneAttention {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: AttentionRequest) -> Result<Empty, OpError> {
        match cx.app.mux.api(|r| Api::Attention(id, req.state, req.why, r)).await {
            Some(true) => Ok(Empty {}),
            _ => Err(OpError::NoPane(id)),
        }
    }
}

/// A follow-up for the agent in a pane (M29), from whoever may drive it:
/// an agent block's next prompt, or Claude Code's in a terminal (through
/// its inbox hook, never typed). Recorded as theirs. `delivered`: it went
/// straight in (else it waits for the agent).
impl Handle for PaneFollowUp {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: FollowUpRequest) -> Result<FollowedUp, OpError> {
        let app = cx.app;
        let who = cx.principal()?;
        let by = crate::api::who_is(app, who).await.ok_or_else(shutting_down)?;
        if let Some(b) = app.mux.api(|r| Api::Block(id, r)).await.flatten() {
            let name = (by.who != "owner").then_some(by.name.as_str());
            b.call_by("send", serde_json::json!({ "text": req.text }), name).await.map_err(bad)?;
            return Ok(FollowedUp { delivered: true });
        }
        match app.mux.api(|r| Api::FollowUp(id, req.text, by, r)).await {
            Some(Ok(now)) => Ok(FollowedUp { delivered: now }),
            Some(Err(e)) => Err(bad(e).into()),
            None => Err(shutting_down().into()),
        }
    }
}

fn shutting_down() -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())
}
