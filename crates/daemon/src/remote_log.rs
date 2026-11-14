//! Who reached this machine through control, and when (aru-141): one line
//! of JSON per channel opened and closed, in the state dir's
//! `remote-sessions.jsonl`. It's for the machine's owner, on the machine:
//! control can open a shell here, so the machine keeps its own record of
//! who did. At [`MAX_BYTES`] the file moves to `remote-sessions.jsonl.1`
//! (replacing the one before) and starts again.

use std::{io::Write as _, path::Path};

use serde::Serialize;
use tracing::warn;

pub const FILE: &str = "remote-sessions.jsonl";

/// The size at which the log starts a new file.
pub const MAX_BYTES: u64 = 1 << 20;

/// One line.
#[derive(Debug, Clone, Serialize)]
pub struct Entry<'a> {
    /// `open` or `close`.
    pub event: &'static str,
    /// When, in ms since the epoch.
    pub at: u64,
    /// The channel's client id here, to pair an open with its close.
    pub client: u64,
    /// The account, as control named it.
    pub account: &'a str,
    /// Its login, to show.
    pub name: &'a str,
    /// What it could do: `owner`, `editor`, `viewer` or `guest`, or `device`
    /// for a channel opened with a device key.
    pub role: &'a str,
    /// `relay` (through control) or `direct`.
    pub path: &'static str,
    /// How it got in: `token` or `device`.
    pub by: &'static str,
    /// For a close: how long it was open, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secs: Option<u64>,
}

/// Adds `e` to the log in `dir`. A failure is logged and otherwise
/// ignored: a full disk doesn't refuse someone a channel.
pub fn append(dir: &Path, e: &Entry) {
    if let Err(err) = try_append(dir, e) {
        warn!(error = %err, "couldn't write the remote sessions log");
    }
}

fn try_append(dir: &Path, e: &Entry) -> std::io::Result<()> {
    let path = dir.join(FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_BYTES) {
        std::fs::rename(&path, dir.join(format!("{FILE}.1")))?;
    }
    let mut line = serde_json::to_vec(e).map_err(std::io::Error::other)?;
    line.push(b'\n');
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    f.write_all(&line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_append_and_the_file_rolls_over() {
        let dir = std::env::temp_dir().join(format!("arugula-remote-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let e = Entry {
            event: "open",
            at: 1,
            client: 7,
            account: "acct",
            name: "someone",
            role: "editor",
            path: "relay",
            by: "token",
            secs: None,
        };
        append(&dir, &e);
        append(&dir, &Entry { event: "close", secs: Some(3), ..e.clone() });
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0]["event"].as_str(), lines[1]["secs"].as_u64()), (Some("open"), Some(3)));
        // Past the limit, the next line starts a new file.
        std::fs::write(dir.join(FILE), vec![b'x'; MAX_BYTES as usize]).unwrap();
        append(&dir, &e);
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap().lines().count(), 1);
        assert!(dir.join(format!("{FILE}.1")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
