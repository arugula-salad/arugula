//! Waking a hosted machine on connect (M182, #1243; "A hosted machine is
//! arugulad in a rented sandbox": the hub wakes it on connect). When
//! someone opens a hosted machine (M178) whose daemon isn't connected,
//! from a page or the phone (`/api/relay/m`'s `OPEN_WAKE`), the CLI or a
//! machine's MCP (`/api/relay/c` with `x-arugula-wake: 1`), the hub asks
//! the platform to wake its box (`PlatformPort::hosted_wake`) and holds
//! the open until the daemon dials in again (it does as it resumes,
//! M180), then goes on as for any machine.
//!
//! - **Asleep or not.** A paused box's daemon is frozen without a word,
//!   so its socket looks open for a while: a quiet hosted machine is
//!   pinged first ([`crate::relay::Relay::answers`]). One that doesn't
//!   answer is hung up on, and its box woken.
//! - **One wake.** Opens at once share one ask of the platform ([`Wakes`]),
//!   and the platform's worker does one resume.
//! - **Only when asked.** Summary connections (the swarm, the fleet) and
//!   a page reconnecting on its own don't wake a box: they hear it's
//!   asleep. Opening it does.
//! - **Why not.** A box that didn't wake says why ([`WakeCode`]), and the
//!   page offers rebuild.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arugula_control_wire::hosted::{WakeAnswer, WakeCode};
use arugula_e2e::mux::Mux;
use arugula_platform_api::PlatformPort as _;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::App;

/// The header that asks `/api/relay/c` to wake a sleeping hosted machine.
pub const HEADER: &str = "x-arugula-wake";
/// A daemon heard from this recently is taken as up without asking.
const FRESH: Duration = Duration::from_millis(500);
/// How long a quiet hosted machine has to answer a ping before it's taken
/// as asleep.
const PROBE: Duration = Duration::from_millis(800);
/// How long the ping goes unanswered before the wake is asked too: most
/// machines that are up answer well within it.
const GRACE: Duration = Duration::from_millis(150);
/// How long its daemon has to dial in once its box runs.
const REDIAL: Duration = Duration::from_secs(15);
/// The most an open waits on a wake, whatever happens.
const MOST: Duration = Duration::from_secs(45);
/// How long a box that woke counts as just woken: opens a moment apart
/// share its wake rather than ask again.
const SHARE: Duration = Duration::from_secs(3);

/// What an open of a hosted machine comes to.
pub enum Reach {
    /// Its daemon is connected: open a stream on this.
    Up(Mux),
    /// Its box is paused, and nobody asked to wake it.
    Asleep,
    /// It was asked to wake and didn't: why.
    Failed { code: WakeCode, why: String },
}

/// What a page hears of a wake (`/api/relay/m`'s `WAKE` frame), as JSON.
pub fn said(r: Option<&Reach>) -> serde_json::Value {
    match r {
        None => serde_json::json!({ "state": "waking" }),
        Some(Reach::Up(_)) => serde_json::json!({ "state": "awake" }),
        Some(Reach::Asleep) => serde_json::json!({ "state": "asleep" }),
        Some(Reach::Failed { code, why }) => serde_json::json!({ "state": "failed", "code": code, "why": why }),
    }
}

/// The wakes the hub has asked of the platform and not heard back on, by
/// workspace: opens at once share one.
#[derive(Default)]
pub struct Wakes(Mutex<HashMap<String, watch::Receiver<Option<WakeAnswer>>>>);

impl Wakes {
    /// The platform's answer to waking `workspace`, asked once however
    /// many open it at once.
    fn start(&self, app: &Arc<App>, project: &str, workspace: &str) -> watch::Receiver<Option<WakeAnswer>> {
        let mut all = self.0.lock().unwrap();
        if let Some(rx) = all.get(workspace) {
            return rx.clone();
        }
        let (tx, rx) = watch::channel(None);
        all.insert(workspace.to_owned(), rx.clone());
        let (app, project, workspace) = (app.clone(), project.to_owned(), workspace.to_owned());
        // Its own task: the open that asked may hang up, and the others
        // still want the answer.
        tokio::spawn(async move {
            let started = Instant::now();
            let a = app.platform.hosted_wake(&project, &workspace).await.unwrap_or_else(|e| {
                warn!(%workspace, error = %e, "the platform didn't take a hosted workspace's wake");
                WakeAnswer::Failed {
                    code: WakeCode::Unavailable,
                    why: "Arugula couldn't ask for the box to wake. Try again in a moment.".into(),
                }
            });
            info!(%workspace, answer = ?a, ms = started.elapsed().as_millis() as u64, "a hosted workspace's wake answered");
            let woke = a == WakeAnswer::Awake;
            let _ = tx.send(Some(a));
            if woke {
                tokio::time::sleep(SHARE).await;
            }
            let mut all = app.relay.wakes.0.lock().unwrap();
            if all.get(&workspace).is_some_and(|rx| rx.same_channel(&tx.subscribe())) {
                all.remove(&workspace);
            }
        });
        rx
    }
}

/// Open hosted machine `id`, waking its box first if `wake` and it's
/// asleep; `waking` is called once, as the wake starts. `None` when `id`
/// isn't a hosted machine (the caller goes on as for any daemon).
pub async fn reach(app: &Arc<App>, id: &str, wake: bool, waking: impl FnOnce()) -> anyhow::Result<Option<Reach>> {
    let Some(row) = app.db.hosted(id)? else { return Ok(None) };
    let (project, workspace) = (row.cert.project, row.cert.workspace);
    let mut waking = Some(waking);
    let mut asked = None;
    let mut stale = None;
    if let Some((mux, generation)) = app.relay.mux_of(id) {
        let probe = app.relay.answers(id, FRESH, PROBE);
        tokio::pin!(probe);
        let mut answered = None;
        if wake {
            tokio::select! {
                a = &mut probe => answered = Some(a),
                _ = tokio::time::sleep(GRACE) => {
                    asked = Some(app.relay.wakes.start(app, &project, &workspace));
                    if let Some(f) = waking.take() {
                        f();
                    }
                }
            }
        }
        let answered = match answered {
            Some(a) => a,
            // Waking: its daemon may dial in afresh before the old socket's
            // ping runs out.
            None => tokio::select! {
                a = probe => a,
                mux = app.relay.came(id, Some(generation)) => return Ok(Some(Reach::Up(mux))),
            },
        };
        if answered == Some(true) && asked.is_none() {
            return Ok(Some(Reach::Up(mux)));
        }
        // Answered only once its wake was asked: its box was resumed, and
        // its old socket came back with it (E2B's can). Its daemon drops
        // that socket and dials in afresh (M180), whenever it notices, so
        // nothing is opened on it: a stream there would die with it, the
        // CLI's mid-handshake. Hung up on here, it dials in at once.
        if answered == Some(true) {
            info!(machine = %id, %workspace, "a hosted machine answered only once its box was woken: waiting for it to dial in afresh");
        } else {
            info!(machine = %id, %workspace, "a hosted machine didn't answer: its box is taken as asleep");
        }
        app.relay.drop_stale(id, generation);
        stale = Some(generation);
    }
    if !wake {
        return Ok(Some(Reach::Asleep));
    }
    if let Some(f) = waking.take() {
        f();
    }
    let started = Instant::now();
    let mut answer = asked.unwrap_or_else(|| app.relay.wakes.start(app, &project, &workspace));
    let most = tokio::time::Instant::now() + MOST;
    let mut until = most;
    let mut heard = false;
    loop {
        tokio::select! {
            mux = app.relay.came(id, stale) => {
                info!(machine = %id, %workspace, ms = started.elapsed().as_millis() as u64, "a hosted machine woke and dialed in");
                return Ok(Some(Reach::Up(mux)));
            }
            a = answer.wait_for(Option::is_some), if !heard => {
                heard = true;
                match a.as_deref().cloned() {
                    Ok(Some(WakeAnswer::Awake)) => until = (tokio::time::Instant::now() + REDIAL).min(most),
                    Ok(Some(WakeAnswer::Failed { code, why })) => return Ok(Some(Reach::Failed { code, why })),
                    _ => return Ok(Some(Reach::Failed {
                        code: WakeCode::Unavailable,
                        why: "Arugula couldn't ask for the box to wake. Try again in a moment.".into(),
                    })),
                }
            }
            _ = tokio::time::sleep_until(until) => {
                warn!(machine = %id, %workspace, "a hosted machine's box woke, but it didn't dial in");
                return Ok(Some(Reach::Failed {
                    code: if heard { WakeCode::Silent } else { WakeCode::Unavailable },
                    why: if heard {
                        "The box woke, but Arugula in it didn't connect. Rebuild the workspace if it doesn't come back.".into()
                    } else {
                        "The box didn't wake in time. Try again in a moment.".into()
                    },
                }));
            }
        }
    }
}
