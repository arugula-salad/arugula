//! Arugula offers arugula.nvim, and installs it in one click (#1733).
//!
//! The daemon carries the plugin (`editors/nvim`, embedded by build.rs, so
//! it's the one from the daemon's own commit). For a pane running nvim:
//!
//! - **The offer.** When the daemon starts reading an nvim over its socket
//!   (`nvim.rs`), it asks that nvim where arugula.nvim is ([`PROBE`]): on
//!   its runtimepath, under any package directory, or held by lazy.nvim
//!   (which keeps a plugin off the runtimepath until it loads). With no copy
//!   anywhere, the pane's editor says `offer_plugin`, which the web client's
//!   tip reads (#1735). A copy somewhere else is a plugin manager's, or the
//!   user's: nothing here offers, writes or deletes it.
//! - **Install** ([`install`], `POST /api/panes/N/nvim-plugin`, the
//!   owner's). It writes the plugin into that nvim's own package directory,
//!   `stdpath("data")/site/pack/arugula/start/arugula.nvim` on the pane's
//!   machine, with a marker file ([`MARKER`]) that says Arugula installed it
//!   and which version. It never writes `init.lua` or anything outside that
//!   directory. Then it loads it into the running nvim ([`LOAD`]: what
//!   `:packadd` does, which only searches `opt/` once nvim has started) and
//!   waits for it to join as the pane. nvim loads it by itself from then on.
//!   Each daemon does this for its own panes, so an ssh or VM pane's nvim
//!   gets it from the daemon on its machine.
//! - **Updates.** A copy with the marker whose version isn't this daemon's
//!   is rewritten: at startup for each copy installed from here (they're
//!   listed in `<state>/editor/nvim-plugin.json`), and whenever a session
//!   finds one. A copy without the marker is never touched.
//! - **Remove** (`DELETE /api/nvim-plugin`, Settings > Editor > nvim >
//!   "Remove arugula.nvim") deletes the copies installed from here that
//!   still have the marker. An nvim that has it loaded keeps it until it
//!   quits.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, Weak},
    time::Duration,
};

use arugula_proto::{
    PaneId,
    api::{Empty, NvimPluginCopies, NvimPluginCopy, NvimPluginInstalled, NvimPluginRemoved},
    op::ops::{NvimPluginGet, NvimPluginInstall, NvimPluginRemove},
};
use arugulad_base::ops::{Cx, Handle, OpError};
use axum::http::StatusCode;
use rmpv::Value as Mp;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::{
    link::Link,
    nvim::{self, NoNvim, Rpc},
};
use crate::{Editor, mux::MuxHandle};

include!(concat!(env!("OUT_DIR"), "/nvim_plugin.rs"));

/// The marker file in a copy Arugula installed.
pub const MARKER: &str = "ARUGULA_INSTALLED";
/// Where in `stdpath("data")` the copy goes: a package of its own, loaded
/// at nvim's start.
const PACK_DIR: &str = "site/pack/arugula/start/arugula.nvim";
/// How long a new copy has to load and join as the pane.
const JOIN: Duration = Duration::from_secs(5);

/// Asked of nvim: its data directory, our copy's place in it, and every
/// copy of arugula.nvim somewhere else (by real path), and whether it's
/// loaded. lazy.nvim's plugins are read only if lazy.nvim is loaded, and
/// nothing is `require`d that isn't.
const PROBE: &str = r#"
local uv = vim.uv or vim.loop
local function real(p)
  p = vim.fs.normalize(p)
  return uv.fs_realpath(p) or p
end
local data = vim.fn.stdpath("data")
local ours = real(data .. "/site/pack/arugula/start/arugula.nvim")
local others, seen = {}, {}
local function add(root)
  root = real(root)
  if root ~= ours and not seen[root] then
    seen[root] = true
    table.insert(others, root)
  end
end
local tail = "/lua/arugula/init.lua"
for _, f in ipairs(vim.api.nvim_get_runtime_file("lua/arugula/init.lua", true)) do
  add(f:sub(1, #f - #tail))
end
for _, f in ipairs(vim.fn.globpath(vim.o.packpath, "pack/*/*/*/lua/arugula/init.lua", false, true)) do
  add(f:sub(1, #f - #tail))
end
local lazy = package.loaded["lazy.core.config"]
if type(lazy) == "table" and type(lazy.plugins) == "table" then
  for name, p in pairs(lazy.plugins) do
    if type(p) == "table" and type(p.dir) == "string"
      and (name == "arugula.nvim" or uv.fs_stat(p.dir .. tail)) then
      add(p.dir)
    end
  end
end
return { data = data, ours = ours, others = others, loaded = vim.g.loaded_arugula == true }
"#;

/// Loads the copy at `dir` into a running nvim, as `:packadd` would: on
/// the runtimepath, then its `plugin/` files sourced. In an Arugula pane
/// the plugin connects by itself (`$ARUGULA_PANE`). `false` when it was
/// loaded already.
const LOAD: &str = r#"
local dir = ...
if vim.g.loaded_arugula then return false end
if not vim.tbl_contains(vim.opt.runtimepath:get(), dir) then vim.opt.runtimepath:append(dir) end
local files = vim.fn.globpath(dir, "plugin/**/*.vim", false, true)
vim.list_extend(files, vim.fn.globpath(dir, "plugin/**/*.lua", false, true))
table.sort(files)
for _, f in ipairs(files) do vim.cmd.source(vim.fn.fnameescape(f)) end
return true
"#;

// ---- the copy on disk

/// The version stamp of the plugin this daemon carries: its release, and
/// a hash of the files (two builds of one release can differ).
pub fn stamp() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    let hash = HASH.get_or_init(|| {
        let mut h = Sha256::new();
        for (path, bytes) in FILES {
            h.update(path.as_bytes());
            h.update([0]);
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(bytes);
        }
        hex::encode(&h.finalize()[..6])
    });
    format!("{}+{hash}", arugula_core::version::get())
}

fn marker_text() -> String {
    format!(
        "Arugula installed this copy of arugula.nvim, and replaces it when its daemon is upgraded.\n\
         Settings > Editor > nvim > Remove arugula.nvim deletes it. Delete this file to keep\n\
         Arugula from ever touching this folder again.\n\
         version={}\n",
        stamp()
    )
}

/// What's at a copy's place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copy {
    /// Nothing.
    None,
    /// A copy Arugula installed: its marker's version stamp.
    Ours(String),
    /// Something without the marker: never touched.
    Theirs,
}

pub fn copy_at(dir: &Path) -> Copy {
    if std::fs::symlink_metadata(dir).is_err() {
        return Copy::None;
    }
    match std::fs::read_to_string(dir.join(MARKER)) {
        Ok(t) => Copy::Ours(t.lines().find_map(|l| l.strip_prefix("version=")).unwrap_or("").trim().to_owned()),
        Err(_) => Copy::Theirs,
    }
}

/// Writes this daemon's plugin, with its marker, at `dir`, replacing what
/// was there: built beside it and moved in, so nvim never reads half a
/// copy. Only for a place that's empty or ours ([`copy_at`]).
pub fn write(dir: &Path) -> io::Result<()> {
    let parent = dir.parent().ok_or_else(|| io::Error::other("no parent directory"))?;
    let name = dir.file_name().ok_or_else(|| io::Error::other("no name"))?.to_string_lossy();
    std::fs::create_dir_all(parent)?;
    let new = parent.join(format!(".{name}.new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&new);
    for (rel, bytes) in FILES {
        let p = new.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, bytes)?;
    }
    let tags = help_tags();
    if !tags.is_empty() {
        std::fs::write(new.join("doc/tags"), tags)?;
    }
    std::fs::write(new.join(MARKER), marker_text())?;
    if std::fs::symlink_metadata(dir).is_ok() {
        let old = parent.join(format!(".{name}.old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(dir, &old)?;
        std::fs::rename(&new, dir)?;
        let _ = std::fs::remove_dir_all(&old);
    } else {
        std::fs::rename(&new, dir)?;
    }
    Ok(())
}

/// `doc/tags` for the help files in `doc/`, as `:helptags` writes it: one
/// `tag<Tab>file<Tab>/*tag*` line per `*tag*`, sorted. A package's help
/// is found only through its tags file, and the repo has none.
fn help_tags() -> String {
    let mut lines = vec![];
    for (path, bytes) in FILES {
        let Some(file) = path.strip_prefix("doc/").filter(|f| f.ends_with(".txt") && !f.contains('/')) else {
            continue;
        };
        for tag in String::from_utf8_lossy(bytes).lines().flat_map(tags_in) {
            lines.push(format!("{tag}\t{file}\t/*{}*", tag.replace('/', "\\/")));
        }
    }
    lines.sort();
    lines.dedup();
    lines.into_iter().map(|l| l + "\n").collect()
}

/// The `*tag*`s on a help line: between stars, with no space, tab, bar or
/// star inside, and a space (or the line's edge) on each side, as
/// `:helptags` reads them.
fn tags_in(line: &str) -> Vec<String> {
    let edge = |c: Option<char>| c.is_none_or(|c| c == ' ' || c == '\t');
    let mut out = vec![];
    let mut rest = line;
    let mut before = None;
    while let Some(a) = rest.find('*') {
        let lead = rest[..a].chars().next_back().or(before);
        let after = &rest[a + 1..];
        let Some(b) = after.find(['*', ' ', '\t', '|']) else { break };
        if after[b..].starts_with('*') && b > 0 && edge(lead) && edge(after[b + 1..].chars().next()) {
            out.push(after[..b].to_owned());
            before = Some('*');
            rest = &after[b + 1..];
        } else {
            before = Some('*');
            rest = after;
        }
    }
    out
}

/// Rewrites the copy at `dir` when it's ours and another version. Whether
/// it did.
pub fn refresh(dir: &Path) -> io::Result<bool> {
    match copy_at(dir) {
        Copy::Ours(v) if v != stamp() => write(dir).map(|()| true),
        _ => Ok(false),
    }
}

/// Deletes the copy at `dir` when it's ours. Whether it did.
pub fn remove(dir: &Path) -> io::Result<bool> {
    match copy_at(dir) {
        Copy::Ours(_) => std::fs::remove_dir_all(dir).map(|()| true),
        _ => Ok(false),
    }
}

// ---- the copies installed from here

/// `<state>/editor/nvim-plugin.json`, once the editor is installed.
fn records() -> Option<PathBuf> {
    super::server::settings().map(|s| s.dir.join("nvim-plugin.json"))
}

static RECORDS: Mutex<()> = Mutex::new(());

pub fn recorded(file: &Path) -> Vec<PathBuf> {
    std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<Vec<PathBuf>>(&b).ok()).unwrap_or_default()
}

fn save(file: &Path, dirs: &[PathBuf]) -> io::Result<()> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(file, serde_json::to_vec_pretty(dirs)?)
}

/// Adds `dir` to the list.
pub fn record(file: &Path, dir: &Path) -> io::Result<()> {
    let _g = RECORDS.lock().unwrap();
    let mut dirs = recorded(file);
    if !dirs.iter().any(|d| d == dir) {
        dirs.push(dir.to_owned());
        save(file, &dirs)?;
    }
    Ok(())
}

/// Refreshes each copy on the list, and forgets those that are gone or no
/// longer ours. What it refreshed.
pub fn refresh_recorded(file: &Path) -> Vec<PathBuf> {
    let _g = RECORDS.lock().unwrap();
    let mut done = vec![];
    let dirs = recorded(file);
    let keep: Vec<PathBuf> = dirs.iter().filter(|d| matches!(copy_at(d), Copy::Ours(_))).cloned().collect();
    for d in &keep {
        match refresh(d) {
            Ok(true) => done.push(d.clone()),
            Ok(false) => {}
            Err(e) => warn!(dir = %d.display(), error = %e, "couldn't refresh arugula.nvim"),
        }
    }
    if keep.len() != dirs.len() {
        let _ = save(file, &keep);
    }
    done
}

/// Removes each copy on the list that's ours, and empties it. What it
/// removed.
pub fn remove_recorded(file: &Path) -> io::Result<Vec<PathBuf>> {
    let _g = RECORDS.lock().unwrap();
    let mut removed = vec![];
    for d in recorded(file) {
        if remove(&d)? {
            removed.push(d);
        }
    }
    save(file, &[])?;
    Ok(removed)
}

/// At startup: an upgraded daemon refreshes the copies it installed.
pub fn refresh_at_start() {
    let Some(file) = records() else { return };
    crate::task::spawn(async move {
        let done = tokio::task::spawn_blocking(move || refresh_recorded(&file)).await.unwrap_or_default();
        for d in done {
            info!(dir = %d.display(), version = stamp(), "refreshed arugula.nvim");
        }
    });
}

// ---- in a running nvim

/// What [`PROBE`] found.
#[derive(Debug)]
struct Found {
    /// Our copy's place in that nvim's data directory.
    ours: PathBuf,
    /// Copies elsewhere: a plugin manager's, or the user's.
    others: Vec<String>,
    loaded: bool,
}

async fn probe(rpc: &Rpc) -> Result<Found, String> {
    let v = rpc.lua(PROBE, vec![]).await?;
    // An absolute path ending in our package's place, or nothing is written.
    let ours = v["ours"]
        .as_str()
        .filter(|p| p.starts_with('/') && Path::new(p).ends_with(PACK_DIR))
        .ok_or("nvim has no data directory")?;
    let others = v["others"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect());
    Ok(Found { ours: PathBuf::from(ours), others: others.unwrap_or_default(), loaded: v["loaded"] == true })
}

/// A session starts reading an nvim: whether to offer arugula.nvim to it
/// (it has no copy anywhere). A copy of ours from another version is
/// refreshed first.
pub async fn on_connect(rpc: &Rpc) -> bool {
    let Ok(found) = probe(rpc).await else { return false };
    if found.loaded || !found.others.is_empty() {
        return false;
    }
    let dir = found.ours;
    tokio::task::spawn_blocking(move || match copy_at(&dir) {
        Copy::None => true,
        Copy::Ours(_) => {
            match refresh(&dir) {
                Ok(true) => info!(dir = %dir.display(), version = stamp(), "refreshed arugula.nvim"),
                Ok(false) => {}
                Err(e) => warn!(dir = %dir.display(), error = %e, "couldn't refresh arugula.nvim"),
            }
            if let Some(f) = records() {
                let _ = record(&f, &dir);
            }
            false
        }
        Copy::Theirs => false,
    })
    .await
    .unwrap_or(false)
}

/// The sessions' links that offer the plugin, by pane: an install takes
/// the offer back at once.
static OFFERED: Mutex<Option<HashMap<PaneId, Weak<Link>>>> = Mutex::new(None);

pub fn offered(pane: PaneId, link: &std::sync::Arc<Link>) {
    OFFERED.lock().unwrap().get_or_insert_with(HashMap::new).insert(pane, std::sync::Arc::downgrade(link));
}

fn withdraw(pane: PaneId) {
    let link = OFFERED.lock().unwrap().as_mut().and_then(|m| m.remove(&pane)).and_then(|w| w.upgrade());
    if let Some(l) = link {
        l.offer_plugin(false);
    }
}

/// Why an install didn't happen.
#[derive(Debug)]
pub enum Refused {
    NoPane(PaneId),
    /// The pane doesn't run an nvim this can reach, or it has arugula.nvim
    /// from elsewhere: the sentence says which.
    Conflict(String),
    Failed(String),
}

/// Installs arugula.nvim into the nvim at `socket` (see the module's
/// header), loads it, and, for `pane`, waits a moment for it to join.
pub async fn install_at(socket: &Path, pane: Option<PaneId>) -> Result<NvimPluginInstalled, Refused> {
    let (rpc, _) = Rpc::connect(socket).await.map_err(|e| Refused::Conflict(format!("can't reach its nvim: {e}")))?;
    let found = probe(&rpc).await.map_err(Refused::Conflict)?;
    if let Some(other) = found.others.first() {
        return Err(Refused::Conflict(format!(
            "this nvim has arugula.nvim already, at {other}: Arugula leaves a copy it didn't install alone"
        )));
    }
    let dir = found.ours.clone();
    let d = dir.clone();
    tokio::task::spawn_blocking(move || match copy_at(&d) {
        Copy::Theirs => Err(Refused::Conflict(format!(
            "{} is there without Arugula's marker ({MARKER}): Arugula leaves it alone",
            d.display()
        ))),
        Copy::Ours(v) if v == stamp() => Ok(()),
        _ => write(&d).map_err(|e| Refused::Failed(format!("couldn't write {}: {e}", d.display()))),
    })
    .await
    .map_err(|e| Refused::Failed(e.to_string()))??;
    if let Some(f) = records() {
        let d = dir.clone();
        let _ = tokio::task::spawn_blocking(move || record(&f, &d)).await;
    }
    info!(dir = %dir.display(), version = stamp(), "installed arugula.nvim");
    if !found.loaded {
        rpc.lua(LOAD, vec![Mp::from(dir.to_string_lossy().as_ref())])
            .await
            .map_err(|e| Refused::Failed(format!("installed, but nvim couldn't load it: {e}")))?;
    }
    let mut joined = false;
    if let Some(pane) = pane {
        withdraw(pane);
        let until = tokio::time::Instant::now() + JOIN;
        while !joined && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(100)).await;
            joined = nvim::plugin_on(pane);
        }
    }
    Ok(NvimPluginInstalled { dir: dir.display().to_string(), version: stamp(), joined })
}

/// Installs arugula.nvim into the nvim terminal `pane` runs.
pub async fn install(mux: &MuxHandle, pane: PaneId) -> Result<NvimPluginInstalled, Refused> {
    let socket = nvim::pane_socket(mux, pane).await.map_err(|e| match e {
        NoNvim::NoPane(p) => Refused::NoPane(p),
        NoNvim::Why(why) => Refused::Conflict(why),
    })?;
    install_at(&socket, Some(pane)).await
}

// ---- the operations (owner only: `Access::Owner`, checked before these run)

impl Handle<Editor> for NvimPluginInstall {
    async fn handle(cx: &Cx<'_>, pane: PaneId, _: Empty) -> Result<NvimPluginInstalled, OpError> {
        install(&cx.app.mux, pane).await.map_err(|r| match r {
            Refused::NoPane(p) => OpError::NoPane(p),
            Refused::Conflict(why) => OpError::Status(StatusCode::CONFLICT, why),
            Refused::Failed(why) => OpError::Failed(why),
        })
    }
}

impl Handle<Editor> for NvimPluginGet {
    async fn handle(_: &Cx<'_>, _: (), _: Empty) -> Result<NvimPluginCopies, OpError> {
        let copies = match records() {
            Some(f) => tokio::task::spawn_blocking(move || {
                recorded(&f)
                    .into_iter()
                    .filter_map(|d| match copy_at(&d) {
                        Copy::Ours(version) => Some(NvimPluginCopy { dir: d.display().to_string(), version }),
                        _ => None,
                    })
                    .collect()
            })
            .await
            .unwrap_or_default(),
            None => vec![],
        };
        Ok(NvimPluginCopies { version: stamp(), copies })
    }
}

impl Handle<Editor> for NvimPluginRemove {
    async fn handle(_: &Cx<'_>, _: (), _: Empty) -> Result<NvimPluginRemoved, OpError> {
        let Some(f) = records() else { return Ok(NvimPluginRemoved { removed: vec![] }) };
        let removed = tokio::task::spawn_blocking(move || remove_recorded(&f))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?
            .map_err(|e| OpError::Failed(format!("couldn't remove arugula.nvim: {e}")))?;
        for d in &removed {
            info!(dir = %d.display(), "removed arugula.nvim");
        }
        Ok(NvimPluginRemoved { removed: removed.iter().map(|d| d.display().to_string()).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-nvim-plugin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_plugin_is_embedded_whole() {
        let names: Vec<_> = FILES.iter().map(|(p, _)| *p).collect();
        for want in [
            "plugin/arugula.lua",
            "lua/arugula/init.lua",
            "lua/arugula/health.lua",
            "lua/arugula/diff.lua",
            "doc/arugula.txt",
        ] {
            assert!(names.contains(&want), "{want} isn't embedded: {names:?}");
        }
        // Every file under editors/nvim, so a new one can't be left out.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/nvim");
        fn count(d: &Path) -> usize {
            std::fs::read_dir(d)
                .unwrap()
                .flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| if e.path().is_dir() { count(&e.path()) } else { 1 })
                .sum()
        }
        assert_eq!(FILES.len(), count(&root));
    }

    #[test]
    fn a_copy_is_written_marked_refreshed_and_removed() {
        let d = temp("copy");
        let dir = d.join("site/pack/arugula/start/arugula.nvim");
        assert_eq!(copy_at(&dir), Copy::None);
        write(&dir).unwrap();
        assert_eq!(copy_at(&dir), Copy::Ours(stamp()));
        assert!(dir.join("plugin/arugula.lua").is_file());
        assert!(std::fs::read_to_string(dir.join(MARKER)).unwrap().contains("Arugula installed"));
        // The same version: nothing to do.
        assert!(!refresh(&dir).unwrap());
        // Another version (an older daemon's): rewritten, files and marker.
        std::fs::write(dir.join(MARKER), "version=0.0.1+000000000000\n").unwrap();
        std::fs::write(dir.join("lua/arugula/init.lua"), "-- old").unwrap();
        std::fs::write(dir.join("lua/arugula/gone.lua"), "-- a file the new one doesn't have").unwrap();
        assert!(refresh(&dir).unwrap());
        assert_eq!(copy_at(&dir), Copy::Ours(stamp()));
        assert_ne!(std::fs::read_to_string(dir.join("lua/arugula/init.lua")).unwrap(), "-- old");
        assert!(!dir.join("lua/arugula/gone.lua").exists());
        // Nothing left beside it.
        assert_eq!(std::fs::read_dir(dir.parent().unwrap()).unwrap().count(), 1);
        assert!(remove(&dir).unwrap());
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A copy without the marker (a plugin manager's, or one the user put
    /// there) is never rewritten or removed.
    #[test]
    fn a_copy_without_the_marker_is_left_alone() {
        let d = temp("theirs");
        let dir = d.join("arugula.nvim");
        std::fs::create_dir_all(dir.join("lua/arugula")).unwrap();
        std::fs::write(dir.join("lua/arugula/init.lua"), "-- mine").unwrap();
        assert_eq!(copy_at(&dir), Copy::Theirs);
        assert!(!refresh(&dir).unwrap());
        assert!(!remove(&dir).unwrap());
        assert_eq!(std::fs::read_to_string(dir.join("lua/arugula/init.lua")).unwrap(), "-- mine");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The list of copies installed from here: refreshed on a version
    /// change (a daemon upgrade), forgotten when gone or no longer ours,
    /// and removed all at once.
    #[test]
    fn the_copies_installed_from_here_refresh_and_remove() {
        let d = temp("records");
        let file = d.join("state/nvim-plugin.json");
        let (a, b, c) = (d.join("a/arugula.nvim"), d.join("b/arugula.nvim"), d.join("c/arugula.nvim"));
        for x in [&a, &b, &c] {
            write(x).unwrap();
            record(&file, x).unwrap();
        }
        record(&file, &a).unwrap();
        assert_eq!(recorded(&file), vec![a.clone(), b.clone(), c.clone()]);
        // An upgrade: a's version is old. c's marker went (the user took it over).
        std::fs::write(a.join(MARKER), "version=0.0.1+000000000000\n").unwrap();
        std::fs::remove_file(c.join(MARKER)).unwrap();
        assert_eq!(refresh_recorded(&file), vec![a.clone()]);
        assert_eq!(copy_at(&a), Copy::Ours(stamp()));
        assert_eq!(recorded(&file), vec![a.clone(), b.clone()]);
        assert_eq!(remove_recorded(&file).unwrap(), vec![a.clone(), b.clone()]);
        assert!(!a.exists() && !b.exists());
        assert!(c.join("lua/arugula/init.lua").exists(), "the user's is still there");
        assert!(recorded(&file).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Whether nvim 0.10 or later is here (what panes need, #1730); it
    /// says why not, loudly, when it isn't.
    fn nvim_here() -> bool {
        let Ok(out) = std::process::Command::new("nvim").arg("--version").output() else {
            eprintln!("SKIPPED: no nvim here");
            return false;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let v = text.split_whitespace().nth(1).unwrap_or("").trim_start_matches('v').to_owned();
        let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if (major, minor) < (0, 10) {
            eprintln!("SKIPPED: nvim {v} is older than 0.10 (CI must install 0.10+)");
            return false;
        }
        true
    }

    /// A headless nvim with these variables, and its socket.
    async fn headless(dir: &Path, env: &[(&str, &Path)], args: &[&str]) -> Option<(std::process::Child, PathBuf)> {
        if !nvim_here() {
            return None;
        }
        let run = dir.join("run");
        std::fs::create_dir_all(&run).unwrap();
        let mut cmd = std::process::Command::new("nvim");
        cmd.arg("--headless")
            .args(args)
            .current_dir(dir)
            .env("XDG_RUNTIME_DIR", &run)
            .env_remove("ARUGULA_PANE")
            .env("ARUGULA_SOCK", dir.join("no-daemon.sock"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().unwrap();
        let places = nvim::Places { runtime: vec![run], temp: vec![] };
        for _ in 0..50 {
            if let Some(s) = nvim::socket_of(child.id(), &places) {
                return Some((child, s));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        quit(child);
        panic!("no socket for the headless nvim");
    }

    fn quit(mut child: std::process::Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    /// An nvim with an empty data directory: offered, installed into its
    /// own package directory, loaded live, and found next time.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_nvim_without_it_is_offered_it_and_gets_it_live() {
        let d = temp("live");
        let (data, config) = (d.join("data"), d.join("config"));
        std::fs::create_dir_all(&config).unwrap();
        let env = [("XDG_DATA_HOME", data.as_path()), ("XDG_CONFIG_HOME", config.as_path())];
        let Some((child, socket)) = headless(&d, &env, &[]).await else { return };
        let (rpc, _) = Rpc::connect(&socket).await.unwrap();
        assert!(on_connect(&rpc).await, "no copy anywhere: offered");
        let done = install_at(&socket, None).await.unwrap();
        let dir = data.join("nvim").join(PACK_DIR);
        assert_eq!(std::fs::canonicalize(&done.dir).unwrap(), std::fs::canonicalize(&dir).unwrap());
        assert_eq!(copy_at(&dir), Copy::Ours(stamp()));
        // Loaded into the running nvim: its commands are there.
        let v =
            rpc.lua("return { vim.g.loaded_arugula == true, vim.fn.exists(':ArugulaJoin') }", vec![]).await.unwrap();
        assert_eq!(v, serde_json::json!([true, 2]));
        // Its help is found (doc/tags).
        let v = rpc.lua("vim.cmd.help('arugula-diff') return vim.fn.expand('%:t')", vec![]).await.unwrap();
        assert_eq!(v, "arugula.txt");
        // Nothing written outside the data directory: no init.lua.
        assert_eq!(std::fs::read_dir(&config).unwrap().count(), 0);
        // Installed and loaded: not offered again.
        assert!(!on_connect(&rpc).await);
        quit(child);
        // The next nvim loads it by itself, from its package directory.
        let Some((child, socket)) = headless(&d, &env, &[]).await else { return };
        let (rpc, _) = Rpc::connect(&socket).await.unwrap();
        let v = rpc.lua("return vim.g.loaded_arugula == true", vec![]).await.unwrap();
        assert_eq!(v, true);
        assert!(!on_connect(&rpc).await);
        quit(child);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// arugula.nvim managed by a plugin manager (on the runtimepath from
    /// elsewhere, or held by lazy.nvim before it loads): not offered, and
    /// an install is refused without writing anything.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_plugin_managers_copy_is_not_offered_or_touched() {
        let d = temp("managed");
        let (data, config) = (d.join("data"), d.join("config"));
        std::fs::create_dir_all(&config).unwrap();
        let theirs = d.join("lazy/arugula.nvim");
        std::fs::create_dir_all(theirs.join("lua/arugula")).unwrap();
        std::fs::write(theirs.join("lua/arugula/init.lua"), "return {}").unwrap();
        let env = [("XDG_DATA_HOME", data.as_path()), ("XDG_CONFIG_HOME", config.as_path())];
        let rtp = format!("set rtp^={}", theirs.display());
        let Some((child, socket)) = headless(&d, &env, &["--cmd", &rtp]).await else { return };
        let (rpc, _) = Rpc::connect(&socket).await.unwrap();
        assert!(!on_connect(&rpc).await, "on the runtimepath from elsewhere: not offered");
        let r = install_at(&socket, None).await;
        assert!(matches!(&r, Err(Refused::Conflict(why)) if why.contains("lazy/arugula.nvim")), "{r:?}");
        assert!(!data.join("nvim").join(PACK_DIR).exists(), "nothing written");
        quit(child);
        // lazy.nvim holding it, not loaded yet (so not on the runtimepath).
        let Some((child, socket)) = headless(&d, &env, &[]).await else { return };
        let (rpc, _) = Rpc::connect(&socket).await.unwrap();
        assert!(on_connect(&rpc).await);
        let lazy = format!(
            "package.loaded['lazy.core.config'] = {{ plugins = {{ ['arugula.nvim'] = {{ dir = '{}' }} }} }}",
            theirs.display()
        );
        rpc.lua(&lazy, vec![]).await.unwrap();
        assert!(!on_connect(&rpc).await, "lazy.nvim's: not offered");
        assert!(matches!(install_at(&socket, None).await, Err(Refused::Conflict(_))));
        assert!(!data.join("nvim").join(PACK_DIR).exists(), "nothing written");
        quit(child);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The tags file written with a copy is the one nvim's `:helptags`
    /// writes for the same help.
    #[test]
    fn help_tags_are_what_helptags_writes() {
        if !nvim_here() {
            return;
        }
        let d = temp("tags");
        std::fs::create_dir_all(d.join("doc")).unwrap();
        for (path, bytes) in FILES.iter().filter(|(p, _)| p.starts_with("doc/") && p.ends_with(".txt")) {
            std::fs::write(d.join(path), bytes).unwrap();
        }
        let status = std::process::Command::new("nvim")
            .args(["--headless", "--clean", "-c", &format!("helptags {}", d.join("doc").display()), "-c", "qa!"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let theirs = std::fs::read_to_string(d.join("doc/tags")).unwrap();
        assert!(theirs.contains("arugula-diff\t"), "{theirs}");
        assert_eq!(help_tags(), theirs);
        assert_eq!(tags_in("a *x* b*y* *z*|w **"), vec!["x"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `:checkhealth arugula` names the problem when there's no socket, or
    /// a socket with no daemon behind it.
    #[tokio::test(flavor = "multi_thread")]
    async fn checkhealth_names_a_missing_socket_or_daemon() {
        if !nvim_here() {
            return;
        }
        let d = temp("health");
        let plugin = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/nvim")).unwrap();
        let health = |sock: &Path| {
            let out = d.join("health.txt");
            let _ = std::fs::remove_file(&out);
            let status = std::process::Command::new("nvim")
                .args(["--headless", "--clean", "--cmd", &format!("set rtp^={}", plugin.display())])
                .args(["-c", "runtime plugin/arugula.lua", "-c", "checkhealth arugula"])
                .args(["-c", &format!("w! {}", out.display()), "-c", "qa!"])
                .current_dir(&d)
                .env("ARUGULA_SOCK", sock)
                .env_remove("ARUGULA_PANE")
                .env("XDG_DATA_HOME", d.join("data"))
                .env("XDG_STATE_HOME", d.join("state"))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
            std::fs::read_to_string(&out).unwrap()
        };
        let missing = d.join("none.sock");
        let t = health(&missing);
        assert!(t.contains("ERROR") && t.contains("No daemon socket"), "{t}");
        // A socket file nobody listens on: a daemon that died.
        let dead = d.join("dead.sock");
        drop(std::os::unix::net::UnixListener::bind(&dead).unwrap());
        let t = health(&dead);
        assert!(t.contains("ERROR") && t.contains("No daemon answers"), "{t}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
