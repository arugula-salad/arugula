//! A failed push, tried again (Arugula #859). A turn's push that fails (the
//! disk was full, GitHub didn't answer) leaves the session unpushed until a
//! later turn happens to push. So a box tries again on its own: after 1
//! minute, then 5, then 15, then every 30, until a push gets there, the
//! session ends, or a turn starts (its own finish pushes, and starts the
//! next retry if that fails too). One loop per session: starting one stops
//! the one before. While a turn runs it waits for the turn. The failure
//! stays on the card until a push succeeds; each failed retry reports its
//! newest reason, and a success reports as a turn does, `pushed`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use arugula_control_wire::work::SessionDetail;
use arugula_e2e::session::{Kind, State};
use tracing::{info, warn};

use super::{AgentSessionLedger, Push as _, pushed_now, read, update};
use crate::server::App;

/// Waits before each retry; the last repeats.
const WAITS: [Duration; 4] =
    [Duration::from_secs(60), Duration::from_secs(5 * 60), Duration::from_secs(15 * 60), Duration::from_secs(30 * 60)];

/// The retry loop each session has now, by a number that's new for each:
/// a loop whose number isn't its session's has been stopped.
static LOOPS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// The waits to use: `ARUGULA_SESSION_PUSH_RETRY_MS` (milliseconds, comma
/// separated, the last repeating) for a test or a dev box, else [`WAITS`].
fn waits() -> Vec<Duration> {
    std::env::var("ARUGULA_SESSION_PUSH_RETRY_MS")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).map(Duration::from_millis).collect::<Vec<_>>())
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| WAITS.to_vec())
}

/// Stop session `id`'s retry loop, if it has one.
pub fn cancel(id: &str) {
    LOOPS.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
}

/// Whether `n` is still session `id`'s loop.
fn current(id: &str, n: u64) -> bool {
    LOOPS.lock().unwrap_or_else(|e| e.into_inner()).get(id) == Some(&n)
}

/// Start session `id`'s retry loop, after a push that failed, in place of
/// any before it.
pub fn start(app: &Arc<App>, id: &str, keys: Arc<arugula_e2e::DeviceKeys>, me: String) {
    let n = {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        LOOPS.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned(), n);
        n
    };
    let (app, id) = (app.clone(), id.to_owned());
    crate::task::spawn(async move {
        let dir = app.control.state_dir().to_owned();
        let push = |l: AgentSessionLedger| {
            let (app, dir) = (app.clone(), dir.clone());
            async move {
                let pushed = super::push::push_turn(&app, &l).await;
                // Stopped while it pushed: what the push left goes too.
                if super::push::clean_if_ended(&dir, &l).await { Err("the session ended".to_owned()) } else { pushed }
            }
        };
        retry(&dir, &id, n, &waits(), &keys, &me, push).await;
    });
}

/// Session `id`'s push, again and again (module docs), as loop `n`, with
/// `push` as [`super::push::push_turn`]: it ends when loop `n` is no longer
/// the session's, the session has ended, a turn is under way, or there's
/// nothing to push; or once a push gets there.
pub async fn retry<Fut: Future<Output = Result<Option<String>, String>>>(
    dir: &Path,
    id: &str,
    n: u64,
    waits: &[Duration],
    keys: &arugula_e2e::DeviceKeys,
    me: &str,
    push: impl Fn(AgentSessionLedger) -> Fut,
) {
    let dir: PathBuf = dir.to_owned();
    let mut tries = 0;
    loop {
        tokio::time::sleep(waits[tries.min(waits.len() - 1)]).await;
        let Ok(Some(l)) = read(&dir, id) else { break };
        if !current(id, n) || l.ended.is_some() || l.turn.is_some() || l.review {
            break;
        }
        tries += 1;
        let pushed = push(l.clone()).await;
        // A turn's end, or the session's, came while it pushed: not ours to
        // report.
        if !current(id, n) {
            break;
        }
        let Ok(Some(now)) = read(&dir, id) else { break };
        if now.ended.is_some() || now.turn.is_some() {
            break;
        }
        match &pushed {
            Ok(None) => {
                info!(session = id, tries, "hosted: a session's branch has nothing left to push");
                break;
            }
            Ok(Some(head)) => info!(session = id, tries, head, "hosted: a session's push got there on a retry"),
            Err(why) => warn!(session = id, tries, error = why, "hosted: a session's push failed again"),
        }
        let done = matches!(pushed, Ok(Some(_)));
        let head = pushed.clone().ok().flatten();
        let branch = l.branch.clone();
        update(&dir, id, |now| {
            if !pushed_now(now, keys, me, pushed) {
                return;
            }
            // As a turn reports its push: pushed, and where.
            let d = SessionDetail {
                branch: Some(branch),
                head: head.map(|h| h.chars().take(7).collect()),
                pushed: Some(true),
                ..Default::default()
            };
            now.push(keys, me, Kind::Turn, State::Working, d);
        });
        if done {
            break;
        }
    }
    let mut loops = LOOPS.lock().unwrap_or_else(|e| e.into_inner());
    if loops.get(id) == Some(&n) {
        loops.remove(id);
    }
}

/// Make loop `n` session `id`'s, as [`start`] does, for a test.
#[cfg(test)]
pub fn own(id: &str, n: u64) {
    LOOPS.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_owned(), n);
}

/// Whether session `id` has a loop.
#[cfg(test)]
pub fn is_running(id: &str) -> bool {
    LOOPS.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id)
}
