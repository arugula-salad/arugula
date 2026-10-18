//! A debug `arugulad` with `ARUGULA_TEST_PANIC` set crashes as it starts,
//! and the crash is written down for the next report (M102): where, never
//! what.

use std::process::Command;

use arugula_proto::telemetry::{self, crash};

/// It runs with telemetry on as its state dir says, whatever the test run's
/// environment says (nextest, CI).
fn crash_with(dir: &std::path::Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_arugulad"))
        .env("ARUGULA_STATE_DIR", dir)
        .env(crash::TEST_PANIC_ENV, "1")
        .env_remove("NEXTEST_RUN_ID")
        .env_remove("CI")
        .env_remove("DO_NOT_TRACK")
        .env_remove(telemetry::ENV)
        // The default hook prints its backtrace: whether this build names
        // our functions at all.
        .env("RUST_BACKTRACE", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_test_panic_is_written_down_where_and_not_what() {
    let dir = std::env::temp_dir().join(format!("arugula-crash-daemon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let stderr = crash_with(&dir);
    assert!(stderr.contains("crashing as asked"), "it panicked, and said so as before: {stderr}");
    assert!(crash::list(&dir).is_empty(), "not asked: nothing written");

    telemetry::set(&dir, true, "cli", 1).unwrap();
    let stderr = crash_with(&dir);
    let files = crash::list(&dir);
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(&files[0].1).unwrap();
    assert!(!text.contains("crashing as asked") && !text.contains(crash::TEST_PANIC_ENV), "never the message: {text}");
    let c: crash::Crash = serde_json::from_str(&text).unwrap();
    assert_eq!((c.process, c.version.as_str()), (crash::Process::Daemon, env!("CARGO_PKG_VERSION")));
    assert!(c.location.starts_with("crates/daemon/src/main.rs:"), "{c:?}");
    assert!(c.frames.iter().all(|f| f.starts_with("arugula")), "{c:?}");
    // Frames are best-effort: only a build whose backtraces name our
    // functions has them. The location always is.
    // A frame named for one of ours, not one of std's whose generic
    // arguments mention ours (macOS's dev builds name ours `{closure#0}`).
    let named = stderr.lines().any(|l| {
        l.trim_start()
            .split_once(": ")
            .is_some_and(|(n, sym)| n.bytes().all(|b| b.is_ascii_digit()) && sym.starts_with("arugulad::"))
    });
    if named {
        assert!(c.frames.iter().any(|f| f == "arugulad::main"), "{c:?}\nfrom:\n{stderr}");
    } else {
        eprintln!("this build's backtraces name none of our functions, so no frames: {stderr}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
