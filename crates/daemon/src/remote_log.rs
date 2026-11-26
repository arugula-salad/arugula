//! Who reached this machine through control, and when (aru-141): one line
//! of JSON per channel opened and closed, in the state dir's
//! `remote-sessions.jsonl`. It's for the machine's owner, on the machine:
//! control can open a shell here, so the machine keeps its own record of
//! who did. At [`MAX_BYTES`] the file moves to `remote-sessions.jsonl.1`
//! (replacing the one before) and starts again.
//!
//! A daemon that starts where the log has lines writes a `start` line
//! first: a session opened before it that has no close ended when the
//! daemon before it stopped. [`sessions`] reads both files back as
//! sessions, for the machine's Settings page (`GET /api/remote-sessions`)
//! and `arugulad sharing log` (Arugula #1094).

use std::{collections::HashMap, io::Write as _, path::Path};

use arugula_proto::sharing::RemoteSession;
use serde::{Deserialize, Serialize};
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

/// An agent a ladder start opened here (aru-143): who asked, from which
/// machine, for which board and issue, wearing what, in which block. Read
/// back by nothing yet; [`sessions`] passes over it.
#[derive(Debug, Clone, Serialize)]
pub struct AgentStarted<'a> {
    /// Always `agent`.
    pub event: &'static str,
    pub at: u64,
    pub account: &'a str,
    /// The machine that asked, by name.
    pub from: &'a str,
    pub board: &'a str,
    /// `lead` or `issue`.
    pub rung: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<&'a str>,
    pub block: u32,
}

/// Adds `e` to the log in `dir`, as [`append`] does.
pub fn agent_started(dir: &Path, e: &AgentStarted) {
    if let Err(err) = try_append(dir, e) {
        warn!(error = %err, "couldn't write the remote sessions log");
    }
}

/// At the daemon's start: a `start` line, where the log has any, so a
/// session still open in it is read as ended (a daemon that stopped hard
/// wrote no close).
pub fn started(dir: &Path) {
    #[derive(Serialize)]
    struct Start {
        event: &'static str,
        at: u64,
    }
    if !dir.join(FILE).exists() {
        return;
    }
    if let Err(err) = try_append(dir, &Start { event: "start", at: arugula_e2e::now_ms() }) {
        warn!(error = %err, "couldn't write the remote sessions log");
    }
}

/// A line as it's read back: anything it doesn't know is left out.
#[derive(Deserialize)]
struct Line {
    event: String,
    at: u64,
    #[serde(default)]
    client: u64,
    #[serde(default)]
    account: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    by: String,
    #[serde(default)]
    secs: Option<u64>,
}

/// The sessions in the log in `dir` (the rolled-over file first), newest
/// first, at most `limit`. Each open is paired with its channel's close; one
/// with none after it is open only if `running` (the daemon is up) and no
/// `start` line came since, and else ended when the daemon stopped, which
/// the log can't say. A close whose open rolled away opens `secs` before it.
pub fn sessions(dir: &Path, running: bool, limit: usize) -> Vec<RemoteSession> {
    let mut text = std::fs::read_to_string(dir.join(format!("{FILE}.1"))).unwrap_or_default();
    text.push_str(&std::fs::read_to_string(dir.join(FILE)).unwrap_or_default());
    let mut out: Vec<RemoteSession> = Vec::new();
    // The sessions not closed yet, by their channel's client id.
    let mut open: HashMap<u64, usize> = HashMap::new();
    let session = |l: &Line, opened_ms| RemoteSession {
        account: l.account.clone(),
        name: l.name.clone(),
        role: l.role.clone(),
        path: l.path.clone(),
        by: l.by.clone(),
        opened_ms,
        closed_ms: None,
        open: false,
    };
    for l in text.lines().filter_map(|l| serde_json::from_str::<Line>(l).ok()) {
        match l.event.as_str() {
            "open" => {
                out.push(session(&l, l.at));
                // A client id the daemon gave again leaves the one before
                // with no close.
                open.insert(l.client, out.len() - 1);
            }
            "close" => match open.remove(&l.client) {
                Some(i) => out[i].closed_ms = Some(l.at),
                None => out.push(RemoteSession {
                    closed_ms: Some(l.at),
                    ..session(&l, l.at.saturating_sub(l.secs.unwrap_or(0) * 1000))
                }),
            },
            "start" => open.clear(),
            _ => {}
        }
    }
    if running {
        for i in open.into_values() {
            out[i].open = true;
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.opened_ms));
    out.truncate(limit);
    out
}

fn try_append(dir: &Path, e: &impl Serialize) -> std::io::Result<()> {
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

    #[test]
    fn opens_pair_with_their_closes_newest_first() {
        let dir = std::env::temp_dir().join(format!("arugula-remote-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let line = |event, at, client, name, secs| Entry {
            event,
            at,
            client,
            account: "acct",
            name,
            role: "viewer",
            path: "relay",
            by: "token",
            secs,
        };
        // Nothing yet, and no start line for an empty log.
        started(&dir);
        assert!(sessions(&dir, true, 10).is_empty() && !dir.join(FILE).exists());
        // A close whose open rolled away, in the older file.
        append(&dir, &line("close", 50_000, 1, "gone", Some(20)));
        std::fs::rename(dir.join(FILE), dir.join(format!("{FILE}.1"))).unwrap();
        // One closed, and one left open when the daemon stopped.
        append(&dir, &line("open", 100_000, 2, "ann", None));
        append(&dir, &line("open", 110_000, 3, "bo", None));
        append(&dir, &line("close", 160_000, 2, "ann", Some(60)));
        started(&dir);
        // After the restart, one still open; its client id is used again.
        append(&dir, &line("open", 200_000, 3, "cy", None));
        let got = sessions(&dir, true, 10);
        let seen: Vec<_> = got.iter().map(|s| (s.name.as_str(), s.opened_ms, s.closed_ms, s.open)).collect();
        assert_eq!(
            seen,
            [
                ("cy", 200_000, None, true),
                ("bo", 110_000, None, false),
                ("ann", 100_000, Some(160_000), false),
                ("gone", 30_000, Some(50_000), false),
            ]
        );
        // With the daemon down, nothing is open; and the limit keeps the newest.
        assert!(!sessions(&dir, false, 10)[0].open);
        assert_eq!(sessions(&dir, true, 1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
