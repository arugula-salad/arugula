//! A box's `/etc/resolv.conf` ends with a newline (Arugula #263). E2B's
//! envd writes the file at each `/init` (a new box, and every resume)
//! with no newline after its last line, and Zig's resolver reads that line
//! as cut off: every one of Ghostty's dependencies fails to fetch, so a
//! session can't build arugulad. A fix made once at setup is gone after the
//! next resume, so the daemon puts the newline back whenever it may have
//! been taken: at its start, after a resume, and before each prompt it
//! hands a session ([`crate::work::sessions`]).
//!
//! Only on a box with the template's session users (`Slots::real`): a
//! test's daemon never touches its host's file.

use tracing::{info, warn};

/// The file.
pub const FILE: &str = "/etc/resolv.conf";

/// Whether `text` needs a newline at its end: it has a last line, and
/// nothing ends it.
pub fn needs_newline(text: &[u8]) -> bool {
    !text.is_empty() && !text.ends_with(b"\n")
}

/// Whether this box is the template's, whose file this daemon fixes.
fn here() -> bool {
    crate::work::sessions::slots().is_some_and(|s| s.real)
}

/// Put the newline back if it's missing: appended as root (the template's
/// sudo rule for `user`). Two at once append one each, and an empty line
/// harms no resolver.
pub async fn fix() {
    if !here() {
        return;
    }
    static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = ONE.lock().await;
    let Ok(text) = tokio::fs::read(FILE).await else { return };
    if !needs_newline(&text) {
        return;
    }
    let mut c = tokio::process::Command::new("sudo");
    c.args(["-n", "sh", "-c", &format!("printf '\\n' >> {FILE}")]);
    match c.stdin(std::process::Stdio::null()).kill_on_drop(true).output().await {
        Ok(o) if o.status.success() => info!("hosted: {FILE} had no newline at its end (Zig can't read it): added one"),
        Ok(o) => {
            warn!(stderr = %String::from_utf8_lossy(&o.stderr).trim(), "hosted: can't add the newline {FILE} lacks")
        }
        Err(e) => warn!(error = %e, "hosted: can't add the newline {FILE} lacks"),
    }
}

/// [`fix`] after a resume: at once, and again a little later, in case
/// envd writes the file after the sandbox id the resume was noticed by.
pub fn after_resume() {
    crate::task::spawn(async {
        fix().await;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        fix().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_last_line_with_no_newline_needs_one() {
        assert!(needs_newline(b"nameserver 8.8.8.8"));
        assert!(needs_newline(b"search x\nnameserver 8.8.8.8"));
        assert!(!needs_newline(b"nameserver 8.8.8.8\n"));
        assert!(!needs_newline(b""), "an empty file has no line to end");
    }
}
