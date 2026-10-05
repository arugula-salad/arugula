//! S32 spike: a file into the pane's host, then its path pasted into the
//! pane. A prototype for measuring, not M65: it writes on this daemon's own
//! filesystem only (no machines), has no quota or sweep, and pastes without
//! the driver rule.
//!
//! `POST /api/panes/<id>/upload?id=<hex>&ext=png&offset=N[&last=1]` with a
//! chunk of the file as the body. The first chunk (offset 0) creates the
//! file; `last=1` finishes it and pastes the path, bracketed when the
//! program asked (2004).

use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::Arc,
};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
};
use illogical_proto::PaneId;
use serde::Deserialize;

use crate::{
    api::{ApiError, Res, pane},
    mux::Cmd,
    server::App,
};

/// The most one request carries; bigger files come in chunks.
pub const CHUNK_MAX: usize = 4 << 20;
/// The most one file may be.
const FILE_MAX: u64 = 20 << 20;

#[derive(Deserialize)]
pub struct UploadQuery {
    id: String,
    ext: String,
    #[serde(default)]
    offset: u64,
    #[serde(default)]
    last: bool,
}

fn err(code: StatusCode, msg: impl Into<String>) -> ApiError {
    ApiError(code, msg.into())
}

/// `$XDG_RUNTIME_DIR`, else `$TMPDIR`, else `/tmp/illogical-<uid>`, then
/// `illogical-uploads/<pane>`: each level made `0700` and refused unless
/// it's ours, a real directory and private.
fn folder(pane: PaneId) -> Result<PathBuf, String> {
    let uid = nix::unistd::getuid().as_raw();
    let base = match std::env::var_os("XDG_RUNTIME_DIR").or_else(|| std::env::var_os("TMPDIR")) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(format!("/tmp/illogical-{uid}")),
    };
    let mut dir = base.clone();
    for part in [None, Some("illogical-uploads".to_string()), Some(pane.to_string())] {
        if let Some(p) = part {
            dir.push(p);
        }
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
        if dir == base && std::env::var_os("XDG_RUNTIME_DIR").or_else(|| std::env::var_os("TMPDIR")).is_some() {
            continue; // the system's own temp dir: not ours to check
        }
        let m = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
        if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
            return Err(format!("{} isn't a private directory of ours", dir.display()));
        }
    }
    Ok(dir)
}

pub async fn upload(
    State(app): State<Arc<App>>,
    Path(id): Path<PaneId>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Res<Json<serde_json::Value>> {
    let p = pane(&app, id).await?;
    if q.id.is_empty() || q.id.len() > 32 || !q.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err(StatusCode::BAD_REQUEST, "id: up to 32 hex digits"));
    }
    let ext = q.ext.to_ascii_lowercase();
    if ext.is_empty() || ext.len() > 8 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(err(StatusCode::BAD_REQUEST, "ext: letters and digits"));
    }
    if q.offset + body.len() as u64 > FILE_MAX {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "files are capped at 20 MB"));
    }
    let dir = folder(id).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let path = dir.join(format!("img-{}.{ext}", q.id));
    let (offset, last) = (q.offset, q.last);
    let at = path.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut o = OpenOptions::new();
        o.write(true).mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
        if offset == 0 {
            o.create_new(true);
        }
        let mut f = o.open(&at)?;
        if f.metadata()?.len() != offset {
            return Err(std::io::Error::other("offset doesn't match what's been written"));
        }
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(&body)
    })
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| err(StatusCode::CONFLICT, e.to_string()))?;
    if !last {
        return Ok(Json(serde_json::json!({ "path": path })));
    }
    // Nothing that could end the bracket or act as a key: the names are
    // ours, so this is belt and braces.
    let text: String = path.to_string_lossy().chars().filter(|c| !c.is_control()).collect();
    let bracketed = tokio::task::spawn_blocking({
        let p = p.clone();
        move || p.bracketed()
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false);
    let data = if bracketed { format!("\x1b[200~{text}\x1b[201~") } else { text.clone() };
    p.mark_input();
    app.mux.send(Cmd::Input { client: None, pane: id, data: data.into_bytes() });
    Ok(Json(serde_json::json!({ "path": text, "bracketed": bracketed })))
}
