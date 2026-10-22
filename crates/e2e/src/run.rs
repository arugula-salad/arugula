//! A run's lifecycle (platform step 7, M210): S37's eight states and the
//! moves between them (`spikes/s37-team-permissions/README.md`, q4), and
//! the line a daemon signs when it reports a move. The platform keeps the
//! runs (`arugula_platform::db::runs`); the daemon that runs one (M212)
//! signs each move it reports, so the record shows which machine said what.
//!
//! **The states.** `pending`, then `approved` or straight to `running`
//! under a standing grant, then `succeeded` or `failed`. `denied`,
//! `canceled` and `expired` are dead ends that never ran. Nothing leaves a
//! terminal state ([`State::terminal`]); a run asked again is a new run.
//!
//! **The signed event.** A fixed line format, as block grants are, so the
//! daemon and anyone checking later sign and verify the same bytes:
//!
//! ```text
//! arugula run event v1
//! run <run id>
//! seq <the event's number in the run, from 1>
//! from <state|->
//! to <state>
//! at <ms since the epoch>
//! detail <SHA-256 of the event's detail, as stored, hex>
//! daemon <the reporting daemon's device id>
//! ```
//!
//! `seq` makes each signature good for one place in one run's history, so
//! a report can't be replayed onto another run or later in the same one.

use std::fmt;

use sha2::{Digest as _, Sha256};

use crate::keys::DeviceKeys;

/// A run's state: S37's eight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Pending,
    Approved,
    Denied,
    Canceled,
    Expired,
    Running,
    Succeeded,
    Failed,
}

impl State {
    pub const ALL: [State; 8] = [
        State::Pending,
        State::Approved,
        State::Denied,
        State::Canceled,
        State::Expired,
        State::Running,
        State::Succeeded,
        State::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Approved => "approved",
            State::Denied => "denied",
            State::Canceled => "canceled",
            State::Expired => "expired",
            State::Running => "running",
            State::Succeeded => "succeeded",
            State::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        State::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// Nothing leaves it.
    pub fn terminal(self) -> bool {
        matches!(self, State::Denied | State::Canceled | State::Expired | State::Succeeded | State::Failed)
    }

    /// Whether S37 lets a run move from `self` to `to` (q4):
    /// - `pending` to `approved` or `denied` (the authority signs), to
    ///   `canceled` (the requester), to `expired` (the clock), to `running`
    ///   (a standing grant covers it), or to `failed` (the daemon's check
    ///   at the start failed, so it never ran);
    /// - `approved` to `running`, `canceled` or `expired` before it
    ///   starts, or to `failed` when the check at the start fails;
    /// - `running` to `succeeded` (exit 0) or `failed`.
    pub fn may_become(self, to: State) -> bool {
        use State::*;
        matches!(
            (self, to),
            (Pending, Approved | Denied | Canceled | Expired | Running | Failed)
                | (Approved, Running | Canceled | Expired | Failed)
                | (Running, Succeeded | Failed)
        )
    }

    /// Only the daemon running it may report this state: it's the one that
    /// knows the process started, or ended well. `expired` and `failed`
    /// may come from either, but only the daemon knows an exit code:
    /// control's `failed` carries a reason instead.
    pub fn daemon_only(self) -> bool {
        matches!(self, State::Running | State::Succeeded)
    }

    /// Only control records this state, as the signed objects behind it
    /// arrive (an approval, which it must show; a deny; the requester's
    /// cancel).
    pub fn control_only(self) -> bool {
        matches!(self, State::Pending | State::Approved | State::Denied | State::Canceled)
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One move a daemon reports, as it's signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event<'a> {
    pub run: &'a str,
    pub seq: u64,
    pub from: Option<State>,
    pub to: State,
    pub at: u64,
    /// The event's detail exactly as it's stored (a JSON object's text).
    pub detail: &'a str,
    /// The reporting daemon's device id.
    pub daemon: &'a str,
}

impl Event<'_> {
    /// What's signed. Frozen once shipped: see `frozen.rs`.
    pub fn body(&self) -> String {
        format!(
            "arugula run event v1\nrun {}\nseq {}\nfrom {}\nto {}\nat {}\ndetail {}\ndaemon {}\n",
            self.run,
            self.seq,
            self.from.map_or("-", State::as_str),
            self.to,
            self.at,
            hex::encode(Sha256::digest(self.detail.as_bytes())),
            self.daemon,
        )
    }

    /// Whether every field can go in the format: a newline or a space in
    /// an id would let one event's body read as another's.
    pub fn well_formed(&self) -> bool {
        let id = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(|c| c.is_control() || c == ' ');
        id(self.run) && id(self.daemon)
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
    fn states_round_trip_and_dead_ends_stay_dead() {
        for s in State::ALL {
            assert_eq!(State::parse(s.as_str()), Some(s));
            if s.terminal() {
                assert!(State::ALL.iter().all(|t| !s.may_become(*t)), "{s} is a dead end");
            }
            assert!(!s.may_become(s), "{s} to itself");
            assert!(!s.may_become(State::Pending), "nothing goes back to pending");
        }
        assert_eq!(State::parse("done"), None);
    }

    #[test]
    fn a_signed_event_verifies_only_as_signed() {
        let keys = DeviceKeys::generate();
        let key = hex::encode(keys.sign_public());
        let ev = Event {
            run: "run_1",
            seq: 2,
            from: Some(State::Pending),
            to: State::Running,
            at: 10,
            detail: "{}",
            daemon: &keys.id(),
        };
        let sig = ev.sign(&keys);
        assert!(ev.verify(&key, &sig));
        for other in [
            Event { seq: 3, ..ev },
            Event { run: "run_2", ..ev },
            Event { from: Some(State::Approved), ..ev },
            Event { to: State::Failed, ..ev },
            Event { at: 11, ..ev },
            Event { detail: "{\"exit\":0}", ..ev },
            Event { daemon: "someone", ..ev },
        ] {
            assert!(!other.verify(&key, &sig), "{other:?}");
        }
        assert!(!ev.verify(&hex::encode(DeviceKeys::generate().sign_public()), &sig), "another key");
        let bad = Event { run: "run_1\nseq 9", ..ev };
        assert!(!bad.verify(&key, &bad.sign(&keys)), "a newline in an id");
        assert!(ev.body().starts_with("arugula run event v1\nrun run_1\nseq 2\nfrom pending\nto running\nat 10\n"));
        assert!(Event { from: None, to: State::Pending, ..ev }.body().contains("\nfrom -\n"));
    }
}
