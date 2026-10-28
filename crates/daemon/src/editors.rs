//! The editor server (VS Code as code-server in an editor block, the
//! editors that join the swarm, Arugula's extension) and Claude Code's IDE
//! (the daemon as the IDE Claude Code connects to) are a crate of their own,
//! `arugulad-editor` (crates/arugulad-editor, #232). The binary registers it
//! ([`register`]) when its `editor` feature is on, which it is by default.
//!
//! **The surface:** what the rest of the daemon calls is below. Each asks the
//! registered [`Editors`] or, with none, does nothing or says
//! [`not_built`], so no caller needs to know whether it's there. A build
//! without it has no editor blocks (a saved one says so when it's opened), no
//! socket for editors to join on, so none ever does, no extension to
//! download and no IDE settings. The mux's half of each is the core's (the
//! `EditorLink` and `PaneCards` traits). The IDE's operations are the editor
//! crate's too (`impl Handle<Editor> for IdeGet`, routed with `op_of`).

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use arugula_proto::{BlockType, DiffInfo, Driver, PaneId, api::IdeInfo};
use axum::Router;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::{
    block::{Block, BlockCtx, BlockKind, BlockKinds},
    mux::{MuxHandle, PaneCards},
    pane::Launcher,
    server::App,
};

/// Where code-server's releases are downloaded from. (The flag that says
/// otherwise is in every build, so its default is too.)
pub const RELEASES: &str = "https://github.com/coder/code-server/releases/download";

/// The editor server's settings, from the command line.
pub struct Settings {
    /// `<state>/editor`: settings, extensions, workspaces, logs.
    pub dir: PathBuf,
    /// This host's server's socket (kept short: Unix socket paths are).
    pub socket: PathBuf,
    /// A code-server to run instead of the pinned release.
    pub binary: Option<PathBuf>,
    /// Where releases are unpacked.
    pub cache: PathBuf,
    /// Where releases come from (tests point it elsewhere).
    pub releases: String,
    /// Seconds without a client before a server stops (at least 60).
    pub idle: u64,
    /// Seconds a window may be away and still reconnect to its session.
    pub grace: u64,
    pub launch: Launcher,
}

/// The editor crate, as the daemon calls it.
pub trait Editors: Send + Sync + 'static {
    /// Registers the editor block type.
    fn kinds(&self, kinds: &mut BlockKinds);
    /// Adds its HTTP routes to the API's: the extension's download, and the
    /// IDE's operations.
    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>>;
    /// Adds the route editors on this machine join the swarm on (M28).
    fn local_routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>>;
    /// Turns editor blocks on (once per process).
    fn install(&self, s: Settings);
    /// Starts looking for the editors that run in terminals (#1730: nvim,
    /// read over its own RPC socket), once the mux runs, and refreshes the
    /// copies of arugula.nvim it installed (#1733).
    fn watch(&self, mux: MuxHandle);
    /// Opens `file` at `line` in the editor terminal `pane` runs (see
    /// [`open_in_pane`]).
    fn open_in_pane(
        &self,
        mux: &MuxHandle,
        pane: PaneId,
        file: &Path,
        line: Option<u32>,
    ) -> BoxFuture<'static, Result<(), String>>;
    /// Makes an nvim editor block (#1734; see [`nvim_block`]).
    fn nvim_block(&self, mux: &MuxHandle, at: NvimBlockAt) -> BoxFuture<'static, Result<PaneId, String>>;
    /// Starts being Claude Code's IDE (M28), if it can.
    fn start_ide(
        &self,
        state_dir: &Path,
        lock_dir: Option<PathBuf>,
        home: &Path,
        launch: Launcher,
    ) -> BoxFuture<'static, Option<Arc<dyn Ide>>>;
    /// Runs `argv` if it's one of its own subcommands (`_vsix`,
    /// `_ide_relay`), before the daemon starts.
    fn subcommand(&self, argv: &[String]) -> Option<anyhow::Result<()>>;
}

/// Claude Code's IDE, as `App` holds it.
pub trait Ide: Send + Sync + std::fmt::Debug {
    /// The port Claude Code finds it by (`CLAUDE_CODE_SSE_PORT`).
    fn port(&self) -> u16;
    /// What it is here, for `GET /api/ide`.
    fn info(&self) -> IdeInfo;
    /// Where its diffs go now (its name, or another IDE's).
    fn diffs_to(&self) -> String;
    /// Send its diffs to another IDE (`Some`), or keep them (`None`).
    fn set_diffs_to(&self, name: Option<String>) -> io::Result<()>;
    /// Tell Claude Code something: one connection, or every one (`None`).
    fn notify(&self, conn: Option<u64>, method: &str, params: Value);
    /// Lets it follow the mux, once it runs.
    fn run(self: Arc<Self>, mux: MuxHandle);
    /// The edits Claude Code proposes, as the mux holds them (#699): cards
    /// that make a pane wait.
    fn cards(self: Arc<Self>) -> Box<dyn PaneCards>;
    /// The IDE connections of Claude Code in a pane.
    fn conns_of(&self, mux: &MuxHandle, pane: PaneId) -> BoxFuture<'static, Vec<u64>>;
    /// The edit a pane's diff card shows: before and after.
    fn diff_of(&self, mux: &MuxHandle, pane: PaneId) -> BoxFuture<'static, Option<(DiffInfo, String, String)>>;
    /// Someone accepted (`accept`) or rejected an edit waiting as a diff.
    #[allow(clippy::too_many_arguments)]
    fn answer_diff(
        &self,
        mux: &MuxHandle,
        pane: PaneId,
        id: Option<String>,
        accept: bool,
        text: Option<String>,
        by: Driver,
    ) -> BoxFuture<'static, Result<(), String>>;
}

static EDITORS: OnceLock<Box<dyn Editors>> = OnceLock::new();

/// Puts the editor in this daemon: the binary calls it before `main`.
pub fn register(e: Box<dyn Editors>) {
    let _ = EDITORS.set(e);
}

fn editors() -> Option<&'static dyn Editors> {
    EDITORS.get().map(|e| &**e)
}

/// What a request that needs the editor, in a build without it, answers.
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without the editor feature)")
}

/// A kind this build doesn't have: it says so ([`not_built`]).
struct NotBuilt(&'static str);

impl BlockKind for NotBuilt {
    fn create(&self, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
        Err(not_built(self.0))
    }
}

/// Registers the editor block type, or one that says it isn't built.
pub fn kinds(kinds: &mut BlockKinds) {
    match editors() {
        Some(e) => e.kinds(kinds),
        None => kinds.add(BlockType::Editor, NotBuilt("An editor block")),
    }
}

/// Adds the editor's HTTP routes to the API's: the extension's download, and
/// the IDE's settings and mentions.
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    match editors() {
        Some(e) => e.routes(r),
        None => r,
    }
}

/// Adds the route editors on this machine join the swarm on (M28), to the
/// Unix socket's router.
pub fn local_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    match editors() {
        Some(e) => e.local_routes(r),
        None => r,
    }
}

/// Whether editors can join here at all: the editor is in this build.
pub fn joinable() -> bool {
    editors().is_some()
}

/// Turns editor blocks on, if they're in this build.
pub fn install(s: Settings) {
    if let Some(e) = editors() {
        e.install(s);
    }
}

/// Starts looking for the editors that run in terminals (#1730), if the
/// editor is in this build: a pane running nvim shows its file, cursor and
/// diagnostics, without arugula.nvim.
pub fn watch_panes(mux: MuxHandle) {
    if let Some(e) = editors() {
        e.watch(mux);
    }
}

/// Opens `file` at `line` (from 1; `None`: wherever the editor had it) in
/// the nvim that terminal `pane` runs in its foreground, as Open in editor
/// does when the Editor preference is nvim (#1731). It goes over nvim's
/// own RPC socket, with or without arugula.nvim, and is nvim's `:drop`: the
/// file's window if it shows the file, else the current one.
///
/// `Err` says why it couldn't, for the person who asked: no such terminal,
/// it doesn't run nvim, its nvim can't be reached (older than 0.10, or its
/// server stopped), or the editor isn't in this build. The caller decides
/// what to do instead (a new nvim pane, say).
pub async fn open_in_pane(mux: &MuxHandle, pane: PaneId, file: &Path, line: Option<u32>) -> Result<(), String> {
    match editors() {
        Some(e) => e.open_in_pane(mux, pane, file, line).await,
        None => Err(not_built("Opening a file in nvim")),
    }
}

/// Where an nvim editor block goes, and what it opens (#1734).
#[derive(Debug, Clone, Default)]
pub struct NvimBlockAt {
    /// Beside this pane, on its machine.
    pub from: PaneId,
    /// The folder nvim starts in (none: the file's, else the pane's).
    pub dir: Option<String>,
    /// A file (whole, or relative to the folder) and its line.
    pub file: Option<String>,
    pub line: Option<u32>,
    /// On this host even if the pane's tab is a VM's.
    pub local: bool,
}

/// Makes an nvim editor block beside `at.from` (#1734): a terminal typed
/// as an editor, running nvim with arugula.nvim from the daemon and the
/// page's colors, on that pane's machine. Its pane. `Err` in a build
/// without the editor; the caller can run plain nvim instead.
pub async fn nvim_block(mux: &MuxHandle, at: NvimBlockAt) -> Result<PaneId, String> {
    match editors() {
        Some(e) => e.nvim_block(mux, at).await,
        None => Err(not_built("An nvim editor block")),
    }
}

/// Runs `argv` if it's one of the editor's subcommands.
pub fn subcommand(argv: &[String]) -> Option<anyhow::Result<()>> {
    editors()?.subcommand(argv)
}

/// Starts being Claude Code's IDE (M28), unless `off` says not to or it
/// can't be.
pub async fn start_ide(
    off: bool,
    lock_dir: Option<PathBuf>,
    state_dir: &Path,
    home: &Path,
    launch: Launcher,
) -> Option<Arc<dyn Ide>> {
    match editors() {
        Some(e) if !off => e.start_ide(state_dir, lock_dir, home, launch).await,
        _ => None,
    }
}

/// The edits Claude Code proposes through the IDE, as the mux holds them
/// (#699): cards that make a pane wait.
pub fn pane_cards(ide: &Option<Arc<dyn Ide>>) -> Option<Box<dyn PaneCards>> {
    ide.clone().map(|i| i.cards())
}

/// What every pane's environment gets from the IDE: the port Claude Code
/// finds it by.
pub fn pane_env(ide: &Option<Arc<dyn Ide>>) -> Vec<(String, String)> {
    ide.iter().map(|i| ("CLAUDE_CODE_SSE_PORT".to_owned(), i.port().to_string())).collect()
}

/// Someone accepted (`accept`) or rejected an edit waiting as a diff in a
/// pane, by its id or whichever waits; accepting may change it first (M28).
pub async fn answer_diff(
    ide: &Option<Arc<dyn Ide>>,
    mux: &MuxHandle,
    pane: PaneId,
    id: Option<String>,
    accept: bool,
    text: Option<String>,
    by: Driver,
) -> Result<(), String> {
    match ide {
        Some(i) => i.answer_diff(mux, pane, id, accept, text, by).await,
        None => Err("Arugula isn't Claude Code's IDE here".into()),
    }
}

/// The edit a pane's diff card shows: before and after.
pub async fn diff_of(ide: &Option<Arc<dyn Ide>>, mux: &MuxHandle, pane: PaneId) -> Option<(DiffInfo, String, String)> {
    ide.as_ref()?.diff_of(mux, pane).await
}

/// Lets the IDE follow the mux, once it runs.
pub fn run_ide(ide: &Option<Arc<dyn Ide>>, mux: MuxHandle) {
    if let Some(i) = ide {
        i.clone().run(mux);
    }
}
