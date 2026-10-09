//! #682: with its output pipe already closed (`| head -c1`, `| grep -q`),
//! the CLI ends quietly: no "failed printing to stdout" panic.

#![cfg(unix)]

use std::{
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
};

#[test]
fn a_closed_pipe_ends_it_without_a_panic() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = Command::new(env!("CARGO_BIN_EXE_arugula"))
        .args(["hooks", "status"])
        .env("HOME", std::env::temp_dir())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked"), "{err}");
    assert!(out.status.success() || out.status.signal() == Some(13), "{:?}: {err}", out.status);
}
