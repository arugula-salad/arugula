//! Busy or idle (M180): what a hosted daemon tells control, so the
//! platform holds its box's timeout while it works and lets it pause
//! between turns (M181).
//!
//! Busy is any pane or block that's working: an agent block's turn, an
//! agent in a terminal working by its screen, or a foreground command in a
//! terminal from its start to its end. A dev server, a log tail or an
//! editor doesn't count unless it's working by its attention: they run
//! until someone stops them, and would hold the box forever. Idle is said
//! only once it has lasted [`SETTLE`], so a burst of short commands doesn't
//! flap; busy is said at once.

use std::time::Duration;

use arugula_control_wire::activity::Activity;
use arugula_e2e::now_ms;
use arugula_proto::{Attention, EventKind, PaneInfo, WorkKind};
use tokio::{sync::broadcast::error::RecvError, time::Instant};
use tracing::debug;

use crate::mux::{Api, MuxHandle};

/// How long idle has to last before it's said.
pub const SETTLE: Duration = Duration::from_secs(5);
/// How often it's checked with nothing happening, in case an event was
/// missed (the event stream lagged).
const RECHECK: Duration = Duration::from_secs(15);

/// Whether a pane holds the box awake.
pub fn busy(p: &PaneInfo) -> bool {
    if matches!(p.work, Some(WorkKind::Server | WorkKind::Logs | WorkKind::Editor)) {
        return false;
    }
    p.attention == Attention::Working || (p.current.is_some() && p.work != Some(WorkKind::Agent))
}

/// What's said, debounced.
#[derive(Default)]
pub struct Signal {
    said: Option<bool>,
    /// When idle started (monotonic, and ms for the message), until said.
    idle_since: Option<(Instant, u64)>,
}

impl Signal {
    /// What to say now that it's `busy` (if anything), and how soon to look
    /// again without an event.
    pub fn see(&mut self, busy: bool, now: Instant, ms: u64) -> (Option<Activity>, Duration) {
        if busy {
            self.idle_since = None;
            if self.said == Some(true) {
                return (None, RECHECK);
            }
            self.said = Some(true);
            return (Some(Activity::new(true, ms)), RECHECK);
        }
        if self.said == Some(false) {
            return (None, RECHECK);
        }
        let (since, since_ms) = *self.idle_since.get_or_insert((now, ms));
        // The first thing said needn't wait: there was no busy to flap from.
        let waited = now.duration_since(since);
        if self.said.is_none() || waited >= SETTLE {
            self.said = Some(false);
            self.idle_since = None;
            return (Some(Activity::new(false, since_ms)), RECHECK);
        }
        (None, SETTLE - waited)
    }
}

/// Events after which it may have changed.
fn matters(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Attention { .. }
            | EventKind::CommandStart { .. }
            | EventKind::CommandEnd { .. }
            | EventKind::Prompt
            | EventKind::Prompted
            | EventKind::Exit { .. }
            | EventKind::Opened
            | EventKind::Closed
    )
}

/// Keep `out` saying whether this machine is working, from the mux's
/// panes as they change.
pub fn start(mux: MuxHandle, out: tokio::sync::watch::Sender<Option<String>>) {
    tokio::spawn(async move {
        let mut events = mux.events();
        let mut signal = Signal::default();
        loop {
            let Some(panes) = mux.api(Api::Panes).await else { return };
            let busy = panes.iter().any(|p| busy(&p.info));
            let (say, wait) = signal.see(busy, Instant::now(), now_ms());
            if let Some(a) = say {
                debug!(busy = a.busy, "activity");
                out.send_replace(serde_json::to_string(&a).ok());
            }
            let deadline = Instant::now() + wait;
            loop {
                tokio::select! {
                    e = events.recv() => match e {
                        Ok(e) if !matters(&e.kind) => continue,
                        Ok(_) | Err(RecvError::Lagged(_)) => break,
                        Err(RecvError::Closed) => return,
                    },
                    _ = tokio::time::sleep_until(deadline) => break,
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(attention: Attention, running: bool, work: Option<WorkKind>) -> PaneInfo {
        let mut p: PaneInfo = serde_json::from_value(serde_json::json!({
            "id": 1, "epoch": 1, "cwd": null, "command": null, "running": true,
            "policy": { "kind": "shell" },
        }))
        .unwrap();
        p.attention = attention;
        p.work = work;
        p.current = running.then(|| arugula_proto::CommandInfo {
            text: Some("make".into()),
            cwd: None,
            exit: None,
            started_ms: 0,
            ended_ms: None,
            start: 0,
            end: None,
            by: None,
        });
        p
    }

    #[test]
    fn what_holds_the_box_awake() {
        // A command from its start to its end; an agent's turn.
        assert!(busy(&pane(Attention::Idle, true, Some(WorkKind::Build))));
        assert!(busy(&pane(Attention::Idle, true, None)));
        assert!(busy(&pane(Attention::Working, false, Some(WorkKind::Agent))));
        // An agent at its prompt, though its command still runs; one
        // waiting on a person.
        assert!(!busy(&pane(Attention::Idle, true, Some(WorkKind::Agent))));
        assert!(!busy(&pane(Attention::NeedsInput, true, Some(WorkKind::Agent))));
        // Servers, logs and editors run until stopped.
        assert!(!busy(&pane(Attention::Working, true, Some(WorkKind::Server))));
        assert!(!busy(&pane(Attention::Idle, true, Some(WorkKind::Editor))));
        assert!(!busy(&pane(Attention::Idle, false, Some(WorkKind::Shell))));
    }

    #[test]
    fn idle_waits_to_settle_and_busy_doesnt() {
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        let mut s = Signal::default();
        // Idle at the start is said at once.
        assert_eq!(s.see(false, at(0.0), 0).0, Some(Activity::new(false, 0)));
        assert_eq!(s.see(true, at(1.0), 1000).0, Some(Activity::new(true, 1000)));
        // A burst of short commands: busy is never taken back.
        let (said, wait) = s.see(false, at(2.0), 2000);
        assert_eq!((said, wait), (None, SETTLE));
        assert_eq!(s.see(true, at(3.0), 3000).0, None);
        assert_eq!(s.see(false, at(4.0), 4000).0, None);
        assert_eq!(s.see(false, at(6.0), 6000), (None, Duration::from_secs(3)));
        // Idle since 4 s, said once it has lasted SETTLE.
        assert_eq!(s.see(false, at(9.0), 9000).0, Some(Activity::new(false, 4000)));
        assert_eq!(s.see(false, at(30.0), 30000).0, None);
    }
}
