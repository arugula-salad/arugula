//! The editor server (`editor/`: VS Code as code-server in an editor block,
//! the editors that join the swarm, Arugula's extension) and Claude Code's
//! IDE (`ide/`: the daemon as the IDE Claude Code connects to), behind the
//! cargo feature `editor`. It's on by default; `--no-default-features`
//! leaves it out.
//!
//! **The pattern** is Labs' (see `labs/mod.rs`): `editor/` and `ide/` are
//! declared in `main.rs` under the feature, and nothing outside them names
//! them but this file. What the rest of the daemon calls is below, each with
//! a twin under `#[cfg(not(feature = "editor"))]` that does nothing or says
//! [`not_built`], so no caller needs a `cfg` of its own. A build without the
//! feature has no editor blocks (a saved one says so when it's opened), no
//! socket for editors to join on, so none ever does, no extension to
//! download and no IDE settings. The mux's half of each is the core's
//! (the `EditorLink` and `PaneCards` traits).
//!
//! Tests that need the editor are `#[cfg(feature = "editor")]`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::{BlockType, DiffInfo, Driver, PaneId};
use axum::Router;
#[cfg(not(feature = "editor"))]
use serde_json::Value;

use crate::block::BlockKinds;
#[cfg(not(feature = "editor"))]
use crate::block::{Block, BlockCtx, BlockKind};
#[cfg(feature = "editor")]
use crate::mux::Api;
use crate::{mux::MuxHandle, pane::Launcher, server::App};

/// Where code-server's releases are downloaded from. (The flag that says
/// otherwise is in every build, so its default is too.)
pub const RELEASES: &str = "https://github.com/coder/code-server/releases/download";

/// What a request that needs the editor, in a build without it, answers.
#[cfg(not(feature = "editor"))]
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without the editor feature)")
}

/// Claude Code's IDE, as `App` holds it.
#[cfg(feature = "editor")]
pub use crate::ide::Ide;

/// There is none: a build without the editor is never Claude Code's IDE.
#[cfg(not(feature = "editor"))]
pub enum Ide {}

/// Registers the editor block type.
#[cfg(feature = "editor")]
pub fn kinds(kinds: &mut BlockKinds) {
    kinds.add(BlockType::Editor, crate::block::Make(crate::editor::Editor::create));
}

/// A kind this build doesn't have: it says so ([`not_built`]).
#[cfg(not(feature = "editor"))]
struct NotBuilt(&'static str);

#[cfg(not(feature = "editor"))]
impl BlockKind for NotBuilt {
    fn create(&self, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
        Err(not_built(self.0))
    }
}

#[cfg(not(feature = "editor"))]
pub fn kinds(kinds: &mut BlockKinds) {
    kinds.add(BlockType::Editor, NotBuilt("An editor block"));
}

/// Adds the editor's HTTP routes to the API's: the extension's download, and
/// the IDE's settings and mentions.
#[cfg(feature = "editor")]
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    use crate::ops::OpRoutes;
    use arugula_proto::op::ops::{IdeGet, IdeMention, IdeSet};
    r.route("/api/editors/vsix", axum::routing::get(crate::editor::vsix::download))
        .op::<IdeGet>()
        .op::<IdeSet>()
        .op::<IdeMention>()
}

#[cfg(not(feature = "editor"))]
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r
}

/// Adds the route editors on this machine join the swarm on (M28), to the
/// Unix socket's router.
#[cfg(feature = "editor")]
pub fn local_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route("/api/editors/connect", axum::routing::get(crate::editor::link::connect))
}

#[cfg(not(feature = "editor"))]
pub fn local_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r
}

/// Starts being Claude Code's IDE (M28), unless `off` says not to or it
/// can't be.
#[cfg(feature = "editor")]
pub async fn start_ide(
    off: bool,
    lock_dir: Option<PathBuf>,
    state_dir: &Path,
    home: &Path,
    launch: Launcher,
) -> Option<Arc<Ide>> {
    if off {
        return None;
    }
    let lock_dir = lock_dir.unwrap_or_else(|| crate::ide::default_lock_dir(home));
    match Ide::start(state_dir.join("ide"), lock_dir, launch).await {
        Ok(i) => Some(i),
        Err(e) => {
            tracing::warn!(error = %e, "can't be Claude Code's IDE");
            None
        }
    }
}

#[cfg(not(feature = "editor"))]
pub async fn start_ide(
    _off: bool,
    _lock_dir: Option<PathBuf>,
    _state_dir: &Path,
    _home: &Path,
    _launch: Launcher,
) -> Option<Arc<Ide>> {
    None
}

/// The edits Claude Code proposes through the IDE, as the mux holds them
/// (#699): cards that make a pane wait.
#[cfg(feature = "editor")]
pub fn pane_cards(ide: &Option<Arc<Ide>>) -> Option<Box<dyn crate::mux::PaneCards>> {
    ide.clone().map(|i| Box::new(crate::ide::Cards::new(i)) as Box<dyn crate::mux::PaneCards>)
}

#[cfg(not(feature = "editor"))]
pub fn pane_cards(_ide: &Option<Arc<Ide>>) -> Option<Box<dyn crate::mux::PaneCards>> {
    None
}

/// What every pane's environment gets from the IDE: the port Claude Code
/// finds it by.
#[cfg(feature = "editor")]
pub fn pane_env(ide: &Option<Arc<Ide>>) -> Vec<(String, String)> {
    ide.iter().map(|i| ("CLAUDE_CODE_SSE_PORT".to_owned(), i.port.to_string())).collect()
}

#[cfg(not(feature = "editor"))]
pub fn pane_env(_ide: &Option<Arc<Ide>>) -> Vec<(String, String)> {
    Vec::new()
}

/// Someone accepted (`accept`) or rejected an edit waiting as a diff in a
/// pane, by its id or whichever waits; accepting may change it first (M28).
#[cfg(feature = "editor")]
pub async fn answer_diff(
    ide: &Option<Arc<Ide>>,
    mux: &MuxHandle,
    pane: PaneId,
    id: Option<String>,
    accept: bool,
    text: Option<String>,
    by: Driver,
) -> Result<(), String> {
    if ide.is_none() {
        return Err("Arugula isn't Claude Code's IDE here".into());
    }
    mux.api(|r| {
        Api::Cards(Box::new(move |cards, cx| {
            if let Some(c) = crate::ide::Cards::of(cards) {
                let _ = r.send(c.answer(cx, pane, id, accept, text, by));
            }
        }))
    })
    .await
    .unwrap_or_else(|| Err("the daemon is stopping".into()))
}

#[cfg(not(feature = "editor"))]
pub async fn answer_diff(
    _ide: &Option<Arc<Ide>>,
    _mux: &MuxHandle,
    _pane: PaneId,
    _id: Option<String>,
    _accept: bool,
    _text: Option<String>,
    _by: Driver,
) -> Result<(), String> {
    Err("Arugula isn't Claude Code's IDE here".into())
}

/// The edit a pane's diff card shows: before and after.
#[cfg(feature = "editor")]
pub async fn diff_of(mux: &MuxHandle, pane: PaneId) -> Option<(DiffInfo, String, String)> {
    mux.api(|r| {
        Api::Cards(Box::new(move |cards, _| {
            if let Some(c) = crate::ide::Cards::of(cards) {
                let _ = r.send(c.diff_of(pane));
            }
        }))
    })
    .await
    .flatten()
}

#[cfg(not(feature = "editor"))]
pub async fn diff_of(_mux: &MuxHandle, _pane: PaneId) -> Option<(DiffInfo, String, String)> {
    None
}

/// Lets the IDE follow the mux, once it runs.
#[cfg(feature = "editor")]
pub fn run_ide(ide: &Option<Arc<Ide>>, mux: MuxHandle) {
    if let Some(i) = ide {
        i.run(mux);
    }
}

#[cfg(not(feature = "editor"))]
pub fn run_ide(_ide: &Option<Arc<Ide>>, _mux: MuxHandle) {}
