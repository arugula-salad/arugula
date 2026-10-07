//! Labs: the features that are not part of the core (Fountain, studio apps
//! with hud, and chant workspaces so far; VMs, guest ssh, huddles, chat and
//! the other forges move here in #454 to #457), behind the cargo feature
//! `labs`. The feature is on by default, so a release build has all of it;
//! `--no-default-features` leaves it out, for fast local builds, for agents
//! working on core, and for the CI job `core`. (The runtime switch, the
//! `labs` file in the state dir, is separate: it decides what a machine
//! shows, and [`enabled`] ANDs it with this build.)
//!
//! **The pattern.** A Labs feature is a module in this directory, declared
//! here under `#[cfg(feature = "labs")]`, and nothing outside `labs/` names
//! it. Core reaches it only through the functions and types below, which are
//! the surface. Each has two definitions: one that calls the feature, and,
//! under `#[cfg(not(feature = "labs"))]`, a twin that does nothing or says
//! [`not_built`]. So core never needs a `cfg` of its own to call Labs. The
//! twin never panics and never quietly does something else: a request that
//! needs a feature this build lacks fails with "X isn't in this build".
//!
//! **What stays unconditional.** Everything in `arugula-proto` (block types,
//! agent kinds, request and answer types), so every client and daemon speaks
//! one protocol, and the argument structs of MCP's tools (their schemas are
//! filtered by [`enabled`], not by the build). Only the daemon's handling is
//! gated.
//!
//! **Adding a feature.** Put its code in `labs/<name>/`, add
//! `#[cfg(feature = "labs")] pub mod <name>;` below, and add what core needs
//! to the surface, twin included: a route set goes into [`routes`], a block
//! type into [`create_block`], host info into the host functions. Where core
//! is too tangled for a function (an MCP handler, say), a `cfg` at the call
//! site with a twin next to it is the fallback; keep the list of those short.
//! Tests that need the feature are `#[cfg(feature = "labs")]`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::{BlockType, hosts::FountainRunnerInfo};
use axum::Router;
use serde_json::Value;

use crate::{
    block::{Block, BlockCtx},
    server::App,
    shellenv::ShellEnv,
};

#[cfg(feature = "labs")]
pub mod apps;
#[cfg(feature = "labs")]
pub mod fountain;
#[cfg(feature = "labs")]
pub mod workspace;

/// Whether this build has Labs.
pub const BUILT: bool = cfg!(feature = "labs");

/// What a request that needs a Labs feature this build lacks answers.
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without labs)")
}

/// Whether Labs is on here: this build has it, and the machine has the
/// `labs` file in its state directory (read on each call).
pub fn enabled(state_dir: &Path) -> bool {
    BUILT && arugula_proto::hosts::labs(state_dir)
}

/// Adds Labs' HTTP routes to the API's.
#[cfg(feature = "labs")]
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    apps::routes::routes(fountain::routes::routes(r))
}

#[cfg(not(feature = "labs"))]
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r
}

/// Makes a block of a Labs type (the others are [`crate::block::create`]'s).
#[cfg(feature = "labs")]
pub fn create_block(kind: BlockType, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
    match kind {
        BlockType::Fountain => fountain::FountainBlock::create(ctx, config),
        BlockType::App => apps::AppBlock::create(ctx, config),
        BlockType::Workspace => workspace::Workspace::create(ctx, config),
        other => Err(format!("{other:?} isn't a Labs block")),
    }
}

#[cfg(not(feature = "labs"))]
pub fn create_block(kind: BlockType, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
    Err(not_built(&match kind {
        BlockType::App => "An app block".to_owned(),
        BlockType::Workspace => "A chant workspace".to_owned(),
        kind => format!("A {kind:?} block"),
    }))
}

/// Sets up the studio token store (M35) at start-up. A build without Labs
/// has no studio: a file given with `--studio` is ignored, with a warning.
#[cfg(feature = "labs")]
pub fn install_studio(file: PathBuf, _given: bool) {
    apps::studio::install(file);
}

#[cfg(not(feature = "labs"))]
pub fn install_studio(file: PathBuf, given: bool) {
    if given {
        tracing::warn!(file = %file.display(), "{}", not_built("Studio"));
    }
}

/// Whether the user is logged in to a studio: `GET /api/host`'s
/// `features.studio`.
#[cfg(feature = "labs")]
pub fn studio_here() -> bool {
    apps::studio::get().and_then(|s| s.url()).is_some()
}

#[cfg(not(feature = "labs"))]
pub fn studio_here() -> bool {
    false
}

/// A studio app block's config (M35) from `{app}`: the box and studio filled
/// in from studio's list when not given.
#[cfg(feature = "labs")]
pub use apps::routes::app_config;

#[cfg(not(feature = "labs"))]
pub async fn app_config(_c: &Value) -> Result<Value, String> {
    Err(not_built("An app block"))
}

/// `GET /api/host`'s `fountain_runner` (M45b): only where the runner's unit
/// is, from what was last read.
#[cfg(feature = "labs")]
pub fn fountain_runner_info(shell_env: &Arc<ShellEnv>) -> Option<FountainRunnerInfo> {
    fountain::runner::host_info(shell_env)
}

#[cfg(not(feature = "labs"))]
pub fn fountain_runner_info(_shell_env: &Arc<ShellEnv>) -> Option<FountainRunnerInfo> {
    None
}

/// Whether a Fountain login is here for the catalog to find: `GET
/// /api/host`'s `features.fountain`.
#[cfg(feature = "labs")]
pub fn fountain_login_here(shell_env: &ShellEnv) -> bool {
    fountain::login::here(shell_env)
}

#[cfg(not(feature = "labs"))]
pub fn fountain_login_here(_shell_env: &ShellEnv) -> bool {
    false
}

/// A diff block's `run_as`, checked when it is made (only the Fountain
/// runner's user, M45b).
#[cfg(feature = "labs")]
pub use fountain::runner::check_run_as;

#[cfg(not(feature = "labs"))]
pub fn check_run_as(_user: &str, _repo: &str, _on_sprite: bool) -> Result<(), String> {
    Err(not_built("A diff run as the Fountain runner"))
}

/// A diff block's git, run as the Fountain runner's user.
#[cfg(feature = "labs")]
pub use fountain::runner::git_as_runner;

#[cfg(not(feature = "labs"))]
pub async fn git_as_runner(_script: &str, _args: &[String]) -> Result<(Vec<u8>, Option<i32>), String> {
    Err(not_built("A diff run as the Fountain runner"))
}

/// Whether the Fountain agent `name` can be worn on this host (M44), or why
/// not.
#[cfg(feature = "labs")]
pub async fn check_wearable(shell_env: &ShellEnv, name: &str) -> Result<(), String> {
    let runner = fountain::local_runner(shell_env).await;
    fountain::wear::find(&runner, None, name).await.map(|_| ())
}

#[cfg(not(feature = "labs"))]
pub async fn check_wearable(_shell_env: &ShellEnv, _name: &str) -> Result<(), String> {
    Err(not_built("Fountain"))
}

/// A Fountain agent worn on this host (M44): put on, what it adds to an
/// agent block, and the secrets to keep out of every log.
#[cfg(feature = "labs")]
pub use fountain::wear::{Worn, scrub, wear};

#[cfg(not(feature = "labs"))]
pub use absent::{Worn, scrub, wear};

/// The twins of what [`Worn`] is, for a build without Labs: nothing is ever
/// worn, so an agent block's `worn` stays `None`.
#[cfg(not(feature = "labs"))]
mod absent {
    use serde_json::Value;

    use crate::review::Runner;

    /// Uninhabited: no build without Labs puts an agent on.
    pub enum Worn {}

    impl Worn {
        pub fn dress(&self, _meta: &mut Value) {
            match *self {}
        }

        pub fn servers(&self) -> &[Value] {
            match *self {}
        }

        pub fn env(&self) -> &[(String, String)] {
            match *self {}
        }

        pub fn secrets(&self) -> &[String] {
            match *self {}
        }

        pub fn info(&self) -> Value {
            match *self {}
        }

        pub fn text(&self) -> String {
            match *self {}
        }
    }

    pub async fn wear(
        _runner: &Runner,
        _profile: Option<&str>,
        _specs: Option<&str>,
        _vault: Option<&str>,
        _which: &str,
    ) -> Result<Worn, String> {
        Err(super::not_built("Fountain"))
    }

    pub fn scrub(_line: &str, _secrets: &[String]) -> Option<String> {
        None
    }
}
