//! Durable run logs and artifacts (M213, platform step 7): what a hosted
//! machine sends of its runs to the run-log bucket, through URLs the
//! platform signs for one run's prefix, and what a project's members read
//! back with every machine asleep. Runs on hosted workspaces only: a
//! person's machine never gets an upload URL (only the hub's hosted
//! signature opens [`WORK_UPLOAD`]), and its daemon never asks.
//!
//! - **The log** is the run's pane's output, as the pane's log has it: a
//!   byte stream from offset 0. The machine sends it in chunks of about
//!   [`CHUNK_BYTES`] while the run runs, and the rest when it ends, each
//!   to `<project>/<run>/log/<n>`. Chunk `n` follows chunk `n - 1`; its
//!   `offset` is where it starts in the stream (past the end of the last
//!   one only when the pane's log dropped bytes nobody sent).
//! - **Artifacts** are the files a run leaves in [`ARTIFACTS_DIR`] under
//!   its home, each to `<project>/<run>/artifacts/<name>` with its SHA-256.
//! - **Two calls each.** [`WORK_UPLOAD`] asks for a signed `PUT` (the
//!   platform checks the run is this machine's and still taking uploads);
//!   the machine puts the bytes; [`WORK_UPLOADED`] says they're there, and
//!   the platform checks the bucket has them before it records them.
//! - **Kept 30 days**, by the bucket's rule; the platform forgets its
//!   rows for them then too.
//! - **Read** through the platform: the log a page at a time with
//!   `crate::runs::RUN_LOG` (M216), artifacts with [`RunArtifacts`].

use serde::{Deserialize, Serialize};

/// `POST` [`Upload`], signed as the hosted machine, answered
/// [`UploadGiven`].
pub const WORK_UPLOAD: &str = "/api/hosted/work/upload";
/// `POST` [`Upload`], signed as the hosted machine, once the bytes are
/// put: answered [`Uploaded`].
pub const WORK_UPLOADED: &str = "/api/hosted/work/uploaded";

/// About how much of a run's log the machine sends at once.
pub const CHUNK_BYTES: u64 = 4 * 1024 * 1024;
/// The most one chunk holds.
pub const MAX_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
/// The most of one run's log that's kept.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024 * 1024;
/// The most one artifact holds.
pub const MAX_ARTIFACT_BYTES: u64 = 100 * 1024 * 1024;
/// The most artifacts one run keeps.
pub const MAX_ARTIFACTS: usize = 50;
/// Where a run leaves its artifacts, under its home (`~runner/work/ID`):
/// `$ARUGULA_ARTIFACTS` in its environment.
pub const ARTIFACTS_DIR: &str = ".arugula/artifacts";
/// The environment variable that names a run's artifacts directory.
pub const ARTIFACTS_ENV: &str = "ARUGULA_ARTIFACTS";

/// Something of one run's for the bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Upload {
    /// Chunk `n` (from 0) of the run's log: `size` bytes from `offset` in
    /// the pane's output.
    Log { run: String, n: u32, offset: u64, size: u64, sha256: String },
    /// A file the run left, by its name in the artifacts directory.
    Artifact { run: String, name: String, size: u64, sha256: String },
}

impl Upload {
    pub fn run(&self) -> &str {
        match self {
            Upload::Log { run, .. } | Upload::Artifact { run, .. } => run,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            Upload::Log { size, .. } | Upload::Artifact { size, .. } => *size,
        }
    }

    pub fn sha256(&self) -> &str {
        match self {
            Upload::Log { sha256, .. } | Upload::Artifact { sha256, .. } => sha256,
        }
    }
}

/// What [`WORK_UPLOAD`] answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UploadGiven {
    /// `PUT` the bytes here before `expires_at` (ms), with exactly
    /// `Content-Length: size`: the URL signs it.
    Put { url: String, expires_at: u64 },
    /// It's recorded already, with the same digest (an answer that was
    /// lost): go on to the next.
    Kept,
    /// This platform has no run-log bucket: the run's log stays in its
    /// pane only. Asking again won't change that while it runs.
    Off,
    /// Not this one, and asking again won't change that. `next` is the
    /// log chunk the platform is at, when it's another.
    Refused {
        why: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<u32>,
    },
}

/// What [`WORK_UPLOADED`] answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Uploaded {
    /// Recorded (or recorded before, the same).
    Kept,
    /// Not recorded: the bucket hasn't the bytes, or they aren't the run's.
    Refused { why: String },
}

/// Whether `name` may be an artifact's: 1 to 128 letters, digits, `.`,
/// `-` and `_`, not starting with `.`. Others stay on the box.
pub fn good_artifact_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Whether `s` is a SHA-256 in lower-case hex.
pub fn good_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// One artifact, as a member reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactLink {
    pub name: String,
    pub size: u64,
    /// `sha256:` and hex.
    pub digest: String,
    /// A signed `GET` of it, until the answer's `expires_at`.
    pub url: String,
}

/// `GET /api/projects/{project}/runs/{run}/artifacts`: what the bucket
/// holds of a run's artifacts. Its log is read with
/// `crate::runs::RUN_LOG` (M216).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunArtifacts {
    pub run: String,
    /// Whether this platform keeps runs' logs and artifacts at all.
    pub on: bool,
    pub artifacts: Vec<ArtifactLink>,
    /// When the URLs stop working (ms).
    pub expires_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_the_wire() {
        let u = Upload::Log { run: "run_1".into(), n: 0, offset: 0, size: 3, sha256: "a".repeat(64) };
        let v = serde_json::to_value(&u).unwrap();
        assert_eq!(v["kind"], "log");
        assert_eq!(serde_json::from_value::<Upload>(v).unwrap(), u);
        let g: UploadGiven = serde_json::from_str(r#"{"kind":"refused","why":"no"}"#).unwrap();
        assert_eq!(g, UploadGiven::Refused { why: "no".into(), next: None });
        assert_eq!(serde_json::to_string(&UploadGiven::Off).unwrap(), r#"{"kind":"off"}"#);
        assert!(good_artifact_name("report-1.tar.gz") && !good_artifact_name(".env"));
        assert!(!good_artifact_name("a/b") && !good_artifact_name("") && !good_artifact_name(&"a".repeat(129)));
        assert!(good_sha256(&"0f".repeat(32)) && !good_sha256(&"0F".repeat(32)));
    }
}
