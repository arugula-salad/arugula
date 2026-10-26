//! An agent session's lifecycle (M254, thread = session, Arugula #129): its
//! seven states, the moves between them, who may make each, and the line
//! a hosted daemon signs when it reports one. The platform keeps the
//! sessions (`arugula_platform::db::sessions`); the hosted daemon that runs
//! one (M255) signs everything it reports, so the record shows which
//! machine said what, as runs' events do (`crate::run`).
//!
//! A session is not a run: it lives across many turns, each started by a
//! prompt someone queued in its thread.
//!
//! **The states.**
//!
//! ```text
//!             daemon takes it            a prompt's turn ends
//!   queued ─────────────────► working ─────────────────────► done
//!                             ▲     │  ◄─────────────────────
//!              the answer     │     │  a new prompt's turn starts
//!                             │     ▼
//!                          needs_input
//!
//!   working, needs_input ── no heartbeat for 60 s ──► unresponsive
//!   unresponsive ── the box is heard from again ──► the state it was in
//!   any taken one ── its box was rebuilt ──► queued (on the new box)
//!   any but failed, stopped ──► failed | stopped
//! ```
//!
//! - `queued`: made, with its first prompt, and no box has taken it. Or
//!   its box was rebuilt (M258): the box's checkout and conversation went
//!   with the old one, so the new box takes it again, from the thread.
//! - `working`: a turn is under way on the box.
//! - `needs_input`: the turn waits for a person (a question, a permission).
//! - `done`: the turn finished; the session waits for its next prompt. Not
//!   an end: a new prompt moves it back to `working`.
//! - `unresponsive`: the platform heard nothing from the box for
//!   [`QUIET_MS`] while a turn was under way. Only the platform enters and
//!   leaves it; the daemon never sees it.
//! - `failed` and `stopped`: the ends ([`State::terminal`]). Nothing leaves
//!   them; asking again in the thread makes a new session.
//!
//! **Who moves what** ([`State::daemon_may`], [`State::control_may`]): the
//! daemon reports what it knows (it took the session, a turn started,
//! asked, finished, failed, it stopped the agent); control records the
//! rest (a session nobody took fails, a person stops it, the box went
//! quiet, the box was heard from again, the box was rebuilt).
//!
//! **The signed event.** A fixed line format, as run events are, so the
//! daemon and anyone checking later sign and verify the same bytes:
//!
//! ```text
//! arugula session event v1
//! session <session id>
//! n <the daemon's report number in the session, from 1>
//! kind <state|heartbeat|error|turn>
//! from <state>
//! to <state>
//! at <ms since the epoch, the daemon's clock>
//! detail <SHA-256 of the event's detail, as stored, hex>
//! daemon <the reporting daemon's device id>
//! ```
//!
//! `n` counts only the daemon's own reports, not the events control
//! records between them, so the platform marking a session unresponsive
//! or a person stopping it never makes the daemon's next report the wrong
//! number. Each `n` is good once in one session, so a report can't be
//! replayed onto another session or later in the same one.
//!
//! **The kinds** ([`Kind`]). `state` moves the session from `from` to
//! `to`. The others leave it where it is, `from` and `to` both the state
//! the daemon sees it in: `heartbeat` (it's alive; what tool runs now),
//! `error` (the agent hit one), `turn` (what a turn left: the branch,
//! whether it pushed, its commits and diffstat). What each detail holds is
//! the platform's (`db::sessions`).

use std::fmt;

use sha2::{Digest as _, Sha256};

use crate::keys::DeviceKeys;

/// How long a turn under way may go without the box reporting anything
/// before the platform marks its session `unresponsive`.
pub const QUIET_MS: u64 = 60_000;

/// A session's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Queued,
    Working,
    NeedsInput,
    Done,
    Failed,
    Unresponsive,
    Stopped,
}

impl State {
    pub const ALL: [State; 7] = [
        State::Queued,
        State::Working,
        State::NeedsInput,
        State::Done,
        State::Failed,
        State::Unresponsive,
        State::Stopped,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Working => "working",
            State::NeedsInput => "needs_input",
            State::Done => "done",
            State::Failed => "failed",
            State::Unresponsive => "unresponsive",
            State::Stopped => "stopped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        State::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// Nothing leaves it.
    pub fn terminal(self) -> bool {
        matches!(self, State::Failed | State::Stopped)
    }

    /// A turn is under way: the box reports at least every [`QUIET_MS`],
    /// or the platform marks the session `unresponsive`.
    pub fn watched(self) -> bool {
        matches!(self, State::Working | State::NeedsInput)
    }

    /// Whether a session may move from `self` to `to` at all:
    /// - `queued` to `working` (a box took it), `failed` or `stopped`;
    /// - `working` to `needs_input`, `done`, `failed`, `stopped` or
    ///   `unresponsive`;
    /// - `needs_input` to `working` (answered), `failed`, `stopped` or
    ///   `unresponsive`;
    /// - `done` to `working` (a new prompt's turn), `failed` or `stopped`;
    /// - `unresponsive` back to `working` or `needs_input` (the box was
    ///   heard from), or to `failed` or `stopped`.
    pub fn may_become(self, to: State) -> bool {
        use State::*;
        matches!(
            (self, to),
            (Queued, Working | Failed | Stopped)
                | (Working, NeedsInput | Done | Failed | Stopped | Unresponsive)
                | (NeedsInput, Working | Failed | Stopped | Unresponsive)
                | (Done, Working | Failed | Stopped)
                | (Unresponsive, Working | NeedsInput | Failed | Stopped)
                | (Working | NeedsInput | Done | Unresponsive, Queued)
        )
    }

    /// The moves the session's daemon reports: every one it knows of, and
    /// none into or out of `unresponsive` (it never sees that state), nor
    /// back to `queued`.
    pub fn daemon_may(self, to: State) -> bool {
        self.may_become(to) && self != State::Unresponsive && to != State::Unresponsive && to != State::Queued
    }

    /// The moves control records: a session nobody took or whose box is
    /// gone fails (with a reason), a person stops it, the box goes quiet
    /// (`unresponsive`) and is heard from again (back to the state it was
    /// in), its box is rebuilt (back to `queued`, for the new box). Never
    /// the moves only the box knows: taking it, a turn's start, question
    /// or end.
    pub fn control_may(self, to: State) -> bool {
        self.may_become(to)
            && match to {
                State::Failed | State::Stopped | State::Unresponsive | State::Queued => true,
                State::Working | State::NeedsInput => self == State::Unresponsive,
                State::Done => false,
            }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a daemon's report says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A move, from `from` to `to`.
    State,
    /// It's alive, and what tool runs now. Kept as the session's latest,
    /// not as an event.
    Heartbeat,
    /// The agent hit an error; the session stays where it is.
    Error,
    /// What a turn left behind: its branch, whether it pushed, its commits
    /// and diffstat.
    Turn,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::State, Kind::Heartbeat, Kind::Error, Kind::Turn];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::State => "state",
            Kind::Heartbeat => "heartbeat",
            Kind::Error => "error",
            Kind::Turn => "turn",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Kind::ALL.into_iter().find(|x| x.as_str() == s)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One report a daemon makes, as it's signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event<'a> {
    pub session: &'a str,
    /// The daemon's report number in the session, from 1.
    pub n: u64,
    pub kind: Kind,
    pub from: State,
    /// `from` itself for every kind but `state`.
    pub to: State,
    pub at: u64,
    /// The report's detail exactly as it's stored (a JSON object's text).
    pub detail: &'a str,
    /// The reporting daemon's device id.
    pub daemon: &'a str,
}

impl Event<'_> {
    /// What's signed. Frozen once shipped: see `frozen.rs`.
    pub fn body(&self) -> String {
        format!(
            "arugula session event v1\nsession {}\nn {}\nkind {}\nfrom {}\nto {}\nat {}\ndetail {}\ndaemon {}\n",
            self.session,
            self.n,
            self.kind,
            self.from,
            self.to,
            self.at,
            hex::encode(Sha256::digest(self.detail.as_bytes())),
            self.daemon,
        )
    }

    /// Whether every field can go in the format (a newline or a space in an
    /// id would let one event's body read as another's), and a report that
    /// isn't a move stays where it is.
    pub fn well_formed(&self) -> bool {
        let id = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(|c| c.is_control() || c == ' ');
        id(self.session) && id(self.daemon) && self.n > 0 && (self.kind == Kind::State || self.from == self.to)
    }

    /// The daemon's signature over [`Event::body`], hex.
    pub fn sign(&self, keys: &DeviceKeys) -> String {
        hex::encode(keys.signature(self.body().as_bytes()))
    }

    /// Whether `sig` (hex) is the signature of the daemon whose Ed25519
    /// key is `sign_hex` over this event.
    pub fn verify(&self, sign_hex: &str, sig: &str) -> bool {
        self.well_formed() && crate::cert::verify_hex(sign_hex, self.body().as_bytes(), sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_round_trip_and_ends_stay_ended() {
        for s in State::ALL {
            assert_eq!(State::parse(s.as_str()), Some(s));
            assert!(!s.may_become(s), "{s} to itself");
            assert!(!s.daemon_may(State::Queued), "only control sends one back to queued");
            assert_eq!(s.control_may(State::Queued), !s.terminal() && s != State::Queued, "{s}: its box rebuilt");
            if s.terminal() {
                assert!(State::ALL.iter().all(|t| !s.may_become(*t)), "{s} is an end");
            }
            for t in State::ALL {
                // Every move someone makes; nobody makes one that isn't one.
                assert_eq!(s.may_become(t), s.daemon_may(t) || s.control_may(t), "{s} to {t}");
            }
        }
        assert_eq!(State::parse("running"), None);
        assert!(State::Done.may_become(State::Working), "a new prompt starts another turn");
        assert!(!State::Done.terminal());
        assert!(!State::Unresponsive.daemon_may(State::Working), "the daemon never sees unresponsive");
        assert!(!State::Working.daemon_may(State::Unresponsive));
        assert!(!State::Working.control_may(State::Done), "only the box knows a turn ended");
        assert!(!State::Queued.control_may(State::Working), "only the box takes a session");
    }

    #[test]
    fn a_signed_event_verifies_only_as_signed() {
        let keys = DeviceKeys::generate();
        let key = hex::encode(keys.sign_public());
        let ev = Event {
            session: "ses_1",
            n: 2,
            kind: Kind::State,
            from: State::Queued,
            to: State::Working,
            at: 10,
            detail: "{}",
            daemon: &keys.id(),
        };
        let sig = ev.sign(&keys);
        assert!(ev.verify(&key, &sig));
        for other in [
            Event { n: 3, ..ev },
            Event { session: "ses_2", ..ev },
            Event { kind: Kind::Turn, from: State::Working, ..ev },
            Event { from: State::Done, ..ev },
            Event { to: State::Failed, ..ev },
            Event { at: 11, ..ev },
            Event { detail: "{\"prompts\":[]}", ..ev },
            Event { daemon: "someone-else", ..ev },
        ] {
            assert!(!other.verify(&key, &sig), "{other:?}");
        }
        let other = DeviceKeys::generate();
        assert!(!ev.verify(&hex::encode(other.sign_public()), &sig), "another daemon's key");
        let moved = Event { kind: Kind::Heartbeat, ..ev };
        assert!(!moved.verify(&key, &moved.sign(&keys)), "a heartbeat doesn't move the session");
        let zero = Event { n: 0, ..ev };
        assert!(!zero.verify(&key, &zero.sign(&keys)), "reports count from 1");
        let spaced = Event { session: "ses_1 n 9", ..ev };
        assert!(!spaced.verify(&key, &spaced.sign(&keys)), "an id with a space");
    }

    #[test]
    fn kinds_round_trip() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Kind::parse("tool"), None);
    }
}
