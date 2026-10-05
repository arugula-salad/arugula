//! M70: a file from a client onto the pane's host, and its path pasted into
//! the pane, so a screenshot reaches a `claude` running there.
//!
//! `POST /api/panes/<id>/upload?id=<hex>&ext=<ext>&offset=N[&last=true]`
//! with a chunk of the file as the body: the first chunk (offset 0) makes
//! the file, each next one goes on its end, and the last answers its path.
//! Chunks keep one upload from holding a relay's channel, and give the
//! client its progress.
//!
//! `POST /api/panes/<id>/paste {paths, force}` then pastes the paths, the
//! way a client pastes text: bracketed when the program asked. Only into a
//! shell or an agent: a path means nothing to whatever runs behind `ssh`,
//! so for anything else it answers what's in front instead, and the client
//! offers *Copy* and *Paste anyway* (`force`).
//!
//! Files go in `illogical-uploads/<pane>/` under `$XDG_RUNTIME_DIR`, else
//! `$TMPDIR`, else `/tmp/illogical-<uid>`: folders `0700` and refused
//! unless they're ours, files `0600`, made new (never through a link), with
//! names we choose. A pane's go when it closes; anything older than a day
//! goes in the sweep, which also runs as the daemon starts.

use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
};
use illogical_proto::PaneId;
use serde::Deserialize;
use tracing::warn;

use crate::{
    api::{ApiError, Res, pane},
    mux::{Api, Cmd},
    server::App,
};

/// The most one request carries; bigger files come in chunks.
pub const CHUNK_MAX: usize = 4 << 20;
/// The most one file may be.
const FILE_MAX: u64 = 20 << 20;
/// The most all the uploads on this host may come to.
const QUOTA: u64 = 200 << 20;
/// How long an upload is kept.
const KEEP: Duration = Duration::from_secs(24 * 3600);
/// How often the sweep runs.
const SWEEP_EVERY: Duration = Duration::from_secs(3600);

fn err(code: StatusCode, msg: impl Into<String>) -> ApiError {
    ApiError(code, msg.into())
}

/// `dir`, made `0700`, or there already as a directory (not a link) of
/// ours that no one else may read.
fn private_dir(dir: &FsPath) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata(dir)?;
    if !m.is_dir() || m.uid() != nix::unistd::geteuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(io::Error::other(format!("{} isn't a private directory of ours", dir.display())));
    }
    Ok(())
}

/// Where every pane's uploads go: `illogical-uploads` in the user's runtime
/// or temp directory (one of the system's, not checked), else in a private
/// `/tmp/illogical-<uid>`.
fn root() -> io::Result<PathBuf> {
    let base = match std::env::var_os("XDG_RUNTIME_DIR").or_else(|| std::env::var_os("TMPDIR")) {
        Some(d) => PathBuf::from(d),
        None => {
            let d = PathBuf::from(format!("/tmp/illogical-{}", nix::unistd::geteuid().as_raw()));
            private_dir(&d)?;
            d
        }
    };
    let root = base.join("illogical-uploads");
    private_dir(&root)?;
    Ok(root)
}

/// A pane's folder, made if it isn't there.
fn folder(pane: PaneId) -> io::Result<PathBuf> {
    let dir = root()?.join(pane.to_string());
    private_dir(&dir)?;
    Ok(dir)
}

/// What the uploads under `root` come to.
fn used(root: &FsPath) -> u64 {
    let Ok(panes) = fs::read_dir(root) else { return 0 };
    panes
        .flatten()
        .filter_map(|p| fs::read_dir(p.path()).ok())
        .flat_map(|files| files.flatten())
        .filter_map(|f| f.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Remove a closed pane's uploads.
pub fn forget(pane: PaneId) {
    std::thread::spawn(move || {
        let Ok(root) = root() else { return };
        let dir = root.join(pane.to_string());
        if let Err(e) = fs::remove_dir_all(&dir)
            && e.kind() != io::ErrorKind::NotFound
        {
            warn!(pane, error = %e, "can't remove the pane's uploads");
        }
    });
}

/// Remove uploads older than `keep`, and folders left empty.
fn sweep(root: &FsPath, keep: Duration) {
    let Ok(panes) = fs::read_dir(root) else { return };
    let now = SystemTime::now();
    for p in panes.flatten() {
        let Ok(files) = fs::read_dir(p.path()) else { continue };
        for f in files.flatten() {
            let old =
                f.metadata().and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).unwrap_or_default() > keep);
            if old {
                let _ = fs::remove_file(f.path());
            }
        }
        // Only if it's empty.
        let _ = fs::remove_dir(p.path());
    }
}

/// The sweep, now (the daemon is starting) and every hour.
pub async fn keep_sweeping() {
    let mut every = tokio::time::interval(SWEEP_EVERY);
    loop {
        every.tick().await;
        let _ = tokio::task::spawn_blocking(|| {
            if let Ok(root) = root() {
                sweep(&root, KEEP);
            }
        })
        .await;
    }
}

#[derive(Deserialize)]
pub struct UploadQuery {
    /// The client's name for this upload, which names the file.
    id: String,
    /// The file's extension, which `claude` reads to know it's an image.
    #[serde(default)]
    ext: String,
    #[serde(default)]
    offset: u64,
    #[serde(default)]
    last: bool,
}

/// Write one chunk at `offset` of `path`: the first makes the file, the
/// rest must follow on from what's there.
fn write_chunk(path: &FsPath, offset: u64, body: &[u8]) -> io::Result<()> {
    let mut o = OpenOptions::new();
    o.write(true).mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    if offset == 0 {
        o.create_new(true);
    }
    let mut f = o.open(path)?;
    if f.metadata()?.len() != offset {
        return Err(io::Error::other("that chunk doesn't follow on from what's been written"));
    }
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(body)
}

pub async fn upload(
    State(app): State<Arc<App>>,
    Path(id): Path<PaneId>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Res<Json<serde_json::Value>> {
    pane(&app, id).await?;
    if let Some(Some(_)) = app.mux.api(|r| Api::MachineOf(id, r)).await {
        return Err(err(StatusCode::NOT_IMPLEMENTED, "uploads into a VM pane aren't in yet"));
    }
    if q.id.is_empty() || q.id.len() > 32 || !q.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err(StatusCode::BAD_REQUEST, "id: up to 32 hex digits"));
    }
    let ext = q.ext.to_ascii_lowercase();
    if ext.len() > 8 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(err(StatusCode::BAD_REQUEST, "ext: up to 8 letters and digits"));
    }
    if q.offset + body.len() as u64 > FILE_MAX {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "a file can be 20 MB at most"));
    }
    let name = if ext.is_empty() { q.id.clone() } else { format!("{}.{ext}", q.id) };
    let (offset, last) = (q.offset, q.last);
    let path = tokio::task::spawn_blocking(move || -> Result<PathBuf, ApiError> {
        let dir = folder(id).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if used(dir.parent().unwrap_or(&dir)) + body.len() as u64 > QUOTA {
            return Err(err(
                StatusCode::INSUFFICIENT_STORAGE,
                "uploads on this host are at their 200 MB: close panes with old ones, or wait a day",
            ));
        }
        let path = dir.join(name);
        write_chunk(&path, offset, &body).map_err(|e| err(StatusCode::CONFLICT, e.to_string()))?;
        Ok(path)
    })
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(serde_json::json!({ "path": path, "done": last })))
}

#[derive(Deserialize)]
pub struct PasteRequest {
    paths: Vec<String>,
    /// Paste whatever's in front (*Paste anyway*).
    #[serde(default)]
    force: bool,
}

/// Paths as one paste: space-separated, quoted where they need it, with
/// no control characters.
fn joined(paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| {
            let p: String = p.chars().filter(|c| !c.is_control()).collect();
            if p.chars().any(|c| c.is_whitespace() || "'\"\\$`".contains(c)) {
                format!("'{}'", p.replace('\'', r"'\''"))
            } else {
                p
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const SHELLS: &[&str] =
    &["sh", "bash", "zsh", "fish", "dash", "ksh", "mksh", "tcsh", "csh", "nu", "elvish", "xonsh", "pwsh"];

/// Whether a path on this host means something to what's in front: a shell
/// or an agent. Not `ssh`, a container's shell, an editor, a password
/// prompt (`sudo`), or anything else.
fn takes_paths(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or("").trim_matches(['\'', '"']);
    let name = first.rsplit('/').next().unwrap_or(first).trim_start_matches('-');
    SHELLS.contains(&name) || crate::classify::agent(command).is_some()
}

pub async fn paste(
    State(app): State<Arc<App>>,
    Path(id): Path<PaneId>,
    Json(req): Json<PasteRequest>,
) -> Res<Json<serde_json::Value>> {
    let p = pane(&app, id).await?;
    if req.paths.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "nothing to paste"));
    }
    let text = joined(&req.paths);
    // What's in front: its foreground job, else its own program. A VM's
    // processes aren't ours to read; its pane is a shell or what's run
    // there.
    let on_machine = matches!(app.mux.api(|r| Api::MachineOf(id, r)).await, Some(Some(_)));
    if !req.force && !on_machine {
        let front = tokio::task::spawn_blocking({
            let p = p.clone();
            move || p.command().or_else(|| p.own_command())
        })
        .await
        .ok()
        .flatten();
        if let Some(front) = front.filter(|c| !takes_paths(c)) {
            return Ok(Json(serde_json::json!({ "pasted": false, "front": front, "text": text })));
        }
    }
    let data = tokio::task::spawn_blocking({
        let p = p.clone();
        move || p.encode_paste(text)
    })
    .await
    .ok()
    .flatten()
    .ok_or_else(|| err(StatusCode::CONFLICT, "the pane isn't answering"))?;
    p.mark_input();
    app.mux.send(Cmd::Input { client: None, pane: id, data });
    Ok(Json(serde_json::json!({ "pasted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ilg-upload-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_folder_someone_could_read_or_a_link_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let t = temp("perm");
        let open = t.join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_dir(&open).is_err());
        let link = t.join("link");
        std::os::unix::fs::symlink(t.join("elsewhere"), &link).unwrap();
        fs::create_dir(t.join("elsewhere")).unwrap();
        assert!(private_dir(&link).is_err());
        let fresh = t.join("fresh");
        private_dir(&fresh).unwrap();
        assert_eq!(fs::metadata(&fresh).unwrap().mode() & 0o777, 0o700);
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn chunks_follow_on_and_a_file_is_never_reused() {
        let t = temp("chunks");
        let f = t.join("a.png");
        write_chunk(&f, 0, b"abc").unwrap();
        assert!(write_chunk(&f, 0, b"x").is_err(), "offset 0 makes a new file");
        assert!(write_chunk(&f, 5, b"x").is_err(), "a gap");
        write_chunk(&f, 3, b"def").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"abcdef");
        assert_eq!(fs::metadata(&f).unwrap().mode() & 0o777, 0o600);
        // Never through a link.
        let link = t.join("b.png");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        assert!(write_chunk(&link, 6, b"g").is_err());
        assert!(write_chunk(&link, 0, b"g").is_err());
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn the_quota_counts_every_pane_and_the_sweep_takes_old_files() {
        let t = temp("sweep");
        for (pane, size) in [("1", 10), ("2", 5)] {
            fs::create_dir(t.join(pane)).unwrap();
            fs::write(t.join(pane).join("x.png"), vec![0; size]).unwrap();
        }
        assert_eq!(used(&t), 15);
        sweep(&t, KEEP);
        assert_eq!(used(&t), 15, "new files stay");
        fs::create_dir(t.join("3")).unwrap();
        sweep(&t, Duration::ZERO);
        assert_eq!(used(&t), 0);
        assert_eq!(fs::read_dir(&t).unwrap().count(), 0, "empty folders go");
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn paths_paste_into_a_shell_or_an_agent_only() {
        for c in ["-zsh", "/bin/bash -l", "fish", "claude", "node /usr/lib/node_modules/.bin/claude", "codex resume"] {
            assert!(takes_paths(c), "{c}");
        }
        for c in
            ["ssh geek", "docker exec -it web sh", "sudo apt upgrade", "vim notes.md", "kubectl exec -it p -- bash"]
        {
            assert!(!takes_paths(c), "{c}");
        }
    }

    #[test]
    fn paths_are_joined_and_quoted_where_they_need_it() {
        let paths = ["/run/u/a.png".to_string(), "/tmp/my shot.png".into(), "/tmp/it's\x1b[201~.png".into()];
        assert_eq!(joined(&paths), r"/run/u/a.png '/tmp/my shot.png' '/tmp/it'\''s[201~.png'");
    }
}
