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
//! (`arugula_mux::ide`, the `EditorLink` and `IdeLink` traits).
//!
//! Tests that need the editor are `#[cfg(feature = "editor")]`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::BlockType;
use axum::Router;
#[cfg(not(feature = "editor"))]
use serde_json::Value;

use crate::block::BlockKinds;
#[cfg(not(feature = "editor"))]
use crate::block::{Block, BlockCtx, BlockKind};
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

/// The IDE as the mux calls it.
#[cfg(feature = "editor")]
pub fn ide_link(ide: &Option<Arc<Ide>>) -> Option<Arc<dyn crate::mux::IdeLink>> {
    ide.clone().map(|i| i as Arc<dyn crate::mux::IdeLink>)
}

#[cfg(not(feature = "editor"))]
pub fn ide_link(_ide: &Option<Arc<Ide>>) -> Option<Arc<dyn crate::mux::IdeLink>> {
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
