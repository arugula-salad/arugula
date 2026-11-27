//! Telling control which of this machine's panes wait on someone, so their
//! asks join their person's one Inbox and its count (Arugula #1431):
//! [`PaneAsksReport`], put whole at [`AGENT_PANE_ASKS`] each time,
//! replacing the last. Control decides who is asked (Arugula #1393): an ask
//! from a pane another agent block started goes to that block first.
//!
//! What is sent is kinds, times and pane numbers only (aru-116): each
//! pane's number, what it waits for ([`PaneAskKind`]), since when, whether
//! only a person may answer, and the agent block that started it. Never
//! the question, the command or anything a pane shows: the apps read those
//! from the machine, as before.
//!
//! - **Which panes:** an agent block that needs input, failed or finished
//!   its turn; any other pane with an open card (a hooked agent's question
//!   in a terminal, an invite, a Doc's share); and a pane at a gate.
//! - **Only a person:** a gate, and a block whose card only the owner
//!   answers ([`BlockType::owner_answers`]). An agent block may answer a
//!   permission of a block it started, so a permission isn't.
//! - **Its parent:** the agent block whose token started it
//!   ([`StartedBy::block`](arugula_proto::StartedBy)), while that block is
//!   among this machine's panes.
//! - **When:** on [`board_state`](crate::board_state)'s tick, when a pane's
//!   attention changed or one closed, or every [`LOOK`], and only when the
//!   list differs from the one control was last told; and whole, changed or
//!   not, whenever every block's state is sent again. So at most one request
//!   a tick, and control hears from a machine that has asks at least every
//!   [`BLOCK_STATE_EVERY_MS`](arugula_control_wire::boards::BLOCK_STATE_EVERY_MS).
//! - **An older control:** one without the route answers 404 (or 405): then
//!   nothing more is sent until the daemon restarts. Any other refusal waits
//!   as a block state's does.

use std::time::Instant;

use arugula_control_wire::boards::{AGENT_PANE_ASKS, PaneAsk, PaneAskKind, PaneAsksReport};
use arugula_proto::{AskWhat, Attention, BlockType, ReasonKind, WAITS_BUNDLE, api::PaneSummary};
use tracing::{debug, warn};

use crate::{
    board_state::{TROUBLE, Wait, wait_of},
    control::CloudRefused,
    machine_sessions::{LOOK, Pane},
};

/// What pane `p` waits for now, if anything.
fn ask_of(p: &PaneSummary, panes: &[PaneSummary]) -> Option<PaneAsk> {
    let i = &p.info;
    let r = i.reason.as_ref()?;
    // It waits on a card or a to-do of its own (Arugula #1464): that is
    // its person's Inbox item already.
    if r.bundle.as_deref().is_some_and(|b| b.starts_with(WAITS_BUNDLE)) {
        return None;
    }
    let agent = i.kind == BlockType::Agent;
    // A hooked terminal's `Notification` (Arugula #1514): the one `Input`
    // reason that says what it asks. Another `Input` (a screen's, a held
    // follow-up's) has no ask and isn't a pane ask.
    let hooked = r.kind == ReasonKind::Input && r.ask.is_some();
    if !(agent || i.ask.is_some() || hooked || r.kind == ReasonKind::Gate) {
        return None;
    }
    let needs = i.attention == Attention::NeedsInput;
    let kind = match r.kind {
        ReasonKind::Gate => PaneAskKind::Gate,
        ReasonKind::Diff if needs => PaneAskKind::Permission,
        ReasonKind::Ask | ReasonKind::Input if needs && r.ask.as_ref().is_some_and(|a| a.what == AskWhat::Approve) => {
            PaneAskKind::Permission
        }
        ReasonKind::Ask | ReasonKind::Input if needs => PaneAskKind::Question,
        ReasonKind::Failed | ReasonKind::Exited if agent => PaneAskKind::Failure,
        ReasonKind::Done if agent => PaneAskKind::Finished,
        _ => return None,
    };
    let parent = i
        .started_by
        .as_ref()
        .and_then(|s| s.block)
        .filter(|b| *b != i.id && panes.iter().any(|q| q.info.id == *b && q.info.kind == BlockType::Agent));
    Some(PaneAsk {
        pane: i.id,
        kind,
        since: r.since_ms,
        for_person: kind == PaneAskKind::Gate || i.kind.owner_answers(),
        parent,
        project: Pane::of(p).place.project,
    })
}

/// Every pane that waits on someone now, by pane.
pub(crate) fn report(panes: &[PaneSummary]) -> PaneAsksReport {
    let mut asks: Vec<PaneAsk> = panes.iter().filter_map(|p| ask_of(p, panes)).collect();
    asks.sort_by_key(|a| a.pane);
    PaneAsksReport { asks }
}

/// When the list is sent, and what control was last told of it.
#[derive(Default)]
pub(crate) struct Asks {
    /// What control last took (or refused for itself), so the same list
    /// isn't sent again.
    last: Option<PaneAsksReport>,
    /// A pane's attention changed since the list was last looked at.
    dirty: bool,
    /// Sent whole, changed or not, at the next look.
    force: bool,
    looked: Option<Instant>,
    quiet_until: Option<Instant>,
    /// Control has no route for it: nothing more is sent until restart.
    off: bool,
    /// Control refused a list with projects and took the same one without
    /// (it is from before Arugula #1514): they go without until restart.
    bare: bool,
}

impl Asks {
    /// A pane's attention changed, or one closed: the list may differ.
    pub fn changed(&mut self) {
        self.dirty = true;
    }

    /// Every block is sent again: so is the list, whole.
    pub fn again(&mut self) {
        self.force = true;
    }

    /// Out of control: joining again sends the list whole.
    pub fn left(&mut self) {
        self.last = None;
        self.force = true;
    }

    /// Whether to build the list now.
    pub fn look(&self, now: Instant) -> bool {
        !self.off
            && self.quiet_until.is_none_or(|t| now >= t)
            && (self.dirty || self.force || self.looked.is_none_or(|t| now.duration_since(t) >= LOOK))
    }

    /// The list as it is now: what to send of it, if anything.
    pub fn worth_sending(&mut self, now: PaneAsksReport, at: Instant) -> Option<PaneAsksReport> {
        self.looked = Some(at);
        self.dirty = false;
        (self.force || self.last.as_ref() != Some(&now)).then_some(now)
    }

    /// What control answered to `sent`.
    pub fn answered(&mut self, sent: PaneAsksReport, answer: Result<(), CloudRefused>, at: Instant) {
        match answer {
            Ok(()) => (self.last, self.force, self.quiet_until) = (Some(sent), false, None),
            Err(CloudRefused::Said { status: 404 | 405, .. } | CloudRefused::OldControl) => {
                self.off = true;
                warn!("control doesn't take a machine's asks yet: not telling it again until this daemon restarts");
            }
            Err(e) => self.timed_out_or(wait_of(e), sent, at),
        }
    }

    /// The request took too long, or control refused: wait, or drop a list
    /// control refused for itself (the same one would be refused again).
    pub fn timed_out_or(&mut self, wait: Wait, sent: PaneAsksReport, at: Instant) {
        match wait {
            Wait::Dropped(why) => {
                warn!("control refused this machine's asks: {why}");
                (self.last, self.force) = (Some(sent), false);
            }
            Wait::Failed(why) => {
                warn!("couldn't tell control this machine's asks: {why}");
                self.quiet_until = Some(at + TROUBLE);
            }
            Wait::For(d, why) => {
                warn!("not telling control this machine's asks for a while: {why}");
                self.quiet_until = Some(at + d);
            }
        }
    }
}

/// Send the list of this machine's asks, if it's time and it changed.
pub(crate) async fn tell(app: &crate::server::App, asks: &mut Asks) {
    let now = Instant::now();
    if !asks.look(now) {
        return;
    }
    let Some(panes) = app.mux.api(crate::mux::Api::Panes).await else { return };
    let Some(report) = asks.worth_sending(report(&panes), now) else { return };
    let mut sent = put(app, &report, asks.bare).await;
    // A control from before projects refuses the field it doesn't know
    // (a 400, 413 or 422, as for a block state's): if the same list without
    // them goes through, it's them.
    if !asks.bare
        && report.asks.iter().any(|a| a.project.is_some())
        && matches!(sent, Some(Err(ref e)) if matches!(wait_of(e.clone()), Wait::Dropped(_)))
    {
        let again = put(app, &report, true).await;
        if matches!(again, Some(Ok(()))) {
            asks.bare = true;
            debug!("control takes a machine's asks without their projects");
        }
        sent = again;
    }
    match sent {
        Some(answer) => {
            if answer.is_ok() {
                debug!(asks = report.asks.len(), "told control this machine's asks");
            }
            asks.answered(report, answer, Instant::now());
        }
        None => asks.timed_out_or(Wait::For(TROUBLE, "control didn't answer".into()), report, Instant::now()),
    }
}

/// `report` put to control, without its projects if `bare`; none if control
/// didn't answer in time.
async fn put(app: &crate::server::App, report: &PaneAsksReport, bare: bool) -> Option<Result<(), CloudRefused>> {
    let mut report = report.clone();
    if bare {
        report.asks.iter_mut().for_each(|a| a.project = None);
    }
    let body = match serde_json::to_value(&report) {
        Ok(b) => b,
        Err(e) => return Some(Err(CloudRefused::Said { status: 400, said: e.to_string(), reason: None })),
    };
    let call = app.control.cloud_now(reqwest::Method::PUT, AGENT_PANE_ASKS, Some(&body));
    tokio::time::timeout(crate::board_state::REQUEST, call).await.ok().map(|a| a.map(|_| ()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use arugula_proto::{AskRef, PaneInfo, Reason, StartedBy};

    fn reason(kind: ReasonKind, what: Option<AskWhat>) -> Reason {
        let mut r: Reason = serde_json::from_value(serde_json::json!({
            "kind": kind, "since_ms": 100, "headline": "what is asked stays here", "actions": []
        }))
        .unwrap();
        r.ask = what.map(|what| AskRef { id: "a".into(), what, agent: "claude".into(), tool: None, questions: None });
        r
    }

    fn pane(id: u32, kind: BlockType, attention: Attention, r: Option<Reason>) -> PaneSummary {
        let mut info: PaneInfo = serde_json::from_value(serde_json::json!({
            "id": id, "epoch": 1, "cwd": null, "command": null, "running": true, "policy": { "kind": "shell" },
        }))
        .unwrap();
        (info.kind, info.attention, info.reason) = (kind, attention, r);
        PaneSummary { session: 1, session_name: "s".into(), tab: 1, tab_name: None, info }
    }

    fn started_by(mut p: PaneSummary, block: u32) -> PaneSummary {
        p.info.started_by = Some(StartedBy { by: "mcp:claude".into(), block: Some(block), ..Default::default() });
        p
    }

    /// A pane waiting on its own card or to-do (Arugula #1464).
    fn waits() -> Reason {
        let mut r = reason(ReasonKind::Input, None);
        r.bundle = Some(format!("{WAITS_BUNDLE}/work"));
        r
    }

    /// Each kind of wait, from an agent block's attention and reason; a
    /// terminal's bell, a working block and one waiting on its own card or
    /// to-do (its person's Inbox item already) are none.
    #[test]
    fn what_each_pane_waits_for() {
        use {Attention::*, BlockType::*, ReasonKind as R};
        let panes = [
            pane(1, Agent, NeedsInput, Some(reason(R::Ask, Some(AskWhat::Question)))),
            pane(2, Agent, NeedsInput, Some(reason(R::Ask, Some(AskWhat::Approve)))),
            pane(3, Agent, NeedsInput, Some(reason(R::Diff, None))),
            pane(4, Agent, NeedsInput, Some(reason(R::Input, None))),
            pane(5, Agent, Done, Some(reason(R::Failed, None))),
            pane(6, Agent, Done, Some(reason(R::Done, None))),
            pane(7, Workspace, NeedsInput, Some(reason(R::Gate, None))),
            pane(8, Terminal, NeedsInput, Some(reason(R::Input, None))),
            pane(9, Terminal, Done, Some(reason(R::Failed, None))),
            pane(10, Agent, Working, None),
            pane(11, Agent, NeedsInput, Some(waits())),
            pane(12, Terminal, NeedsInput, Some(waits())),
        ];
        let got: Vec<_> = report(&panes).asks.iter().map(|a| (a.pane, a.kind, a.for_person)).collect();
        use PaneAskKind as K;
        assert_eq!(
            got,
            [
                (1, K::Question, false),
                (2, K::Permission, false),
                (3, K::Permission, false),
                (4, K::Question, false),
                (5, K::Failure, false),
                (6, K::Finished, false),
                (7, K::Gate, true),
            ]
        );
    }

    /// A terminal's open card counts (a hooked agent's question); a card
    /// only the owner answers is a person's.
    #[test]
    fn a_card_on_any_pane_and_the_owners_alone() {
        let mut hooked = pane(1, BlockType::Terminal, Attention::NeedsInput, Some(reason(ReasonKind::Ask, None)));
        hooked.info.ask = Some(
            serde_json::from_value(serde_json::json!({
                "id": "q", "kind": "questions", "message": "", "source": "hook", "at_ms": 100
            }))
            .unwrap(),
        );
        let mut share = pane(2, BlockType::Doc, Attention::NeedsInput, Some(reason(ReasonKind::Ask, None)));
        share.info.ask = hooked.info.ask.clone();
        let r = report(&[hooked, share]);
        assert_eq!(r.asks.iter().map(|a| (a.pane, a.for_person)).collect::<Vec<_>>(), [(1, false), (2, true)]);
    }

    /// Its parent is the agent block that started it, while that block is
    /// here; and nothing it shows goes.
    #[test]
    fn the_parent_while_it_is_here_and_no_text() {
        let lead = pane(1, BlockType::Agent, Attention::Working, None);
        let ask = || pane(2, BlockType::Agent, Attention::NeedsInput, Some(reason(ReasonKind::Ask, None)));
        let r = report(&[lead.clone(), started_by(ask(), 1)]);
        assert_eq!(r.asks[0].parent, Some(1));
        assert_eq!(report(&[started_by(ask(), 1)]).asks[0].parent, None, "its lead closed");
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(text, r#"{"asks":[{"pane":2,"kind":"question","since":100,"parent":1}]}"#);
        // One in a project says it, after the rest; one in none is the
        // same JSON as before (an older control refuses a field it doesn't
        // know).
        let mut placed = ask();
        placed.info.session_project =
            serde_json::from_value(serde_json::json!({ "id": "prj_7", "name": "Seven" })).unwrap();
        let text = serde_json::to_string(&report(&[placed])).unwrap();
        assert_eq!(text, r#"{"asks":[{"pane":2,"kind":"question","since":100,"project":"prj_7"}]}"#);
        assert!(!text.contains("asked"), "no text");
        serde_json::from_str::<PaneAsksReport>(&text).unwrap();
    }

    /// Sent once until it changes; whole when every block is; and nothing
    /// more to a control without the route.
    #[test]
    fn the_list_goes_once_until_it_changes() {
        let t = Instant::now();
        let none = PaneAsksReport::default();
        let one = report(&[pane(1, BlockType::Agent, Attention::NeedsInput, Some(reason(ReasonKind::Input, None)))]);
        let mut a = Asks::default();
        assert!(a.look(t));
        let sent = a.worth_sending(none.clone(), t).expect("the first list, empty too");
        a.answered(sent, Ok(()), t);
        assert!(!a.look(t + Duration::from_secs(2)));
        a.changed();
        assert_eq!(a.worth_sending(none.clone(), t + Duration::from_secs(2)), None, "the same list isn't sent");
        a.changed();
        assert_eq!(a.worth_sending(one.clone(), t + Duration::from_secs(4)), Some(one.clone()));
        a.answered(one.clone(), Ok(()), t);
        a.again();
        assert_eq!(a.worth_sending(one.clone(), t + LOOK), Some(one.clone()), "whole when every block is");

        let not_found = CloudRefused::Said { status: 404, said: "not found".into(), reason: None };
        a.answered(one, Err(not_found), t);
        a.changed();
        a.again();
        assert!(!a.look(t + LOOK * 100), "off until restart");
    }
}
