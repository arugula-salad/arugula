//! nvim in a terminal, without arugula.nvim (#1730): the daemon finds the
//! nvim a pane runs and reads it over its own RPC socket.
//!
//! nvim (0.10 and later) always listens: `v:servername` is
//! `<appname>.<pid>.<n>` in `stdpath("run")`, which is `$XDG_RUNTIME_DIR`
//! when that's set and otherwise a directory of its own under
//! `${TMPDIR:-/tmp}/nvim.$USER/`. Its TUI is a process of its own since
//! 0.10, and `<pid>` is the server's, the TUI's child; a headless nvim is
//! its own server. So a pane whose foreground program is nvim has its
//! socket found from that pid ([`socket_of`]): in those directories first,
//! then in what the OS lists (`/proc/net/unix` on Linux, `lsof` on macOS),
//! which finds a socket wherever the pane's own environment put it.
//!
//! Once a second, [`watch`] looks at what each terminal here runs. A pane
//! running nvim gets a session ([`session`]): it connects, checks nvim's
//! version, and registers autocmds in nvim (`ATTACH`) that notify it of
//! the current file, the cursor and the lines around it, unsaved buffers,
//! diagnostic counts and a merge conflict, on nvim's own events, at most
//! every 100 ms. What they say goes through a [`Link`], as an editor that
//! joined would say it, so the pane's card shows it and errors after a
//! save or a conflict become a reason, as for VS Code. There's no follow
//! stream without the plugin: the link says it can't be followed. nvim
//! quitting, crashing or being killed closes the socket, and the pane
//! drops its editor at once; the next look also drops it when nvim is no
//! longer what the pane runs.
//!
//! arugula.nvim in a pane says which (`hello.pane`, `link.rs`) and takes
//! the pane's place from a session here ([`plugin_attached`]): it adds the
//! follow stream and the debugger. While it's there, nothing here
//! connects; when it leaves, the next look connects again.
//!
//! An nvim older than 0.10, or with its server stopped, stays a terminal:
//! the log says why, once for that nvim.
//!
//! [`open_in_pane`] opens a file at a line in a pane's nvim (#1731's Open
//! in editor), over the same socket, with or without the plugin.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use arugula_proto::PaneId;
use rmpv::Value as Mp;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{mpsc, oneshot},
};
use tracing::{debug, info};

use super::link::{Hello, Link};
use crate::{
    block::EditorLink,
    mux::{Api, Cmd, MuxHandle},
    procinfo,
};

/// How often [`watch`] looks at what the terminals run.
const LOOK: Duration = Duration::from_secs(1);
/// How long a new nvim has to make its socket.
const SOCKET_WAIT: Duration = Duration::from_secs(3);
/// How long nvim has to answer a request.
const ANSWER: Duration = Duration::from_secs(5);
/// The most a message from nvim may be.
const MAX_MESSAGE: usize = 16 << 20;
/// The oldest nvim read here: `vim.diagnostic.count()` and the TUI as a
/// process of its own are 0.10's.
const OLDEST: (u64, u64) = (0, 10);

/// Registered in nvim over RPC, with our channel id: autocmds that notify
/// it (`arugula`) of what the pane shows, on nvim's own events, at most
/// every 100 ms; `arugula_bye` as nvim quits.
const ATTACH: &str = r#"
local chan = ...
local g = vim.api.nvim_create_augroup("arugula_pane", { clear = true })
local ABOVE, LINES = 3, 7
local due, saved, conflict = false, false, vim.NIL
local function file_of(b)
  if vim.bo[b].buftype ~= "" then return nil end
  local n = vim.api.nvim_buf_get_name(b)
  if n == "" then return nil end
  return vim.fn.fnamemodify(n, ":p")
end
local function scan()
  conflict = vim.NIL
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(b) and file_of(b) and vim.api.nvim_buf_line_count(b) < 50000 then
      local a, m, z = false, false, false
      for _, l in ipairs(vim.api.nvim_buf_get_lines(b, 0, -1, false)) do
        if l:sub(1, 8) == "<<<<<<< " then a = true
        elseif l == "=======" then m = true
        elseif l:sub(1, 8) == ">>>>>>> " then z = true end
      end
      if a and m and z then conflict = file_of(b) return end
    end
  end
end
local function state()
  local dirty = 0
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(b) and vim.bo[b].modified and file_of(b) then dirty = dirty + 1 end
  end
  local c = vim.diagnostic.count(nil)
  local s = {
    dirty = dirty,
    diag = { e = c[1] or 0, w = c[2] or 0, i = (c[3] or 0) + (c[4] or 0) },
    conflict = conflict,
    saved = saved,
    file = vim.NIL,
  }
  local buf = vim.api.nvim_get_current_buf()
  local f = file_of(buf)
  if f then
    local cur = vim.api.nvim_win_get_cursor(0)
    local n = vim.api.nvim_buf_line_count(buf)
    local top = math.max(0, math.min(cur[1] - 1 - ABOVE, n - LINES))
    local lines = vim.api.nvim_buf_get_lines(buf, top, math.min(n, top + LINES), false)
    for i, l in ipairs(lines) do lines[i] = l:sub(1, 240) end
    s.file, s.line, s.col, s.top, s.lines = f, cur[1], cur[2] + 1, top + 1, lines
  end
  return s
end
local function send()
  due = false
  local ok, s = pcall(state)
  saved = false
  if ok then pcall(vim.rpcnotify, chan, "arugula", s) end
end
local function soon()
  if due then return end
  due = true
  vim.defer_fn(send, 100)
end
local function au(ev, f) vim.api.nvim_create_autocmd(ev, { group = g, callback = f }) end
au({ "BufEnter", "CursorMoved", "CursorMovedI", "BufModifiedSet", "TextChanged", "TextChangedI",
     "DiagnosticChanged", "BufDelete", "BufWipeout" }, soon)
au("BufReadPost", function() scan() soon() end)
au("BufWritePost", function() saved = true scan() soon() end)
au("VimLeavePre", function() pcall(vim.rpcnotify, chan, "arugula_bye") end)
scan()
send()
"#;

/// Takes the autocmds out again (arugula.nvim took the pane's place).
const DETACH: &str = r#"pcall(vim.api.nvim_del_augroup_by_name, "arugula_pane")"#;

/// Opens a file (`:drop`: its window if it's shown, else here, splitting
/// when the buffer here can't be left), at a line.
const OPEN: &str = r#"
local f, l = ...
if vim.fn.mode() ~= "n" then vim.cmd("stopinsert") end
vim.cmd("drop " .. vim.fn.fnameescape(f))
if l ~= vim.NIL and l then
  pcall(vim.api.nvim_win_set_cursor, 0, { l, 0 })
  vim.cmd("normal! zz")
end
"#;

// ---- msgpack-RPC

/// A connection to nvim's server, speaking msgpack-RPC: requests
/// (`[0, id, method, params]`) answered by id (`[1, id, error, result]`),
/// and notifications (`[2, method, params]`) handed out on a channel that
/// closes when nvim does.
pub struct Rpc {
    w: tokio::sync::Mutex<OwnedWriteHalf>,
    next: AtomicU32,
    waiting: Waiting,
}

/// Who waits for which answer.
type Answer = oneshot::Sender<Result<Value, String>>;
type Waiting = Arc<Mutex<HashMap<u32, Answer>>>;

/// A notification from nvim: its method and its (first) argument, as JSON.
pub type Note = (String, Value);

impl Rpc {
    pub async fn connect(path: &Path) -> io::Result<(Self, mpsc::UnboundedReceiver<Note>)> {
        let (r, w) = UnixStream::connect(path).await?.into_split();
        let waiting: Waiting = Default::default();
        let (tx, rx) = mpsc::unbounded_channel();
        crate::task::spawn(read(r, waiting.clone(), tx));
        Ok((Self { w: tokio::sync::Mutex::new(w), next: AtomicU32::new(1), waiting }, rx))
    }

    /// Call `method` and wait (at most [`ANSWER`]) for its result.
    pub async fn call(&self, method: &str, params: Vec<Mp>) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().unwrap().insert(id, tx);
        let msg = Mp::Array(vec![Mp::from(0), Mp::from(id), Mp::from(method), Mp::Array(params)]);
        let mut buf = Vec::new();
        rmpv::encode::write_value(&mut buf, &msg).map_err(|e| e.to_string())?;
        let sent = self.w.lock().await.write_all(&buf).await;
        if let Err(e) = sent {
            self.waiting.lock().unwrap().remove(&id);
            return Err(format!("nvim's socket: {e}"));
        }
        match tokio::time::timeout(ANSWER, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("nvim went away".into()),
            Err(_) => {
                self.waiting.lock().unwrap().remove(&id);
                Err(format!("nvim didn't answer {method}"))
            }
        }
    }

    /// Run Lua in nvim with these arguments (`...`), for what it returns.
    pub async fn lua(&self, code: &str, args: Vec<Mp>) -> Result<Value, String> {
        self.call("nvim_exec_lua", vec![Mp::from(code), Mp::Array(args)]).await
    }
}

/// What nvim sends, until it closes.
async fn read(mut r: OwnedReadHalf, waiting: Waiting, notes: mpsc::UnboundedSender<Note>) {
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 << 10];
    'read: loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        loop {
            match next_message(&mut buf) {
                Ok(Some(m)) => dispatch(m, &waiting, &notes),
                Ok(None) if buf.len() <= MAX_MESSAGE => break,
                Ok(None) => break 'read,
                Err(e) => {
                    debug!(error = %e, "nvim sent something unreadable");
                    break 'read;
                }
            }
        }
    }
    // Whoever still waits hears it went away.
    waiting.lock().unwrap().clear();
}

/// The first whole message in `buf`, taken out of it; `None` until all of
/// it has come.
fn next_message(buf: &mut Vec<u8>) -> Result<Option<Mp>, String> {
    use rmpv::decode::Error;
    let mut cur = io::Cursor::new(&buf[..]);
    match rmpv::decode::read_value(&mut cur) {
        Ok(v) => {
            let n = cur.position() as usize;
            buf.drain(..n);
            Ok(Some(v))
        }
        Err(Error::InvalidMarkerRead(e) | Error::InvalidDataRead(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn dispatch(m: Mp, waiting: &Mutex<HashMap<u32, Answer>>, notes: &mpsc::UnboundedSender<Note>) {
    let Mp::Array(a) = m else { return };
    match a.first().and_then(Mp::as_u64) {
        Some(1) if a.len() == 4 => {
            let Some(id) = a[1].as_u64() else { return };
            let Some(tx) = waiting.lock().unwrap().remove(&(id as u32)) else { return };
            let r = if a[2].is_nil() { Ok(json_of(&a[3])) } else { Err(error_text(&a[2])) };
            let _ = tx.send(r);
        }
        Some(2) if a.len() == 3 => {
            // `rpcnotify(chan, method, arg)`: its params are `[arg]`.
            let method = a[1].as_str().unwrap_or("").to_owned();
            let arg = a[2].as_array().and_then(|p| p.first()).map_or(Value::Null, json_of);
            let _ = notes.send((method, arg));
        }
        // A request to us: nvim waits for an answer we never make, so
        // nothing here asks one (`rpcrequest`).
        _ => {}
    }
}

/// nvim's error, `[type, message]`.
fn error_text(e: &Mp) -> String {
    match e {
        Mp::Array(a) if a.len() == 2 => a[1].as_str().unwrap_or("nvim refused").to_owned(),
        other => json_of(other).to_string(),
    }
}

/// A msgpack value as JSON (maps' keys as text; binary as text, lossily).
pub fn json_of(v: &Mp) -> Value {
    match v {
        Mp::Nil => Value::Null,
        Mp::Boolean(b) => Value::Bool(*b),
        Mp::Integer(i) => match (i.as_u64(), i.as_i64()) {
            (Some(u), _) => json!(u),
            (_, Some(s)) => json!(s),
            _ => Value::Null,
        },
        Mp::F32(f) => json!(f),
        Mp::F64(f) => json!(f),
        Mp::String(s) => Value::String(String::from_utf8_lossy(s.as_bytes()).into_owned()),
        Mp::Binary(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
        Mp::Array(a) => Value::Array(a.iter().map(json_of).collect()),
        Mp::Map(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let k = match k {
                        Mp::String(s) => String::from_utf8_lossy(s.as_bytes()).into_owned(),
                        other => json_of(other).to_string(),
                    };
                    (k, json_of(v))
                })
                .collect(),
        ),
        // Buffer, window and tabpage handles: their number.
        Mp::Ext(_, data) => json!(data.iter().fold(0u64, |n, b| n << 8 | *b as u64)),
    }
}

// ---- where its socket is

/// The directories nvim's sockets are in, by its rule, from these
/// variables: `$XDG_RUNTIME_DIR` itself, and each `nvim.*` directory under
/// `$TMPDIR` (else `/tmp`), whose own directories hold one nvim each.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Places {
    pub runtime: Vec<PathBuf>,
    pub temp: Vec<PathBuf>,
}

impl Places {
    /// For the process `pid`: its own environment where the OS shows it
    /// (Linux), and the daemon's, which a pane's is made from.
    pub fn of(pid: u32) -> Self {
        let mut p = Self::default();
        let mut add = |var: Option<String>, temp: bool| {
            let Some(v) = var.filter(|v| !v.is_empty()) else { return };
            let list = if temp { &mut p.temp } else { &mut p.runtime };
            let d = PathBuf::from(v);
            if !list.contains(&d) {
                list.push(d);
            }
        };
        add(procinfo::env_var(pid, "XDG_RUNTIME_DIR"), false);
        add(std::env::var("XDG_RUNTIME_DIR").ok(), false);
        add(procinfo::env_var(pid, "TMPDIR"), true);
        add(std::env::var("TMPDIR").ok(), true);
        add(Some("/tmp".into()), true);
        p
    }

    /// Every socket named like nvim's in them: its path and the pid in
    /// its name.
    fn sockets(&self) -> Vec<(PathBuf, u32)> {
        let mut out = vec![];
        for d in &self.runtime {
            sockets_in(d, &mut out);
        }
        for t in &self.temp {
            let Ok(dirs) = std::fs::read_dir(t) else { continue };
            for nv in dirs.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("nvim.")) {
                let Ok(own) = std::fs::read_dir(nv.path()) else { continue };
                for d in own.flatten() {
                    sockets_in(&d.path(), &mut out);
                }
            }
        }
        out
    }
}

fn sockets_in(dir: &Path, out: &mut Vec<(PathBuf, u32)>) {
    use std::os::unix::fs::FileTypeExt;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Some(pid) = server_pid(&e.file_name().to_string_lossy()) else { continue };
        if e.file_type().is_ok_and(|t| t.is_socket()) {
            out.push((e.path(), pid));
        }
    }
}

/// The pid in a server's socket name, `<appname>.<pid>.<n>`.
fn server_pid(name: &str) -> Option<u32> {
    let (rest, n) = name.rsplit_once('.')?;
    let (app, pid) = rest.rsplit_once('.')?;
    (!app.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(())?;
    pid.parse().ok()
}

/// The socket of the nvim whose TUI (or headless self) is `nvim`: its
/// server is that process or its child. Blocks: it reads directories and
/// may run `lsof`.
pub fn socket_of(nvim: u32, places: &Places) -> Option<PathBuf> {
    let ours = |pid: u32| pid == nvim || procinfo::ppid(pid) == Some(nvim);
    let pick = |all: Vec<(PathBuf, u32)>| {
        let mut mine: Vec<_> = all.into_iter().filter(|(_, pid)| ours(*pid)).map(|(p, _)| p).collect();
        mine.sort();
        mine.into_iter().next()
    };
    pick(places.sockets()).or_else(|| pick(listed_sockets()))
}

/// The Unix sockets the OS lists that are named like nvim's, wherever they
/// are: `/proc/net/unix` on Linux, `lsof` (nvim's processes) on macOS.
fn listed_sockets() -> Vec<(PathBuf, u32)> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/net/unix").map(|t| proc_net_unix(&t)).unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("lsof")
            .args(["-a", "-U", "-c", "nvim", "-Fn"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map(|o| lsof_sockets(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        vec![]
    }
}

/// `/proc/net/unix`'s listening sockets with nvim's names: the path is the
/// last column (`Num RefCount Protocol Flags Type St Inode Path`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn proc_net_unix(text: &str) -> Vec<(PathBuf, u32)> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let path = l.split_whitespace().nth(7)?;
            let name = path.rsplit('/').next()?;
            Some((PathBuf::from(path), server_pid(name)?))
        })
        .collect()
}

/// `lsof -Fn`'s names (`n/path`) that are nvim's sockets.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn lsof_sockets(text: &str) -> Vec<(PathBuf, u32)> {
    text.lines()
        .filter_map(|l| {
            let path = l.strip_prefix('n')?.split_whitespace().next()?;
            let name = path.rsplit('/').next()?;
            Some((PathBuf::from(path), server_pid(name)?))
        })
        .collect()
}

/// Whether process `pid` is nvim: by its name, or by the executable it
/// runs (a `vim` that links to nvim).
fn is_nvim(pid: u32) -> bool {
    procinfo::comm(pid).is_some_and(|c| c == "nvim")
        || procinfo::exe(pid).is_some_and(|e| e.file_name().is_some_and(|n| n == "nvim"))
}

/// The nvim a terminal runs in its foreground, if it does: the process
/// whose terminal it is (an `arugula run` pane's program), or the job in
/// the foreground under its shell.
fn nvim_in(pid: u32) -> Option<u32> {
    let fg = procinfo::foreground(pid).unwrap_or(pid);
    is_nvim(fg).then_some(fg)
}

// ---- the terminals' nvims

/// What's known of each terminal's editor.
enum Seen {
    /// arugula.nvim is attached: nothing here connects.
    Plugin,
    /// A session with the nvim `tui` (connecting, or reading it); dropping
    /// `stop` ends it.
    Rpc { tui: u32, era: u64, _stop: oneshot::Sender<()> },
    /// The nvim `tui` can't be read (said once): it stays a terminal.
    Not { tui: u32 },
}

static SEEN: Mutex<Option<HashMap<PaneId, Seen>>> = Mutex::new(None);
static ERA: AtomicU64 = AtomicU64::new(1);

fn seen<T>(f: impl FnOnce(&mut HashMap<PaneId, Seen>) -> T) -> T {
    f(SEEN.lock().unwrap().get_or_insert_with(HashMap::new))
}

/// arugula.nvim attached to `pane`: a session reading it here ends (its
/// autocmds go), and none starts while it's there.
pub fn plugin_attached(pane: PaneId) {
    seen(|s| s.insert(pane, Seen::Plugin));
}

/// ...and it left: the next look may read nvim here again.
pub fn plugin_left(pane: PaneId) {
    seen(|s| {
        if matches!(s.get(&pane), Some(Seen::Plugin)) {
            s.remove(&pane);
        }
    });
}

/// Looks at what each terminal here runs, once a second, for as long as
/// the daemon runs, and reads each nvim it finds.
pub fn watch(mux: MuxHandle) {
    crate::task::spawn(async move {
        loop {
            let Some(panes) = mux.api(Api::Terminals).await else { return };
            look(&mux, panes);
            tokio::time::sleep(LOOK).await;
        }
    });
}

fn look(mux: &MuxHandle, panes: Vec<(PaneId, Option<u32>)>) {
    let now: HashMap<PaneId, Option<u32>> = panes.into_iter().map(|(id, pid)| (id, pid.and_then(nvim_in))).collect();
    seen(|s| {
        // Closed panes, and those whose nvim isn't what they run now.
        s.retain(|id, v| match (v, now.get(id)) {
            (Seen::Plugin, Some(_)) => true,
            (Seen::Rpc { tui, .. } | Seen::Not { tui }, Some(Some(n))) => tui == n,
            _ => false,
        });
        for (id, nvim) in &now {
            let Some(tui) = *nvim else { continue };
            if s.contains_key(id) {
                continue;
            }
            let (stop, stopped) = oneshot::channel();
            let era = ERA.fetch_add(1, Ordering::Relaxed);
            s.insert(*id, Seen::Rpc { tui, era, _stop: stop });
            crate::task::spawn(run(mux.clone(), *id, tui, era, stopped));
        }
    });
}

/// Why a session ended.
enum End {
    /// nvim went (or the pane took arugula.nvim instead).
    Gone,
    /// It can't be read here, for this reason: it stays a terminal.
    Not(String),
}

async fn run(mux: MuxHandle, pane: PaneId, tui: u32, era: u64, stop: oneshot::Receiver<()>) {
    let end = session(&mux, pane, tui, stop).await;
    seen(|s| {
        let ours = matches!(s.get(&pane), Some(Seen::Rpc { era: e, .. }) if *e == era);
        match end {
            End::Not(why) => {
                info!(pane, pid = tui, "nvim stays a terminal here: {why}");
                if ours {
                    s.insert(pane, Seen::Not { tui });
                }
            }
            End::Gone if ours => {
                s.remove(&pane);
            }
            End::Gone => {}
        }
    });
}

/// Reads the nvim `tui` in `pane` until it goes or `stop` says to.
async fn session(mux: &MuxHandle, pane: PaneId, tui: u32, mut stop: oneshot::Receiver<()>) -> End {
    let places = Places::of(tui);
    let Some(socket) = find_socket(tui, places).await else {
        return End::Not("no RPC socket of its own (its server is stopped?)".into());
    };
    let (rpc, mut notes) = match Rpc::connect(&socket).await {
        Ok(c) => c,
        Err(e) => return End::Not(format!("can't connect to {}: {e}", socket.display())),
    };
    let api = match rpc.call("nvim_get_api_info", vec![]).await {
        Ok(v) => v,
        Err(e) => return End::Not(format!("it didn't answer: {e}")),
    };
    let (chan, version) = (api[0].as_u64().unwrap_or(0), &api[1]["version"]);
    let v = (version["major"].as_u64().unwrap_or(0), version["minor"].as_u64().unwrap_or(0));
    if v < OLDEST {
        return End::Not(format!("nvim {}.{} is older than 0.10", v.0, v.1));
    }
    let (tx, _) = mpsc::unbounded_channel();
    let link = Link::new(Hello { editor: "nvim".into(), ..Hello::default() }, tx);
    link.no_follow();
    let handle: Arc<dyn EditorLink> = link.clone();
    match mux.api(|r| Api::EditorAttach(pane, handle.clone(), false, r)).await {
        Some(Ok(())) => {}
        // arugula.nvim got there first.
        _ => return End::Gone,
    }
    debug!(pane, pid = tui, socket = %socket.display(), "reading nvim over its socket");
    let mut last = Value::Null;
    match rpc.lua(ATTACH, vec![Mp::from(chan)]).await {
        Ok(_) => loop {
            tokio::select! {
                n = notes.recv() => match n {
                    Some((m, state)) if m == "arugula" => apply(&link, &state, &mut last),
                    Some((m, _)) if m == "arugula_bye" => break,
                    Some(_) => {}
                    None => break,
                },
                _ = &mut stop => {
                    let _ = rpc.lua(DETACH, vec![]).await;
                    break;
                }
            }
        },
        Err(e) => debug!(pane, error = %e, "nvim refused our autocmds"),
    }
    mux.send(Cmd::Api(Api::EditorDetach(pane, handle)));
    End::Gone
}

/// nvim makes its socket as it starts: give it a moment.
async fn find_socket(tui: u32, places: Places) -> Option<PathBuf> {
    let until = tokio::time::Instant::now() + SOCKET_WAIT;
    loop {
        let p = places.clone();
        if let Ok(Some(s)) = tokio::task::spawn_blocking(move || socket_of(tui, &p)).await {
            return Some(s);
        }
        if tokio::time::Instant::now() >= until {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// What nvim's autocmds said, as an editor's `summary` and `peek` say it:
/// the summary only when it changed (or on a save).
fn apply(link: &Link, s: &Value, last: &mut Value) {
    let file = s["file"].as_str();
    link.handle(json!({
        "t": "peek",
        "file": file,
        "line": s["line"],
        "col": s["col"],
        "top": s["top"],
        "lines": s["lines"].as_array().cloned().unwrap_or_default(),
        "dirty": s["dirty"],
    }));
    let summary = json!({ "file": file, "diag": s["diag"], "dirty": s["dirty"], "conflict": s["conflict"] });
    let saved = s["saved"].as_bool().unwrap_or(false);
    if summary != *last || saved {
        let mut m = summary.clone();
        m["t"] = "summary".into();
        m["saved"] = saved.into();
        link.handle(m);
        *last = summary;
    }
}

// ---- opening a file

/// Opens `file` at `line` (from 1; `None`: where nvim last had it) in the
/// nvim that terminal `pane` runs in its foreground, over nvim's own RPC
/// socket, whether arugula.nvim is there or not. It's `:drop`: the file's
/// window if nvim shows it, else the current one (split when its buffer
/// can't be left). This is what Open in editor calls when the Editor
/// preference says nvim (#1731).
///
/// `Err` says why not: no such terminal here, it doesn't run nvim, nvim
/// has no socket we can find (older than 0.10, or its server stopped), or
/// nvim refused.
pub async fn open_in_pane(mux: &MuxHandle, pane: PaneId, file: &Path, line: Option<u32>) -> Result<(), String> {
    let terminals = mux.api(Api::Terminals).await.ok_or("the daemon is stopping")?;
    let pid = terminals
        .into_iter()
        .find(|(id, _)| *id == pane)
        .ok_or_else(|| format!("no terminal %{pane} here"))?
        .1
        .ok_or_else(|| format!("%{pane} runs nothing"))?;
    let nvim = tokio::task::spawn_blocking(move || nvim_in(pid))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| format!("%{pane} doesn't run nvim"))?;
    let socket = find_socket(nvim, Places::of(nvim)).await.ok_or("its nvim has no RPC socket (0.10 or later has)")?;
    open_at(&socket, file, line).await
}

/// Opens `file` at `line` in the nvim listening on `socket`.
pub async fn open_at(socket: &Path, file: &Path, line: Option<u32>) -> Result<(), String> {
    let (rpc, _) = Rpc::connect(socket).await.map_err(|e| format!("can't reach nvim: {e}"))?;
    let line = line.map_or(Mp::Nil, Mp::from);
    rpc.lua(OPEN, vec![Mp::from(file.to_string_lossy().as_ref()), line]).await.map(|_| ())
}

/// Whether process `pid` (an editor that said it's in `pane`) runs under
/// that terminal's process: a pane id from another daemon's pane (over
/// ssh, or a second daemon here) isn't this pane.
pub async fn runs_in(mux: &MuxHandle, pane: PaneId, pid: Option<u32>) -> bool {
    let Some(pid) = pid else { return true };
    let Some(top) = mux.api(Api::Terminals).await.and_then(|t| t.into_iter().find(|(id, _)| *id == pane)?.1) else {
        return false;
    };
    tokio::task::spawn_blocking(move || {
        let mut at = pid;
        for _ in 0..64 {
            if at == top {
                return true;
            }
            match procinfo::ppid(at) {
                Some(p) if p != at => at = p,
                _ => return false,
            }
        }
        false
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_servers_name_has_its_pid() {
        assert_eq!(server_pid("nvim.4242.0"), Some(4242));
        assert_eq!(server_pid("foo.17.3"), Some(17));
        assert_eq!(server_pid("nvim.4242"), None);
        assert_eq!(server_pid(".4242.0"), None);
        assert_eq!(server_pid("nvim.x.0"), None);
        assert_eq!(server_pid("nvim.1.x"), None);
    }

    #[test]
    fn the_os_lists_sockets_wherever_they_are() {
        let proc = "Num       RefCount Protocol Flags    Type St Inode Path\n\
            0000000000000000: 00000002 00000000 00010000 0001 01 23456 /run/user/1000/nvim.900.0\n\
            0000000000000000: 00000002 00000000 00010000 0001 01 23457 /tmp/x/sock\n\
            0000000000000000: 00000003 00000000 00000000 0001 03 23458\n";
        assert_eq!(proc_net_unix(proc), vec![(PathBuf::from("/run/user/1000/nvim.900.0"), 900)]);
        let lsof = "p901\nf5\nn/private/tmp/t/nvim.902.0\nf6\nn->0x1234\n";
        assert_eq!(lsof_sockets(lsof), vec![(PathBuf::from("/private/tmp/t/nvim.902.0"), 902)]);
    }

    #[test]
    fn messages_come_whole_or_wait() {
        let mut one = Vec::new();
        let m = Mp::Array(vec![Mp::from(2), Mp::from("arugula"), Mp::Array(vec![Mp::from("x")])]);
        rmpv::encode::write_value(&mut one, &m).unwrap();
        let mut buf = one[..one.len() - 1].to_vec();
        assert_eq!(next_message(&mut buf).unwrap(), None);
        buf.push(*one.last().unwrap());
        buf.extend_from_slice(&one);
        assert_eq!(next_message(&mut buf).unwrap(), Some(m.clone()));
        assert_eq!(next_message(&mut buf).unwrap(), Some(m));
        assert!(buf.is_empty());
    }

    #[test]
    fn a_state_is_a_peek_and_a_summary_when_it_changed() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let link = Link::new(Hello { editor: "nvim".into(), ..Default::default() }, tx);
        let s = json!({ "file": "/r/a.rs", "line": 3, "col": 1, "top": 1, "lines": ["a", "b", "c"], "dirty": 1,
            "diag": { "e": 0, "w": 2, "i": 0 }, "conflict": null, "saved": false });
        let mut last = Value::Null;
        apply(&link, &s, &mut last);
        assert_eq!(link.file().as_deref(), Some("/r/a.rs"));
        let i = link.info();
        assert_eq!((i.line, i.dirty, i.diag.w), (Some(3), 1, 2));
        // The same again: nothing for the summary.
        let before = last.clone();
        apply(&link, &s, &mut last);
        assert_eq!(last, before);
        assert!(rx.try_recv().is_err());
    }

    /// A real nvim: found by its pid where its rule puts the socket, read
    /// over RPC as its autocmds notify, and told to open a file.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_real_nvim_is_found_read_and_opens_a_file() {
        if std::process::Command::new("nvim").arg("--version").output().is_err() {
            eprintln!("SKIPPED: no nvim here");
            return;
        }
        let dir = std::env::temp_dir().join(format!("arugula-nvim-rpc-{}", std::process::id()));
        let run = dir.join("run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let mut nvim = std::process::Command::new("nvim")
            .args(["--headless", "--clean"])
            .current_dir(&dir)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let places = Places { runtime: vec![run.clone()], temp: vec![] };
        let pid = nvim.id();
        let mut socket = None;
        for _ in 0..40 {
            socket = socket_of(pid, &places);
            if socket.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let socket = socket.expect("its socket, by its pid");
        assert_eq!(socket, run.join(format!("nvim.{pid}.0")));
        let (rpc, mut notes) = Rpc::connect(&socket).await.unwrap();
        let api = rpc.call("nvim_get_api_info", vec![]).await.unwrap();
        let v = &api[1]["version"];
        if (v["major"].as_u64().unwrap(), v["minor"].as_u64().unwrap()) < OLDEST {
            eprintln!("SKIPPED: nvim {v} is older than 0.10");
            let _ = nvim.kill();
            let _ = nvim.wait();
            return;
        }
        rpc.lua(ATTACH, vec![Mp::from(api[0].as_u64().unwrap())]).await.unwrap();
        // Opening a file at a line: its autocmds say so.
        open_at(&socket, &dir.join("a.txt"), Some(3)).await.unwrap();
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        let state = loop {
            let left = until.saturating_duration_since(tokio::time::Instant::now());
            let (m, s) = tokio::time::timeout(left, notes.recv()).await.expect("a notification").unwrap();
            if m == "arugula" && s["line"] == 3 {
                break s;
            }
        };
        assert!(state["file"].as_str().unwrap().ends_with("/a.txt"), "{state}");
        assert_eq!(
            state["lines"][state["line"].as_u64().unwrap() as usize - state["top"].as_u64().unwrap() as usize],
            "three"
        );
        assert_eq!(state["diag"], json!({ "e": 0, "w": 0, "i": 0 }));
        // A diagnostic: its count comes.
        rpc.lua(
            "local ns = vim.api.nvim_create_namespace('t') vim.diagnostic.set(ns, 0, {{ lnum = 0, col = 0, message = 'x', severity = 1 }})",
            vec![],
        )
        .await
        .unwrap();
        loop {
            let (m, s) =
                tokio::time::timeout(Duration::from_secs(5), notes.recv()).await.expect("diagnostics").unwrap();
            if m == "arugula" && s["diag"]["e"] == 1 {
                break;
            }
        }
        // Quitting: it says goodbye, then the socket closes.
        let _ = rpc.lua("vim.schedule(function() vim.cmd('qa!') end)", vec![]).await;
        let mut bye = false;
        while let Ok(Some((m, _))) = tokio::time::timeout(Duration::from_secs(5), notes.recv()).await {
            bye |= m == "arugula_bye";
        }
        assert!(bye, "no goodbye");
        let _ = nvim.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
