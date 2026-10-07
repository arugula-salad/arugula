//! A panic's message and backtrace, written down (#560).
//!
//! Opened from Finder or the Dock, the app's stderr goes nowhere, and a
//! panic inside a WebKit callback aborts the app before anything else can
//! say why (#556). So every panic is also appended to
//! `~/Library/Logs/arugula-desktop-panics.log` on macOS, beside the
//! daemon's `arugulad.log`, and to `arugula-desktop-panics.log` in the
//! state directory elsewhere. Past [`KEEP`] bytes the file starts over,
//! with the previous one kept as `.old`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const NAME: &str = "arugula-desktop-panics.log";

/// How big the log gets before it starts over.
const KEEP: u64 = 256 * 1024;

/// Where panics are written, if there's a home to write them in.
pub fn path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| Path::new(&h).join("Library/Logs").join(NAME));
    }
    Some(arugula_proto::dirs::default_state_dir()?.join(NAME))
}

/// Write down every panic, after the default hook prints it to stderr.
pub fn install() {
    if let Some(path) = path() {
        install_at(path);
    }
}

/// [`install`], writing to `path`.
fn install_at(path: PathBuf) {
    let print = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        print(info);
        let thread = std::thread::current();
        let report = report(
            SystemTime::now(),
            thread.name().unwrap_or("unnamed"),
            &info.to_string(),
            &std::backtrace::Backtrace::force_capture().to_string(),
        );
        // A panic hook that fails has nowhere left to say so.
        let _ = append(&path, &report);
    }));
}

/// One panic's entry: when, which version and thread, what, and where from.
fn report(at: SystemTime, thread: &str, panic: &str, backtrace: &str) -> String {
    format!(
        "{} arugula-desktop {} on {} ({thread}): {panic}\n{backtrace}\n",
        utc(at),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
    )
}

fn append(path: &Path, report: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > KEEP) {
        std::fs::rename(path, path.with_extension("log.old"))?;
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(report.as_bytes())
}

/// `2026-10-07T09:59:28Z`, without a date crate for one line.
fn utc(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn writes_utc_times() {
        assert_eq!(super::utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(UNIX_EPOCH + Duration::from_secs(1_791_367_168)), "2026-10-07T09:59:28Z");
        assert_eq!(super::utc(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn appends_and_starts_over_when_big() {
        let dir = std::env::temp_dir().join(format!("arugula-panics-{}", std::process::id()));
        let path = dir.join("logs").join(super::NAME);
        let entry = super::report(UNIX_EPOCH, "main", "panicked at x.rs:1:2:\nboom", "0: main\n");
        assert!(entry.starts_with("1970-01-01T00:00:00Z arugula-desktop "));
        assert!(entry.contains("(main): panicked at x.rs:1:2:\nboom\n0: main\n"));
        super::append(&path, &entry).unwrap();
        super::append(&path, &entry).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry.repeat(2));
        std::fs::write(&path, vec![b'x'; super::KEEP as usize + 1]).unwrap();
        super::append(&path, &entry).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry);
        assert_eq!(std::fs::metadata(path.with_extension("log.old")).unwrap().len(), super::KEEP + 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The child of `a_panic_is_written_down`: with the hook on the file it
    /// names, a thread panics. Run alone, it does nothing.
    #[test]
    fn panic_in_child() {
        let Some(path) = std::env::var_os(CHILD) else { return };
        super::install_at(path.into());
        let worker = std::thread::Builder::new().name("worker".into());
        let _ = worker.spawn(|| panic!("boom in the worker")).unwrap().join();
    }

    const CHILD: &str = "ARUGULA_TEST_PANIC_LOG";

    /// The hook is the process's, so it's tried in a child: this test binary
    /// again, running only `panic_in_child`.
    #[test]
    fn a_panic_is_written_down() {
        let dir = std::env::temp_dir().join(format!("arugula-panics-child-{}", std::process::id()));
        let path = dir.join("logs").join(super::NAME);
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "panics::tests::panic_in_child", "--nocapture", "--test-threads=1"])
            .env(CHILD, &path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        // The default hook still printed it.
        assert!(stderr.contains("boom in the worker"), "stderr: {stderr}");
        let log = std::fs::read_to_string(&path).unwrap();
        let first = log.lines().next().unwrap();
        let head = format!(" arugula-desktop {} on {} (worker): ", env!("CARGO_PKG_VERSION"), std::env::consts::OS);
        assert!(first.contains(&head), "{log}");
        assert!(log.contains("boom in the worker"), "{log}");
        // A forced backtrace: frames, whatever RUST_BACKTRACE says.
        assert!(log.lines().any(|l| l.trim_start().starts_with("0: ")), "{log}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
