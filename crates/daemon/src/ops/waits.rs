//! The pane calls that wait on someone, for as long as it takes (#576):
//! `ask` and `permit` put a card beside the pane, `inbox` waits for a
//! follow-up, and `ask_withdraw` takes a card back. HTTP only. Each wait
//! runs in its route's own handler task, never spawned or buffered, so
//! that axum dropping it when the client goes away drops its guard, which
//! takes the card (or the waiter) back.

use arugula_proto::{
    PaneId,
    api::{AskAnswer, AskRequest, Empty, FollowUpAck, InboxAnswer, PermitAnswer, PermitRequest, WithdrawRequest},
    op::ops::{PaneAsk, PaneAskWithdraw, PaneFollowUpAck, PaneInbox, PanePermit},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    api::{ApiError, bad, is_doc, is_invite, not_on_docs, not_on_invites},
    mux::{Api, AskReply, Cmd, InboxReply, MuxHandle},
    server::App,
    store::now_ms,
};

/// Whether pane `pane` takes a Claude Code hook from the CLI process `pid`
/// (Arugula #469, [`HOOK_PID`](arugula_proto::api::HOOK_PID)): the CLI
/// found the pane in `ARUGULA_PANE`, which an editor or the desktop app
/// started from the pane inherits, so a terminal takes it only when `pid`
/// is in front of it: in its foreground job, or in the pane a `tmux` client
/// there shows (as live signals, #453). No pid (an older CLI), or a pane
/// that isn't a terminal (a block, or none: its own errors say so), takes
/// it as before.
pub(crate) async fn takes_hook(app: &App, pane: PaneId, pid: Option<u32>) -> bool {
    let Some(pid) = pid else { return true };
    let Some(h) = app.mux.api(|r| Api::Pane(pane, r)).await.flatten() else { return true };
    let sessions = crate::agent::sessions();
    let front = crate::task::spawn_blocking(move || {
        // A pane just started knows its program's pid only once the shim
        // has recorded it, a moment after the exec: what the program runs
        // at once can hook first (Arugula #1179). Until the pane has begun,
        // it hasn't ended; a pane that has answers at once.
        let leader = h.pid_now().or_else(|| {
            h.begun(STARTING);
            h.pid_now()
        });
        takes(Some(pid), leader, |leader, pid| crate::procinfo::in_front(leader, pid, |c| sessions.through(c)))
    });
    let takes = front.await.unwrap_or(false);
    if !takes {
        tracing::debug!(pane, pid, "a Claude Code hook from outside the pane's front: not the pane's");
    }
    takes
}

/// How long [`takes_hook`] waits for a starting pane to know its program:
/// as long as the pane waits for its shim to say.
const STARTING: std::time::Duration = std::time::Duration::from_secs(5);

/// [`takes_hook`] for a terminal whose own process is `leader` (none: it
/// has ended), with `in_front(leader, pid)` the process table's answer.
fn takes(pid: Option<u32>, leader: Option<u32>, in_front: impl FnOnce(u32, u32) -> bool) -> bool {
    match (pid, leader) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(pid), Some(leader)) => in_front(leader, pid),
    }
}

/// A hook input's [`HOOK_PID`](arugula_proto::api::HOOK_PID), if the CLI
/// added one.
pub(crate) fn hook_pid(hook: &serde_json::Value) -> Option<u32> {
    hook[arugula_proto::api::HOOK_PID].as_u64().and_then(|p| u32::try_from(p).ok())
}

/// What a hook from outside a terminal's front (Arugula #469) is told by
/// `inbox`: nothing waits for it there.
fn not_in_front(pane: PaneId) -> OpError {
    OpError::Status(
        StatusCode::CONFLICT,
        format!("this Claude Code isn't in front of %{pane}: nothing waits for it there"),
    )
}

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
        if is_doc(app, id).await {
            return Err(not_on_docs().into());
        }
        let questions = req.questions.as_array().filter(|q| !q.is_empty()).ok_or_else(|| bad("no questions"))?;
        // Asked from an app started from the pane: its own picker.
        if !takes_hook(app, id, req.pid).await {
            return Ok(AskAnswer::Terminal);
        }
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
        if is_doc(app, id).await {
            return Err(not_on_docs().into());
        }
        let tool = hook.tool_name.ok_or_else(|| bad("no tool_name"))?;
        // From an app started from the pane: Claude Code's own dialog.
        if !takes_hook(app, id, hook.pid).await {
            return Ok(PermitAnswer::Withdrawn);
        }
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
        if !takes_hook(app, id, hook_pid(&hook)).await {
            return Err(not_in_front(id));
        }
        let (token, rx) = match app.mux.api(|r| Api::Inbox(id, hook, r)).await {
            Some(Ok(r)) => r,
            Some(Err(e)) => return Err(ApiError(StatusCode::NOT_FOUND, e).into()),
            None => return Err(shutting_down().into()),
        };
        let _guard = InboxGuard { mux: app.mux.clone(), pane: id, token };
        Ok(match rx.await {
            Ok(InboxReply::FollowUp { text, by, id }) => InboxAnswer::FollowUp { text, by, id },
            Ok(InboxReply::Replaced) => InboxAnswer::Replaced,
            Err(_) => return Err(shutting_down().into()),
        })
    }
}

/// `arugula inbox` or `arugula hook` printed a follow-up it was given
/// (#630): the mux stops keeping it. One it doesn't have (acked already, or
/// offered again elsewhere) is no error.
impl Handle for PaneFollowUpAck {
    async fn handle(cx: &Cx<'_>, id: PaneId, ack: FollowUpAck) -> Result<Empty, OpError> {
        match cx.app.mux.api(|r| Api::FollowUpAck(id, ack.id, r)).await {
            Some(_) => Ok(Empty {}),
            None => Err(shutting_down().into()),
        }
    }
}

impl Handle for PaneAskWithdraw {
    async fn handle(cx: &Cx<'_>, id: PaneId, req: WithdrawRequest) -> Result<Empty, OpError> {
        if is_invite(cx.app, id).await {
            return Err(not_on_invites().into());
        }
        if is_doc(cx.app, id).await {
            return Err(not_on_docs().into());
        }
        // It put nothing there, so it takes nothing back (an id-less
        // withdraw would take the pane's own card).
        if !takes_hook(cx.app, id, req.pid).await {
            return Ok(Empty {});
        }
        cx.app.mux.send(Cmd::Api(Api::AskWithdraw(id, req.id, None)));
        Ok(Empty {})
    }
}

fn shutting_down() -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::takes;
    use crate::procinfo::in_front_of;

    /// The processes the tests read: pid → (parent, process group). Claude
    /// Code starts each command hook in a session of its own (`sh -c
    /// 'arugula …'`), so a hook's CLI is never in its Claude Code's group:
    /// only its ancestry ties it to one.
    ///
    /// Pane A's shell (100) runs `claude` at its prompt (200, the
    /// terminal's foreground job; its hook 210, CLI 211). An editor started
    /// from it as a job in the background (400) runs Claude Code (410, CLI
    /// 412), and the desktop app started from it left it (500, now init's;
    /// its CLI 512).
    ///
    /// Pane B's shell (700) has a `tmux` client (701) in front. The `tmux`
    /// server (800) has the window the client shows (pane 810, `claude` 811,
    /// CLI 813) and one behind it (pane 820, `claude` 821, CLI 823).
    fn table() -> HashMap<u32, (u32, u32)> {
        [
            (100, (1, 100)),
            (200, (100, 200)),
            (210, (200, 210)),
            (211, (210, 210)),
            (400, (100, 400)),
            (410, (400, 400)),
            (411, (410, 411)),
            (412, (411, 411)),
            (500, (1, 500)),
            (510, (500, 500)),
            (511, (510, 511)),
            (512, (511, 511)),
            (700, (1, 700)),
            (701, (700, 701)),
            (800, (1, 800)),
            (810, (800, 810)),
            (811, (810, 811)),
            (812, (811, 812)),
            (813, (812, 812)),
            (820, (800, 820)),
            (821, (820, 821)),
            (822, (821, 822)),
            (823, (822, 822)),
        ]
        .into()
    }

    /// Whether the pane whose own process is `leader` takes a hook from
    /// `pid`, read from [`table`].
    fn pane_takes(leader: Option<u32>, pid: Option<u32>) -> bool {
        let t = table();
        let ppid = |p: u32| t.get(&p).map(|e| e.0);
        let pgid = |p: u32| t.get(&p).map(|e| e.1);
        let fg = |leader: u32| match leader {
            100 => Some(200),
            700 => Some(701),
            810 => Some(811),
            820 => Some(821),
            _ => None,
        };
        let shows = |client: u32| (client == 701).then_some(810);
        takes(pid, leader, |leader, pid| in_front_of(leader, pid, fg, ppid, pgid, shows))
    }

    /// Arugula #469: `claude` at the pane's prompt is the pane's: its
    /// cards, waits, attention and session go there.
    #[test]
    fn a_hook_in_front_of_the_pane_is_the_panes() {
        assert!(pane_takes(Some(100), Some(211)));
    }

    /// An editor or the desktop app started from the pane inherits its
    /// `ARUGULA_PANE`, but isn't in front of it: none of its sessions' hooks
    /// are the pane's.
    #[test]
    fn a_hook_from_an_app_started_from_the_pane_isnt_the_panes() {
        assert!(!pane_takes(Some(100), Some(412)), "a job in the background");
        assert!(!pane_takes(Some(100), Some(512)), "an app that left the pane");
        // Nor is anything once the pane's own process has ended.
        assert!(!pane_takes(None, Some(211)));
    }

    /// Through a `tmux` client in front of the pane: the session in the
    /// window it shows is the pane's, one in a window behind it isn't.
    #[test]
    fn through_tmux_only_the_window_it_shows_is_the_panes() {
        assert!(pane_takes(Some(700), Some(813)));
        assert!(!pane_takes(Some(700), Some(823)));
    }

    /// An older CLI sends no pid: the pane takes its hook as it always did,
    /// without asking the process table.
    #[test]
    fn a_hook_without_a_pid_is_taken_as_before() {
        assert!(takes(None, Some(100), |_, _| unreachable!("no pid: nothing to look up")));
        assert!(takes(None, None, |_, _| unreachable!("no pid: nothing to look up")));
    }
}
