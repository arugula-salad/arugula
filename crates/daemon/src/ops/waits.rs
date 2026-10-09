//! The pane calls that wait on someone, for as long as it takes (#576):
//! `ask` and `permit` put a card beside the pane, `inbox` waits for a
//! follow-up, and `ask_withdraw` takes a card back. HTTP only. Each wait
//! runs in its route's own handler task, never spawned or buffered, so
//! that axum dropping it when the client goes away drops its guard, which
//! takes the card (or the waiter) back.

use arugula_proto::{
    PaneId,
    api::{AskAnswer, AskRequest, Empty, InboxAnswer, PermitAnswer, PermitRequest, WithdrawRequest},
    op::ops::{PaneAsk, PaneAskWithdraw, PaneInbox, PanePermit},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    api::{ApiError, bad, is_invite, not_on_invites},
    mux::{Api, AskReply, Cmd, InboxReply, MuxHandle},
    store::now_ms,
};

/// Withdraws a terminal's question if whoever asked it goes away first.
struct AskGuard {
    mux: MuxHandle,
    pane: PaneId,
    token: u64,
    armed: bool,
}

impl Drop for AskGuard {
    fn drop(&mut self) {
        if self.armed {
            self.mux.send(Cmd::Api(Api::AskWithdraw(self.pane, None, Some(self.token))));
        }
    }
}

/// `arugula ask`: show AskUserQuestion's questions beside a terminal and
/// wait for the answer. Answers `{action: accept, content, output, by}`
/// (the hook's output for Claude Code, and who answered), `{action:
/// decline, output, by}`, `{action: terminal}` (answer in the terminal) or
/// `{action: withdrawn}`. A browser or app block takes questions too
/// (M35), from whatever follows a page's agent: `source` and `agent` say
/// who asks.
impl Handle for PaneAsk {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: AskRequest) -> Result<AskAnswer, OpError> {
        use arugula_proto::ask::{self, Ask, AskKind};
        let app = cx.app;
        if is_invite(app, id).await {
            return Err(not_on_invites().into());
        }
        let questions = req.questions.as_array().filter(|q| !q.is_empty()).ok_or_else(|| bad("no questions"))?;
        let message = match questions.as_slice() {
            [q] => q["question"].as_str().unwrap_or_default().to_owned(),
            _ => "Please answer the following questions.".to_owned(),
        };
        let a = Ask {
            id: req.id.clone().unwrap_or_else(|| format!("q{}", now_ms())),
            kind: AskKind::Questions,
            message,
            questions: Some(req.questions.clone()),
            schema: None,
            url: None,
            accepted: false,
            tool_call_id: req.id,
            source: req.source.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "hook".into()),
            agent: req.agent.filter(|s| !s.trim().is_empty()),
            at_ms: now_ms(),
            tool: None,
            input: None,
            suggestions: None,
            session: None,
        };
        let (token, rx) = match app.mux.api(|r| Api::Ask(id, Box::new(a), r)).await {
            Some(Ok(r)) => r,
            Some(Err(e)) => return Err(ApiError(StatusCode::NOT_FOUND, e).into()),
            None => return Err(shutting_down().into()),
        };
        let mut guard = AskGuard { mux: app.mux.clone(), pane: id, token, armed: true };
        let reply = rx.await;
        guard.armed = false;
        Ok(match reply {
            Ok((AskReply::Answer(content), by)) => {
                let output = ask::hook_output(&req.questions, &content);
                AskAnswer::Accept { content, output, by }
            }
            Ok((AskReply::Decline, by)) => AskAnswer::Decline { output: ask::hook_declined(), by },
            Ok((AskReply::Terminal, _)) => AskAnswer::Terminal,
            // A question is never allowed or denied (the mux refuses that).
            Ok((AskReply::Withdrawn | AskReply::Allow { .. } | AskReply::Deny { .. }, _)) => AskAnswer::Withdrawn,
            // The daemon is going away; the asker asks the next one.
            Err(_) => return Err(shutting_down().into()),
        })
    }
}

/// `arugula hook` on Claude Code's `PermissionRequest` (M29): a card
/// with the tool, its input and Claude's suggestions, beside the terminal,
/// until someone answers it or the terminal does. Answers `{action: allow
/// | deny, output}` (the hook's output) or `{action: withdrawn}`.
impl Handle for PanePermit {
    async fn handle(cx: &Cx<'_>, id: PaneId, hook: PermitRequest) -> Result<PermitAnswer, OpError> {
        use arugula_proto::ask::{self, Ask, AskKind};
        let app = cx.app;
        if is_invite(app, id).await {
            return Err(not_on_invites().into());
        }
        let tool = hook.tool_name.ok_or_else(|| bad("no tool_name"))?;
        let input = hook.tool_input;
        let session =
            format!("{}/{}", hook.session_id.as_deref().unwrap_or(""), hook.agent_id.as_deref().unwrap_or(""));
        let a = Ask {
            id: format!("p{}", now_ms()),
            kind: AskKind::Permission,
            message: ask::permission_message(&tool, &input),
            questions: None,
            schema: None,
            url: None,
            accepted: false,
            tool_call_id: None,
            source: "hook".into(),
            agent: None,
            at_ms: now_ms(),
            tool: Some(tool),
            input: Some(input),
            suggestions: hook.permission_suggestions.filter(|s| s.is_array()),
            session: Some(session),
        };
        let (token, rx) = match app.mux.api(|r| Api::Ask(id, Box::new(a), r)).await {
            Some(Ok(r)) => r,
            Some(Err(e)) => return Err(ApiError(StatusCode::NOT_FOUND, e).into()),
            None => return Err(shutting_down().into()),
        };
        let mut guard = AskGuard { mux: app.mux.clone(), pane: id, token, armed: true };
        let reply = rx.await;
        guard.armed = false;
        Ok(match reply {
            Ok((AskReply::Allow { always }, _)) => PermitAnswer::Allow { output: ask::permit_allow(always.as_ref()) },
            Ok((AskReply::Deny { message }, _)) => PermitAnswer::Deny { output: ask::permit_deny(&message) },
            Ok(_) => PermitAnswer::Withdrawn,
            Err(_) => return Err(shutting_down().into()),
        })
    }
}

/// Drops a follow-up waiter if whoever waits goes away first.
struct InboxGuard {
    mux: MuxHandle,
    pane: PaneId,
    token: u64,
}

impl Drop for InboxGuard {
    fn drop(&mut self) {
        self.mux.send(Cmd::Api(Api::InboxGone(self.pane, self.token)));
    }
}

/// `arugula inbox` (Claude Code's background `Stop` and `SessionStart`
/// hook, M29): wait for a follow-up. `{action: follow_up, text, by}`, or
/// `{action: replaced}` when a newer waiter took over.
impl Handle for PaneInbox {
    async fn handle(cx: &Cx<'_>, id: PaneId, hook: serde_json::Value) -> Result<InboxAnswer, OpError> {
        let app = cx.app;
        let (token, rx) = match app.mux.api(|r| Api::Inbox(id, hook, r)).await {
            Some(Ok(r)) => r,
            Some(Err(e)) => return Err(ApiError(StatusCode::NOT_FOUND, e).into()),
            None => return Err(shutting_down().into()),
        };
        let _guard = InboxGuard { mux: app.mux.clone(), pane: id, token };
        Ok(match rx.await {
            Ok(InboxReply::FollowUp { text, by }) => InboxAnswer::FollowUp { text, by },
            Ok(InboxReply::Replaced) => InboxAnswer::Replaced,
            Err(_) => return Err(shutting_down().into()),
        })
    }
}

impl Handle for PaneAskWithdraw {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: WithdrawRequest) -> Result<Empty, OpError> {
        if is_invite(cx.app, id).await {
            return Err(not_on_invites().into());
        }
        cx.app.mux.send(Cmd::Api(Api::AskWithdraw(id, req.id, None)));
        Ok(Empty {})
    }
}

fn shutting_down() -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())
}
