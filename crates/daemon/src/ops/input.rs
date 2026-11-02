//! The pane verbs that act on a pane: `send`, `keys`, `mouse`, `followup`
//! (each types into it, so each is `DRIVES`) and `attention` (#574). HTTP
//! only. `authz::check` made the pane's role and trust check before a
//! handler runs.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

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

/// How long the Enter waits after the text (#573), so the program has read
/// the text before the Enter reaches it.
const ENTER_AFTER: Duration = Duration::from_millis(30);

/// Until when each pane has an Enter still to come. A later send waits
/// behind it, or its text would land before the earlier line's Enter.
static BUSY: LazyLock<Mutex<HashMap<PaneId, Instant>>> = LazyLock::new(Default::default);

/// Type `text` into a pane, and with `enter` press Enter as a write of its
/// own after a short pause (#573). Returns at once; the Enter follows, and
/// so does anything sent to the pane meanwhile.
pub(crate) fn send_text(
    app: &crate::server::App,
    id: PaneId,
    p: &arugula_mux::pane::PaneHandle,
    text: &str,
    enter: bool,
) {
    p.mark_input();
    let (data, enter) = keys::send_text(text, enter, p.status().modes);
    let now = Instant::now();
    let behind = {
        let mut busy = BUSY.lock().unwrap();
        busy.retain(|_, until| *until > now);
        // Past the Enter ahead by more than a timer tick, or the two could swap.
        let start = busy.get(&id).map(|until| *until + Duration::from_millis(10));
        if let Some(at) = enter.as_ref().map(|_| start.unwrap_or(now) + ENTER_AFTER) {
            busy.insert(id, at);
        }
        start
    };
    let mux = app.mux.clone();
    // Now, or after what's ahead of it.
    let later = match behind {
        None => {
            mux.send(Cmd::Input { client: None, pane: id, data });
            None
        }
        Some(start) => Some((start, data)),
    };
    if later.is_none() && enter.is_none() {
        return;
    }
    crate::task::spawn(async move {
        if let Some((start, data)) = later {
            tokio::time::sleep_until(start.into()).await;
            mux.send(Cmd::Input { client: None, pane: id, data });
        }
        if let Some(data) = enter {
            tokio::time::sleep(ENTER_AFTER).await;
            mux.send(Cmd::Input { client: None, pane: id, data });
        }
    });
}

impl Handle for PaneSend {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: SendRequest) -> Result<Empty, OpError> {
        let p = pane(cx.app, id).await?;
        send_text(cx.app, id, &p, &req.text, req.enter);
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
        let by = cx.driver().await?.ok_or_else(shutting_down)?;
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
