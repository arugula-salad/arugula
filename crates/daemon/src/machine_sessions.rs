//! Telling control every session on this machine, with the project and card
//! each is on, so a project can list its sessions across machines (Arugula
//! inversion, L2): [`MachineSessionsReport`], put whole at
//! [`AGENT_MACHINE_SESSIONS`] each time, replacing the last.
//!
//! What is sent is ids and names only (aru-116): each session's id, its
//! name, its project (`prj_…`) and card (a native issue, `#N` or `iss_…`),
//! and each of its panes' id, kind and card when it isn't the session's.
//! No terminal text, no titles, no file names, no transcripts; a card that
//! isn't a native issue's (GitHub's `owner/repo#N`) isn't sent.
//!
//! - **When:** on [`board_state`](crate::board_state)'s tick, beside the
//!   block states: when the layout changed (a pane opened or closed, a
//!   session's project or card set) or every [`LOOK`] (a card inferred
//!   says nothing), and only when the list differs from the one control
//!   was last told; and whole, changed or not, whenever every block's
//!   state is sent again (on joining, after a refusal's wait, and every
//!   [`BLOCK_STATE_EVERY_MS`](arugula_control_wire::boards::BLOCK_STATE_EVERY_MS)).
//!   So at most one request a tick.
//! - **An older control:** one without the route answers 404 (or 405):
//!   then nothing more is sent until the daemon restarts. Any other
//!   refusal waits as a block state's does.

use std::time::{Duration, Instant};

use arugula_control_wire::boards::{MachineSession, MachineSessionPane, MachineSessionsReport};
use arugula_proto::{BlockType, OnIssue, PaneId, ProjectRef, Session, SessionId, api::PaneSummary};
use tracing::warn;

use crate::{
    board_state::{TROUBLE, Wait, wait_of},
    control::CloudRefused,
};

/// How often the list is looked at when nothing said it changed.
pub(crate) const LOOK: Duration = Duration::from_secs(30);

/// A project and card as control is told them: ids only, or none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Place {
    pub project: Option<String>,
    pub work_item: Option<String>,
}

impl Place {
    pub fn is_none(&self) -> bool {
        self.project.is_none() && self.work_item.is_none()
    }
}

/// A `prj_…` id, or none.
fn project_id(p: &str) -> Option<String> {
    p.strip_prefix("prj_")
        .is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| p.to_owned())
}

/// A native issue (`#N` or `iss_…`), or none: a GitHub issue isn't a card.
fn card(i: &str) -> Option<String> {
    crate::mcp::issues::Native::parse(i).map(|_| i.trim().to_owned())
}

/// A pane's project and card: its block's own issue, else its session's
/// card (`on_issue`, as the mux says it), in that issue's project, else
/// its session's. A card whose project isn't known isn't one.
pub(crate) fn place(on_issue: Option<&OnIssue>, session_project: Option<&ProjectRef>) -> Place {
    let project =
        on_issue.and_then(|o| o.project.as_deref()).or(session_project.map(|p| p.id.as_str())).and_then(project_id);
    let work_item = on_issue.and_then(|o| card(&o.issue)).filter(|_| project.is_some());
    Place { project, work_item }
}

/// A session's own project and card ([`Session::work`], else its project).
fn session_place(s: &Session) -> Place {
    let project = s.work.as_ref().map(|w| w.project.as_str()).or(s.project.as_ref().map(|p| p.id.as_str()));
    let project = project.and_then(project_id);
    let work_item = s.work.as_ref().and_then(|w| w.item.as_deref()).and_then(card).filter(|_| project.is_some());
    Place { project, work_item }
}

/// What the report needs of a pane.
#[derive(Debug, Clone)]
pub(crate) struct Pane {
    pub session: SessionId,
    pub id: PaneId,
    pub kind: BlockType,
    pub place: Place,
}

impl Pane {
    pub fn of(p: &PaneSummary) -> Self {
        Pane {
            session: p.session,
            id: p.info.id,
            kind: p.info.kind,
            place: place(p.info.on_issue.as_ref(), p.info.session_project.as_ref()),
        }
    }
}

/// Every session, with its panes, as control is told them.
pub(crate) fn report(sessions: &[Session], panes: &[Pane]) -> MachineSessionsReport {
    let sessions = sessions
        .iter()
        .map(|s| {
            let own = session_place(s);
            let panes = panes
                .iter()
                .filter(|p| p.session == s.id)
                .filter_map(|p| {
                    Some(MachineSessionPane {
                        pane: u32::try_from(p.id).ok()?,
                        kind: serde_json::to_value(p.kind).ok()?.as_str()?.to_owned(),
                        work_item: p.place.work_item.clone().filter(|w| own.work_item.as_ref() != Some(w)),
                    })
                })
                .collect();
            MachineSession {
                session: s.id.to_string(),
                name: s.name.clone(),
                project: own.project,
                work_item: own.work_item,
                panes,
            }
        })
        .collect();
    MachineSessionsReport { sessions }
}

/// When the list is sent, and what control was last told of it.
#[derive(Default)]
pub(crate) struct Sessions {
    /// What control last took (or refused for itself), so the same list
    /// isn't sent again.
    last: Option<MachineSessionsReport>,
    /// The layout changed since the list was last looked at.
    dirty: bool,
    /// Sent whole, changed or not, at the next look.
    force: bool,
    looked: Option<Instant>,
    quiet_until: Option<Instant>,
    /// Control has no route for it: nothing more is sent until restart.
    off: bool,
}

impl Sessions {
    /// A layout change: the list may differ.
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
    pub fn to_send(&mut self, now: MachineSessionsReport, at: Instant) -> Option<MachineSessionsReport> {
        self.looked = Some(at);
        self.dirty = false;
        (self.force || self.last.as_ref() != Some(&now)).then_some(now)
    }

    /// What control answered to `sent`.
    pub fn answered(&mut self, sent: MachineSessionsReport, answer: Result<(), CloudRefused>, at: Instant) {
        let e = match answer {
            Ok(()) => {
                (self.last, self.force, self.quiet_until) = (Some(sent), false, None);
                return;
            }
            Err(CloudRefused::Said { status: 404 | 405, .. } | CloudRefused::OldControl) => {
                self.off = true;
                warn!("control doesn't take a machine's sessions yet: not telling it again until this daemon restarts");
                return;
            }
            Err(e) => e,
        };
        self.timed_out_or(wait_of(e), sent, at);
    }

    /// The request took too long, or control refused: wait, or drop a list
    /// control refused for itself (the same one would be refused again).
    pub fn timed_out_or(&mut self, wait: Wait, sent: MachineSessionsReport, at: Instant) {
        match wait {
            Wait::Dropped(why) => {
                warn!("control refused this machine's sessions: {why}");
                (self.last, self.force) = (Some(sent), false);
            }
            Wait::Failed(why) => {
                warn!("couldn't tell control this machine's sessions: {why}");
                self.quiet_until = Some(at + TROUBLE);
            }
            Wait::For(d, why) => {
                warn!("not telling control this machine's sessions for a while: {why}");
                self.quiet_until = Some(at + d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arugula_proto::WorkRef;

    fn session(id: SessionId, project: Option<&str>, item: Option<&str>) -> Session {
        Session {
            id,
            name: format!("s{id}"),
            tabs: vec![],
            project: project.map(|p| ProjectRef { id: p.into(), name: "Arugula".into() }),
            project_by_hand: false,
            work: project.map(|p| WorkRef { project: p.into(), item: item.map(Into::into) }),
            work_by_hand: false,
            project_repo: None,
        }
    }

    fn pane(session: SessionId, id: PaneId, kind: BlockType, work_item: Option<&str>) -> Pane {
        let place = Place { project: Some("prj_1".into()), work_item: work_item.map(Into::into) };
        Pane { session, id, kind, place }
    }

    /// Every session goes, with its project and card, and each of its
    /// panes with its kind, and its own card only when it isn't the
    /// session's. A session with no project goes too, with none.
    #[test]
    fn every_session_goes_with_its_panes_and_cards() {
        let sessions = [session(1, Some("prj_1"), Some("#7")), session(2, None, None)];
        let panes = [
            pane(1, 3, BlockType::Terminal, Some("#7")),
            pane(1, 4, BlockType::Agent, Some("#9")),
            pane(2, 5, BlockType::Browser, None),
        ];
        let r = serde_json::to_value(report(&sessions, &panes)).unwrap();
        assert_eq!(
            r,
            serde_json::json!({ "sessions": [
                { "session": "1", "name": "s1", "project": "prj_1", "work_item": "#7", "panes": [
                    { "pane": 3, "kind": "terminal" },
                    { "pane": 4, "kind": "agent", "work_item": "#9" },
                ] },
                { "session": "2", "name": "s2", "panes": [{ "pane": 5, "kind": "browser" }] },
            ] })
        );
        // Control's own type takes it.
        serde_json::from_value::<MachineSessionsReport>(r).unwrap();
    }

    /// Ids only: a GitHub issue isn't a card, and a card with no project
    /// known goes without.
    #[test]
    fn only_a_native_issue_is_a_card() {
        let on = |issue: &str, project: Option<&str>| OnIssue {
            issue: issue.into(),
            project: project.map(Into::into),
            boards: vec![],
        };
        let session = ProjectRef { id: "prj_2".into(), name: "x".into() };
        assert_eq!(
            place(Some(&on("#4", Some("prj_1"))), Some(&session)),
            Place { project: Some("prj_1".into()), work_item: Some("#4".into()) }
        );
        let iss = format!("iss_{}", "a".repeat(32));
        assert_eq!(place(Some(&on(&iss, None)), Some(&session)).work_item, Some(iss));
        assert_eq!(
            place(Some(&on("owner/repo#4", None)), Some(&session)),
            Place { project: Some("prj_2".into()), work_item: None }
        );
        assert_eq!(place(Some(&on("#4", None)), None), Place::default());
        assert_eq!(place(None, Some(&ProjectRef { id: "personal".into(), name: "x".into() })), Place::default());
    }

    /// The list is looked at when the layout changed, or every `LOOK`, and
    /// sent only when it differs from the one control took, or when every
    /// block is sent again.
    #[test]
    fn the_list_goes_once_until_it_changes() {
        let t = Instant::now();
        let one = report(&[session(1, Some("prj_1"), None)], &[]);
        let two = report(&[session(1, Some("prj_1"), Some("#7"))], &[]);
        let mut s = Sessions::default();
        assert!(s.look(t), "at first");
        let sent = s.to_send(one.clone(), t).expect("the first list");
        s.answered(sent, Ok(()), t);
        assert!(!s.look(t + Duration::from_secs(2)), "nothing changed, and it's not time to look");
        s.changed();
        s.changed();
        assert!(s.look(t + Duration::from_secs(2)));
        assert_eq!(s.to_send(one.clone(), t + Duration::from_secs(2)), None, "the same list isn't sent again");
        assert!(!s.look(t + Duration::from_secs(4)), "two changes, one look");
        assert!(s.look(t + Duration::from_secs(2) + LOOK), "looked at after a while");
        assert_eq!(s.to_send(two.clone(), t + LOOK), Some(two.clone()), "a changed list goes");
        s.answered(two.clone(), Ok(()), t + LOOK);
        s.again();
        assert!(s.look(t + LOOK));
        assert_eq!(s.to_send(two.clone(), t + LOOK), Some(two), "whole when every block is sent again");
    }

    /// A control without the route answers 404: nothing more is sent until
    /// the daemon restarts, however the list changes.
    #[test]
    fn a_control_without_the_route_is_left_alone() {
        let t = Instant::now();
        let one = report(&[session(1, None, None)], &[]);
        let mut s = Sessions::default();
        let sent = s.to_send(one.clone(), t).unwrap();
        let not_found = CloudRefused::Said { status: 404, said: "not found".into(), reason: None };
        s.answered(sent, Err(not_found), t);
        s.changed();
        s.again();
        assert!(!s.look(t + LOOK * 100), "off until restart");
    }

    /// A failure to reach control waits, then the same list goes again; a
    /// list control refused for itself isn't sent again until it changes.
    #[test]
    fn a_failure_waits_and_a_refused_list_waits_for_a_change() {
        let t = Instant::now();
        let one = report(&[session(1, None, None)], &[]);
        let mut s = Sessions::default();
        let sent = s.to_send(one.clone(), t).unwrap();
        s.answered(sent, Err(CloudRefused::Unreachable("down".into())), t);
        s.changed();
        assert!(!s.look(t + Duration::from_secs(2)), "waits");
        assert!(s.look(t + TROUBLE));
        assert_eq!(s.to_send(one.clone(), t + TROUBLE), Some(one.clone()), "the same list, again");
        let refused = CloudRefused::Said { status: 422, said: "no".into(), reason: None };
        s.answered(one.clone(), Err(refused), t + TROUBLE);
        assert_eq!(s.to_send(one, t + TROUBLE + LOOK), None);
    }
}
