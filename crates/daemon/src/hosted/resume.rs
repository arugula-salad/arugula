//! Noticing a resume (M180). A paused VM's processes carry on where they
//! were, with sockets that died while it slept and timers that don't know
//! any time went by. Two things say it happened:
//!
//! - **The wall clock jumped** past the monotonic one: the monotonic clock
//!   stood still while it was paused, and the wall clock was set right on
//!   resume (E2B's envd sets it at `/init` when it's more than 50 ms
//!   behind).
//! - **The box was initialized again:** E2B's envd rewrites
//!   `/run/e2b/.E2B_SANDBOX_ID` at each `/init`, on resume and on a new box
//!   made from the snapshot.
//!
//! Either one redials control's relay at once ([`Control::resumed`]), so
//! the hub's wake (M182) isn't kept waiting for the next backoff or for the
//! dead socket to go quiet.
//!
//! [`Control::resumed`]: crate::control::Control::resumed

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use tokio::time::Instant;
use tracing::info;

/// How often the clocks are compared.
const TICK: Duration = Duration::from_millis(200);
/// A wall clock ahead of the monotonic one by this much was a pause.
const JUMP: Duration = Duration::from_secs(1);

/// What envd writes at each `/init`.
pub const SANDBOX_ID_FILE: &str = "/run/e2b/.E2B_SANDBOX_ID";

/// [`SANDBOX_ID_FILE`], or another for tests (`ARUGULA_E2B_ID_FILE`).
pub fn sandbox_id_file() -> PathBuf {
    std::env::var_os("ARUGULA_E2B_ID_FILE").filter(|v| !v.is_empty()).map_or(SANDBOX_ID_FILE.into(), PathBuf::from)
}

/// How far the wall clock moved past the monotonic one between two looks,
/// if that's a pause.
pub fn jumped(mono: Duration, wall: Option<Duration>) -> Option<Duration> {
    let gap = wall?.checked_sub(mono)?;
    (gap >= JUMP).then_some(gap)
}

/// The sandbox id file's contents and when it was written, if it's there.
pub fn stamp(path: &Path) -> Option<(String, SystemTime)> {
    let at = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let id = std::fs::read_to_string(path).ok()?;
    Some((id.trim().to_owned(), at))
}

/// Call `resumed` whenever the VM comes back from a pause.
pub fn watch(resumed: Arc<dyn Fn() + Send + Sync>) {
    watch_file(resumed, sandbox_id_file());
}

fn watch_file(resumed: Arc<dyn Fn() + Send + Sync>, file: PathBuf) {
    tokio::spawn(async move {
        let (mut mono, mut wall, mut init) = (Instant::now(), SystemTime::now(), stamp(&file));
        loop {
            tokio::time::sleep(TICK).await;
            let (m, w, i) = (Instant::now(), SystemTime::now(), stamp(&file));
            let gap = jumped(m.duration_since(mono), w.duration_since(wall).ok());
            let again = i.is_some() && i != init;
            if gap.is_some() || again {
                info!(paused_for = ?gap, init = again, "resumed");
                resumed();
            }
            (mono, wall, init) = (m, w, i);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn a_pause_is_the_wall_clock_running_ahead() {
        let s = Duration::from_secs;
        assert_eq!(jumped(Duration::from_millis(200), Some(s(30))), Some(s(30) - Duration::from_millis(200)));
        // Ordinary ticks, a little drift, and the wall clock set back.
        assert_eq!(jumped(Duration::from_millis(200), Some(Duration::from_millis(210))), None);
        assert_eq!(jumped(s(1), Some(Duration::from_millis(1900))), None);
        assert_eq!(jumped(s(1), None), None);
    }

    #[tokio::test]
    async fn a_new_init_is_a_resume() {
        let file = super::super::scratch("resume").join(".E2B_SANDBOX_ID");
        std::fs::write(&file, "sbx1").unwrap();
        let n = Arc::new(AtomicUsize::new(0));
        let m = n.clone();
        watch_file(Arc::new(move || _ = m.fetch_add(1, Ordering::SeqCst)), file.clone());
        tokio::time::sleep(TICK * 3).await;
        assert_eq!(n.load(Ordering::SeqCst), 0);
        // envd writes it again at /init: same id, a new write.
        let later = SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
        tokio::time::sleep(TICK * 3).await;
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }
}
