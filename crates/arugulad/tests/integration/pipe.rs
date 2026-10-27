//! #682: `arugulad` with its output pipe already closed (`| head -c1`) ends
//! quietly: no "failed printing to stdout" panic.

#![cfg(unix)]

use std::{os::unix::process::ExitStatusExt, process::Stdio};

#[test]
fn a_closed_pipe_ends_it_without_a_panic() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = arugula_testkit::command(env!("CARGO_BIN_EXE_arugulad"))
        .arg("flags")
        .arg("--state-dir")
        .arg(std::env::temp_dir().join(format!("ilg-pipe-{}", std::process::id())))
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked"), "{err}");
    assert!(out.status.success() || out.status.signal() == Some(13), "{:?}: {err}", out.status);
}
