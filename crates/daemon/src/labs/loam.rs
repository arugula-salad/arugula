//! The loam adapter: sandboxes from loam (Firecracker microVMs placed on
//! Kubernetes nodes, `docs/API.md` in arugula-salad/loam), through its API
//! with a project key. Everything is under `/v1/sandboxes`:
//!
//! - `GET /` (filtered by name prefix here: loam filters by label),
//!   `GET /{name}` (the record: never wakes it), `POST /` (waits for the
//!   boot), `DELETE /{name}`, `POST /{name}/wake`;
//! - **exec**, a WebSocket at `/{name}/exec`: `cmd` repeated, `tty`,
//!   `cols`/`rows`, `cwd`, `env`, and `detachable=true` so a session
//!   outlives its socket. Binary frames carry a stream byte first (`0`
//!   stdin; `1` stdout, `2` stderr, and with a TTY everything is `1`); text
//!   frames are `started` (the session's id), `resize`, `signal`, `exit`
//!   and `error`. `/{name}/exec/sessions/{id}?offset=N` reattaches, its
//!   first frame `attached` saying where the replay really starts (1 MiB is
//!   kept per session); `POST …/signal` signals one;
//! - `POST /{name}/exec/run`: a command to completion, output in base64;
//! - **ports**, a WebSocket at `/{name}/ports/{port}/connect` carrying one
//!   TCP connection in binary frames; nothing listening is 502
//!   `port_closed`, before the upgrade;
//! - **fs**: `fs/stat`, `fs/list`, `fs/file` (GET and PUT), with absolute
//!   paths: `~` and relative paths are the `loam` user's home here;
//! - **services**: `PUT`/`GET`/`DELETE /{name}/services/{svc}`. They start
//!   at every boot, restart when they crash, and survive a pause.
//!
//! **Pausing.** A loam sandbox pauses (memory kept) after its idle timeout
//! with nothing attached and nothing happening in it, and any call on it
//! wakes it with its processes. An attached exec socket holds it awake,
//! and a VM pane's terminal is one, so a pane that has been quiet (no
//! byte either way) for [`DETACH_AFTER`] lets go of its session: the
//! sandbox may then pause by its own rule. The pane's next keystroke or
//! resize reattaches from the byte count it has, which wakes it. While
//! detached the adapter reads the record (which doesn't wake it) every
//! [`POLL`]: a sandbox that has outlived its idle timeout since then is
//! busy (a command printing, someone else's call), so the pane reattaches
//! to follow it. A sandbox whose idle rule is `none` is never let go of.
//! A pause under an attached session closes its socket (1011); the pane
//! then waits, detached, for input the same way.
//!
//! A command that prints nothing for that long is paused with the rest of
//! the sandbox, and carries on when the pane wakes it: that is what pausing
//! on idle means.

use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use futures_util::{FutureExt, SinkExt, StreamExt, future::BoxFuture};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{self, Message, client::IntoClientRequest, handshake::client::Request},
};
use tracing::{info, warn};

use super::sprites::{bridge, rfc3339_ms};
use crate::pane::Spawn;
use crate::provider::{
    Begin, Caps, Cold, Conn, Exec, ExecEvent, ExecInput, ExecSink, PATIENCE, Pipe, PipeBegin, PipeEvent, Provider,
    Sandbox, ServiceDef,
};
use arugula_proto::fs::{FsEntry, FsKind, FsList};

/// The sandbox user (loam's default for exec) and its home, where `~` and
/// relative paths start.
const USER: &str = "loam";
const HOME: &str = "/home/loam";

/// What our own machines are labelled with, so they can be told apart in
/// a project that holds other sandboxes.
const LABEL: (&str, &str) = ("owner", "arugulad");

/// The idle rule our own machines are made with: pause (memory kept)
/// after five minutes with nothing attached and nothing happening.
const IDLE_TIMEOUT_S: u64 = 300;

/// How long a pane's session is quiet before the pane lets go of it, so
/// the sandbox can pause.
pub const DETACH_AFTER: Duration = Duration::from_secs(300);

/// How often a detached pane reads its sandbox's record.
pub const POLL: Duration = Duration::from_secs(30);

/// Opening a port stream takes at most this long (a sandbox may be waking).
const DIAL_TIMEOUT: Duration = Duration::from_secs(30);

/// A one-shot command's own limit, and the request's (a sleeping sandbox
/// wakes first).
const RUN_TIMEOUT_S: u64 = 60;
const RUN_REQUEST: Duration = Duration::from_secs(90);

/// Creating waits for the boot, which may pull an image first.
const CREATE_REQUEST: Duration = Duration::from_secs(300);

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A loam API, by its URL and a project key.
pub struct Loam {
    base: Url,
    key: String,
    http: reqwest::Client,
    /// Without an overall timeout, for file bodies.
    slow: reqwest::Client,
    caps: Caps,
    detach_after: Duration,
    poll: Duration,
}

impl std::fmt::Debug for Loam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loam").field("base", &self.base.as_str()).finish_non_exhaustive()
    }
}

/// loam: 1 MiB of replay from any offset, any attached socket may type and
/// resize (there is no owner), signals by name and at once (no grace before
/// a SIGKILL), and idle is a pause that keeps memory.
fn loam_caps() -> Caps {
    Caps {
        exec_replay: 1 << 20,
        exec_offset: true,
        reattach_owner: true,
        reattach_resize: true,
        kill_signal: true,
        kill_grace_ms: 0,
        cold: Cold::Restore,
        fs: true,
        fs_browse: true,
        services: true,
    }
}

/// A sandbox's record, as much of it as we read.
#[derive(Deserialize)]
struct Record {
    name: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    idle: Idle,
}

#[derive(Deserialize, Default)]
struct Idle {
    #[serde(default)]
    timeout_s: u64,
    #[serde(default)]
    action: String,
}

impl Record {
    /// As the other providers say it: `running`, `warm` (paused, memory
    /// kept), `cold` (stopped, disk kept); `creating` and `error` as they are.
    fn sandbox(self) -> Sandbox {
        let status = match self.state.as_str() {
            "paused" => "warm".to_owned(),
            "stopped" => "cold".to_owned(),
            _ => self.state,
        };
        Sandbox { name: self.name, status }
    }

    fn asleep(&self) -> bool {
        matches!(self.state.as_str(), "paused" | "stopped")
    }
}

#[derive(Deserialize)]
struct Stat {
    path: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: i64,
    /// Octal, `"0644"`.
    #[serde(default)]
    mode: String,
    /// RFC 3339, UTC.
    #[serde(default)]
    mtime: String,
}

impl Stat {
    fn into_entry(self) -> FsEntry {
        let kind = match self.kind.as_str() {
            "dir" => FsKind::Directory,
            "symlink" => FsKind::Symlink,
            "file" => FsKind::File,
            _ => FsKind::Other,
        };
        FsEntry {
            name: self.name,
            path: self.path,
            kind,
            size: self.size.max(0) as u64,
            mode: u32::from_str_radix(&self.mode, 8).unwrap_or(0),
            mtime_ms: rfc3339_ms(&self.mtime).unwrap_or(0),
            // The kind of what a link points at; loam gives its text only.
            target: None,
        }
    }
}

/// Why a WebSocket wasn't opened: loam's error (its status and code), or
/// no answer at all.
#[derive(Debug)]
enum Refused {
    Api { status: u16, code: String, message: String },
    Net(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Api { status, code, message } => write!(f, "{status} {code}: {message}"),
            Refused::Net(e) => f.write_str(e),
        }
    }
}

/// loam's error body, `{"error": {"code", "message"}}`, as (code, message).
fn api_error(body: &[u8]) -> (String, String) {
    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    let s = |k: &str| v["error"][k].as_str().unwrap_or_default().to_owned();
    (s("code"), s("message"))
}

async fn failed(what: &str, r: reqwest::Response) -> anyhow::Error {
    let status = r.status();
    let (code, message) = api_error(&r.bytes().await.unwrap_or_default());
    anyhow::anyhow!("{what}: {status} {code} {message}")
}

/// `~`, `~/x` and relative paths in the sandbox user's home; an absolute
/// path as it is.
fn absolute(path: &str) -> String {
    let rest = match path.strip_prefix('~') {
        Some(r) if r.is_empty() || r.starts_with('/') => r.trim_start_matches('/'),
        _ if path.starts_with('/') => return path.to_owned(),
        _ => path,
    };
    let rest = rest.trim_start_matches("./");
    if rest.is_empty() || rest == "." { HOME.to_owned() } else { format!("{HOME}/{rest}") }
}

/// A frame of output past the bytes we already have: `skip` is how many of
/// the replay's first bytes are repeats.
fn trim<'a>(data: &'a [u8], skip: &mut u64) -> &'a [u8] {
    let n = (*skip).min(data.len() as u64) as usize;
    *skip -= n as u64;
    &data[n..]
}

impl Loam {
    pub fn new(base: &str, key: String) -> anyhow::Result<Self> {
        let base = Url::parse(base)?;
        let http = crate::roots::http().timeout(Duration::from_secs(30)).build()?;
        let slow = crate::roots::http().connect_timeout(Duration::from_secs(30)).build()?;
        Ok(Self { base, key, http, slow, caps: loam_caps(), detach_after: DETACH_AFTER, poll: POLL })
    }

    /// From a URL and a key file; `None` (VM panes unavailable) if there's
    /// no key.
    pub fn open(base: &str, key_file: &std::path::Path) -> Option<Self> {
        let key = match std::fs::read_to_string(key_file) {
            Ok(k) => k.trim().to_owned(),
            Err(e) => {
                warn!(file = %key_file.display(), error = %e, "no loam key; VM panes are off");
                return None;
            }
        };
        if key.is_empty() {
            warn!(file = %key_file.display(), "the loam key file is empty; VM panes are off");
            return None;
        }
        match Self::new(base, key) {
            Ok(l) => Some(l),
            Err(e) => {
                warn!(error = %e, base, "bad loam API URL; VM panes are off");
                None
            }
        }
    }

    /// How long a quiet pane keeps its session, and how often a detached
    /// one reads the record (the tests shorten both).
    #[cfg(test)]
    pub fn with_idle(mut self, detach_after: Duration, poll: Duration) -> Self {
        (self.detach_after, self.poll) = (detach_after, poll);
        self
    }

    fn url(&self, path: &str) -> Url {
        let mut u = self.base.clone();
        u.set_path(&format!("/v1/sandboxes{path}"));
        u
    }

    fn ws_url(&self, path: &str) -> Url {
        let mut u = self.url(path);
        let scheme = if u.scheme() == "https" { "wss" } else { "ws" };
        let _ = u.set_scheme(scheme);
        u
    }

    fn request(&self, url: Url) -> anyhow::Result<Request> {
        let mut req = url.as_str().into_client_request()?;
        req.headers_mut().insert("Authorization", format!("Bearer {}", self.key).parse()?);
        Ok(req)
    }

    async fn connect(&self, url: Url) -> Result<Ws, Refused> {
        let req = self.request(url).map_err(|e| Refused::Net(e.to_string()))?;
        match tokio_tungstenite::connect_async(req).await {
            Ok((ws, _)) => Ok(ws),
            Err(tungstenite::Error::Http(r)) => {
                let (code, message) = api_error(r.body().as_deref().unwrap_or_default());
                Err(Refused::Api { status: r.status().as_u16(), code, message })
            }
            Err(e) => Err(Refused::Net(e.to_string())),
        }
    }

    async fn record(&self, name: &str) -> anyhow::Result<Option<Record>> {
        let r = self.http.get(self.url(&format!("/{name}"))).bearer_auth(&self.key).send().await?;
        match r.status() {
            s if s.is_success() => Ok(Some(r.json().await?)),
            StatusCode::NOT_FOUND => Ok(None),
            _ => Err(failed(&format!("looking up {name}"), r).await),
        }
    }

    async fn do_create(&self, name: &str, image: Option<&str>) -> anyhow::Result<()> {
        let mut body = json!({
            "name": name,
            "labels": { LABEL.0: LABEL.1 },
            "idle": { "timeout_s": IDLE_TIMEOUT_S, "action": "pause" },
        });
        // An image is pinned by digest (loam refuses a tag); anything else
        // names a template.
        match image {
            Some(i) if i.contains('@') => body["image"] = i.into(),
            Some(t) => body["template"] = t.into(),
            None => {}
        }
        let r = self.slow.post(self.url("")).bearer_auth(&self.key).timeout(CREATE_REQUEST).json(&body).send().await?;
        if r.status().is_success() {
            return Ok(());
        }
        let status = r.status();
        let (code, message) = api_error(&r.bytes().await.unwrap_or_default());
        if status == StatusCode::CONFLICT && code == "already_exists" {
            return Ok(());
        }
        anyhow::bail!("creating {name}: {status} {code} {message}")
    }

    async fn do_run(&self, name: &str, argv: &[&str]) -> anyhow::Result<(Vec<u8>, Option<i32>)> {
        let body = json!({ "cmd": argv, "timeout_s": RUN_TIMEOUT_S });
        let r = self
            .slow
            .post(self.url(&format!("/{name}/exec/run")))
            .bearer_auth(&self.key)
            .timeout(RUN_REQUEST)
            .json(&body)
            .send()
            .await?;
        if !r.status().is_success() {
            return Err(failed(&format!("running {} in {name}", argv.first().unwrap_or(&"")), r).await);
        }
        let v: Value = r.json().await?;
        let out = B64.decode(v["stdout"].as_str().unwrap_or_default())?;
        let code = if v["timed_out"] == true { None } else { v["exit_code"].as_i64().map(|c| c as i32) };
        Ok((out, code))
    }

    fn exec_url(&self, name: &str, argv: &[&str], tty: Option<(u16, u16)>, spawn: Option<&Spawn>) -> Url {
        let mut u = self.ws_url(&format!("/{name}/exec"));
        {
            let mut q = u.query_pairs_mut();
            for a in argv {
                q.append_pair("cmd", a);
            }
            if let Some((cols, rows)) = tty {
                q.append_pair("tty", "true");
                q.append_pair("cols", &cols.to_string());
                q.append_pair("rows", &rows.to_string());
            }
            if let Some(s) = spawn {
                if !s.cwd.as_os_str().is_empty() {
                    q.append_pair("cwd", &absolute(&s.cwd.display().to_string()));
                }
                for (k, v) in &s.env {
                    q.append_pair("env", &format!("{k}={v}"));
                }
            }
            q.append_pair("detachable", "true");
        }
        u
    }

    fn tty_url(&self, name: &str, spawn: &Spawn, size: (u16, u16)) -> Url {
        let argv: Vec<&str> =
            std::iter::once(spawn.program.as_str()).chain(spawn.args.iter().map(String::as_str)).collect();
        self.exec_url(name, &argv, Some(size), Some(spawn))
    }

    fn attach_url(&self, name: &str, session: &str, offset: u64) -> Url {
        let mut u = self.ws_url(&format!("/{name}/exec/sessions/{session}"));
        u.query_pairs_mut().append_pair("offset", &offset.to_string());
        u
    }

    async fn signal(&self, name: &str, session: &str, signal: &str) -> anyhow::Result<()> {
        let u = self.url(&format!("/{name}/exec/sessions/{session}/signal"));
        let r = self.http.post(u).bearer_auth(&self.key).json(&json!({ "signal": signal })).send().await?;
        if !r.status().is_success() {
            return Err(failed(&format!("signalling {session} in {name}"), r).await);
        }
        Ok(())
    }

    async fn do_pipe(&self, name: &str, begin: PipeBegin) -> anyhow::Result<Pipe> {
        let (url, mut want) = match begin {
            PipeBegin::New { argv } => {
                let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                (self.exec_url(name, &argv, None, None), 0)
            }
            PipeBegin::Resume { session, received } => (self.attach_url(name, &session, received), received),
        };
        let mut ws = self.connect(url).await.map_err(|e| anyhow::anyhow!("exec in {name}: {e}"))?;
        let (ev_tx, events) = mpsc::unbounded_channel();
        let (stdin, mut in_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            let mut skip = 0u64;
            loop {
                tokio::select! {
                    m = ws.next() => {
                        let ev = match m {
                            Some(Ok(Message::Binary(b))) if b.len() > 1 => {
                                let data = trim(&b[1..], &mut skip);
                                if data.is_empty() {
                                    continue;
                                }
                                match b[0] {
                                    1 => PipeEvent::Stdout(data.to_vec()),
                                    2 => PipeEvent::Stderr(data.to_vec()),
                                    _ => continue,
                                }
                            }
                            Some(Ok(Message::Text(t))) => {
                                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                                match v["type"].as_str() {
                                    Some("started") => PipeEvent::Session(v["session"].as_str().unwrap_or_default().to_owned()),
                                    Some("attached") => {
                                        // A replay from before what we have
                                        // repeats; one from after it lost
                                        // what was in between.
                                        let at = v["offset"].as_u64().unwrap_or(want);
                                        skip = want.saturating_sub(at);
                                        want = want.max(at);
                                        PipeEvent::Session(v["session"].as_str().unwrap_or_default().to_owned())
                                    }
                                    Some("exit") => PipeEvent::Exited(v["code"].as_i64().map(|c| c as i32)),
                                    Some("error") => {
                                        let why = v["error"]["message"].as_str().unwrap_or("couldn't start");
                                        let _ = ev_tx.send(PipeEvent::Stderr(format!("loam: {why}\n").into_bytes()));
                                        PipeEvent::Exited(None)
                                    }
                                    _ => continue,
                                }
                            }
                            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                            Some(Ok(_)) => continue,
                        };
                        if ev_tx.send(ev).is_err() {
                            let _ = ws.close(None).await;
                            return;
                        }
                    }
                    data = in_rx.recv() => match data {
                        Some(d) => {
                            let mut f = Vec::with_capacity(d.len() + 1);
                            f.push(0u8);
                            f.extend_from_slice(&d);
                            let _ = ws.send(Message::Binary(f.into())).await;
                        }
                        // Let go: detach, leaving the process running.
                        None => {
                            let _ = ws.close(None).await;
                            return;
                        }
                    },
                }
            }
        });
        Ok(Pipe { events, stdin })
    }

    async fn do_dial(&self, name: &str, port: u16) -> io::Result<Conn> {
        let url = self.ws_url(&format!("/{name}/ports/{port}/connect"));
        let ws = match tokio::time::timeout(DIAL_TIMEOUT, self.connect(url)).await {
            Ok(Ok(ws)) => ws,
            Ok(Err(Refused::Api { status: 502, message, .. })) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("nothing is answering on port {port}: {message}"),
                ));
            }
            Ok(Err(Refused::Api { status: 404, .. })) => {
                return Err(io::Error::new(io::ErrorKind::NotFound, format!("machine {name} is gone")));
            }
            Ok(Err(e)) => return Err(io::Error::other(format!("loam port stream: {e}"))),
            Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{name}:{port} didn't answer"))),
        };
        Ok(bridge(ws))
    }

    fn fs_url(&self, name: &str, op: &str, path: &str) -> Url {
        let mut u = self.url(&format!("/{name}/fs/{op}"));
        u.query_pairs_mut().append_pair("path", path);
        u
    }

    async fn fs_failed(&self, name: &str, path: &str, r: reqwest::Response) -> anyhow::Error {
        use crate::fs::FsError;
        let status = r.status();
        let (code, message) = api_error(&r.bytes().await.unwrap_or_default());
        match status {
            // loam says `not_found` for the sandbox and for the path alike.
            StatusCode::NOT_FOUND if matches!(self.record(name).await, Ok(None)) => {
                FsError::Unavailable(format!("{name}: no such sandbox")).into()
            }
            StatusCode::NOT_FOUND => FsError::NotFound(format!("{path}: {message}")).into(),
            StatusCode::FORBIDDEN => FsError::Denied(format!("{path}: {message}")).into(),
            StatusCode::BAD_REQUEST => FsError::Bad(format!("{path}: {message}")).into(),
            s => FsError::Unavailable(format!("{name}: {s} {code} {message}")).into(),
        }
    }

    async fn do_write(&self, name: &str, path: &str, data: Vec<u8>, mode: u32) -> anyhow::Result<()> {
        let path = absolute(path);
        let mut u = self.fs_url(name, "file", &path);
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("mode", &format!("{mode:04o}")).append_pair("mkdir", "true");
            // What goes in the home directory is the sandbox user's, as it
            // would be had they written it; anything else is root's.
            if path.starts_with(&format!("{HOME}/")) {
                q.append_pair("user", USER);
            }
        }
        let r = self.slow.put(u).bearer_auth(&self.key).body(data).send().await?;
        if !r.status().is_success() {
            return Err(failed(&format!("writing {path} in {name}"), r).await);
        }
        Ok(())
    }

    async fn do_fs_list(&self, name: &str, path: &str) -> anyhow::Result<FsList> {
        let path = absolute(path);
        let r = self.http.get(self.fs_url(name, "stat", &path)).bearer_auth(&self.key).send().await?;
        if !r.status().is_success() {
            return Err(self.fs_failed(name, &path, r).await);
        }
        let stat: Stat = r.json().await?;
        let resolved = stat.path.clone();
        let parent = std::path::Path::new(&resolved).parent().map(|p| p.display().to_string());
        if stat.kind != "dir" {
            return Ok(FsList { path: resolved, parent, entries: vec![stat.into_entry()], truncated: false });
        }
        let r = self.http.get(self.fs_url(name, "list", &path)).bearer_auth(&self.key).send().await?;
        if !r.status().is_success() {
            return Err(self.fs_failed(name, &path, r).await);
        }
        #[derive(Deserialize)]
        struct Entries {
            #[serde(default)]
            entries: Vec<Stat>,
        }
        let list: Entries = r.json().await?;
        let mut entries: Vec<FsEntry> = list.entries.into_iter().map(Stat::into_entry).collect();
        let truncated = entries.len() > arugula_proto::fs::LIST_MAX;
        entries.truncate(arugula_proto::fs::LIST_MAX);
        Ok(FsList { path: resolved, parent, entries, truncated })
    }

    async fn do_fs_read(&self, name: &str, path: &str, offset: u64, len: u64) -> anyhow::Result<(Vec<u8>, u64)> {
        let path = absolute(path);
        let mut r = self.slow.get(self.fs_url(name, "file", &path)).bearer_auth(&self.key).send().await?;
        if !r.status().is_success() {
            return Err(self.fs_failed(name, &path, r).await);
        }
        let size = r.content_length().unwrap_or(0);
        let (mut skip, mut out) = (offset, Vec::new());
        let read = async {
            // Dropping the response mid-file stops the agent's copy.
            while (out.len() as u64) < len
                && let Some(chunk) = r.chunk().await?
            {
                let rest = trim(&chunk, &mut skip);
                let want = (len - out.len() as u64) as usize;
                out.extend_from_slice(&rest[..rest.len().min(want)]);
            }
            anyhow::Ok(())
        };
        tokio::time::timeout(Duration::from_secs(60), read).await??;
        Ok((out, size))
    }

    async fn do_put_service(&self, name: &str, service: &str, def: &ServiceDef) -> anyhow::Result<()> {
        let url = self.url(&format!("/{name}/services/{service}"));
        let cmd: Vec<&str> = std::iter::once(def.cmd.as_str()).chain(def.args.iter().map(String::as_str)).collect();
        let env: serde_json::Map<String, Value> = def.env.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect();
        let mut body = json!({ "cmd": cmd, "env": env });
        if let Some(d) = &def.dir {
            body["cwd"] = absolute(d).into();
        }
        let r = self.slow.put(url.clone()).bearer_auth(&self.key).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(failed(&format!("starting service {service} in {name}"), r).await);
        }
        // Long enough to see it crash at once, short enough not to wait.
        tokio::time::sleep(Duration::from_secs(2)).await;
        let r = self.http.get(url).bearer_auth(&self.key).send().await?;
        if !r.status().is_success() {
            return Err(failed(&format!("service {service} in {name}"), r).await);
        }
        let v: Value = r.json().await?;
        if v["state"] == "crashed" {
            let mut logs = self.url(&format!("/{name}/services/{service}/logs"));
            logs.query_pairs_mut().append_pair("tail", "5");
            let tail = match self.http.get(logs).bearer_auth(&self.key).send().await {
                Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
                _ => String::new(),
            };
            anyhow::bail!("service {service} in {name} crashed: {}", tail.trim());
        }
        Ok(())
    }

    async fn do_delete_service(&self, name: &str, service: &str) -> anyhow::Result<()> {
        let r =
            self.http.delete(self.url(&format!("/{name}/services/{service}"))).bearer_auth(&self.key).send().await?;
        match r.status() {
            s if s.is_success() || s == StatusCode::NOT_FOUND => Ok(()),
            _ => Err(failed(&format!("removing service {service} from {name}"), r).await),
        }
    }
}

impl Provider for Loam {
    fn name(&self) -> &str {
        "loam"
    }

    fn caps(&self) -> &Caps {
        &self.caps
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Sandbox>>> {
        async move {
            let r = self.http.get(self.url("")).bearer_auth(&self.key).send().await?;
            if !r.status().is_success() {
                return Err(failed("listing sandboxes", r).await);
            }
            #[derive(Deserialize)]
            struct List {
                #[serde(default)]
                sandboxes: Vec<Record>,
            }
            let list: List = r.json().await?;
            Ok(list.sandboxes.into_iter().filter(|s| s.name.starts_with(prefix)).map(Record::sandbox).collect())
        }
        .boxed()
    }

    fn status<'a>(&'a self, name: &'a str) -> BoxFuture<'a, anyhow::Result<Option<Sandbox>>> {
        async move { Ok(self.record(name).await?.map(Record::sandbox)) }.boxed()
    }

    fn create<'a>(&'a self, name: &'a str, image: Option<&'a str>) -> BoxFuture<'a, anyhow::Result<()>> {
        self.do_create(name, image).boxed()
    }

    fn delete<'a>(&'a self, name: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        async move {
            let r = self.http.delete(self.url(&format!("/{name}"))).bearer_auth(&self.key).send().await?;
            match r.status() {
                s if s.is_success() || s == StatusCode::NOT_FOUND => Ok(()),
                _ => Err(failed(&format!("deleting {name}"), r).await),
            }
        }
        .boxed()
    }

    fn wake<'a>(&'a self, name: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        async move {
            let r = self
                .slow
                .post(self.url(&format!("/{name}/wake")))
                .bearer_auth(&self.key)
                .timeout(CREATE_REQUEST)
                .send()
                .await?;
            if !r.status().is_success() {
                return Err(failed(&format!("waking {name}"), r).await);
            }
            Ok(())
        }
        .boxed()
    }

    fn dial<'a>(&'a self, name: &'a str, port: u16) -> BoxFuture<'a, io::Result<Conn>> {
        self.do_dial(name, port).boxed()
    }

    fn exec_tty(
        self: Arc<Self>,
        rt: &tokio::runtime::Handle,
        name: String,
        begin: Begin,
        size: (u16, u16),
        sink: ExecSink,
    ) -> Exec {
        let (tx, rx) = mpsc::unbounded_channel();
        rt.spawn(drive(self, name, begin, size, rx, sink));
        Exec { tx }
    }

    fn kill<'a>(&'a self, name: &'a str, session: &'a str, signal: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        self.signal(name, session, signal).boxed()
    }

    fn run<'a>(&'a self, name: &'a str, argv: &'a [&'a str]) -> BoxFuture<'a, anyhow::Result<(Vec<u8>, Option<i32>)>> {
        self.do_run(name, argv).boxed()
    }

    fn pipe<'a>(&'a self, name: &'a str, begin: PipeBegin) -> BoxFuture<'a, anyhow::Result<Pipe>> {
        self.do_pipe(name, begin).boxed()
    }

    fn write_file<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
        data: Vec<u8>,
        mode: u32,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        self.do_write(name, path, data, mode).boxed()
    }

    fn put_service<'a>(
        &'a self,
        name: &'a str,
        service: &'a str,
        def: &'a ServiceDef,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        self.do_put_service(name, service, def).boxed()
    }

    fn delete_service<'a>(&'a self, name: &'a str, service: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        self.do_delete_service(name, service).boxed()
    }

    fn fs_list<'a>(&'a self, name: &'a str, path: &'a str) -> BoxFuture<'a, anyhow::Result<FsList>> {
        self.do_fs_list(name, path).boxed()
    }

    fn fs_read<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
        offset: u64,
        len: u64,
    ) -> BoxFuture<'a, anyhow::Result<(Vec<u8>, u64)>> {
        self.do_fs_read(name, path, offset, len).boxed()
    }
}

/// How an attached spell on a session ended.
enum Off {
    /// The socket dropped without an `exit`: a pause, a blip, or the
    /// sandbox went.
    Dropped,
    /// Quiet for long enough: we let go so the sandbox can pause.
    Quiet,
}

async fn send_resize(ws: &mut Ws, (cols, rows): (u16, u16)) {
    let m = json!({"type": "resize", "cols": cols, "rows": rows});
    let _ = ws.send(Message::Text(m.to_string().into())).await;
}

async fn send_input(ws: &mut Ws, d: &[u8]) {
    let mut f = Vec::with_capacity(d.len() + 1);
    f.push(0u8);
    f.extend_from_slice(d);
    let _ = ws.send(Message::Binary(f.into())).await;
}

/// A VM pane's terminal: attached while it's in use, let go while it's
/// quiet (see the module's comment), and reattached from the byte count
/// the pane has.
async fn drive(
    lo: Arc<Loam>,
    sandbox: String,
    begin: Begin,
    mut size: (u16, u16),
    mut rx: mpsc::UnboundedReceiver<ExecInput>,
    sink: ExecSink,
) {
    let (mut session, mut received, mut fresh) = match begin {
        Begin::New { spawn, image, create } => {
            // New, or lost with its sandbox: (re)create it, if it's ours to
            // create. loam may not be answering yet (a network blip, a
            // rollout); wait.
            let mut tries = 0;
            let made = loop {
                match lo.record(&sandbox).await {
                    Ok(Some(_)) => break Ok(()),
                    Ok(None) if create => break lo.do_create(&sandbox, image.as_deref()).await,
                    Ok(None) => break Err(anyhow::anyhow!("{sandbox} doesn't exist")),
                    Err(e) if tries >= PATIENCE => break Err(e),
                    Err(_) => {
                        tries += 1;
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            };
            if let Err(e) = made {
                warn!(sandbox, error = %e, "can't reach machine");
                sink(ExecEvent::Lost { machine_gone: true });
                return;
            }
            (None, 0, Some(lo.tty_url(&sandbox, &spawn, size)))
        }
        Begin::Resume { session, received } => (Some(session), received, None),
    };
    // The session's offset is `received + skew`: more than the pane's count
    // if output was lost to the replay limit while nothing was attached.
    let mut skew = 0u64;
    // What was typed while detached, sent once reattached.
    let mut pending: Vec<Vec<u8>> = Vec::new();
    let mut hung_up = false;
    let mut may_detach = true;
    let mut failures = 0u32;
    loop {
        let url = match (&session, &fresh) {
            (Some(id), _) => lo.attach_url(&sandbox, id, received + skew),
            (None, Some(u)) => u.clone(),
            (None, None) => {
                sink(ExecEvent::Lost { machine_gone: false });
                return;
            }
        };
        let mut ws = match lo.connect(url).await {
            Ok(ws) => ws,
            Err(e) => {
                match lo.record(&sandbox).await {
                    Ok(None) => {
                        sink(ExecEvent::Lost { machine_gone: true });
                        return;
                    }
                    // The sandbox is there and says no: the session is gone
                    // (a stop, a restore) or the command can't be run.
                    Ok(Some(_)) if matches!(e, Refused::Api { status: 400 | 404 | 409, .. }) => {
                        info!(sandbox, error = %e, "session refused");
                        if session.is_none() {
                            sink(ExecEvent::Output(format!("\r\n\x1b[2m[loam: {e}]\x1b[0m\r\n").into_bytes()));
                        }
                        sink(ExecEvent::Lost { machine_gone: false });
                        return;
                    }
                    _ if failures >= PATIENCE => {
                        warn!(sandbox, error = %e, "can't reach the session");
                        sink(ExecEvent::Lost { machine_gone: false });
                        return;
                    }
                    _ => {}
                }
                failures += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        failures = 0;
        // Where the replay should start, and how much of it repeats.
        let want = received + skew;
        let mut skip = 0u64;
        let mut last = Instant::now();
        let off = loop {
            let quiet = tokio::time::sleep_until((last + lo.detach_after).into());
            tokio::select! {
                m = ws.next() => match m {
                    Some(Ok(Message::Binary(b))) if b.len() > 1 && (b[0] == 1 || b[0] == 2) => {
                        let data = trim(&b[1..], &mut skip);
                        if data.is_empty() {
                            continue;
                        }
                        received += data.len() as u64;
                        last = Instant::now();
                        if !sink(ExecEvent::Output(data.to_vec())) {
                            return;
                        }
                    }
                    Some(Ok(Message::Text(t))) => {
                        let v: Value = serde_json::from_str(&t).unwrap_or_default();
                        match v["type"].as_str() {
                            Some(kind @ ("started" | "attached")) => {
                                let id = v["session"].as_str().unwrap_or_default().to_owned();
                                if kind == "attached" {
                                    let at = v["offset"].as_u64().unwrap_or(want);
                                    if at > want {
                                        info!(sandbox, lost = at - want, "output went past the replay while detached");
                                        skew += at - want;
                                    }
                                    skip = want.saturating_sub(at);
                                }
                                session = Some(id.clone());
                                fresh = None;
                                sink(ExecEvent::Session(id));
                                // The size may have changed while we were
                                // away (or since the URL was made).
                                send_resize(&mut ws, size).await;
                                for d in pending.drain(..) {
                                    send_input(&mut ws, &d).await;
                                }
                            }
                            Some("exit") => {
                                sink(ExecEvent::Exited(v["code"].as_i64().map(|c| c as i32)));
                                return;
                            }
                            Some("error") => {
                                let why = v["error"]["message"].as_str().unwrap_or("the command couldn't start");
                                sink(ExecEvent::Output(format!("\r\n\x1b[2m[loam: {why}]\x1b[0m\r\n").into_bytes()));
                                sink(ExecEvent::Exited(None));
                                return;
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break Off::Dropped,
                    Some(Ok(_)) => {}
                },
                i = rx.recv() => match i {
                    Some(ExecInput::Data(d)) => {
                        last = Instant::now();
                        send_input(&mut ws, &d).await;
                    }
                    Some(ExecInput::Resize(c, r)) => {
                        size = (c, r);
                        send_resize(&mut ws, size).await;
                    }
                    Some(ExecInput::HangUp) if !hung_up => {
                        hung_up = true;
                        let m = json!({"type": "signal", "signal": "HUP"});
                        let _ = ws.send(Message::Text(m.to_string().into())).await;
                    }
                    Some(ExecInput::HangUp) => {}
                    // The pane let go: detach, leaving the session running.
                    None => {
                        let _ = ws.close(None).await;
                        return;
                    }
                },
                _ = quiet, if may_detach && session.is_some() => {
                    // A sandbox that never idles gains nothing from it.
                    if let Ok(Some(r)) = lo.record(&sandbox).await && r.idle.action == "none" {
                        may_detach = false;
                        continue;
                    }
                    let _ = ws.close(None).await;
                    break Off::Quiet;
                }
            }
        };
        if let Off::Dropped = off {
            // Dropped without an exit: is the machine still there, and awake?
            match lo.record(&sandbox).await {
                Ok(None) => {
                    sink(ExecEvent::Lost { machine_gone: true });
                    return;
                }
                Ok(Some(_)) if session.is_none() => {
                    sink(ExecEvent::Lost { machine_gone: false });
                    return;
                }
                // Paused or stopped under us: don't wake it again until
                // the pane is used.
                Ok(Some(r)) if r.asleep() => {
                    info!(sandbox, state = %r.state, "sandbox went to sleep; waiting for input")
                }
                _ => {
                    info!(sandbox, received, "exec dropped; reattaching");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            }
        } else {
            info!(sandbox, "pane quiet; let go of its session so the sandbox can pause");
        }
        // Detached: wait for the pane to be used, or for the sandbox to turn
        // out busy.
        let since = Instant::now();
        loop {
            tokio::select! {
                i = rx.recv() => match i {
                    Some(ExecInput::Data(d)) => {
                        pending.push(d);
                        break;
                    }
                    Some(ExecInput::Resize(c, r)) => {
                        size = (c, r);
                        break;
                    }
                    Some(ExecInput::HangUp) => {
                        if !hung_up && let Some(id) = &session {
                            hung_up = true;
                            if let Err(e) = lo.signal(&sandbox, id, "HUP").await {
                                info!(sandbox, error = %e, "hang up");
                            }
                        }
                        // Reattach to hear how it ended.
                        break;
                    }
                    None => return,
                },
                _ = tokio::time::sleep(lo.poll) => match lo.record(&sandbox).await {
                    Ok(None) => {
                        sink(ExecEvent::Lost { machine_gone: true });
                        return;
                    }
                    Ok(Some(r)) if r.state == "running"
                        && since.elapsed() > Duration::from_secs(r.idle.timeout_s) + lo.poll * 2 =>
                    {
                        info!(sandbox, "sandbox still awake; following the session again");
                        break;
                    }
                    _ => {}
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Against a fake loam: the routes the adapter uses, with sessions that
    //! echo what's typed, keep their output for replay, and close with 1011
    //! when their sandbox pauses or goes.

    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{
        Json, Router,
        body::Bytes,
        extract::{
            Path as UrlPath, Query, RawQuery, State, WebSocketUpgrade,
            ws::{CloseFrame, Message as WsMsg, WebSocket},
        },
        http::StatusCode as Code,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use tokio::sync::broadcast;

    use super::*;

    #[derive(Clone, Debug)]
    enum Ev {
        Out(Vec<u8>),
        Exit(i32),
    }

    struct Sess {
        out: Vec<u8>,
        exit: Option<i32>,
        tx: broadcast::Sender<Ev>,
        attached: usize,
    }

    struct Sb {
        state: String,
        idle_action: String,
        labels: Value,
        sessions: HashMap<String, Sess>,
        kick: broadcast::Sender<()>,
    }

    #[derive(Default)]
    struct Fake {
        sandboxes: HashMap<String, Sb>,
        created: Vec<Value>,
        execs: Vec<Vec<(String, String)>>,
        writes: Vec<(HashMap<String, String>, Vec<u8>)>,
        wakes: u32,
        next: u32,
    }

    type F = Arc<Mutex<Fake>>;

    fn err(status: Code, code: &str) -> Response {
        (status, Json(json!({"error": {"code": code, "message": format!("{code} (fake)")}}))).into_response()
    }

    fn sandbox(state: &str) -> Sb {
        Sb {
            state: state.into(),
            idle_action: "pause".into(),
            labels: json!({}),
            sessions: HashMap::new(),
            kick: broadcast::channel(4).0,
        }
    }

    fn record(name: &str, s: &Sb) -> Value {
        json!({"id": "sb-x", "name": name, "state": s.state, "labels": s.labels,
               "idle": {"timeout_s": 1, "action": s.idle_action}})
    }

    /// Every sub-API call wakes the sandbox first, as loam's do.
    #[allow(clippy::result_large_err)]
    fn acquire(f: &F, sb: &str) -> Result<(), Response> {
        let mut g = f.lock().unwrap();
        let fake = &mut *g;
        let s = fake.sandboxes.get_mut(sb).ok_or_else(|| err(Code::NOT_FOUND, "not_found"))?;
        if s.state != "running" {
            s.state = "running".into();
            fake.wakes += 1;
        }
        Ok(())
    }

    fn say(f: &F, sb: &str, id: &str, data: &[u8]) {
        let mut g = f.lock().unwrap();
        let s = g.sandboxes.get_mut(sb).unwrap().sessions.get_mut(id).unwrap();
        s.out.extend_from_slice(data);
        let _ = s.tx.send(Ev::Out(data.to_vec()));
    }

    fn finish(f: &F, sb: &str, id: &str, code: i32) {
        let mut g = f.lock().unwrap();
        let s = g.sandboxes.get_mut(sb).unwrap().sessions.get_mut(id).unwrap();
        s.exit = Some(code);
        let _ = s.tx.send(Ev::Exit(code));
    }

    fn put_to_sleep(f: &F, sb: &str, state: &str) {
        let mut g = f.lock().unwrap();
        let s = g.sandboxes.get_mut(sb).unwrap();
        s.state = state.into();
        let _ = s.kick.send(());
    }

    fn attached(f: &F, sb: &str) -> usize {
        f.lock().unwrap().sandboxes.get(sb).map_or(0, |s| s.sessions.values().map(|x| x.attached).sum())
    }

    /// One socket on a session: the replay from `from`, then live output,
    /// until the session exits or its sandbox pauses or goes (1011).
    async fn serve(f: F, sb: String, id: String, mut ws: WebSocket, first: Value, from: usize) {
        let (mut evs, mut kick, replay, exit) = {
            let mut g = f.lock().unwrap();
            let s = g.sandboxes.get_mut(&sb).unwrap();
            let kick = s.kick.subscribe();
            let x = s.sessions.get_mut(&id).unwrap();
            x.attached += 1;
            (x.tx.subscribe(), kick, x.out[from.min(x.out.len())..].to_vec(), x.exit)
        };
        let _ = ws.send(WsMsg::Text(first.to_string().into())).await;
        let frame = |d: &[u8]| WsMsg::Binary([&[1u8][..], d].concat().into());
        if !replay.is_empty() {
            let _ = ws.send(frame(&replay)).await;
        }
        let exit_frame = |c: i32| WsMsg::Text(json!({"type": "exit", "code": c}).to_string().into());
        if let Some(c) = exit {
            let _ = ws.send(exit_frame(c)).await;
        } else {
            loop {
                tokio::select! {
                    ev = evs.recv() => match ev {
                        Ok(Ev::Out(d)) => { let _ = ws.send(frame(&d)).await; }
                        Ok(Ev::Exit(c)) => { let _ = ws.send(exit_frame(c)).await; break; }
                        Err(_) => break,
                    },
                    _ = kick.recv() => {
                        let _ = ws.send(WsMsg::Close(Some(CloseFrame { code: 1011, reason: "gone".into() }))).await;
                        break;
                    }
                    m = ws.recv() => match m {
                        Some(Ok(WsMsg::Binary(b))) if b.first() == Some(&0) => {
                            let input = b[1..].to_vec();
                            if input.starts_with(b"exit") {
                                finish(&f, &sb, &id, 0);
                            } else {
                                // A terminal echoes what's typed.
                                say(&f, &sb, &id, &input);
                            }
                        }
                        Some(Ok(WsMsg::Text(t))) => {
                            let v: Value = serde_json::from_str(&t).unwrap_or_default();
                            if v["type"] == "signal" && v["signal"] == "HUP" {
                                finish(&f, &sb, &id, 129);
                            }
                        }
                        Some(Ok(WsMsg::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(_)) => {}
                    },
                }
            }
        }
        if let Some(x) = f.lock().unwrap().sandboxes.get_mut(&sb).and_then(|s| s.sessions.get_mut(&id)) {
            x.attached -= 1;
        }
    }

    fn new_session(f: &F, sb: &str) -> String {
        let mut g = f.lock().unwrap();
        g.next += 1;
        let id = format!("s-{}", g.next);
        let s = Sess { out: Vec::new(), exit: None, tx: broadcast::channel(64).0, attached: 0 };
        g.sandboxes.get_mut(sb).unwrap().sessions.insert(id.clone(), s);
        id
    }

    async fn fake() -> (String, F) {
        type S = State<F>;
        let f: F = Arc::default();
        let app = Router::new()
            .route(
                "/v1/sandboxes",
                post(|State(f): S, Json(b): Json<Value>| async move {
                    let mut g = f.lock().unwrap();
                    let name = b["name"].as_str().unwrap().to_owned();
                    if g.sandboxes.contains_key(&name) {
                        return err(Code::CONFLICT, "already_exists");
                    }
                    let mut s = sandbox("running");
                    s.labels = b["labels"].clone();
                    let out = record(&name, &s);
                    g.sandboxes.insert(name, s);
                    g.created.push(b);
                    (Code::CREATED, Json(out)).into_response()
                })
                .get(|State(f): S| async move {
                    let g = f.lock().unwrap();
                    let all: Vec<Value> = g.sandboxes.iter().map(|(n, s)| record(n, s)).collect();
                    Json(json!({"sandboxes": all})).into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}",
                get(|State(f): S, UrlPath(sb): UrlPath<String>| async move {
                    let g = f.lock().unwrap();
                    match g.sandboxes.get(&sb) {
                        Some(s) => Json(record(&sb, s)).into_response(),
                        None => err(Code::NOT_FOUND, "not_found"),
                    }
                })
                .delete(|State(f): S, UrlPath(sb): UrlPath<String>| async move {
                    match f.lock().unwrap().sandboxes.remove(&sb) {
                        Some(s) => {
                            let _ = s.kick.send(());
                            Code::NO_CONTENT.into_response()
                        }
                        None => err(Code::NOT_FOUND, "not_found"),
                    }
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/wake",
                post(|State(f): S, UrlPath(sb): UrlPath<String>| async move {
                    acquire(&f, &sb).err().unwrap_or_else(|| Code::OK.into_response())
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/exec",
                get(|State(f): S, UrlPath(sb): UrlPath<String>, RawQuery(q): RawQuery, up: WebSocketUpgrade| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    let q: Vec<(String, String)> =
                        url::form_urlencoded::parse(q.unwrap_or_default().as_bytes()).into_owned().collect();
                    f.lock().unwrap().execs.push(q);
                    let id = new_session(&f, &sb);
                    let first = json!({"type": "started", "session": id, "pid": 7});
                    up.on_upgrade(move |ws| serve(f, sb, id, ws, first, 0)).into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/exec/sessions/{id}",
                get(|State(f): S,
                     UrlPath((sb, id)): UrlPath<(String, String)>,
                     Query(q): Query<HashMap<String, u64>>,
                     up: WebSocketUpgrade| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    if !f.lock().unwrap().sandboxes[&sb].sessions.contains_key(&id) {
                        return err(Code::NOT_FOUND, "not_found");
                    }
                    let from = q.get("offset").copied().unwrap_or(0);
                    let first = json!({"type": "attached", "session": id, "offset": from});
                    up.on_upgrade(move |ws| serve(f, sb, id, ws, first, from as usize)).into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/exec/sessions/{id}/signal",
                post(|State(f): S, UrlPath((sb, id)): UrlPath<(String, String)>, Json(b): Json<Value>| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    if b["signal"] == "HUP" {
                        finish(&f, &sb, &id, 129);
                    }
                    Code::NO_CONTENT.into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/exec/run",
                post(|State(f): S, UrlPath(sb): UrlPath<String>, Json(b): Json<Value>| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    let argv: Vec<&str> = b["cmd"].as_array().unwrap().iter().filter_map(|a| a.as_str()).collect();
                    let out = B64.encode(argv.join(" "));
                    Json(json!({"exit_code": 3, "stdout": out, "stderr": "", "timed_out": false})).into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/fs/stat",
                get(|State(f): S, UrlPath(sb): UrlPath<String>, Query(q): Query<HashMap<String, String>>| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    let p = q["path"].clone();
                    let kind = match p.as_str() {
                        "/home/loam" => "dir",
                        "/home/loam/notes.txt" => "file",
                        _ => return err(Code::NOT_FOUND, "not_found"),
                    };
                    let name = p.rsplit('/').next().unwrap().to_owned();
                    Json(json!({"path": p, "name": name, "type": kind, "size": 11, "mode": "0644",
                                "mtime": "1970-01-01T00:00:01Z"}))
                    .into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/fs/list",
                get(|State(f): S, UrlPath(sb): UrlPath<String>| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    Json(json!({"entries": [
                        {"path": "/home/loam/notes.txt", "name": "notes.txt", "type": "file", "size": 11, "mode": "0644", "mtime": "1970-01-01T00:00:01Z"},
                        {"path": "/home/loam/src", "name": "src", "type": "dir", "size": 0, "mode": "0755", "mtime": "1970-01-01T00:00:02Z"},
                        {"path": "/home/loam/l", "name": "l", "type": "symlink", "size": 3, "mode": "0777", "mtime": "1970-01-01T00:00:03Z", "target": "src"},
                    ]}))
                    .into_response()
                }),
            )
            .route(
                "/v1/sandboxes/{sb}/fs/file",
                get(|State(f): S, UrlPath(sb): UrlPath<String>, Query(q): Query<HashMap<String, String>>| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    match q["path"].as_str() {
                        "/home/loam/notes.txt" => b"hello world".to_vec().into_response(),
                        "/root/secret" => err(Code::FORBIDDEN, "permission_denied"),
                        _ => err(Code::NOT_FOUND, "not_found"),
                    }
                })
                .put(
                    |State(f): S,
                     UrlPath(sb): UrlPath<String>,
                     Query(q): Query<HashMap<String, String>>,
                     body: Bytes| async move {
                        if let Err(r) = acquire(&f, &sb) {
                            return r;
                        }
                        f.lock().unwrap().writes.push((q, body.to_vec()));
                        Json(json!({})).into_response()
                    },
                ),
            )
            .route(
                "/v1/sandboxes/{sb}/ports/{port}/connect",
                get(|State(f): S, UrlPath((sb, port)): UrlPath<(String, u16)>, up: WebSocketUpgrade| async move {
                    if let Err(r) = acquire(&f, &sb) {
                        return r;
                    }
                    if port != 8080 {
                        return err(Code::BAD_GATEWAY, "port_closed");
                    }
                    // An echo server.
                    up.on_upgrade(|mut ws| async move {
                        while let Some(Ok(m)) = ws.recv().await {
                            if let WsMsg::Binary(b) = m
                                && ws.send(WsMsg::Binary(b)).await.is_err()
                            {
                                break;
                            }
                        }
                    })
                    .into_response()
                }),
            )
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(axum::serve(l, app).into_future());
        (base, f)
    }

    /// What a pane sees: events in order, on a channel.
    fn pane(lo: &Arc<Loam>, sb: &str, begin: Begin) -> (Exec, mpsc::UnboundedReceiver<ExecEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let exec = lo.clone().exec_tty(
            &tokio::runtime::Handle::current(),
            sb.into(),
            begin,
            (80, 24),
            Box::new(move |e| tx.send(e).is_ok()),
        );
        (exec, rx)
    }

    fn shell() -> Begin {
        let spawn = Spawn {
            program: "bash".into(),
            args: vec!["-l".into()],
            cwd: "src".into(),
            env: vec![("A".into(), "1".into())],
        };
        Begin::New { spawn, image: None, create: true }
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<ExecEvent>) -> ExecEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("an event in time")
            .expect("the pane's events")
    }

    /// Output until `n` bytes have come, as text.
    async fn output(rx: &mut mpsc::UnboundedReceiver<ExecEvent>, n: usize) -> String {
        let mut got = Vec::new();
        while got.len() < n {
            match next(rx).await {
                ExecEvent::Output(d) => got.extend(d),
                ExecEvent::Session(_) => {}
                e => panic!("{e:?} before {n} bytes of output"),
            }
        }
        String::from_utf8(got).unwrap()
    }

    async fn session(rx: &mut mpsc::UnboundedReceiver<ExecEvent>) -> String {
        match next(rx).await {
            ExecEvent::Session(s) => s,
            e => panic!("{e:?} instead of the session"),
        }
    }

    async fn until(what: &str, ok: impl Fn() -> bool) {
        for _ in 0..200 {
            if ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{what}: not in time");
    }

    #[tokio::test]
    async fn a_pane_detaches_and_resumes_from_its_byte_count() {
        let (base, f) = fake().await;
        let lo = Arc::new(Loam::new(&base, "loam_k".into()).unwrap());
        let (exec, mut rx) = pane(&lo, "illogical-eph-ab-1", shell());
        let id = session(&mut rx).await;
        {
            let g = f.lock().unwrap();
            // Made with our label and idle rule, from the default template.
            let made = &g.created[0];
            assert_eq!(made["labels"]["owner"], "arugulad");
            assert_eq!(made["idle"], json!({"timeout_s": 300, "action": "pause"}));
            assert!(made.get("image").is_none() && made.get("template").is_none());
            let q = &g.execs[0];
            let has = |k: &str, v: &str| q.iter().any(|(a, b)| a == k && b == v);
            assert!(has("cmd", "bash") && has("cmd", "-l") && has("tty", "true") && has("detachable", "true"));
            assert!(has("cwd", "/home/loam/src") && has("env", "A=1") && has("cols", "80"));
        }
        exec.input(b"hello".to_vec());
        assert_eq!(output(&mut rx, 5).await, "hello");
        // The pane lets go (a daemon restart): the session carries on.
        drop(exec);
        until("detached", || attached(&f, "illogical-eph-ab-1") == 0).await;
        say(&f, "illogical-eph-ab-1", &id, b" while away");
        let (exec, mut rx) = pane(&lo, "illogical-eph-ab-1", Begin::Resume { session: id.clone(), received: 5 });
        assert_eq!(session(&mut rx).await, id);
        // Only what the pane didn't have.
        assert_eq!(output(&mut rx, 11).await, " while away");
        exec.input(b"exit\r".to_vec());
        assert!(matches!(next(&mut rx).await, ExecEvent::Exited(Some(0))));
    }

    #[tokio::test]
    async fn a_quiet_pane_lets_its_sandbox_pause_and_wakes_it_when_used() {
        let (base, f) = fake().await;
        let lo = Arc::new(
            Loam::new(&base, "k".into()).unwrap().with_idle(Duration::from_millis(300), Duration::from_secs(60)),
        );
        let sb = "illogical-eph-ab-2";
        let (exec, mut rx) = pane(&lo, sb, shell());
        session(&mut rx).await;
        assert_eq!(attached(&f, sb), 1);
        // Quiet: the pane lets go, so loam's idle rule may pause it.
        until("let go", || attached(&f, sb) == 0).await;
        put_to_sleep(&f, sb, "paused");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(f.lock().unwrap().wakes, 0, "nothing wakes a sleeping pane's sandbox");
        // A keystroke wakes it, and arrives.
        exec.input(b"ls".to_vec());
        assert_eq!(output(&mut rx, 2).await, "ls");
        assert_eq!(f.lock().unwrap().wakes, 1);
        assert_eq!(attached(&f, sb), 1);
    }

    #[tokio::test]
    async fn a_pause_under_an_attached_pane_waits_for_input() {
        let (base, f) = fake().await;
        let lo = Arc::new(Loam::new(&base, "k".into()).unwrap());
        let sb = "illogical-eph-ab-3";
        let (exec, mut rx) = pane(&lo, sb, shell());
        let id = session(&mut rx).await;
        exec.input(b"a".to_vec());
        assert_eq!(output(&mut rx, 1).await, "a");
        // Someone pauses it: the socket closes with 1011.
        put_to_sleep(&f, sb, "paused");
        until("dropped", || attached(&f, sb) == 0).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let wakes = f.lock().unwrap().wakes;
        assert_eq!((wakes, attached(&f, sb)), (0, 0), "not woken again by the pane");
        // Output made before the pause comes with the reattach, once.
        say(&f, sb, &id, b"b");
        exec.resize(100, 30);
        assert_eq!(output(&mut rx, 1).await, "b");
        exec.input(b"c".to_vec());
        assert_eq!(output(&mut rx, 1).await, "c");
        assert_eq!(f.lock().unwrap().wakes, 1);
    }

    #[tokio::test]
    async fn the_machine_going_and_a_hang_up_are_told_apart() {
        let (base, f) = fake().await;
        let lo = Arc::new(Loam::new(&base, "k".into()).unwrap());
        let (exec, mut rx) = pane(&lo, "illogical-eph-ab-4", shell());
        session(&mut rx).await;
        exec.hang_up();
        assert!(matches!(next(&mut rx).await, ExecEvent::Exited(Some(129))));

        let (_exec, mut rx) = pane(&lo, "illogical-eph-ab-5", shell());
        session(&mut rx).await;
        Provider::delete(&*lo, "illogical-eph-ab-5").await.unwrap();
        assert!(matches!(next(&mut rx).await, ExecEvent::Lost { machine_gone: true }));
        // A session that isn't there any more (a stop, a restore).
        let (_exec, mut rx) = pane(&lo, "illogical-eph-ab-4", Begin::Resume { session: "s-404".into(), received: 0 });
        assert!(matches!(next(&mut rx).await, ExecEvent::Lost { machine_gone: false }));
        // Someone else's sandbox that doesn't exist is never made.
        let spawn = Spawn { program: "bash".into(), args: vec![], cwd: Default::default(), env: vec![] };
        let (_exec, mut rx) = pane(&lo, "theirs", Begin::New { spawn, image: None, create: false });
        assert!(matches!(next(&mut rx).await, ExecEvent::Lost { machine_gone: true }));
        assert!(!f.lock().unwrap().sandboxes.contains_key("theirs"));
    }

    #[tokio::test]
    async fn the_rest_of_the_provider() {
        let (base, f) = fake().await;
        let lo = Loam::new(&base, "k".into()).unwrap();
        lo.create("illogical-eph-ab-6", Some("ghcr.io/arugula-salad/box@sha256:00")).await.unwrap();
        lo.create("illogical-eph-ab-6", None).await.unwrap();
        lo.create("other", Some("base")).await.unwrap();
        {
            let g = f.lock().unwrap();
            assert_eq!(g.created[0]["image"], "ghcr.io/arugula-salad/box@sha256:00");
            assert_eq!(g.created[1]["template"], "base");
        }
        put_to_sleep(&f, "other", "stopped");
        let names: Vec<_> = lo.list("illogical-eph-").await.unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["illogical-eph-ab-6"]);
        assert_eq!(lo.status("other").await.unwrap().unwrap().status, "cold");
        assert!(lo.status("nope").await.unwrap().is_none());
        lo.wake("other").await.unwrap();
        assert_eq!(lo.status("other").await.unwrap().unwrap().status, "running");

        assert_eq!(lo.run("other", &["echo", "hi"]).await.unwrap(), (b"echo hi".to_vec(), Some(3)));

        lo.write_file("other", "/home/loam/.local/bin/x", b"#!".to_vec(), 0o755).await.unwrap();
        lo.write_file("other", "/tmp/y", b"y".to_vec(), 0o644).await.unwrap();
        {
            let g = f.lock().unwrap();
            let (q, body) = &g.writes[0];
            assert_eq!(
                (q["path"].as_str(), q["mode"].as_str(), q["mkdir"].as_str()),
                ("/home/loam/.local/bin/x", "0755", "true")
            );
            assert_eq!((q.get("user").map(String::as_str), body.as_slice()), (Some("loam"), &b"#!"[..]));
            assert!(!g.writes[1].0.contains_key("user"), "outside the home directory, as root");
        }

        let home = lo.fs_list("other", "~").await.unwrap();
        assert_eq!((home.path.as_str(), home.parent.as_deref()), ("/home/loam", Some("/home")));
        let kinds: Vec<_> = home.entries.iter().map(|e| (e.name.as_str(), e.kind)).collect();
        assert_eq!(kinds, [("notes.txt", FsKind::File), ("src", FsKind::Directory), ("l", FsKind::Symlink)]);
        assert_eq!((home.entries[0].mode, home.entries[0].mtime_ms), (0o644, 1000));
        let one = lo.fs_list("other", "notes.txt").await.unwrap();
        assert_eq!((one.path.as_str(), one.entries.len()), ("/home/loam/notes.txt", 1));
        assert_eq!(lo.fs_read("other", "~/notes.txt", 6, 3).await.unwrap(), (b"wor".to_vec(), 11));
        let e = lo.fs_read("other", "/root/secret", 0, 10).await.unwrap_err();
        assert!(matches!(e.downcast_ref::<crate::fs::FsError>(), Some(crate::fs::FsError::Denied(_))), "{e}");
        let e = lo.fs_list("other", "/nope").await.unwrap_err();
        assert!(matches!(e.downcast_ref::<crate::fs::FsError>(), Some(crate::fs::FsError::NotFound(_))), "{e}");
        let e = lo.fs_list("gone", "/").await.unwrap_err();
        assert!(matches!(e.downcast_ref::<crate::fs::FsError>(), Some(crate::fs::FsError::Unavailable(_))), "{e}");

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut c = lo.dial("other", 8080).await.unwrap();
        c.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        c.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        assert_eq!(lo.dial("other", 9).await.err().unwrap().kind(), io::ErrorKind::ConnectionRefused);
        assert_eq!(lo.dial("gone", 8080).await.err().unwrap().kind(), io::ErrorKind::NotFound);

        lo.delete("other").await.unwrap();
        lo.delete("other").await.unwrap();
        // The key never shows in debug output, and the caps say what loam does.
        assert!(!format!("{lo:?}").contains("\"k\""));
        assert_eq!((lo.name(), lo.caps().exec_replay, lo.caps().cold), ("loam", 1 << 20, Cold::Restore));
    }

    #[test]
    fn paths_are_the_sandbox_users() {
        assert_eq!(absolute("~"), "/home/loam");
        assert_eq!(absolute(""), "/home/loam");
        assert_eq!(absolute("."), "/home/loam");
        assert_eq!(absolute("~/src/app"), "/home/loam/src/app");
        assert_eq!(absolute("src"), "/home/loam/src");
        assert_eq!(absolute("/etc"), "/etc");
        assert_eq!(absolute("~other"), "/home/loam/~other");
    }
}
