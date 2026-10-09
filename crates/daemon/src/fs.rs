//! Files on a host (M7): `list`, `stat`, `read` and `watch`, read-only, for
//! the directory picker, `arugula fs`, and M11's file and diff blocks.
//! Routes and shapes are in `arugula_proto::fs`.
//!
//! The types are `arugula_mux::fs`'s ([`Scope`], [`Machine`], [`Target`], [`FsError`]),
//! re-exported here; this file is the routes and the handlers.
//!
//! **Who.** `/api/fs/…` is part of the owner's API: share-link viewers only
//! ever reach `/share/<token>` and its socket, host tokens only the dial-in
//! and sync paths, and everything else needs the owner (`server.rs`'s
//! `guard`). Through `/h/NAME` and `/tunnel/NAME` the home daemon checks
//! the owner before forwarding.

use std::{
    collections::{BTreeMap, HashSet},
    convert::Infallible,
    sync::Arc,
    time::Duration,
};

use arugula_proto::{
    Attention, BlockType, PaneId,
    fs::{CdRequest, FsChange, FsEntry, FsList, FsQuery, READ_DEFAULT, READ_MAX},
    op::ops::{FsListGet, FsRecentGet, FsStatGet, PaneCd},
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::stream;

use crate::{
    api::ApiError,
    history::{self, Filter},
    mux::{Api, Cmd},
    ops::OpRoutes,
    server::App,
};

pub use arugula_mux::fs::*;

/// What the caller sees for each kind of failure.
pub(crate) fn status_of(e: &FsError) -> StatusCode {
    match e {
        FsError::NotFound(_) => StatusCode::NOT_FOUND,
        FsError::Denied(_) => StatusCode::FORBIDDEN,
        FsError::Bad(_) => StatusCode::BAD_REQUEST,
        FsError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// `FsError`'s answer: its status and `{"error": …}`.
impl From<FsError> for ApiError {
    fn from(e: FsError) -> Self {
        ApiError(status_of(&e), e.to_string())
    }
}

// ---------------------------------------------------------------- routes

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .op::<FsListGet>()
        .op::<FsStatGet>()
        .route("/api/fs/read", get(read))
        .route("/api/fs/watch", get(watch))
        .op::<FsRecentGet>()
        .op::<PaneCd>()
}

type AppState = State<Arc<App>>;

/// The machine `pane` or `machine` names: `None` for this host.
async fn machine_of(app: &App, q: &FsQuery) -> Res<Option<arugula_proto::Machine>> {
    match (q.pane, q.machine) {
        (Some(p), _) => {
            let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
            let info = panes.iter().find(|s| s.info.id == p).ok_or(FsError::NotFound(format!("no block %{p}")))?;
            let Some(m) = info.info.host else { return Ok(None) };
            let machines = app.mux.api(Api::Machines).await.unwrap_or_default();
            Ok(machines.into_iter().find(|x| x.id == m))
        }
        (None, Some(m)) => {
            let machines = app.mux.api(Api::Machines).await.unwrap_or_default();
            machines.into_iter().find(|x| x.id == m).map(Some).ok_or(FsError::NotFound(format!("no machine m{m}")))
        }
        (None, None) => Ok(None),
    }
}

async fn target(app: &App, q: &FsQuery) -> Res<Target> {
    let Some(m) = machine_of(app, q).await? else { return Ok(Target::Local(app.mux.fs.clone())) };
    let provider = app.mux.provider.clone().ok_or(FsError::Unavailable("no sandbox provider".into()))?;
    if !provider.caps().fs_browse {
        return Err(FsError::Unavailable(format!("{} can't browse files", provider.name())));
    }
    Ok(Target::machine(provider, m.sprite))
}

fn path_of(q: &FsQuery) -> String {
    q.path.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| "~".into())
}

pub(crate) async fn list(app: &App, q: &FsQuery) -> Res<FsList> {
    let t = target(app, q).await?;
    t.list(path_of(q), q.dirs == Some(1)).await
}

pub(crate) async fn stat(app: &App, q: &FsQuery) -> Res<FsEntry> {
    let path = path_of(q);
    Ok(match target(app, q).await? {
        Target::Local(s) => blocking(move || s.stat(&path)).await?,
        Target::Machine(m) => m.stat(&path).await?,
    })
}

async fn read(State(app): AppState, Query(q): Query<FsQuery>) -> Result<Response, ApiError> {
    let (path, offset) = (path_of(&q), q.offset.unwrap_or(0));
    let len = q.len.unwrap_or(READ_DEFAULT).min(READ_MAX);
    let (bytes, size) = match target(&app, &q).await? {
        Target::Local(s) => blocking(move || s.read(&path, offset, len)).await?,
        Target::Machine(m) => m.read(&path, offset, len).await?,
    };
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    h.insert("x-arugula-size", HeaderValue::from(size));
    h.insert("x-arugula-offset", HeaderValue::from(offset));
    h.insert(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    Ok(res)
}

/// A file's bytes from `offset` (at most `len`, capped at [`READ_MAX`]) and
/// its size, on the host `pane` runs on (this host without one): MCP's
/// `read_file` (M16).
pub async fn read_on(
    app: &App,
    pane: Option<PaneId>,
    path: &str,
    offset: u64,
    len: u64,
) -> Result<(Vec<u8>, u64), String> {
    let q = FsQuery { path: Some(path.to_owned()), pane, ..Default::default() };
    let (path, len) = (path_of(&q), len.min(READ_MAX));
    let read = match target(app, &q).await.map_err(|e| e.to_string())? {
        Target::Local(s) => blocking(move || s.read(&path, offset, len)).await,
        Target::Machine(m) => m.read(&path, offset, len).await,
    };
    read.map_err(|e| e.to_string())
}

/// Polls and reports differences; ends when the caller hangs up or the
/// thing can't be read any more.
async fn watch(State(app): AppState, Query(q): Query<FsQuery>) -> Result<Response, ApiError> {
    let t = target(&app, &q).await?;
    let every = match t {
        Target::Local(_) => Duration::from_secs(1),
        Target::Machine(_) => Duration::from_secs(3),
    };
    let path = path_of(&q);
    let first = t.snapshot(path.clone()).await?;
    let line = |c: &FsChange| {
        let mut s = serde_json::to_string(c).unwrap_or_default();
        s.push('\n');
        Bytes::from(s)
    };
    let seen = by_path(&first);
    let head = line(&FsChange::Listing { list: first });
    let tail = stream::unfold(Some((t, seen)), move |st| {
        let path = path.clone();
        async move {
            let (t, seen) = st?;
            loop {
                tokio::time::sleep(every).await;
                let now = match t.snapshot(path.clone()).await {
                    Ok(l) => by_path(&l),
                    Err(e) => {
                        let out = line(&FsChange::Error { error: e.to_string() });
                        return Some((Ok::<_, Infallible>(out), None));
                    }
                };
                let changes = diff(&seen, &now);
                if changes.is_empty() {
                    continue;
                }
                let out: Vec<u8> = changes.iter().flat_map(|c| line(c).to_vec()).collect();
                return Some((Ok(Bytes::from(out)), Some((t, now))));
            }
        }
    });
    let body = futures_util::StreamExt::chain(stream::once(async move { Ok::<_, Infallible>(head) }), tail);
    Ok(([(axum::http::header::CONTENT_TYPE, "application/x-ndjson")], Body::from_stream(body)).into_response())
}

fn by_path(l: &FsList) -> BTreeMap<String, FsEntry> {
    l.entries.iter().map(|e| (e.path.clone(), e.clone())).collect()
}

fn diff(before: &BTreeMap<String, FsEntry>, after: &BTreeMap<String, FsEntry>) -> Vec<FsChange> {
    let mut out = Vec::new();
    for (p, e) in after {
        match before.get(p) {
            None => out.push(FsChange::Created { entry: e.clone() }),
            Some(b) if b != e => out.push(FsChange::Modified { entry: e.clone() }),
            _ => {}
        }
    }
    out.extend(before.keys().filter(|p| !after.contains_key(*p)).map(|p| FsChange::Removed { path: p.clone() }));
    out
}

/// Directories used lately on the host `pane`/`machine` names, newest
/// first: where its blocks are now, then where commands ran (the shell
/// integration's history). On this host, only ones that still exist.
pub(crate) async fn recent(app: &App, q: &FsQuery) -> Res<Vec<String>> {
    let host = machine_of(app, q).await?.map(|m| m.id);
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    let same: HashSet<PaneId> = panes.iter().filter(|s| s.info.host == host).map(|s| s.info.id).collect();
    let mut dirs: Vec<(u64, String)> =
        panes.iter().filter(|s| s.info.host == host).filter_map(|s| Some((u64::MAX, s.info.cwd.clone()?))).collect();
    let store = app.mux.store.clone();
    let commands = tokio::task::spawn_blocking(move || history::history(&store, &Filter::default(), 2000))
        .await
        .unwrap_or_default();
    // A closed pane's host isn't kept: only this host takes its directories
    // (checked below), never a machine's.
    dirs.extend(
        commands
            .into_iter()
            .filter(|c| same.contains(&c.pane) || (host.is_none() && !c.open))
            .filter_map(|c| Some((c.started_ms, c.cwd?))),
    );
    dirs.sort_by_key(|d| std::cmp::Reverse(d.0));
    let mut seen = HashSet::new();
    let dirs: Vec<String> = dirs.into_iter().map(|(_, d)| d).filter(|d| seen.insert(d.clone())).collect();
    let out = match host {
        Some(_) => dirs.into_iter().filter(|d| Machine::refused(d).is_ok()).take(20).collect(),
        None => {
            let scope = app.mux.fs.clone();
            blocking(move || {
                Ok(dirs.into_iter().filter(|d| scope.resolve(d).is_ok_and(|p| p.is_dir())).take(20).collect::<Vec<_>>())
            })
            .await?
        }
    };
    Ok(out)
}

/// `cd` typed into a shell waiting at its prompt; refused otherwise (it
/// would be typed into whatever program is running).
pub(crate) async fn cd(app: &App, id: PaneId, req: CdRequest) -> Res<()> {
    let line = cd_line(&req.path).ok_or_else(|| FsError::Bad(format!("can't cd to {:?}", req.path)))?;
    type_line(app, id, line, "cd").await
}

/// Type `line` into a terminal whose shell waits at its prompt: `cd`, and
/// M11's rerun of a failed command. `what` names it in the refusal.
pub(crate) async fn type_line(app: &App, id: PaneId, line: String, what: &str) -> Res<()> {
    let busy = |why: String| FsError::Bad(format!("not sent: {why}"));
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    let info = panes.into_iter().find(|s| s.info.id == id).ok_or(FsError::NotFound(format!("no pane %{id}")))?.info;
    if info.kind != BlockType::Terminal {
        return Err(busy(format!("%{id} isn't a terminal")));
    }
    if !info.integration {
        return Err(busy(format!("%{id} has shell integration off, so it can't tell whether the shell is idle")));
    }
    let p = app.mux.api(|r| Api::Pane(id, r)).await.flatten().ok_or(FsError::NotFound(format!("no pane %{id}")))?;
    let st = p.status();
    if !p.running() {
        return Err(busy(format!("nothing is running in %{id} (start its shell first)")));
    }
    if st.current.is_some() || matches!(info.attention, Attention::Working | Attention::NeedsInput) {
        return Err(busy(format!("%{id} is running something; {what} only goes to a shell waiting at its prompt")));
    }
    // This host's panes: the foreground process must be the shell itself.
    if info.host.is_none() && p.command().is_some() {
        return Err(busy(format!("%{id} is running {}", p.command().unwrap_or_default())));
    }
    if !st.at_prompt {
        return Err(busy(format!("%{id}'s shell hasn't shown its prompt yet")));
    }
    p.mark_input();
    app.mux.send(Cmd::Input { client: None, pane: id, data: line.into_bytes() });
    Ok(())
}

/// End of line, erase it (what was half typed), then `cd -- 'PATH'`.
fn cd_line(path: &str) -> Option<String> {
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let arg = match path.strip_prefix('~') {
        Some("") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~/{}", quote(rest.trim_start_matches('/'))),
        _ => quote(path),
    };
    Some(format!("\x05\x15cd -- {arg}\r"))
}

// Unix: they make symlinks, which Windows only allows in developer mode.
#[cfg(all(test, unix))]
mod tests {
    use std::path::{Path, PathBuf};

    use arugula_proto::fs::FsKind;

    use super::*;

    #[test]
    fn diffs_and_cd_lines() {
        let e = |p: &str, size| FsEntry {
            name: p.into(),
            path: p.into(),
            kind: FsKind::File,
            size,
            mode: 0o644,
            mtime_ms: 0,
            target: None,
        };
        let a: BTreeMap<_, _> = [("a".to_owned(), e("a", 1)), ("b".to_owned(), e("b", 1))].into();
        let b: BTreeMap<_, _> = [("a".to_owned(), e("a", 2)), ("c".to_owned(), e("c", 1))].into();
        let d = diff(&a, &b);
        assert_eq!(
            d,
            vec![
                FsChange::Modified { entry: e("a", 2) },
                FsChange::Created { entry: e("c", 1) },
                FsChange::Removed { path: "b".into() }
            ]
        );
        assert_eq!(cd_line("/srv/my app").unwrap(), "\x05\x15cd -- '/srv/my app'\r");
        assert_eq!(cd_line("~/it's").unwrap(), "\x05\x15cd -- ~/'it'\\''s'\r");
        assert_eq!(cd_line("~").unwrap(), "\x05\x15cd -- ~\r");
        assert!(cd_line("/x\ny").is_none());
        assert_eq!(rerun_line(" make build ").unwrap(), "\x05\x15make build\r");
        assert!(rerun_line("a\nb").is_none());
        assert_eq!(lexical(Path::new("/a/../../proc/./self")), PathBuf::from("/proc/self"));
    }
}
