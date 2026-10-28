//! `arugulad --version` names the binary and its release, not the
//! `arugulad-base` library's 0.0.0: `arugulad update` reads it from the
//! build it downloads and refuses one that says another version (#279).

#[test]
fn the_version_is_the_releases() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_arugulad")).arg("--version").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("arugulad {}", env!("CARGO_PKG_VERSION")));
}
