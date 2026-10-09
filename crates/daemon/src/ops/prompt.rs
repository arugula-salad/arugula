//! Prompting the agent in a pane and waiting through its turn (#147, #576):
//! `pane.prompt`, and [`prompt`], the wait itself, which MCP's
//! `prompt_agent` and an agents-block task also run.

use std::time::Duration;

use arugula_proto::{
    EventKind, PaneId,
    api::{PromptRequest, PromptResult},
    op::ops::PanePrompt,
};

use super::{Cx, Handle, OpError, Via};
use crate::{
    api::{bad, pane},
    mux::{Api, Cmd},
    pane::{CaptureFormat, CaptureScope},
    server::App,
};

/// `arugula send %N --wait`: prompt the agent there and wait for its
/// turn (#147). `StillRunning` when `timeout` runs out first. Whoever
/// prompts through MCP is recorded as the author of the turn.
impl Handle for PanePrompt {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: PromptRequest) -> Result<PromptResult, OpError> {
        let stall = secs(req.stall, STALL);
        let limit = secs(req.timeout, Duration::from_secs(100));
        let by = match &cx.via {
            Via::Http { .. } => None,
            Via::Mcp(call) => Some(call.by()),
        };
        match tokio::time::timeout(limit, prompt(cx.app, id, req.text, req.answering, stall, by)).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(bad(e).into()),
            Err(_) => Ok(PromptResult::StillRunning),
        }
    }
}

/// How long a prompted agent has to show any sign of work (#147).
pub(crate) const STALL: Duration = Duration::from_secs(5);

/// Seconds from a request, or a default.
pub(crate) fn secs(s: Option<f64>, default: Duration) -> Duration {
    s.filter(|s| s.is_finite() && *s >= 0.0).map(Duration::from_secs_f64).unwrap_or(default)
}

/// Prompt the agent in a pane (a terminal running one, or an agent block)
/// and wait for its turn: `Done` when it ends, `NeedsInput` when it asks
/// for someone, `Stalled` when nothing shows it working within `stall` (no
/// agent there, the prompt not submitted, the agent gone). The wait starts
/// before anything is typed, so a quick turn can't slip past it. An agent
/// already waiting on someone isn't typed at, unless `answering`: typing
/// into its approval dialog would answer it.
pub(crate) async fn prompt(
    app: &App,
    id: PaneId,
    text: String,
    answering: bool,
    stall: Duration,
    by: Option<String>,
) -> Result<PromptResult, String> {
    use arugula_proto::{Attention, BlockType, WorkKind};
    let info = pane_info(app, id).await.ok_or_else(|| format!("no pane %{id}"))?;
    let block = match info.kind {
        BlockType::Terminal => None,
        BlockType::Agent => Some(app.mux.api(|r| Api::Block(id, r)).await.flatten().ok_or(format!("no block %{id}"))?),
        k => return Err(format!("%{id} is a {k:?} block, not an agent")),
    };
    if block.is_none() && info.work != Some(WorkKind::Agent) {
        let why = match &info.command {
            Some(c) => format!("%{id} runs `{c}`, not an agent; nothing was typed"),
            None => format!("%{id} is at its shell, with no agent running; nothing was typed"),
        };
        return Ok(PromptResult::Stalled { why, screen: screen(app, id).await });
    }
    // What it waits on: a terminal's question card, or an agent block's
    // first question not yet opened (as `wait` finds it).
    let question = |info: &arugula_proto::PaneInfo| {
        let ask = info.ask.clone().or_else(|| {
            let s = block.as_ref()?.state();
            let a = s["asks"].as_array()?.iter().find(|a| a["accepted"] != true)?.clone();
            serde_json::from_value::<arugula_proto::ask::Ask>(a).ok()
        });
        (info.reason.as_ref().map(|r| r.headline.clone()), ask.map(Box::new))
    };
    if info.attention == Attention::NeedsInput && !answering {
        let (question, ask) = question(&info);
        return Ok(PromptResult::Blocked { question, ask });
    }
    // Listen first, then type.
    let mut events = app.mux.events();
    // Typing alone makes a terminal pane "working" (someone's busy in
    // it), so where its agent's screen can be read, that says when the
    // agent starts instead.
    let reads_screen = block.is_none() && screen_state(app, id).await.is_some();
    let started_now = async |attention: Attention| {
        if reads_screen {
            matches!(screen_state(app, id).await.flatten(), Some("working" | "blocked"))
        } else {
            attention == Attention::Working
        }
    };
    let mut started = started_now(info.attention).await;
    match &block {
        Some(b) => {
            b.call_by("send", serde_json::json!({ "text": text }), by.as_deref()).await?;
        }
        None => {
            let p = pane(app, id).await.map_err(|e| e.1)?;
            p.mark_input();
            let input = |data: Vec<u8>| match &by {
                Some(by) => Cmd::Api(Api::InputBy(id, data, by.clone())),
                None => Cmd::Input { client: None, pane: id, data },
            };
            app.mux.send(input(text.into_bytes()));
            // Enter on its own: in the same read as the text, an agent can
            // take it for part of a paste and not submit.
            tokio::time::sleep(Duration::from_millis(150)).await;
            app.mux.send(input(b"\r".to_vec()));
        }
    }
    let stall_at = tokio::time::Instant::now() + stall;
    let mut attention = info.attention;
    loop {
        let next = if started {
            Some(events.recv().await)
        } else {
            if tokio::time::Instant::now() >= stall_at {
                let why = format!("no sign of work within {}s of the prompt", stall.as_secs_f64());
                return Ok(PromptResult::Stalled { why, screen: screen(app, id).await });
            }
            // Look at its screen again now and then: it can start working
            // with its attention already "working" from the typing.
            tokio::time::timeout(Duration::from_millis(100), events.recv()).await.ok()
        };
        let state = match next {
            None => attention,
            Some(Ok(e)) if e.pane != Some(id) => continue,
            Some(Ok(e)) => match e.kind {
                EventKind::Attention { state, .. } => state,
                EventKind::Closed => return Err(format!("%{id} closed")),
                EventKind::Exit { .. } if !started => {
                    let why = "its program exited".to_owned();
                    return Ok(PromptResult::Stalled { why, screen: screen(app, id).await });
                }
                _ => continue,
            },
            // Missed some: where it is now.
            Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                pane_info(app, id).await.ok_or_else(|| format!("%{id} closed"))?.attention
            }
            Some(Err(_)) => return Err("the daemon is shutting down".into()),
        };
        attention = state;
        if state == Attention::NeedsInput {
            let info = pane_info(app, id).await.ok_or_else(|| format!("%{id} closed"))?;
            let (question, ask) = question(&info);
            return Ok(PromptResult::NeedsInput { question, ask });
        }
        if !started {
            started = started_now(state).await;
        } else if matches!(state, Attention::Idle | Attention::Done) {
            return Ok(PromptResult::Done);
        }
    }
}

/// What a terminal pane's agent screen was last read as (`working`,
/// `blocked`, `idle`; `Some(None)` before its first reading), or `None`
/// when no agent's screen is read there.
async fn screen_state(app: &App, id: PaneId) -> Option<Option<&'static str>> {
    let p = pane(app, id).await.ok()?;
    tokio::task::spawn_blocking(move || p.detection()).await.ok().flatten().filter(|d| !d.unread).map(|d| d.shown)
}

async fn pane_info(app: &App, id: PaneId) -> Option<arugula_proto::PaneInfo> {
    app.mux.api(Api::Panes).await.unwrap_or_default().into_iter().find(|p| p.info.id == id).map(|p| p.info)
}

/// The last lines of a pane's screen, for a caller to see why.
async fn screen(app: &App, id: PaneId) -> String {
    let Ok(p) = pane(app, id).await else {
        return match app.mux.api(|r| Api::Block(id, r)).await.flatten() {
            Some(b) => {
                b.text().lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
            }
            None => String::new(),
        };
    };
    let text = tokio::task::spawn_blocking(move || p.capture(CaptureFormat::Text, CaptureScope::Screen))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(15)..].join("\n")
}
