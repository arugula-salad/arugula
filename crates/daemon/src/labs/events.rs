//! What a conversation's links do, in its timeline (#857).
//!
//! - **A watcher per open link**, on the home machine: the mux's event bus
//!   for a pane here, or `/api/events?pane=N&type=attention,exit&follow=1`
//!   followed over the standing channel to the pane's machine (its grant
//!   lets that through). Watchers start with the daemon ([`resume`]) and
//!   when a link is made ([`watch`]), and stop when it closes ([`stop`]); a
//!   follow that drops is taken up again.
//! - **[`Rules`]** turn a pane's attention into lines worth a person's time,
//!   as S36's run showed they must:
//!   - a turn ends only after the link worked (prompting it sets it idle
//!     first, which isn't a turn);
//!   - only an ask (`ReasonKind::Ask`) needs input: Claude Code's "waiting
//!     for your input", a minute after each turn, is dropped;
//!   - the same line twice in a row is one;
//!   - lines are coalesced per turn: an ask a standing rule answers within
//!     [`HOLD_MS`] is counted, not posted, and the turn's end says "worked
//!     40s, 3 approvals" with its last message.
//! - **Cards:** an ask still open after [`HOLD_MS`] is posted as a typed
//!   event (`needs_input`, with the ask's id), which the page shows with
//!   Allow and Deny, and the home pushes to every member (the mux,
//!   `notify_card`). [`act`] answers it through the link's own machine,
//!   naming the ask's id, so an answer can't land on a newer prompt; the
//!   home records who answered (an `answered` event, as them), and the
//!   pane's machine records them "via conversation …" (`api::ConvVia`).
//!   An edit waiting as a diff (#887) is a card in the place of the ask
//!   it replaced, accepted or rejected ([`Rules::on_diff`]).
//! - **PRs:** a linked forge block's checks finishing (its `done` or
//!   `failed` reason) is posted as it comes.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_proto::{
    Action, Attention, ConvAsk, ConvEvent, ConvEventKind, ConvId, ConvLink, Driver, EventKind, ReasonKind, ThreadMsg,
    ThreadTarget,
    api::{ActRequest, ActResponse, ConvActRequest},
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::links::{self, Refusal};
use crate::{
    acl::Principal,
    mux::{Api, ConvReq, ThreadPost},
    server::App,
};

/// How long an ask waits for a standing rule (or someone at the pane)
/// before it's a card here.
pub const HOLD_MS: u64 = 2_000;

/// A turn shorter than this, with no summary and nothing asked, isn't
/// posted: Claude Code starting in a terminal does it.
const BLINK_MS: u64 = 2_000;

/// How long a dropped follow waits before it's taken up again, at first
/// and at most (doubling while its machine stays away).
const RETRY: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// How long a link's machine has to answer a card.
const ACT_WAIT: Duration = Duration::from_secs(20);

/// What [`Rules`] keep of one link between its events.
#[derive(Debug, Default)]
pub struct Rules {
    link: String,
    /// When the turn started working (ms).
    since: Option<u64>,
    /// Asks answered this turn.
    approvals: u32,
    /// An ask held to see whether something answers it first, and when it
    /// becomes a card.
    held: Option<(ConvAsk, u64)>,
    /// A card posted and not answered yet: its ask's id, and whether it's
    /// an edit's diff (#887).
    carded: Option<(String, bool)>,
    /// #887: the permission ask an edit's diff stands for now (arugulad is
    /// Claude Code's IDE, so the same Write waits as both). It's never a
    /// card of its own.
    replaced: Option<String>,
    /// The last line posted.
    last: Option<ConvEvent>,
}

/// What a terminal's or a block's turn end says when it has nothing to
/// say: not a summary.
fn says_nothing(s: &str) -> bool {
    let s = s.trim();
    s.is_empty() || ["set by the API", "finished", "idle", "done"].iter().any(|n| s.eq_ignore_ascii_case(n))
}

impl Rules {
    pub fn new(link: &str) -> Self {
        Rules { link: link.to_owned(), ..Default::default() }
    }

    fn event(&self, kind: ConvEventKind) -> ConvEvent {
        ConvEvent {
            kind,
            link: self.link.clone(),
            ask: None,
            summary: None,
            worked_ms: None,
            approvals: 0,
            action: None,
            code: None,
            ok: None,
        }
    }

    /// Post `e` unless it's the line just posted.
    fn once(&mut self, e: ConvEvent) -> Option<ConvEvent> {
        if self.last.as_ref() == Some(&e) {
            return None;
        }
        self.last = Some(e.clone());
        Some(e)
    }

    /// One of the link's pane's events, at `now` (ms): the line to post,
    /// if it's worth one.
    pub fn on(&mut self, kind: &EventKind, now: u64) -> Option<ConvEvent> {
        match kind {
            EventKind::Attention { state, reason } => {
                // A forge block's checks finishing (or its PR merging or
                // closing): a PR's line, whatever came before.
                if let Some(r) = reason.as_ref().filter(|r| r.bundle.as_deref().is_some_and(|b| b.contains("forge:")))
                    && matches!(r.kind, ReasonKind::Done | ReasonKind::Failed)
                {
                    let mut e = self.event(ConvEventKind::Pr);
                    e.summary = Some(r.headline.clone());
                    e.ok = Some(r.kind == ReasonKind::Done);
                    return self.once(e);
                }
                match state {
                    Attention::Working => {
                        self.since.get_or_insert(now);
                        self.replaced = None;
                        // Whatever was asked was answered: by a rule, at the
                        // pane, or by a card here.
                        if self.held.take().is_some() {
                            self.approvals += 1;
                        }
                        if self.carded.take().is_some() {
                            self.approvals += 1;
                        }
                        None
                    }
                    Attention::NeedsInput => {
                        let r = reason.as_ref()?;
                        if r.kind == ReasonKind::Diff {
                            return self.on_diff(r, now);
                        }
                        // Only an ask (or a diff) has something to answer here.
                        let a = r.ask.as_ref().filter(|_| r.kind == ReasonKind::Ask)?;
                        if self.carded.as_ref().is_some_and(|(c, _)| *c == a.id)
                            || self.held.as_ref().is_some_and(|(h, _)| h.id == a.id)
                            || self.replaced.as_deref() == Some(a.id.as_str())
                        {
                            return None;
                        }
                        // Another ask: the one before it was answered.
                        if self.held.take().is_some() || self.carded.take().is_some() {
                            self.approvals += 1;
                        }
                        self.since.get_or_insert(now);
                        let ask = ConvAsk {
                            id: a.id.clone(),
                            what: a.what,
                            tool: a.tool.clone(),
                            input: r.headline.clone(),
                            questions: a.questions.as_deref().cloned(),
                            diff: None,
                        };
                        self.held = Some((ask, now + HOLD_MS));
                        None
                    }
                    Attention::Done | Attention::Idle => {
                        self.held = None;
                        self.carded = None;
                        self.replaced = None;
                        // Not a turn unless it worked.
                        let since = self.since.take()?;
                        let mut e = self.event(ConvEventKind::TurnEnded);
                        e.worked_ms = Some(now.saturating_sub(since));
                        e.approvals = std::mem::take(&mut self.approvals);
                        e.summary = reason
                            .as_ref()
                            .filter(|r| *state == Attention::Done && r.kind == ReasonKind::Done)
                            .map(|r| r.headline.trim().to_owned())
                            .filter(|s| !says_nothing(s));
                        // Claude Code starting in a terminal (#859's real
                        // run): a blink of work with nothing to say.
                        if e.summary.is_none() && e.approvals == 0 && e.worked_ms < Some(BLINK_MS) {
                            return None;
                        }
                        self.last = Some(e.clone());
                        Some(e)
                    }
                }
            }
            EventKind::Exit { code, .. } => {
                self.since = None;
                self.held = None;
                self.carded = None;
                self.replaced = None;
                self.approvals = 0;
                let mut e = self.event(ConvEventKind::Exited);
                e.code = *code;
                self.once(e)
            }
            _ => None,
        }
    }

    /// An edit waiting as a diff (#887). Claude Code asks the permission
    /// for it first (a hook's ask), then the pane shows the diff instead, so
    /// that ask can't be answered any more: the diff takes its place.
    /// - Held: the diff is held in its place (one edit, one approval).
    /// - Already a card: the diff is a card at once, which retires it (the
    ///   page says a card a link moved on from was answered elsewhere).
    /// - Another diff before it: that one was answered.
    fn on_diff(&mut self, r: &arugula_proto::Reason, now: u64) -> Option<ConvEvent> {
        // A machine on an older build doesn't say which edit: nothing to
        // card, but the ask it replaced isn't one any more either.
        let Some(d) = r.diff.as_deref() else {
            if let Some((h, _)) = self.held.take() {
                self.replaced = Some(h.id);
            }
            return None;
        };
        if self.carded.as_ref().is_some_and(|(c, _)| *c == d.id)
            || self.held.as_ref().is_some_and(|(h, _)| h.id == d.id)
        {
            return None;
        }
        self.since.get_or_insert(now);
        let ask = diff_ask(d);
        let held = self.held.take();
        let carded = self.carded.take();
        match (held, carded) {
            (Some((h, _)), _) if h.diff.is_none() => self.replaced = Some(h.id),
            (None, Some((c, false))) => {
                self.replaced = Some(c);
                self.carded = Some((ask.id.clone(), true));
                let mut e = self.event(ConvEventKind::NeedsInput);
                e.ask = Some(ask);
                return self.once(e);
            }
            (None, None) => {}
            // A diff before it, or an ask carded before a diff held: answered.
            _ => self.approvals += 1,
        }
        self.held = Some((ask, now + HOLD_MS));
        None
    }

    /// When a held ask becomes a card (ms), if one is held.
    pub fn due(&self) -> Option<u64> {
        self.held.as_ref().map(|(_, at)| *at)
    }

    /// The time is `now`: a held ask nothing answered is a card.
    pub fn tick(&mut self, now: u64) -> Option<ConvEvent> {
        if self.held.as_ref().is_none_or(|(_, at)| *at > now) {
            return None;
        }
        let (ask, _) = self.held.take()?;
        self.carded = Some((ask.id.clone(), ask.diff.is_some()));
        let mut e = self.event(ConvEventKind::NeedsInput);
        e.ask = Some(ask);
        self.once(e)
    }
}

/// An edit waiting as a diff, as a conversation's card has it (#887):
/// `approve`, so a page that doesn't know diffs offers Allow and Deny.
pub fn diff_ask(d: &arugula_proto::DiffInfo) -> ConvAsk {
    let verb = if d.new { "create" } else { "edit" };
    ConvAsk {
        id: d.id.clone(),
        what: arugula_proto::AskWhat::Approve,
        tool: Some(if d.new { "Write" } else { "Edit" }.into()),
        input: format!("{verb} {} (+{} −{})", d.file, d.added, d.removed),
        questions: None,
        diff: Some(Box::new(d.clone())),
    }
}

/// An event's line, for a reader that doesn't know events (MCP's
/// `read_thread`, search).
pub fn line(e: &ConvEvent, who: &str) -> String {
    let ask = e.ask.as_ref().map(|a| a.input.as_str()).unwrap_or_default();
    match e.kind {
        ConvEventKind::TurnEnded => {
            let mut head = "turn ended".to_owned();
            if let Some(ms) = e.worked_ms {
                head.push_str(&format!(" · worked {}", crate::mux::human_took(ms)));
            }
            if e.approvals > 0 {
                let s = if e.approvals == 1 { "" } else { "s" };
                head.push_str(&format!(", {} approval{s}", e.approvals));
            }
            match &e.summary {
                Some(s) => format!("{head}: {s}"),
                None => head,
            }
        }
        ConvEventKind::NeedsInput => format!("needs you: {ask}"),
        ConvEventKind::Answered => {
            let did = match e.action {
                Some(Action::Allow) => "allowed",
                Some(Action::Answer) => "answered",
                Some(Action::Accept) => "accepted",
                Some(Action::Reject) => "rejected",
                _ if e.ask.as_ref().is_some_and(|a| a.what == arugula_proto::AskWhat::Question) => "skipped",
                _ => "denied",
            };
            match &e.summary {
                Some(said) => format!("{who} {did} {}: {said}", e.link),
                None => format!("{who} {did} {}: {ask}", e.link),
            }
        }
        ConvEventKind::Exited => match e.code {
            Some(c) => format!("exited ({c})"),
            None => "exited".into(),
        },
        ConvEventKind::Pr => e.summary.clone().unwrap_or_else(|| "its PR changed".into()),
    }
}

// ---------------------------------------------------------------- watchers

/// The watchers running, by conversation and link.
static WATCHING: LazyLock<Mutex<HashMap<(ConvId, String), tokio::task::AbortHandle>>> = LazyLock::new(Default::default);

/// Who a link's lines are by: the link, named as the header names it.
fn poster(l: &ConvLink) -> Driver {
    let what = if l.title.is_empty() { format!("%{}", l.pane) } else { l.title.clone() };
    Driver { who: format!("link:{}", l.name), name: format!("{} · {what} on {}", l.name, l.machine_name) }
}

async fn post(app: &App, id: ConvId, l: &ConvLink, e: ConvEvent) -> bool {
    // #858: a link the lead waits on that's done wakes it.
    let ended = matches!(e.kind, ConvEventKind::TurnEnded | ConvEventKind::Exited);
    let posted = post_line(app, id, l, e).await;
    if posted && ended {
        super::lead::link_ended(app, id, &l.name).await;
    }
    posted
}

async fn post_line(app: &App, id: ConvId, l: &ConvLink, e: ConvEvent) -> bool {
    let post = ThreadPost {
        target: ThreadTarget::Conv(id),
        who: Principal::Owner,
        as_agent: Some(poster(l)),
        text: line(&e, ""),
        quote: None,
        event: Some(e),
    };
    match app.mux.api(|r| Api::ThreadPost(post, r)).await {
        Some(Ok(_)) => true,
        // The conversation went.
        Some(Err(e)) if e.0 == 404 => false,
        Some(Err(e)) => {
            warn!(conv = %id, link = l.name, error = e.1, "can't post a link's event");
            true
        }
        None => false,
    }
}

/// Watch every open link of every conversation kept here (at start-up).
pub fn resume(app: &Arc<App>) {
    let app = app.clone();
    tokio::spawn(async move {
        let convs = app.mux.api(Api::Convs).await.unwrap_or_default();
        for c in convs {
            for l in c.links.iter().filter(|l| l.closed.is_none()) {
                watch(&app, c.id, l);
            }
        }
    });
}

/// Stop watching a conversation's link, or all of them (`None`).
pub fn stop(id: ConvId, link: Option<&str>) {
    WATCHING.lock().unwrap().retain(|(c, n), h| {
        let gone = *c == id && link.is_none_or(|l| l.eq_ignore_ascii_case(n));
        if gone {
            h.abort();
        }
        !gone
    });
}

/// Close a link for good (its pane went, or its machine refused it) and
/// stop watching it.
async fn close(app: &App, id: ConvId, l: &ConvLink, why: String) {
    info!(conv = %id, link = l.name, why, "a link closed");
    let req = ConvReq::LinkClosed { id, name: l.name.clone(), why };
    let _ = app.mux.api(|r| Api::Conv(req, Principal::Owner, r)).await;
    super::lead::link_ended(app, id, &l.name).await;
}

/// Watch link `l` of conversation `id`, unless it's watched already.
pub fn watch(app: &Arc<App>, id: ConvId, l: &ConvLink) {
    let key = (id, l.name.clone());
    let mut w = WATCHING.lock().unwrap();
    if w.get(&key).is_some_and(|h| !h.is_finished()) {
        return;
    }
    let (app, l) = (app.clone(), l.clone());
    let task = tokio::spawn(async move {
        info!(conv = %id, link = l.name, "watching a link");
        let (tx, rx) = mpsc::unbounded_channel::<Got>();
        let source = if links::is_here(&app, &l.machine) {
            tokio::spawn(here(app.clone(), l.pane, tx))
        } else {
            tokio::spawn(there(app.clone(), id, l.clone(), tx))
        };
        follow(&app, id, &l, rx).await;
        source.abort();
        WATCHING.lock().unwrap().remove(&(id, l.name.clone()));
    });
    w.insert(key, task.abort_handle());
}

/// What a link's source says.
enum Got {
    Event(Box<EventKind>),
    /// It's closed for good, and why.
    Closed(String),
}

/// A pane here: the mux's events for it.
async fn here(app: Arc<App>, pane: arugula_proto::PaneId, tx: mpsc::UnboundedSender<Got>) {
    let mut events = app.mux.events();
    // One linked mid-turn (a lead's helper, #858, linked as it starts):
    // that turn's end is a turn's end too.
    let now = app.mux.api(Api::Panes).await.unwrap_or_default();
    if now.iter().any(|p| p.info.id == pane && p.info.attention == Attention::Working) {
        let working = EventKind::Attention { state: Attention::Working, reason: None };
        if tx.send(Got::Event(Box::new(working))).is_err() {
            return;
        }
    }
    loop {
        match events.recv().await {
            Ok(e) if e.pane == Some(pane) => {
                let got = match e.kind {
                    EventKind::Closed => Got::Closed("its pane closed".into()),
                    k @ (EventKind::Attention { .. } | EventKind::Exit { .. }) => Got::Event(Box::new(k)),
                    _ => continue,
                };
                if tx.send(got).is_err() {
                    return;
                }
            }
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(_) => return,
        }
    }
}

/// A pane on another machine: its events, followed over the standing
/// channel, taken up again whenever the follow drops.
async fn there(app: Arc<App>, id: ConvId, l: ConvLink, tx: mpsc::UnboundedSender<Got>) {
    let path = format!("/api/events?pane={}&type=attention,exit&follow=1", l.pane);
    let conv = id.to_string();
    let headers = [(links::CONV_HEADER, conv.as_str())];
    let machine = l.machine.clone().unwrap_or_default();
    let mut wait = RETRY;
    loop {
        let began = std::time::Instant::now();
        let r = async {
            let m = crate::peer::find_by_id(&app.control, &machine)
                .await?
                .ok_or_else(|| anyhow::anyhow!("{} isn't reachable from here", l.machine_name))?;
            let mut buf = Vec::new();
            crate::peer::follow(&app.control, &m, &path, &headers, |part| {
                buf.extend_from_slice(part);
                while let Some(n) = buf.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=n).collect();
                    if let Ok(e) = serde_json::from_slice::<arugula_proto::Event>(&line)
                        && tx.send(Got::Event(Box::new(e.kind))).is_err()
                    {
                        return false;
                    }
                }
                true
            })
            .await
            .map(|(status, body)| (status, body, m.name))
        }
        .await;
        if tx.is_closed() {
            return;
        }
        match r {
            // Refused on a channel it got in on: no grant for it there now.
            Ok((410, _, name)) => {
                let _ = tx.send(Got::Closed(format!("{name} revoked it, or its pane closed")));
                return;
            }
            Ok((403 | 404, body, name)) => {
                let _ = tx.send(Got::Closed(format!("{name} refused it: {}", links::said(&body))));
                return;
            }
            Ok((status, _, name)) => info!(machine = name, pane = l.pane, status, "a link's events ended; again soon"),
            Err(e) => info!(machine = l.machine_name, pane = l.pane, error = %e, "a link's events dropped; again soon"),
        }
        // A follow that lasted was fine: start the waits over.
        if began.elapsed() > RETRY_MAX {
            wait = RETRY;
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(RETRY_MAX);
    }
}

/// Post what a link's events come to, by [`Rules`], until it closes.
async fn follow(app: &App, id: ConvId, l: &ConvLink, mut rx: mpsc::UnboundedReceiver<Got>) {
    let mut rules = Rules::new(&l.name);
    loop {
        let now = crate::store::now_ms();
        let wait = rules.due().map(|at| Duration::from_millis(at.saturating_sub(now)));
        let got = match wait {
            Some(d) => match tokio::time::timeout(d, rx.recv()).await {
                Ok(g) => g,
                Err(_) => {
                    if let Some(e) = rules.tick(crate::store::now_ms())
                        && !post(app, id, l, e).await
                    {
                        return;
                    }
                    continue;
                }
            },
            None => rx.recv().await,
        };
        match got {
            Some(Got::Event(k)) => {
                if let Some(e) = rules.on(&k, crate::store::now_ms())
                    && !post(app, id, l, e).await
                {
                    return;
                }
            }
            Some(Got::Closed(why)) => {
                close(app, id, l, why).await;
                return;
            }
            None => return,
        }
    }
}

// ---------------------------------------------------------------- cards

/// The card `ask` names in a conversation's timeline: the newest
/// `needs_input` event of `link` with that ask.
fn card<'a>(msgs: &'a [ThreadMsg], link: &str, ask: &str) -> Option<&'a ConvAsk> {
    msgs.iter().rev().filter_map(|m| m.event.as_ref()).find_map(|e| {
        (e.kind == ConvEventKind::NeedsInput && e.link.eq_ignore_ascii_case(link))
            .then_some(e.ask.as_ref())
            .flatten()
            .filter(|a| a.id == ask)
    })
}

/// A question card's answer in a line (#859): "Which colour? → Red",
/// each question's, as Claude Code is given them.
fn answer_line(ask: &ConvAsk, content: Option<&serde_json::Value>) -> Option<String> {
    let (qs, content) = (ask.questions.as_ref()?, content?);
    let (answers, notes) = arugula_proto::ask::answers(qs, content);
    let parts: Vec<String> = answers
        .iter()
        .map(|(q, a)| {
            let note = notes.get(q).and_then(|n| n["notes"].as_str()).map(|n| format!(" ({n})")).unwrap_or_default();
            format!("{q} → {}{note}", a.as_str().unwrap_or_default())
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Record `who`'s answer to `link`'s card in the conversation.
async fn answered(
    app: &App,
    id: ConvId,
    link: &str,
    ask: ConvAsk,
    req: &ConvActRequest,
    who: &Principal,
    member: &str,
) {
    let summary = if req.action == Action::Answer { answer_line(&ask, req.content.as_ref()) } else { None };
    let e = ConvEvent {
        kind: ConvEventKind::Answered,
        link: link.to_owned(),
        ask: Some(ask),
        summary,
        worked_ms: None,
        approvals: 0,
        action: Some(req.action),
        code: None,
        ok: None,
    };
    let target = ThreadTarget::Conv(id);
    let post =
        ThreadPost { target, who: who.clone(), as_agent: None, text: line(&e, member), quote: None, event: Some(e) };
    if let Some(Err(e)) = app.mux.api(|r| Api::ThreadPost(post, r)).await {
        warn!(conv = %id, error = e.1, "answered a card, but can't record it");
    }
}

/// What answering `ask` with `action` does: an edit's diff (#887) is
/// accepted or rejected (Allow and Deny from a page that doesn't know diffs
/// do the same); anything else is allowed, denied or answered.
fn card_action(ask: &ConvAsk, action: Action) -> Result<Action, Refusal> {
    match (ask.diff.is_some(), action) {
        (true, Action::Accept | Action::Allow) => Ok(Action::Accept),
        (true, Action::Reject | Action::Deny) => Ok(Action::Reject),
        (true, _) => Err((400, "an edit is accepted or rejected".into())),
        (false, Action::Accept | Action::Reject) => Err((400, "only an edit is accepted or rejected".into())),
        (false, a) => Ok(a),
    }
}

/// The lead's own ask (#859), a tool outside its allow list or a question
/// of its own, as a card in its conversation: posted by the lead, so the
/// people answer it there.
pub async fn post_lead_card(app: &App, id: ConvId, e: ConvEvent) {
    let post = ThreadPost {
        target: ThreadTarget::Conv(id),
        who: Principal::Owner,
        as_agent: Some(Driver { who: super::lead::WHO.into(), name: super::lead::WHO.into() }),
        text: line(&e, ""),
        quote: None,
        event: Some(e),
    };
    if let Some(Err(e)) = app.mux.api(|r| Api::ThreadPost(post, r)).await {
        warn!(conv = %id, error = e.1, "can't post the lead's card");
    }
}

/// `POST /api/convs/{id}/act`: answer link `req.link`'s card, as `who`
/// (someone in the conversation), through the link's machine; or the
/// lead's own card (`lead`), here.
pub async fn act(app: &App, id: ConvId, req: ConvActRequest, who: &Principal) -> Result<(), Refusal> {
    let c = links::conv(app, id, who).await?;
    if c.floor(who.id()).is_none() {
        return Err((403, "only the people in a conversation answer its cards".into()));
    }
    if !matches!(req.action, Action::Allow | Action::Deny | Action::Answer | Action::Accept | Action::Reject) {
        return Err((400, "action: allow, deny, answer, accept or reject".into()));
    }
    if req.link.eq_ignore_ascii_case(super::lead::WHO) {
        return act_on_lead(app, &c, req, who).await;
    }
    let l = c
        .links
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(&req.link))
        .ok_or((404, format!("no link {}", req.link)))?;
    if let Some(why) = &l.closed {
        return Err((410, format!("{} is closed: {why}", l.name)));
    }
    let target = ThreadTarget::Conv(id);
    let msgs = app
        .mux
        .api(|r| Api::ThreadGet(target, Principal::Owner, r))
        .await
        .ok_or((503, "shutting down".to_owned()))?
        .map_err(|e| (e.0, e.1))?;
    let ask = card(&msgs, &l.name, &req.id).cloned().ok_or((404, format!("{} has no card {}", l.name, req.id)))?;
    let mut req = req;
    req.action = card_action(&ask, req.action)?;
    let member = crate::api::chat_name(app, who.clone()).await;
    let body = ActRequest {
        action: req.action,
        pane: Some(l.pane),
        panes: Vec::new(),
        id: Some(req.id.clone()),
        content: req.content.clone(),
        option: None,
        suggestion: None,
        message: req.message.clone(),
        text: None,
    };
    if links::is_here(app, &l.machine) {
        // Here, by the grant on the pane, as whoever made it.
        let here = links::me(app);
        let g = links::grants(app, l.pane)
            .await
            .into_iter()
            .find(|g| g.device == here && g.conv == id)
            .ok_or((410, format!("{}'s grant was revoked", l.name)))?;
        let maker = if g.by == "owner" {
            Principal::Owner
        } else {
            Principal::User { id: g.by.clone(), name: g.by_name.clone(), pic: None }
        };
        let via = crate::api::ConvVia { conv_title: c.title.clone(), member: member.clone() };
        crate::api::act_for_conv(app, l.pane, &body, maker, &via).await.map_err(|e| (409, e))?;
    } else {
        let mid = l.machine.clone().unwrap_or_default();
        let m = crate::peer::find_by_id(&app.control, &mid)
            .await
            .map_err(|e| (502, e.to_string()))?
            .ok_or((502, format!("{} isn't reachable from here", l.machine_name)))?;
        let conv = id.to_string();
        let headers = [(links::CONV_HEADER, conv.as_str()), (links::BY_HEADER, member.as_str())];
        let v = serde_json::to_value(&body).map_err(|e| (500, e.to_string()))?;
        let (status, got) =
            crate::peer::request_within(&app.control, &m, "POST", "/api/attention/act", Some(&v), &headers, ACT_WAIT)
                .await
                .map_err(|e| (502, format!("{} didn't answer: {e}", m.name)))?;
        let refused = match serde_json::from_slice::<ActResponse>(&got) {
            Ok(r) => r.results.into_iter().find(|r| !r.ok).map(|r| r.error.unwrap_or_default()),
            Err(_) if status >= 400 => Some(links::said(&got)),
            Err(_) => None,
        };
        if let Some(why) = refused {
            return Err((if status == 410 { 410 } else { 409 }, format!("{}: {why}", m.name)));
        }
    }
    // Recorded here as the member who answered.
    answered(app, id, &l.name, ask, &req, who, &member).await;
    Ok(())
}

/// The lead's own card (#859), answered by anyone in the conversation: the
/// lead acts with the owner's rights, and everyone in it may drive through
/// it already (`@lead`). Done here, on the lead's block, by the ask's id.
async fn act_on_lead(
    app: &App,
    c: &arugula_proto::Conversation,
    req: ConvActRequest,
    who: &Principal,
) -> Result<(), Refusal> {
    let lead = c.lead.as_ref().ok_or((404, "this conversation has no lead".to_owned()))?;
    let target = ThreadTarget::Conv(c.id);
    let msgs = app
        .mux
        .api(|r| Api::ThreadGet(target, Principal::Owner, r))
        .await
        .ok_or((503, "shutting down".to_owned()))?
        .map_err(|e| (e.0, e.1))?;
    let ask =
        card(&msgs, super::lead::WHO, &req.id).cloned().ok_or((404, format!("the lead has no card {}", req.id)))?;
    // Never an edit's diff: the lead's Claude Code has no IDE (#887).
    let mut req = req;
    req.action = card_action(&ask, req.action)?;
    let member = crate::api::chat_name(app, who.clone()).await;
    let body = ActRequest {
        action: req.action,
        pane: Some(lead.block),
        panes: Vec::new(),
        id: Some(req.id.clone()),
        content: req.content.clone(),
        option: None,
        suggestion: None,
        message: req.message.clone(),
        text: None,
    };
    let via = crate::api::ConvVia { conv_title: c.title.clone(), member: member.clone() };
    crate::api::act_for_lead(app, lead.block, &body, who, &via).await.map_err(|e| (409, e))?;
    answered(app, c.id, super::lead::WHO, ask, &req, who, &member).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use arugula_proto::{AskRef, AskWhat, Reason};

    use super::*;

    fn reason(kind: ReasonKind, headline: &str) -> Option<Reason> {
        Some(Reason {
            kind,
            since_ms: 0,
            headline: headline.into(),
            command: None,
            exit: None,
            duration_ms: None,
            bundle: None,
            ask: None,
            gate: None,
            diff: None,
            actions: vec![],
        })
    }

    fn att(state: Attention, reason: Option<Reason>) -> EventKind {
        EventKind::Attention { state, reason }
    }

    fn ask(id: &str) -> EventKind {
        let mut r = reason(ReasonKind::Ask, "Bash: pytest -q").unwrap();
        r.ask = Some(AskRef {
            id: id.into(),
            what: AskWhat::Approve,
            agent: "claude".into(),
            tool: Some("Bash".into()),
            questions: None,
        });
        att(Attention::NeedsInput, Some(r))
    }

    fn diff_info(id: &str) -> arugula_proto::DiffInfo {
        arugula_proto::DiffInfo {
            id: id.into(),
            file: "calc.py".into(),
            added: 3,
            removed: 1,
            text: "@@ -1 +1,3 @@\n-x\n+a\n+b\n+c".into(),
            new: false,
            at_ms: 0,
            ide: "arugula".into(),
        }
    }

    /// The pane shows an edit's diff (#887), as ide::cards says it.
    fn diff(id: &str) -> EventKind {
        let mut r = reason(ReasonKind::Diff, "Claude Code wants to edit calc.py (+3 −1)").unwrap();
        r.diff = Some(Box::new(diff_info(id)));
        att(Attention::NeedsInput, Some(r))
    }

    fn working() -> EventKind {
        att(Attention::Working, None)
    }

    fn done(summary: &str) -> EventKind {
        att(Attention::Done, reason(ReasonKind::Done, summary))
    }

    #[test]
    fn a_turn_ends_only_after_the_link_worked() {
        let mut r = Rules::new("L1");
        // Prompting a link sets it idle first: not a turn.
        assert_eq!(r.on(&att(Attention::Idle, None), 0), None);
        assert_eq!(r.on(&done("finished"), 10), None);
        assert_eq!(r.on(&working(), 1_000), None);
        let e = r.on(&done("Fixed mean() and the tests pass."), 41_000).unwrap();
        assert_eq!(e.kind, ConvEventKind::TurnEnded);
        assert_eq!(e.worked_ms, Some(40_000));
        assert_eq!(e.summary.as_deref(), Some("Fixed mean() and the tests pass."));
        assert_eq!(line(&e, ""), "turn ended · worked 40s: Fixed mean() and the tests pass.");
        // Seen and set idle: nothing more.
        assert_eq!(r.on(&att(Attention::Idle, None), 42_000), None);
    }

    #[test]
    fn only_an_ask_needs_input_and_the_minute_later_nudge_is_dropped() {
        let mut r = Rules::new("L2");
        r.on(&working(), 0);
        r.on(&done("ok"), 5_000).unwrap();
        // Claude Code's Notification hook, a minute later.
        let nudge = att(Attention::NeedsInput, reason(ReasonKind::Input, "Claude is waiting for your input"));
        assert_eq!(r.on(&nudge, 65_000), None);
        assert_eq!(r.due(), None);
        assert_eq!(r.tick(70_000), None);
        // ...and the turn after it is a turn of its own.
        r.on(&working(), 80_000);
        assert_eq!(r.on(&done("again"), 81_000).unwrap().worked_ms, Some(1_000));
    }

    #[test]
    fn an_ask_nothing_answers_is_a_card_once_and_counts_when_answered() {
        let mut r = Rules::new("L2");
        r.on(&working(), 0);
        assert_eq!(r.on(&ask("p1"), 1_000), None, "held first");
        assert_eq!(r.due(), Some(1_000 + HOLD_MS));
        assert_eq!(r.tick(1_500), None);
        let card = r.tick(1_000 + HOLD_MS).unwrap();
        assert_eq!(card.kind, ConvEventKind::NeedsInput);
        let a = card.ask.as_ref().unwrap();
        assert_eq!((a.id.as_str(), a.tool.as_deref(), a.input.as_str()), ("p1", Some("Bash"), "Bash: pytest -q"));
        assert_eq!(line(&card, ""), "needs you: Bash: pytest -q");
        // The same ask again (a second reason for it) is the same card.
        assert_eq!(r.on(&ask("p1"), 4_000), None);
        assert_eq!(r.tick(10_000), None);
        // Answered: it works on, and the turn counts it.
        r.on(&working(), 12_000);
        let e = r.on(&done("done it"), 20_000).unwrap();
        assert_eq!(e.approvals, 1);
        assert_eq!(line(&e, ""), "turn ended · worked 20s, 1 approval: done it");
    }

    #[test]
    fn asks_a_rule_answers_at_once_are_counted_not_posted() {
        let mut r = Rules::new("L1");
        r.on(&working(), 0);
        for (i, id) in ["p1", "p2", "p3"].iter().enumerate() {
            let t = 1_000 * (i as u64 + 1);
            assert_eq!(r.on(&ask(id), t), None);
            assert_eq!(r.on(&working(), t + 300), None);
        }
        assert_eq!(r.tick(60_000), None, "nothing held");
        let e = r.on(&done("three commands later"), 40_000).unwrap();
        assert_eq!((e.approvals, e.worked_ms), (3, Some(40_000)));
        assert_eq!(line(&e, ""), "turn ended · worked 40s, 3 approvals: three commands later");
        // One ask straight after another: the first was answered.
        r.on(&working(), 50_000);
        r.on(&ask("p4"), 50_100);
        r.on(&ask("p5"), 50_200);
        assert_eq!(r.tick(50_200 + HOLD_MS).unwrap().ask.unwrap().id, "p5");
    }

    #[test]
    fn a_blink_with_nothing_to_say_is_no_turn() {
        let mut r = Rules::new("L1");
        // Claude Code starting: working, then idle at once, saying nothing.
        r.on(&working(), 0);
        assert_eq!(r.on(&att(Attention::Idle, None), 400), None);
        // A short turn that says something is one.
        r.on(&working(), 1_000);
        assert_eq!(r.on(&done("ok"), 1_500).unwrap().summary.as_deref(), Some("ok"));
        // So is a long one with nothing to say.
        r.on(&working(), 2_000);
        assert_eq!(r.on(&att(Attention::Idle, None), 30_000).unwrap().worked_ms, Some(28_000));
    }

    #[test]
    fn the_same_line_twice_in_a_row_is_one() {
        let mut r = Rules::new("L1");
        let exit = EventKind::Exit { code: Some(1), machine_gone: false };
        assert_eq!(line(&r.on(&exit, 0).unwrap(), ""), "exited (1)");
        assert_eq!(r.on(&exit, 1), None);
        let mut pr = reason(ReasonKind::Failed, "o/r#7 checks failed: calc").unwrap();
        pr.bundle = Some("failed:forge:github.com/o/r".into());
        let failed = att(Attention::Done, Some(pr.clone()));
        let e = r.on(&failed, 2).unwrap();
        assert_eq!(
            (e.kind, e.ok, e.summary.as_deref()),
            (ConvEventKind::Pr, Some(false), Some("o/r#7 checks failed: calc"))
        );
        assert_eq!(r.on(&failed, 3), None);
        pr.kind = ReasonKind::Done;
        pr.headline = "o/r#7 checks green: calc".into();
        pr.bundle = Some("forge:github.com/o/r".into());
        assert_eq!(r.on(&att(Attention::Done, Some(pr)), 4).unwrap().ok, Some(true));
    }

    #[test]
    fn a_question_card_carries_its_choices_and_its_answer_says_what_was_picked() {
        let qs = serde_json::json!([
            { "question": "Which colour?", "header": "Colour", "multiSelect": false,
              "options": [{ "label": "Red" }, { "label": "Blue" }] },
            { "question": "Which fruits?", "header": "Fruit", "multiSelect": true,
              "options": [{ "label": "Apple" }, { "label": "Pear" }] },
        ]);
        let mut r = Rules::new("L2");
        r.on(&working(), 0);
        let mut q = reason(ReasonKind::Ask, "Which colour?").unwrap();
        q.ask = Some(AskRef {
            id: "q1".into(),
            what: AskWhat::Question,
            agent: "claude".into(),
            tool: None,
            questions: Some(Box::new(qs.clone())),
        });
        r.on(&att(Attention::NeedsInput, Some(q)), 1_000);
        let card = r.tick(1_000 + HOLD_MS).unwrap();
        let ask = card.ask.unwrap();
        assert_eq!((ask.what, ask.questions.as_ref()), (AskWhat::Question, Some(&qs)));
        let content = serde_json::json!({ "question_0": "Red", "question_0_custom": "dark",
            "question_1": ["Apple"], "question_1_custom": "kiwi" });
        let said = answer_line(&ask, Some(&content)).unwrap();
        assert_eq!(said, "Which colour? → Red (dark); Which fruits? → Apple, kiwi");
        let e = ConvEvent {
            kind: ConvEventKind::Answered,
            link: "L2".into(),
            ask: Some(ask.clone()),
            summary: Some(said),
            worked_ms: None,
            approvals: 0,
            action: Some(Action::Answer),
            code: None,
            ok: None,
        };
        assert_eq!(line(&e, "bob"), "bob answered L2: Which colour? → Red (dark); Which fruits? → Apple, kiwi");
        // Skipped: said so, with the question.
        let skipped = ConvEvent { action: Some(Action::Deny), summary: None, ..e };
        assert_eq!(line(&skipped, "bob"), "bob skipped L2: Which colour?");
        // Nothing picked: no line of its own.
        assert_eq!(answer_line(&ask, Some(&serde_json::json!({}))), None);
    }

    #[test]
    fn a_diff_replacing_a_held_ask_is_the_card_in_its_place() {
        // geek's run (#887): the Write's permission, then its diff, at once.
        let mut r = Rules::new("L1");
        r.on(&working(), 0);
        assert_eq!(r.on(&ask("toolu_1"), 1_000), None);
        assert_eq!(r.on(&diff("d30-7"), 1_010), None, "held too");
        assert_eq!(r.due(), Some(1_010 + HOLD_MS));
        let card = r.tick(1_010 + HOLD_MS).unwrap();
        assert_eq!(card.kind, ConvEventKind::NeedsInput);
        let a = card.ask.as_ref().unwrap();
        assert_eq!((a.id.as_str(), a.what, a.tool.as_deref()), ("d30-7", AskWhat::Approve, Some("Edit")));
        assert_eq!(a.diff.as_deref(), Some(&diff_info("d30-7")));
        assert_eq!(line(&card, ""), "needs you: edit calc.py (+3 −1)");
        // The ask it stands for, or the diff, seen again: the same card.
        assert_eq!(r.on(&ask("toolu_1"), 5_000), None);
        assert_eq!(r.on(&diff("d30-7"), 5_100), None);
        assert_eq!(r.tick(20_000), None);
        // Accepted: one edit, one approval.
        r.on(&working(), 30_000);
        assert_eq!(r.on(&done("fixed"), 40_000).unwrap().approvals, 1);
    }

    #[test]
    fn a_diff_with_no_ask_before_it_is_a_card() {
        // jake-mini's run (#887): the diff came first, so the pane never
        // showed the ask (a diff wins over it there).
        let mut r = Rules::new("L2");
        r.on(&working(), 0);
        assert_eq!(r.on(&diff("d1-2"), 1_000), None);
        assert_eq!(r.on(&diff("d1-2"), 1_001), None, "the hook's ask, shown as the diff");
        assert_eq!(r.tick(1_000 + HOLD_MS).unwrap().ask.unwrap().id, "d1-2");
    }

    #[test]
    fn a_carded_ask_a_diff_replaces_is_retired_by_the_diff_card_at_once() {
        let mut r = Rules::new("L1");
        r.on(&working(), 0);
        r.on(&ask("toolu_1"), 1_000);
        assert_eq!(r.tick(1_000 + HOLD_MS).unwrap().ask.unwrap().id, "toolu_1");
        // The diff comes later: its card now, no hold, so the page shows the
        // ask's card as answered elsewhere straight away.
        let card = r.on(&diff("d30-7"), 9_000).unwrap();
        assert_eq!(card.ask.unwrap().id, "d30-7");
        assert_eq!(r.due(), None);
        assert_eq!(r.on(&ask("toolu_1"), 9_500), None, "never carded again");
        r.on(&working(), 12_000);
        assert_eq!(r.on(&done("ok"), 20_000).unwrap().approvals, 1);
    }

    #[test]
    fn a_diff_the_terminal_answers_inside_the_hold_is_counted_not_posted() {
        let mut r = Rules::new("L1");
        r.on(&working(), 0);
        r.on(&ask("toolu_1"), 1_000);
        r.on(&diff("d30-7"), 1_010);
        r.on(&working(), 1_500);
        assert_eq!(r.tick(10_000), None);
        // A second edit straight after a first: the first was answered.
        r.on(&diff("d30-8"), 11_000);
        r.on(&diff("d30-9"), 12_000);
        assert_eq!(r.tick(12_000 + HOLD_MS).unwrap().ask.unwrap().id, "d30-9");
        r.on(&working(), 20_000);
        assert_eq!(r.on(&done("ok"), 30_000).unwrap().approvals, 3);
    }

    #[test]
    fn a_diff_from_an_older_machine_still_stops_its_ask_being_a_card() {
        let mut r = Rules::new("L1");
        r.on(&working(), 0);
        r.on(&ask("toolu_1"), 1_000);
        // No `diff` on the reason: nothing to card, and the ask can't be
        // answered any more.
        let old = att(Attention::NeedsInput, reason(ReasonKind::Diff, "Claude Code wants to edit calc.py (+3 −1)"));
        assert_eq!(r.on(&old, 1_010), None);
        assert_eq!(r.tick(10_000), None);
        assert_eq!(r.on(&ask("toolu_1"), 10_001), None);
        assert_eq!(r.tick(20_000), None);
    }

    #[test]
    fn a_diff_card_is_accepted_or_rejected_and_says_so() {
        let edit = diff_ask(&diff_info("d1-1"));
        assert_eq!(card_action(&edit, Action::Accept), Ok(Action::Accept));
        // A page that doesn't know diffs sends allow and deny.
        assert_eq!(card_action(&edit, Action::Allow), Ok(Action::Accept));
        assert_eq!(card_action(&edit, Action::Deny), Ok(Action::Reject));
        assert!(card_action(&edit, Action::Answer).is_err());
        let bash = ConvAsk { diff: None, ..edit.clone() };
        assert!(card_action(&bash, Action::Accept).is_err());
        assert_eq!(card_action(&bash, Action::Allow), Ok(Action::Allow));
        let e = ConvEvent {
            kind: ConvEventKind::Answered,
            link: "L1".into(),
            ask: Some(edit),
            summary: None,
            worked_ms: None,
            approvals: 0,
            action: Some(Action::Accept),
            code: None,
            ok: None,
        };
        assert_eq!(line(&e, "Jake"), "Jake accepted L1: edit calc.py (+3 −1)");
        let rejected = ConvEvent { action: Some(Action::Reject), ..e };
        assert_eq!(line(&rejected, "Jake"), "Jake rejected L1: edit calc.py (+3 −1)");
    }

    #[test]
    fn an_answered_card_names_who_and_what() {
        let e = ConvEvent {
            kind: ConvEventKind::Answered,
            link: "L2".into(),
            ask: Some(ConvAsk {
                id: "p1".into(),
                what: AskWhat::Approve,
                tool: None,
                input: "Edit: calc.py".into(),
                questions: None,
                diff: None,
            }),
            summary: None,
            worked_ms: None,
            approvals: 0,
            action: Some(Action::Allow),
            code: None,
            ok: None,
        };
        assert_eq!(line(&e, "Jake"), "Jake allowed L2: Edit: calc.py");
        let msgs = vec![ThreadMsg {
            id: 1,
            at: 1,
            who: "link:L2".into(),
            name: "L2".into(),
            pic: None,
            text: String::new(),
            quote: None,
            mentions: vec![],
            landed: vec![],
            to_agent: false,
            agent: true,
            event: Some(ConvEvent { kind: ConvEventKind::NeedsInput, ..e.clone() }),
        }];
        assert!(card(&msgs, "l2", "p1").is_some());
        assert!(card(&msgs, "L2", "p2").is_none(), "an answer names its ask");
        assert!(card(&msgs, "L1", "p1").is_none());
    }
}
