//! What needs a person: a pane's attention state and its reasons, the cards
//! that make a terminal wait, questions asked in a terminal, hooks and
//! follow-ups.

use super::{AskReply, Daemon, InboxReply, Replied, SAVE_DEBOUNCE, agent_in, plain_reason};
use crate::{acl::Principal, store::now_ms};
use arugula_proto::{
    Action, AskRef, AskWhat, Attention, BlockType, Driver, EventKind, PaneId, Policy, Reason, ReasonKind,
    api::HistoryKind,
    ask::{Ask, AskKind},
};
use arugula_vt::detect::AgentState;
use std::collections::HashMap;
use tokio::{sync::oneshot, time::Instant};
use tracing::{debug, info, warn};

/// A follow-up waiter.
pub(super) struct Waiter {
    pub(super) token: u64,
    pub(super) reply: oneshot::Sender<InboxReply>,
}

/// How many follow-ups wait for an agent that isn't listening yet.
const MAX_QUEUED: usize = 8;

/// The least time between two notifications a pane's status records
/// (#720) push.
const STATUS_PUSH_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// A question open in a terminal, or raised on a block (M35).
pub(super) struct TermAsk {
    pub(super) ask: Ask,
    pub(super) token: u64,
    pub(super) reply: oneshot::Sender<Replied>,
}

/// Claude Code's session and subagent, from a hook's input: `session/agent`.
fn session_key(hook: &serde_json::Value) -> String {
    format!("{}/{}", hook["session_id"].as_str().unwrap_or(""), hook["agent_id"].as_str().unwrap_or(""))
}

/// A hook input about the same tool call as a permission card: same
/// session and subagent, tool and input.
fn same_call(hook: &serde_json::Value, ask: &Ask) -> bool {
    ask.session.as_deref() == Some(session_key(hook).as_str())
        && ask.tool.as_deref() == hook["tool_name"].as_str()
        && ask.input.as_ref() == Some(&hook["tool_input"])
}

/// A reason as a notification carries it: its actions, and for a gate its
/// key, so the worker's Approve and Expire (#310) act on that gate and not
/// one that came after it. A decision point's question (#621) brings its
/// choices when there are one or two, a button each.
fn push_reason(r: &Reason) -> serde_json::Value {
    let mut v = serde_json::json!({ "kind": r.kind, "actions": r.actions, "bundle": r.bundle });
    if let Some(g) = &r.gate {
        v["gate"] = serde_json::json!({ "id": g.key(), "title": r.headline });
        if let arugula_proto::GateSource::Point { choices, .. } = &g.source
            && (1..=2).contains(&choices.len())
        {
            v["gate"]["choices"] =
                choices.iter().map(|c| serde_json::json!({ "value": c.value, "label": c.label })).collect();
        }
    }
    v
}

/// What a notification about a reason is titled.
fn push_title(state: Attention, reason: Option<&Reason>) -> &'static str {
    match reason.map(|r| r.kind) {
        Some(ReasonKind::Failed) => "Failed",
        Some(ReasonKind::Exited) => "Exited",
        Some(ReasonKind::Done) => "Done",
        Some(ReasonKind::Ask | ReasonKind::Input) => "Needs you",
        Some(ReasonKind::Paused) => "Paused",
        Some(ReasonKind::Errors) => "Errors",
        Some(ReasonKind::Conflict) => "Merge conflict",
        Some(ReasonKind::Diff) => "Wants to edit",
        Some(ReasonKind::Gate)
            if reason
                .and_then(|r| r.gate.as_ref())
                .is_some_and(|g| matches!(g.source, arugula_proto::GateSource::Point { .. })) =>
        {
            "A decision waits on you"
        }
        Some(ReasonKind::Gate) => "Waits at a gate",
        None if state == Attention::Done => "Done",
        None => "Needs you",
    }
}

/// What asks bundle by: the project a directory is in (its git root, from
/// M23's cached lookup), else the directory. A directory on a machine (not
/// this host) is taken as is.
pub fn project_key(cwd: Option<&str>, local: bool) -> String {
    let Some(cwd) = cwd else { return String::new() };
    match local.then(|| crate::classify::project(cwd)).flatten() {
        Some(p) => p.root,
        None => cwd.to_owned(),
    }
}

impl Daemon {
    pub(super) fn set_attention(&mut self, pane: PaneId, state: Attention, why: &str) {
        let reason = match state {
            Attention::NeedsInput => Some(plain_reason(ReasonKind::Input, why)),
            Attention::Done => Some(plain_reason(ReasonKind::Done, why)),
            _ => None,
        };
        self.set_attention_with(pane, state, why, reason);
    }

    /// Set a pane's attention, and why (M24). An open ask overrides the
    /// stored reason while it's open (see [`Self::live_reason`]).
    pub(super) fn set_attention_with(&mut self, pane: PaneId, state: Attention, why: &str, reason: Option<Reason>) {
        self.set_attention_as(pane, state, why, reason, true);
    }

    /// [`Self::set_attention_with`], pushing a notification only if `push`.
    fn set_attention_as(&mut self, pane: PaneId, state: Attention, why: &str, reason: Option<Reason>, push: bool) {
        let old = self.attention.get(&pane).copied().unwrap_or_default();
        if !(self.panes.contains_key(&pane) || self.blocks.contains_key(&pane)) {
            return;
        }
        if old == state {
            // Another reason for the same state (a second command done, or a
            // failure after a bell): keep the newer one if it says more.
            let newer = match (&reason, self.reasons.get(&pane)) {
                (Some(r), Some(o)) => r.kind != o.kind && r.kind != ReasonKind::Input,
                (Some(_), None) => true,
                _ => false,
            };
            if newer && let Some(r) = reason {
                self.reasons.insert(pane, r);
                self.emit(Some(pane), EventKind::Attention { state, reason: self.live_reason(pane) });
                self.broadcast();
            }
            return;
        }
        info!(pane, ?state, why, "attention");
        self.attention.insert(pane, state);
        match reason {
            Some(r) => self.reasons.insert(pane, r),
            None => self.reasons.remove(&pane),
        };
        let reason = self.live_reason(pane);
        self.emit(Some(pane), EventKind::Attention { state, reason: reason.clone() });
        if push && matches!(state, Attention::NeedsInput | Attention::Done) && !self.focused(pane) {
            let title = push_title(state, reason.as_ref());
            let body = reason.as_ref().map_or(why, |r| r.headline.as_str()).to_owned();
            // What lets a notification answer it from its buttons: an
            // approval (an agent block's, or a terminal's permission card,
            // M29), or a question with one or two answers.
            let mut extra = self.blocks.get(&pane).and_then(|b| b.push_extra()).or_else(|| {
                let a = &self.asks.get(&pane)?.ask;
                if a.kind == AskKind::Permission {
                    return Some(serde_json::json!({ "approve": { "id": a.id, "title": a.headline() } }));
                }
                Some(serde_json::json!({ "ask": a.push_choice()? }))
            });
            if let Some(r) = &reason {
                let x = extra.get_or_insert_with(|| serde_json::json!({}));
                x["reason"] = push_reason(r);
            }
            // An agent's invite (#234) is for the owner alone, and opens at
            // its card: no buttons to send it from.
            if self.is_invite(pane) {
                if let Some(x) = extra.as_mut().and_then(|x| x.as_object_mut()) {
                    x.remove("approve");
                    x.remove("ask");
                }
                self.config.notify.notify(pane, title, &body, extra.clone(), extra, &|who| who.is_owner());
                self.touch(pane);
                return;
            }
            // The owner, and whoever may edit the session and opted in
            // (M29): on this daemon's own push, and through control (M21),
            // where the payload is encrypted to each device, so it can carry
            // what to approve too.
            let session = self.session_of(pane);
            let acl = self.config.acl.clone();
            self.config.notify.notify(pane, title, &body, extra.clone(), extra, &|who| acl.notifies(who.id(), session));
        }
        self.touch(pane);
    }

    /// Why a pane wants you now (M24): an open question or approval while it
    /// needs input, else what was recorded when its state changed.
    pub(super) fn live_reason(&self, pane: PaneId) -> Option<Reason> {
        let state = self.attention.get(&pane).copied().unwrap_or_default();
        if !matches!(state, Attention::NeedsInput | Attention::Done) {
            return None;
        }
        if state == Attention::NeedsInput
            && let Some(r) = self.card_of.get(&pane).map(|c| c.0.clone()).or_else(|| self.ask_reason(pane))
        {
            return Some(r);
        }
        self.reasons.get(&pane).cloned()
    }

    /// A pane's card changed (#699): its reason and attention follow.
    fn card_changed(&mut self, pane: PaneId, card: Option<(Reason, arugula_proto::DiffInfo)>) {
        match card {
            Some((r, d)) => {
                self.card_of.insert(pane, (r.clone(), d));
                let state = self.attention.get(&pane).copied().unwrap_or_default();
                if state == Attention::NeedsInput {
                    // Already asking: the card shows now.
                    self.reasons.insert(pane, r);
                    self.emit(Some(pane), EventKind::Attention { state, reason: self.live_reason(pane) });
                } else {
                    let why = r.headline.clone();
                    self.set_attention_with(pane, Attention::NeedsInput, &why, Some(r));
                }
            }
            None => {
                self.card_of.remove(&pane);
                if self.reasons.get(&pane).is_some_and(|r| r.kind == ReasonKind::Diff) {
                    if self.asks.contains_key(&pane) {
                        self.reasons.remove(&pane);
                    } else {
                        self.set_attention(pane, Attention::Idle, "diff closed");
                    }
                }
            }
        }
        self.touch(pane);
    }

    /// The process `pid` runs in one of our terminals: which.
    fn pane_of_pid(&self, pid: u32) -> Option<PaneId> {
        let shells: HashMap<u32, PaneId> = self.panes.iter().filter_map(|(id, h)| Some((h.pid_now()?, *id))).collect();
        let mut p = pid;
        for _ in 0..64 {
            if let Some(id) = shells.get(&p) {
                return Some(*id);
            }
            p = crate::procinfo::ppid(p).filter(|p| *p > 1)?;
        }
        None
    }

    /// The cards' turn on each tick of the mux task.
    pub(super) fn tick_cards(&mut self) {
        if let Some(mut cards) = self.cards.take() {
            cards.tick(self);
            self.cards = Some(cards);
        }
    }

    /// An open question in a terminal (Claude Code's hook), or a block's
    /// open permission request or question.
    fn ask_reason(&self, pane: PaneId) -> Option<Reason> {
        let local = self.meta.get(&pane).and_then(|m| m.host).is_none();
        let (id, what, headline, agent, project, at_ms, tool, questions) = if let Some(a) = self.asks.get(&pane) {
            let what = if a.ask.kind == AskKind::Permission { AskWhat::Approve } else { AskWhat::Question };
            let (agent, project) = match self.blocks.get(&pane) {
                // Raised on a block (M35): who asks is the ask's, and the
                // project the block's (a studio box's app), not a cwd.
                Some(b) => {
                    let s = b.summary();
                    let project = match s.project {
                        Some(p) => p.root,
                        None => project_key(s.cwd.as_deref(), local),
                    };
                    (a.ask.agent.clone().unwrap_or_else(|| a.ask.source.clone()), project)
                }
                None => {
                    let agent =
                        a.ask.agent.clone().or_else(|| self.agent_name(pane)).unwrap_or_else(|| "claude".into());
                    let cwd = self
                        .panes
                        .get(&pane)
                        .and_then(|h| h.status().cwd.or_else(|| h.cwd().map(|c| c.display().to_string())));
                    (agent, project_key(cwd.as_deref(), local))
                }
            };
            let questions = a.ask.questions.clone().filter(|_| a.ask.kind == AskKind::Questions);
            (a.ask.id.clone(), what, a.ask.headline(), agent, project, a.ask.at_ms, a.ask.tool.clone(), questions)
        } else {
            let w = self.blocks.get(&pane)?.waiting()?;
            (w.id, w.what, w.headline, w.agent, project_key(w.cwd.as_deref(), local), w.at_ms, None, w.questions)
        };
        let actions = match what {
            AskWhat::Approve => vec![Action::Allow, Action::Deny, Action::Dismiss],
            AskWhat::Question => vec![Action::Answer, Action::Deny, Action::Dismiss],
        };
        Some(Reason {
            kind: ReasonKind::Ask,
            since_ms: at_ms,
            headline,
            command: None,
            exit: None,
            duration_ms: None,
            bundle: Some(format!("ask:{project}:{agent}")),
            ask: Some(AskRef { id, what, agent, tool, questions: questions.map(Box::new) }),
            gate: None,
            diff: None,
            actions,
        })
    }

    /// The agent running in a terminal, by its command line.
    fn agent_name(&self, pane: PaneId) -> Option<String> {
        let h = self.panes.get(&pane)?;
        let text = h.status().current.and_then(|c| c.text).or_else(|| h.command()).unwrap_or_default();
        agent_in(&text)
    }

    /// The agent running in a terminal now: what the OS says runs in the
    /// foreground first (it sees past `cd x && claude`), else the command
    /// line the shell reported.
    fn agent_running(&self, pane: PaneId) -> Option<String> {
        let h = self.panes.get(&pane)?;
        match h.command() {
            Some(argv) => agent_in(&argv),
            None => agent_in(&h.status().current.and_then(|c| c.text)?),
        }
    }

    /// What a pane's failures bundle by: the machine it runs on.
    pub(super) fn machine_key(&self, pane: PaneId) -> String {
        match self.machine_of(pane) {
            Some(m) => m.name.clone().unwrap_or_else(|| m.sprite.clone()),
            None => "here".into(),
        }
    }

    /// Read the screen of the agent the pane runs now, if it has rules
    /// (#145), and stop reading it once it's gone.
    pub(super) fn watch_agent(&mut self, pane: PaneId) {
        let name = self.agent_running(pane);
        self.watch_named(pane, name);
    }

    pub(super) fn watch_named(&mut self, pane: PaneId, name: Option<String>) {
        let found = name.and_then(|name| arugula_vt::detect::agent(&name));
        // On this machine, only the rule sets of the agents configured here
        // run (the inventory, #145); a machine's panes aren't this
        // machine's config.
        let local = self.machine_of(pane).is_none();
        let off = found.filter(|a| local && !self.inventory.runs(a.id));
        if let Some(a) = off {
            self.inventory.missing(a.id);
        }
        let agent = found.filter(|_| off.is_none());
        let id = agent.map(|a| a.id);
        if self.watching.get(&pane).copied() == id && self.unread.get(&pane).copied() == off.map(|a| a.id) {
            return;
        }
        let Some(h) = self.panes.get(&pane) else { return };
        h.watch_agent(agent, off);
        match off {
            Some(a) => self.unread.insert(pane, a.id),
            None => self.unread.remove(&pane),
        };
        if self.watching.get(&pane).copied() == id {
            return;
        }
        self.screen.remove(&pane);
        // The line that started it isn't a turn.
        self.turn_typed.remove(&pane);
        match id {
            Some(id) => self.watching.insert(pane, id),
            None => self.watching.remove(&pane),
        };
    }

    /// What an agent's screen says it's doing now (#145). Hooks and open
    /// questions say more, so they win; a bell or a notification still
    /// wants you until you answer it.
    ///
    /// A program that reports its status (#720) says more than its screen
    /// does: screen rules step aside for it.
    pub(super) fn agent_screen(&mut self, pane: PaneId, state: AgentState, headline: Option<String>) {
        let before = self.screen.insert(pane, state);
        if self.asks.contains_key(&pane) || self.has_status(pane) {
            return;
        }
        let now = self.attention.get(&pane).copied().unwrap_or_default();
        let name =
            self.watching.get(&pane).and_then(|id| arugula_vt::detect::agent(id)).map_or("The agent", |a| a.name);
        match state {
            AgentState::Working => {
                let answered = now == Attention::NeedsInput && before == Some(AgentState::Blocked);
                if matches!(now, Attention::Idle | Attention::Done) || answered {
                    self.set_attention(pane, Attention::Working, "agent working");
                }
            }
            AgentState::Blocked => {
                let why = headline.unwrap_or_else(|| format!("{name} is waiting for you"));
                self.set_attention(pane, Attention::NeedsInput, &why);
            }
            AgentState::Idle => match now {
                // A turn someone started ended: done, if nobody watched it
                // end.
                Attention::Working
                    if before == Some(AgentState::Working) && self.turn_typed.remove(&pane) && !self.focused(pane) =>
                {
                    let why = format!("{name} finished its turn");
                    self.set_attention(pane, Attention::Done, &why);
                }
                Attention::Working => {
                    self.turn_typed.remove(&pane);
                    self.set_attention(pane, Attention::Idle, "agent idle")
                }
                // Its prompt went away without an answer typed here (Esc in
                // another terminal attached to it, say).
                Attention::NeedsInput if before == Some(AgentState::Blocked) => {
                    self.set_attention(pane, Attention::Idle, "agent idle")
                }
                _ => {}
            },
        }
    }

    /// Whether a program in the pane has status records (#720).
    pub(super) fn has_status(&self, pane: PaneId) -> bool {
        self.status.get(&pane).is_some_and(|b| !b.is_empty())
    }

    /// A program reported its status (OSC 7501, #720).
    pub(super) fn status_report(&mut self, pane: PaneId, s: &crate::osc::Status) {
        if !self.panes.contains_key(&pane) {
            return;
        }
        debug!(pane, id = ?s.id, state = ?s.state, "program status");
        self.status.entry(pane).or_default().apply(s);
        self.status_apply(pane);
    }

    /// A full reset (RIS) clears every record.
    pub(super) fn status_reset(&mut self, pane: PaneId) {
        if let Some(b) = self.status.get_mut(&pane)
            && !b.is_empty()
        {
            b.reset();
            self.status_apply(pane);
        }
    }

    /// A prompt started: what was going on is over, what ended stays.
    /// Whether the pane had records.
    pub(super) fn status_settle(&mut self, pane: PaneId) -> bool {
        let Some(b) = self.status.get_mut(&pane).filter(|b| !b.is_empty()) else { return false };
        b.settle();
        true
    }

    /// Put the pane in the attention its records say (#720). Open questions
    /// and cards (hooks) carry the actual question and how to answer it, so
    /// they win. With no records left, what they had put it in goes back to
    /// idle. Pushes about record changes go at most once per
    /// [`STATUS_PUSH_EVERY`] a pane: a program can flip its state as often
    /// as it likes.
    pub(super) fn status_apply(&mut self, pane: PaneId) {
        if self.asks.contains_key(&pane) || self.card_of.contains_key(&pane) {
            return;
        }
        let now = self.attention.get(&pane).copied().unwrap_or_default();
        let Some((state, reason, why)) = self.status.get(&pane).and_then(|b| b.attention(now_ms())) else {
            if self.status_shown.remove(&pane).is_some_and(|s| s == now && s != Attention::Idle) {
                self.set_attention(pane, Attention::Idle, "status cleared");
            }
            return;
        };
        self.status_shown.insert(pane, state);
        if now == state {
            // The same state, maybe saying something else.
            let same = match (&reason, self.reasons.get(&pane)) {
                (Some(a), Some(b)) => a.kind == b.kind && a.headline == b.headline,
                (None, None) => true,
                _ => false,
            };
            if !same {
                match reason {
                    Some(r) => self.reasons.insert(pane, r),
                    None => self.reasons.remove(&pane),
                };
                self.emit(Some(pane), EventKind::Attention { state, reason: self.live_reason(pane) });
                self.touch(pane);
            }
            return;
        }
        let push = matches!(state, Attention::NeedsInput | Attention::Done)
            && self.status_pushed.get(&pane).is_none_or(|t| t.elapsed() >= STATUS_PUSH_EVERY);
        if push && !self.focused(pane) {
            self.status_pushed.insert(pane, Instant::now());
        }
        self.set_attention_as(pane, state, &why, reason, push);
    }

    pub(super) fn looks_like_agent(&self, pane: PaneId) -> bool {
        let Some(h) = self.panes.get(&pane) else { return false };
        let text = h.status().current.and_then(|c| c.text).or_else(|| h.command()).unwrap_or_default();
        agent_in(&text).is_some()
    }

    /// Whether a block is an agent's invites (#234), the owner's to answer.
    pub(super) fn is_invite(&self, pane: PaneId) -> bool {
        self.blocks.get(&pane).is_some_and(|b| b.kind() == BlockType::Invite)
    }

    /// Show a terminal's question on every client, and ask for you.
    pub(super) fn ask(&mut self, pane: PaneId, ask: Ask) -> Result<(u64, oneshot::Receiver<Replied>), String> {
        // A terminal, or a block that doesn't ask through its own methods
        // (a web page, a studio box: M35). An agent block's questions are
        // its own.
        let block = self.blocks.get(&pane).map(|b| b.kind());
        match block {
            None if !self.panes.contains_key(&pane) => return Err(format!("no pane %{pane}")),
            Some(BlockType::Agent | BlockType::Chat | BlockType::Remote) => {
                return Err(format!("%{pane} is an agent, chat or remote block: it asks through its own methods"));
            }
            _ => {}
        }
        let mut ask = ask;
        if ask.kind == AskKind::Permission && ask.tool_call_id.is_none() {
            // The tool call it's for: the `PreToolUse` just before it.
            if let Some(id) = self.pre.get(&pane).and_then(|pre| pre.iter().rev().find(|p| same_call(p, &ask))) {
                let id = id["tool_use_id"].as_str().map(str::to_owned);
                ask.id = id.clone().unwrap_or(ask.id);
                ask.tool_call_id = id;
            }
        }
        self.answered.remove(&pane);
        let (tx, rx) = oneshot::channel();
        let token = self.next_ask;
        self.next_ask += 1;
        let why = ask.headline();
        info!(pane, id = ask.id, "question asked");
        // The same question again (its asker reconnected) or a newer one:
        // either way the older registration is over.
        if let Some(old) = self.asks.insert(pane, TermAsk { ask, token, reply: tx }) {
            let _ = old.reply.send((AskReply::Withdrawn, None));
        }
        if self.attention.get(&pane) == Some(&Attention::NeedsInput) {
            // Already asking for you (Claude Code's own hook, say): this is
            // what it wants, and the card changed.
            self.touch(pane);
        } else {
            self.set_attention(pane, Attention::NeedsInput, &why);
        }
        Ok((token, rx))
    }

    pub(super) fn ask_reply(
        &mut self,
        pane: PaneId,
        id: Option<String>,
        answer: AskReply,
        by: Option<Driver>,
    ) -> Result<Ask, String> {
        let a = self
            .asks
            .get(&pane)
            .filter(|a| id.as_ref().is_none_or(|id| *id == a.ask.id))
            .ok_or_else(|| format!("no open question in %{pane} (it was answered, or withdrawn)"))?;
        // An agent's invite (#234) is the owner's to answer, by whatever
        // route: editors may answer other cards, an agent none of these.
        if self.is_invite(pane) && !by.as_ref().is_some_and(|b| b.who == "owner") {
            return Err(super::OWNER_ONLY.into());
        }
        let ask = a.ask.clone();
        let permission = ask.kind == AskKind::Permission;
        let answer = match answer {
            AskReply::Allow { .. } | AskReply::Deny { .. } if !permission => {
                return Err(format!("%{pane} asks a question: answer it"));
            }
            AskReply::Answer(_) if permission => return Err(format!("%{pane} asks for approval: allow or deny it")),
            // "Always": one of Claude Code's own suggestions, by index.
            AskReply::Allow { always: Some(i) } => {
                let pick = ask.suggestions.as_ref().and_then(|s| s.get(i.as_u64().unwrap_or(0) as usize)).cloned();
                if pick.is_none() {
                    return Err("Claude Code suggested no rule to keep: allow it once".into());
                }
                AskReply::Allow { always: pick }
            }
            a => a,
        };
        let a = self.asks.remove(&pane).expect("just found");
        info!(pane, id = ask.id, ?answer, "question answered");
        let terminal = answer == AskReply::Terminal;
        let how = match &answer {
            AskReply::Answer(_) => Some("answered"),
            AskReply::Decline => Some("skipped"),
            AskReply::Allow { always: None } => Some("allowed"),
            AskReply::Allow { always: Some(_) } => Some("allowed always"),
            AskReply::Deny { .. } => Some("denied"),
            AskReply::Terminal | AskReply::Withdrawn => None,
        };
        let by = by.unwrap_or_else(|| self.driver_of(&Principal::Owner));
        let _ = a.reply.send((answer, Some(by.clone())));
        if let Some(how) = how {
            self.record_answer(pane, &by, &ask.id, how, &ask.headline());
        }
        if terminal {
            // It asks again in the terminal: still wants you.
            self.touch(pane);
        } else {
            self.after_ask(pane);
        }
        Ok(ask)
    }

    /// A terminal's question went: back to work, and its card off every
    /// client.
    pub(super) fn after_ask(&mut self, pane: PaneId) {
        if self.attention.get(&pane) == Some(&Attention::NeedsInput) {
            // A block still waiting for something else (a studio box's
            // gate) keeps wanting you, for that.
            if self.blocks.contains_key(&pane)
                && self.reasons.get(&pane).is_some_and(|r| !matches!(r.kind, ReasonKind::Ask | ReasonKind::Input))
            {
                self.emit(
                    Some(pane),
                    EventKind::Attention { state: Attention::NeedsInput, reason: self.live_reason(pane) },
                );
                self.touch(pane);
                return;
            }
            // A terminal's program goes on; a block that was asked for
            // someone else (a studio box) is just a page again.
            let next = if self.blocks.contains_key(&pane) { Attention::Idle } else { Attention::Working };
            self.set_attention(pane, next, "answered");
        } else {
            self.touch(pane);
        }
    }

    /// Someone answered a card (M29): every client's card closes saying who,
    /// and the pane's history and the audit log say so too.
    pub(super) fn record_answer(&mut self, pane: PaneId, by: &Driver, id: &str, how: &str, headline: &str) {
        info!(pane, who = by.who, how, "answered");
        let at_ms = now_ms();
        self.answered.insert(
            pane,
            arugula_proto::ask::Answered {
                id: id.to_owned(),
                how: how.to_owned(),
                who: by.who.clone(),
                name: by.name.clone(),
                at_ms,
                headline: headline.to_owned(),
            },
        );
        if let Some(p) = self.panes.get(&pane) {
            p.note(format!("{how}: {headline}"), by.name.clone(), HistoryKind::Answer);
        }
        self.config.acl.record(serde_json::json!({
            "at": at_ms, "by": by.who, "name": by.name, "action": "answer", "pane": pane, "how": how,
            "headline": headline,
        }));
        self.touch(pane);
    }

    /// The terminal answered a permission card first (M29): Claude Code
    /// never tells its hook about a "Yes", so its next step closes the card.
    pub(super) fn terminal_answered(&mut self, pane: PaneId, ask: &Ask, how: &str) {
        self.answered.insert(
            pane,
            arugula_proto::ask::Answered {
                id: ask.id.clone(),
                how: how.to_owned(),
                who: "terminal".into(),
                name: "the terminal".into(),
                at_ms: now_ms(),
                headline: ask.headline(),
            },
        );
    }

    /// One of Claude Code's hook events in a terminal (M29).
    /// The conversation Claude Code in a pane holds, from any of its hooks
    /// (#146): every hook's input names it.
    fn note_session(&mut self, pane: PaneId, hook: &serde_json::Value) {
        let Some(id) = hook["session_id"].as_str() else { return };
        if !self.panes.contains_key(&pane) {
            return;
        }
        if !crate::resume::valid_id(id) {
            warn!(pane, "a hook gave a session id that isn't one; not keeping it");
            return;
        }
        let transcript = hook["transcript_path"].as_str().map(str::to_owned);
        let cwd = hook["cwd"].as_str().map(str::to_owned);
        self.set_session(pane, "claude", id, transcript, cwd);
    }

    pub(super) fn set_session(
        &mut self,
        pane: PaneId,
        agent: &str,
        id: &str,
        transcript: Option<String>,
        cwd: Option<String>,
    ) {
        let m = self.meta.entry(pane).or_default();
        let old = m.session.as_ref().filter(|s| s.agent == agent && s.id == id);
        let new = |a: Option<String>, b: Option<&String>| a.is_none() || a.as_ref() == b;
        // The same conversation, still known to be running: nothing new.
        if old.is_some_and(|s| {
            s.running && new(transcript.clone(), s.transcript.as_ref()) && new(cwd.clone(), s.cwd.as_ref())
        }) {
            return;
        }
        let transcript = transcript.or_else(|| old.and_then(|s| s.transcript.clone()));
        let cwd = cwd.or_else(|| old.and_then(|s| s.cwd.clone()));
        info!(pane, agent, session = id, "agent conversation");
        // Its hooks speak while it runs.
        m.session =
            Some(crate::store::AgentSession { agent: agent.into(), id: id.into(), transcript, cwd, running: true });
        // A pane running Claude Code resumes it, unless someone said
        // otherwise.
        if !m.policy_set && m.policy == Policy::Shell {
            m.policy = Policy::Resume;
        }
        self.mark(pane);
        self.save_due.get_or_insert_with(|| Instant::now() + SAVE_DEBOUNCE);
    }

    pub(super) fn hook(&mut self, pane: PaneId, hook: &serde_json::Value) {
        self.note_session(pane, hook);
        let event = hook["hook_event_name"].as_str().unwrap_or_default();
        let session = session_key(hook);
        let open = self
            .asks
            .get(&pane)
            .filter(|a| a.ask.kind == AskKind::Permission)
            .map(|a| a.ask.clone())
            .filter(|a| a.session.as_deref().is_some_and(|s| s.split('/').next() == session.split('/').next()));
        let close = match (event, &open) {
            ("PreToolUse", Some(a)) if a.session.as_deref() == Some(session.as_str()) => {
                if a.tool_call_id.is_none() && same_call(hook, a) {
                    // The card came first: this is its tool call.
                    if let Some(t) = self.asks.get_mut(&pane) {
                        t.ask.tool_call_id = hook["tool_use_id"].as_str().map(str::to_owned);
                    }
                    None
                } else if a.tool_call_id.as_deref() != hook["tool_use_id"].as_str() {
                    Some("closed")
                } else {
                    None
                }
            }
            ("PostToolUse" | "PostToolUseFailure", Some(a))
                if a.tool_call_id.is_some() && a.tool_call_id.as_deref() == hook["tool_use_id"].as_str()
                    || a.tool_call_id.is_none() && same_call(hook, a) =>
            {
                Some("allowed in the terminal")
            }
            ("Stop" | "UserPromptSubmit" | "SessionStart", Some(_)) => Some("closed"),
            _ => None,
        };
        // #888: a turn someone started, for `/api/events`.
        if event == "UserPromptSubmit" && self.panes.contains_key(&pane) {
            self.emit(Some(pane), EventKind::Prompted);
        }
        if event == "PreToolUse" {
            let pre = self.pre.entry(pane).or_default();
            pre.push_back(hook.clone());
            while pre.len() > 16 {
                pre.pop_front();
            }
        }
        if let (Some(how), Some(a)) = (close, open)
            && let Some(t) = self.asks.remove(&pane)
        {
            info!(pane, id = a.id, event, "permission card closed: the terminal answered");
            let _ = t.reply.send((AskReply::Withdrawn, None));
            self.terminal_answered(pane, &a, how);
            self.after_ask(pane);
        }
    }

    /// `arugula inbox` waits for a follow-up: one queued goes now.
    pub(super) fn wait_inbox(&mut self, pane: PaneId) -> Result<(u64, oneshot::Receiver<InboxReply>), String> {
        if !self.panes.contains_key(&pane) {
            return Err(format!("no terminal %{pane}"));
        }
        let (tx, rx) = oneshot::channel();
        let token = self.next_ask;
        self.next_ask += 1;
        if let Some((text, by)) = self.queued.get_mut(&pane).and_then(|q| q.pop_front()) {
            let _ = tx.send(InboxReply::FollowUp { text, by });
            self.emit(Some(pane), EventKind::Prompted);
            return Ok((token, rx));
        }
        if let Some(old) = self.inbox.insert(pane, Waiter { token, reply: tx }) {
            let _ = old.reply.send(InboxReply::Replaced);
        } else {
            self.touch(pane);
        }
        Ok((token, rx))
    }

    /// A follow-up for Claude Code in a terminal (M29), recorded as input
    /// from whoever sent it.
    pub(super) fn follow_up(&mut self, pane: PaneId, text: String, by: Driver) -> Result<bool, String> {
        let Some(p) = self.panes.get(&pane) else { return Err(format!("no terminal %{pane}")) };
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err("an empty follow-up".into());
        }
        info!(pane, who = by.who, "follow-up");
        p.note(format!("follow-up: {text}"), by.name.clone(), HistoryKind::Answer);
        self.config.acl.record(serde_json::json!({
            "at": now_ms(), "by": by.who, "name": by.name, "action": "follow_up", "pane": pane, "text": text,
        }));
        let now = match self.inbox.remove(&pane) {
            Some(w) => match w.reply.send(InboxReply::FollowUp { text: text.clone(), by: by.clone() }) {
                Ok(()) => {
                    self.emit(Some(pane), EventKind::Prompted);
                    true
                }
                Err(InboxReply::FollowUp { text, by }) => {
                    self.queued.entry(pane).or_default().push_back((text, by));
                    false
                }
                Err(_) => false,
            },
            None => {
                let q = self.queued.entry(pane).or_default();
                if q.len() >= MAX_QUEUED {
                    return Err("too many follow-ups waiting: is Claude Code's inbox hook set up?".into());
                }
                q.push_back((text, by));
                false
            }
        };
        self.touch(pane);
        Ok(now)
    }
}

#[cfg(feature = "labs")]
/// Whether a block's [`What::Update`](crate::pane::What::Update) changes
/// its pane's reason: only while the pane still needs input for a reason of
/// that kind. Dismissed (idle), or wanting you for something else, it's
/// left alone, and nothing is asked again.
pub(super) fn updates(attention: Option<Attention>, current: Option<&Reason>, new: &Reason) -> bool {
    attention == Some(Attention::NeedsInput) && current.is_some_and(|r| r.kind == new.kind)
}

/// What a card's owner may do (#699), each as the mux does it for its own.
impl super::CardsCx for Daemon {
    fn pane_of_pid(&self, pid: u32) -> Option<PaneId> {
        Daemon::pane_of_pid(self, pid)
    }

    fn pane_cwd(&self, pane: PaneId) -> Option<String> {
        self.panes.get(&pane).and_then(|h| h.status().cwd.or_else(|| h.cwd().map(|c| c.display().to_string())))
    }

    fn card(&mut self, pane: PaneId, card: Option<(Reason, arugula_proto::DiffInfo)>) {
        self.card_changed(pane, card);
    }

    fn answered(&mut self, pane: PaneId, answered: arugula_proto::ask::Answered) {
        self.answered.insert(pane, answered);
    }

    fn record_answer(&mut self, pane: PaneId, by: &Driver, id: &str, how: &str, headline: &str) {
        Daemon::record_answer(self, pane, by, id, how, headline);
    }

    fn touch(&mut self, pane: PaneId) {
        Daemon::touch(self, pane);
    }

    fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<super::Cmd> {
        self.tx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::push_reason;

    /// What the daemon's `gate::reason` makes of these gates (kept in step by
    /// hand: the daemon's tests build the same one): the first gate, with how
    /// many more. Its `since_ms` is now, which nothing here reads.
    fn reason(gates: &[arugula_proto::Gate]) -> Option<arugula_proto::Reason> {
        use arugula_proto::{Action, GateSource, Reason, ReasonKind};
        let g = gates.first()?;
        let actions = match g.source {
            GateSource::Chant { .. } => vec![Action::Allow, Action::Expire, Action::Dismiss],
            GateSource::Point { .. } => vec![Action::Answer, Action::Dismiss],
            _ => vec![Action::Allow, Action::Dismiss],
        };
        let more = if gates.len() > 1 { format!(" (+{} more)", gates.len() - 1) } else { String::new() };
        Some(Reason {
            kind: ReasonKind::Gate,
            since_ms: crate::store::now_ms(),
            headline: format!("{}{more}", g.headline()),
            command: g.command.clone(),
            exit: None,
            duration_ms: None,
            bundle: Some(g.bundle()),
            ask: None,
            gate: Some(Box::new(g.clone())),
            diff: None,
            actions,
        })
    }

    #[cfg(feature = "labs")]
    #[test]
    fn an_update_says_more_only_while_the_reason_still_wants_you() {
        let gate = |why: bool| arugula_proto::Gate {
            member: "delivery".into(),
            op: "ship".into(),
            gate: "approve-ship".into(),
            env: None,
            since: Some("2026-10-07T00:00:00Z".into()),
            expires: None,
            approvals: 0,
            needed: 1,
            command: None,
            source: arugula_proto::GateSource::Chant { root: "/w".into(), dir: "/w/delivery".into(), machine: None },
            why: why.then(Default::default),
        };
        use super::updates;
        use arugula_proto::{Attention, ReasonKind};
        let was = reason(&[gate(false)]).unwrap();
        let more = reason(&[gate(true)]).unwrap();
        assert!(updates(Some(Attention::NeedsInput), Some(&was), &more));
        // Dismissed: idle, and it stays so.
        assert!(!updates(Some(Attention::Idle), Some(&was), &more));
        assert!(!updates(None, None, &more));
        // Wanting you for something else.
        let other = arugula_proto::Reason { kind: ReasonKind::Failed, ..was.clone() };
        assert!(!updates(Some(Attention::NeedsInput), Some(&other), &more));
    }

    #[test]
    fn a_gates_push_names_it_for_approve_and_expire() {
        let gate = arugula_proto::Gate {
            member: "delivery".into(),
            op: "ship".into(),
            gate: "approve-ship".into(),
            env: None,
            since: None,
            expires: None,
            approvals: 0,
            needed: 1,
            command: Some("chant approve ship approve-ship".into()),
            source: arugula_proto::GateSource::Chant { root: "/w".into(), dir: "/w/delivery".into(), machine: None },
            why: None,
        };
        let v = push_reason(&reason(&[gate]).unwrap());
        assert_eq!(v["kind"], "gate");
        assert_eq!(v["actions"], serde_json::json!(["allow", "expire", "dismiss"]));
        assert_eq!(v["gate"]["id"], "delivery/ship/approve-ship");
        assert_eq!(v["gate"]["title"], "delivery: ship waits at gate approve-ship");
        assert!(v["gate"].get("choices").is_none());
    }

    #[test]
    fn a_decision_points_push_brings_two_choices_to_answer_with() {
        use arugula_proto::{GateSource, workspace::PointChoice};
        let choice = |v: &str, l: &str| PointChoice { value: v.into(), label: l.into(), means: None };
        let mut q = arugula_proto::Gate {
            member: String::new(),
            op: "ship-skip".into(),
            gate: "ship-skip-1724faf3af79".into(),
            env: None,
            since: None,
            expires: None,
            approvals: 0,
            needed: 1,
            command: None,
            source: GateSource::Point {
                root: "/w".into(),
                machine: None,
                id: "ship-skip-1724faf3af79".into(),
                point: "ship-skip".into(),
                question: "May this release skip the human gate".into(),
                choices: vec![choice("true", "yes"), choice("false", "no")],
                proposed: None,
            },
            why: None,
        };
        let r = reason(std::slice::from_ref(&q)).unwrap();
        assert_eq!(super::push_title(arugula_proto::Attention::NeedsInput, Some(&r)), "A decision waits on you");
        let v = push_reason(&r);
        assert_eq!(v["actions"], serde_json::json!(["answer", "dismiss"]));
        assert_eq!(v["gate"]["id"], "/ship-skip/ship-skip-1724faf3af79");
        assert_eq!(v["gate"]["title"], "May this release skip the human gate");
        assert_eq!(
            v["gate"]["choices"],
            serde_json::json!([{ "value": "true", "label": "yes" }, { "value": "false", "label": "no" }])
        );
        // Three are picked on the card.
        if let GateSource::Point { choices, .. } = &mut q.source {
            choices.push(choice("maybe", "maybe"));
        }
        assert!(push_reason(&reason(&[q]).unwrap())["gate"].get("choices").is_none());
    }
}
