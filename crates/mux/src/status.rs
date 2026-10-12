//! What the programs in a pane say they're doing (OSC 7501, #720): its
//! status records, and the attention they put the pane in.
//!
//! Each report replaces its record whole; `clear` removes a record and its
//! children. A prompt or the process exiting drops what was going on
//! (`working`, `blocked`, `idle`); what ended (`done`, `error`) stays until
//! someone has seen it. The records live in the mux, beside the pane's
//! attention, and nothing in them is ever written back to the program.

use crate::osc::{Status, StatusKind, StatusState};
use arugula_proto::{Action, Attention, Reason, ReasonKind};

/// The most records a pane keeps; the one updated longest ago goes first.
pub const MAX_RECORDS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Its path; the root's is empty.
    pub id: String,
    pub state: StatusState,
    pub kind: Option<StatusKind>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    pub progress: Option<u8>,
    /// When it was last reported, in reports.
    updated: u64,
}

/// A pane's records.
#[derive(Debug, Default)]
pub struct Book {
    records: Vec<Record>,
    clock: u64,
}

impl Book {
    /// A report: it replaces its record, or clears.
    pub fn apply(&mut self, s: &Status) {
        let id = s.id.clone().unwrap_or_default();
        if s.state == StatusState::Clear {
            if id.is_empty() {
                self.records.clear();
            } else {
                let children = format!("{id}/");
                self.records.retain(|r| r.id != id && !r.id.starts_with(&children));
            }
            return;
        }
        self.clock += 1;
        self.records.retain(|r| r.id != id);
        self.records.push(Record {
            id,
            state: s.state,
            kind: s.kind,
            app: s.app.clone(),
            title: s.title.clone(),
            msg: s.msg.clone(),
            progress: s.progress,
            updated: self.clock,
        });
        if self.records.len() > MAX_RECORDS
            && let Some(oldest) = self.records.iter().enumerate().min_by_key(|(_, r)| r.updated).map(|(i, _)| i)
        {
            self.records.remove(oldest);
        }
    }

    /// A prompt started, or the process exited: only what ended stays.
    pub fn settle(&mut self) {
        self.records.retain(|r| ended(r.state));
    }

    /// Someone saw the pane: what ended has been seen.
    pub fn seen(&mut self) {
        self.records.retain(|r| !ended(r.state));
    }

    /// A full reset (RIS).
    pub fn reset(&mut self) {
        self.records.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Whether a program said how it ended (`done` or `error`).
    pub fn ended(&self) -> bool {
        self.records.iter().any(|r| ended(r.state))
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// A record's app: its own, else its nearest ancestor's.
    pub fn app_of(&self, id: &str) -> Option<&str> {
        let mut path = id;
        loop {
            if let Some(app) = self.records.iter().find(|r| r.id == path).and_then(|r| r.app.as_deref()) {
                return Some(app);
            }
            if path.is_empty() {
                return None;
            }
            path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    /// The record the pane's attention follows: the most urgent, the root
    /// before a child as urgent, then the newest.
    pub fn top(&self) -> Option<&Record> {
        self.records.iter().max_by_key(|r| (urgency(r.state), r.id.is_empty(), r.updated))
    }

    /// The pane's attention, and why, by [`Self::top`].
    pub fn attention(&self, since_ms: u64) -> Option<(Attention, Option<Reason>, String)> {
        let r = self.top()?;
        Some(attention(r, self.app_of(&r.id), since_ms))
    }
}

fn ended(s: StatusState) -> bool {
    matches!(s, StatusState::Done | StatusState::Error)
}

fn urgency(s: StatusState) -> u8 {
    match s {
        StatusState::Blocked => 4,
        StatusState::Error => 3,
        StatusState::Done => 2,
        StatusState::Working => 1,
        StatusState::Idle | StatusState::Clear => 0,
    }
}

/// What a record means for its pane: its attention, the reason a card
/// shows (with the program's `msg` as the headline), and a line for the
/// log.
pub fn attention(r: &Record, app: Option<&str>, since_ms: u64) -> (Attention, Option<Reason>, String) {
    let app = app.unwrap_or("A program");
    let said = r.msg.clone().or_else(|| r.title.clone());
    let reason = |kind: ReasonKind, default: String| {
        let headline = said.clone().unwrap_or(default);
        Reason {
            kind,
            since_ms,
            headline,
            command: None,
            exit: None,
            duration_ms: None,
            bundle: None,
            ask: None,
            gate: None,
            actions: vec![Action::Dismiss],
        }
    };
    let (state, reason) = match r.state {
        StatusState::Working => (Attention::Working, None),
        StatusState::Idle | StatusState::Clear => (Attention::Idle, None),
        StatusState::Blocked => {
            let (kind, default) = match r.kind {
                Some(StatusKind::Question) => (ReasonKind::Ask, format!("{app} has a question")),
                Some(StatusKind::Permission) => (ReasonKind::Input, format!("{app} asks for permission")),
                Some(StatusKind::Auth) => (ReasonKind::Input, format!("{app} needs you to sign in")),
                None => (ReasonKind::Input, format!("{app} is waiting for you")),
            };
            (Attention::NeedsInput, Some(reason(kind, default)))
        }
        StatusState::Done => (Attention::Done, Some(reason(ReasonKind::Done, format!("{app} is done")))),
        StatusState::Error => (Attention::Done, Some(reason(ReasonKind::Failed, format!("{app} failed")))),
    };
    let doing = if r.state == StatusState::Working { "working" } else { "idle" };
    let why = match (&reason, r.progress) {
        (Some(r), _) => r.headline.clone(),
        (None, Some(p)) => format!("{app} {doing} ({p}%)"),
        (None, None) => format!("{app} {doing}"),
    };
    (state, reason, why)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: &str, state: StatusState) -> Status {
        Status {
            id: Some(id.to_owned()).filter(|i| !i.is_empty()),
            state,
            kind: None,
            app: None,
            title: None,
            msg: None,
            progress: None,
        }
    }

    fn ids(b: &Book) -> Vec<&str> {
        let mut v: Vec<&str> = b.records().iter().map(|r| r.id.as_str()).collect();
        v.sort();
        v
    }

    #[test]
    fn a_report_replaces_its_record_whole() {
        let mut b = Book::default();
        b.apply(&Status {
            app: Some("brew".into()),
            msg: Some("upgrading".into()),
            ..report("", StatusState::Working)
        });
        b.apply(&report("", StatusState::Done));
        assert_eq!(b.records().len(), 1);
        let r = &b.records()[0];
        assert_eq!((r.state, r.app.as_deref(), r.msg.as_deref()), (StatusState::Done, None, None));
    }

    #[test]
    fn clear_takes_a_record_and_its_children() {
        let mut b = Book::default();
        for id in ["", "deploy", "deploy/us-east", "deploy/eu-west", "deployer"] {
            b.apply(&report(id, StatusState::Working));
        }
        b.apply(&report("deploy", StatusState::Clear));
        assert_eq!(ids(&b), vec!["", "deployer"]);
        b.apply(&report("", StatusState::Clear));
        assert!(b.is_empty());
    }

    #[test]
    fn children_inherit_app_from_the_nearest_ancestor() {
        let mut b = Book::default();
        b.apply(&Status { app: Some("deploy".into()), ..report("", StatusState::Working) });
        b.apply(&report("us/east/a", StatusState::Working));
        b.apply(&Status { app: Some("terraform".into()), ..report("us", StatusState::Working) });
        assert_eq!(b.app_of("us/east/a"), Some("terraform"));
        assert_eq!(b.app_of("eu"), Some("deploy"));
        b.apply(&report("", StatusState::Working));
        assert_eq!(b.app_of("eu"), None);
    }

    #[test]
    fn at_most_64_records_and_the_stalest_goes() {
        let mut b = Book::default();
        for i in 0..MAX_RECORDS {
            b.apply(&report(&format!("r{i}"), StatusState::Working));
        }
        // r0 is touched again: r1 is the stalest now.
        b.apply(&report("r0", StatusState::Working));
        b.apply(&report("new", StatusState::Working));
        assert_eq!(b.records().len(), MAX_RECORDS);
        assert!(b.records().iter().any(|r| r.id == "r0"));
        assert!(!b.records().iter().any(|r| r.id == "r1"));
    }

    #[test]
    fn a_prompt_or_exit_keeps_only_what_ended_until_seen() {
        let mut b = Book::default();
        for (id, s) in [
            ("a", StatusState::Working),
            ("b", StatusState::Blocked),
            ("c", StatusState::Idle),
            ("d", StatusState::Done),
            ("e", StatusState::Error),
        ] {
            b.apply(&report(id, s));
        }
        b.settle();
        assert_eq!(ids(&b), vec!["d", "e"]);
        assert!(b.ended());
        b.seen();
        assert!(b.is_empty());
    }

    #[test]
    fn the_most_urgent_record_leads() {
        let mut b = Book::default();
        b.apply(&report("", StatusState::Working));
        b.apply(&Status {
            kind: Some(StatusKind::Question),
            msg: Some("Ship it?".into()),
            ..report("deploy/us", StatusState::Blocked)
        });
        b.apply(&report("deploy/eu", StatusState::Error));
        let (state, reason, _) = b.attention(1).unwrap();
        assert_eq!(state, Attention::NeedsInput);
        let reason = reason.unwrap();
        assert_eq!((reason.kind, reason.headline.as_str()), (ReasonKind::Ask, "Ship it?"));
        b.apply(&report("deploy/us", StatusState::Clear));
        let (state, reason, _) = b.attention(1).unwrap();
        assert_eq!((state, reason.unwrap().kind), (Attention::Done, ReasonKind::Failed));
    }

    #[test]
    fn each_state_maps_to_attention_and_a_reason() {
        let rec = |state, kind, msg: Option<&str>| Record {
            id: String::new(),
            state,
            kind,
            app: None,
            title: None,
            msg: msg.map(str::to_owned),
            progress: Some(30),
            updated: 0,
        };
        let map = |r: &Record| {
            let (a, reason, why) = attention(r, Some("cargo"), 5);
            (a, reason.map(|r| (r.kind, r.headline)), why)
        };
        assert_eq!(
            map(&rec(StatusState::Working, None, None)),
            (Attention::Working, None, "cargo working (30%)".into())
        );
        assert_eq!(map(&rec(StatusState::Idle, None, None)).0, Attention::Idle);
        assert_eq!(
            map(&rec(StatusState::Blocked, Some(StatusKind::Permission), Some("Allow rm?"))).1,
            Some((ReasonKind::Input, "Allow rm?".into()))
        );
        assert_eq!(
            map(&rec(StatusState::Blocked, Some(StatusKind::Auth), None)).1,
            Some((ReasonKind::Input, "cargo needs you to sign in".into()))
        );
        assert_eq!(
            map(&rec(StatusState::Done, None, Some("built"))),
            (Attention::Done, Some((ReasonKind::Done, "built".into())), "built".into())
        );
        assert_eq!(map(&rec(StatusState::Error, None, None)).1, Some((ReasonKind::Failed, "cargo failed".into())));
    }
}
