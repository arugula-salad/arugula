//! Labs: the features that are not part of the core (Fountain, studio apps
//! with hud, chant workspaces, VMs with their sandboxes, guest ssh, huddles
//! and chat, and the Forgejo and GitLab forges: everything listed in
//! #452 to #457 is here), behind the cargo feature `labs`. The feature is on by default, so a release build has all of it;
//! `--no-default-features` leaves it out, for fast local builds, for agents
//! working on core, and for the CI job `core`. (The runtime switches, the
//! flags in the state dir, are separate: they decide what a machine
//! shows, and [`flags`] ANDs them with this build.)
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
//! filtered by [`flags`], not by the build). Only the daemon's handling is
//! gated.
//!
//! **Adding a feature.** Put its code in `labs/<name>/`, add
//! `#[cfg(feature = "labs")] pub mod <name>;` below, and add what core needs
//! to the surface, twin included: a route set goes into [`routes`], a block
//! type into [`kinds`], host info into the host functions. Where core
//! is too tangled for a function (an MCP handler, say), a `cfg` at the call
//! site with a twin next to it is the fallback; keep the list of those short.
//! Tests that need the feature are `#[cfg(feature = "labs")]`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::{BlockType, flags::On, hosts::FountainRunnerInfo};
use axum::Router;
#[cfg(not(feature = "labs"))]
use serde_json::Value;

#[cfg(not(feature = "labs"))]
use crate::block::{Block, BlockCtx, BlockKind};
#[cfg(feature = "forge")]
use crate::forge::model::Provider as ForgeProvider;
use crate::{block::BlockKinds, provider::Provider, server::App, shellenv::ShellEnv};

// Agent recipes and their A2A cards (M76): the `agents` flag.
#[cfg(feature = "labs")]
pub mod agents;
#[cfg(feature = "labs")]
pub mod apps;
// Forgejo and GitLab as forges (#457): their adapters, the `tea` logins
// Forgejo reads with, and the webhooks they send. GitHub is core's. The
// parts of a forge block that connect to them are `forge/labs_forges.rs`,
// which needs the block's private fields; `forge/mod.rs` has their twins.
#[cfg(feature = "labs")]
pub mod forge_live;
#[cfg(feature = "labs")]
pub mod forgejo;
#[cfg(feature = "labs")]
pub mod fountain;
#[cfg(feature = "labs")]
pub mod gitlab;
// Guest ssh: the russh server and its invites (M65). `russh` is an optional
// dependency of the daemon, enabled by this feature.
#[cfg(feature = "labs")]
pub mod guest_ssh;
// VMs: the machines panes run on, and the sandboxes they come from. The
// terminal on one is the core's (`arugula_mux::labs::machine`).
#[cfg(feature = "labs")]
pub mod provider_tunnel;
#[cfg(feature = "labs")]
pub mod resident;
// The tailnet sandbox supervisor: Linux boxes.
#[cfg(all(feature = "labs", unix))]
pub mod sandbox;
#[cfg(feature = "labs")]
pub mod sprites;
#[cfg(feature = "labs")]
pub mod tea;
// Chat (M61) and huddles (M63) are the core's: `arugula_mux::labs::{threads,
// calls}`, with the mux's handling in `mux/thread_ops.rs` and
// `mux/call_ops.rs` (twins in `mux/labs_off.rs`).
#[cfg(feature = "labs")]
pub mod workspace;

/// Where the guest ssh server listens unless `--guest-ssh` says otherwise.
/// (The flag is in every build, so the default is too.)
pub const GUEST_SSH_LISTEN: &str = "0.0.0.0:7684";

/// The raw stream kind control opens for a guest through its jump host.
pub const GUEST_STREAM_KIND: &[u8] = b"ssh-guest";

/// Whether this build has Labs.
pub const BUILT: bool = cfg!(feature = "labs");

pub use arugula_mux::labs::{not_built, thread_names};

/// The Labs flags that are on here: none in a build without Labs, else
/// what the machine's state directory says (read on each call).
pub fn flags(state_dir: &Path) -> On {
    if BUILT { arugula_proto::flags::on(state_dir) } else { On::none() }
}

/// Whether `flag` is on here: this build has Labs, and the machine has
/// turned the flag on.
#[cfg(feature = "forge")]
pub fn on(state_dir: &Path, flag: &str) -> bool {
    flags(state_dir).has(flag)
}

/// Whether this machine offers agents to its teams' members (#399): the
/// `agents` flag is on and at least one recipe is offered. Until then no
/// teammate's daemon gets in to a personal machine.
#[cfg(feature = "labs")]
pub fn offers_agents(state_dir: &Path) -> bool {
    on(state_dir, arugula_proto::flags::AGENTS) && !agents::offers(state_dir).is_empty()
}

#[cfg(not(feature = "labs"))]
pub fn offers_agents(_state_dir: &Path) -> bool {
    false
}

/// Follow again the tasks for this machine's agents (M78) that a restart
/// interrupted, and ask again for those waiting on the owner.
#[cfg(feature = "labs")]
pub fn resume_tasks(app: &Arc<App>) {
    if on(app.control.state_dir(), arugula_proto::flags::AGENTS) {
        agents::tasks::resume(app);
    }
}

#[cfg(not(feature = "labs"))]
pub fn resume_tasks(_app: &Arc<App>) {}

/// Adds Labs' HTTP routes to the API's.
#[cfg(feature = "labs")]
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    agents::routes(apps::routes::routes(r))
}

#[cfg(not(feature = "labs"))]
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r
}

/// Registers the Labs block types (the others are in `main.rs`'s table).
/// `invite` is what the agent catalog's card sends an invite with.
#[cfg(feature = "labs")]
pub fn kinds(kinds: &mut BlockKinds, invite: &crate::invite::Hook) {
    kinds.add(BlockType::Fountain, crate::block::Make(fountain::FountainBlock::create));
    kinds.add(BlockType::Agents, agents::block::AgentsKind(invite.clone()));
    kinds.add(BlockType::App, crate::block::Make(apps::AppBlock::create));
    kinds.add(BlockType::Workspace, crate::block::Make(workspace::Workspace::create));
}

/// A kind this build doesn't have: it says so ([`not_built`]).
#[cfg(not(feature = "labs"))]
struct NotBuilt(&'static str);

#[cfg(not(feature = "labs"))]
impl BlockKind for NotBuilt {
    fn create(&self, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
        Err(not_built(self.0))
    }
}

#[cfg(not(feature = "labs"))]
pub fn kinds(kinds: &mut BlockKinds, _invite: &crate::invite::Hook) {
    kinds.add(BlockType::Fountain, NotBuilt("A Fountain block"));
    kinds.add(BlockType::Agents, NotBuilt("The agent catalog"));
    kinds.add(BlockType::App, NotBuilt("An app block"));
    kinds.add(BlockType::Workspace, NotBuilt("A chant workspace"));
}

/// Whether a forge block of `provider` may be made or opened here. GitHub's
/// always may; Forgejo's and GitLab's are Labs, so they need this build to
/// have it and the machine to have turned on the `forges` flag.
#[cfg(feature = "forge")]
pub fn forge_allowed(provider: ForgeProvider, state_dir: &Path) -> Result<(), String> {
    let name = match provider {
        ForgeProvider::Github => return Ok(()),
        ForgeProvider::Forgejo => "Forgejo",
        ForgeProvider::Gitlab => "GitLab",
    };
    if on(state_dir, arugula_proto::flags::FORGES) {
        Ok(())
    } else if BUILT {
        Err(format!("{name} blocks are in Labs (turn Labs on to open them)"))
    } else {
        Err(not_built(&format!("A {name} block")))
    }
}

/// The daemon is stopping: stop every behold a workspace block runs for
/// its graph (#620), which is in a process group of its own. Blocks.
#[cfg(feature = "labs")]
pub fn stop_beholds() {
    workspace::stop_beholds();
}

#[cfg(not(feature = "labs"))]
pub fn stop_beholds() {}

/// Adds the Forgejo and GitLab webhook routes to the router, outside the
/// API's authorization: a delivery is trusted by its signature.
#[cfg(feature = "labs")]
pub fn forge_hook_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    forge_live::routes(r)
}

#[cfg(not(feature = "labs"))]
pub fn forge_hook_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r
}

/// Whether `path` is a forge webhook's, which the guard lets through to be
/// checked by its signature.
#[cfg(feature = "labs")]
pub fn is_forge_hook(path: &str) -> bool {
    forge_live::is_hook(path)
}

#[cfg(not(feature = "labs"))]
pub fn is_forge_hook(_path: &str) -> bool {
    false
}

/// The path a webhook for `provider` is made to deliver to.
#[cfg(all(feature = "labs", feature = "forge"))]
pub fn hook_path(provider: ForgeProvider) -> Result<&'static str, String> {
    Ok(forge_live::path(provider))
}

#[cfg(all(not(feature = "labs"), feature = "forge"))]
pub fn hook_path(provider: ForgeProvider) -> Result<&'static str, String> {
    Err(not_built(&format!("A {provider:?} webhook")))
}

/// `GET /api/fountain/agents`'s answer: Fountain's agents, or in a build
/// without Labs the 404 a route that isn't there gives.
#[cfg(feature = "labs")]
pub async fn fountain_agents(
    app: &crate::server::App,
    q: arugula_proto::api::FountainQuery,
) -> Result<arugula_proto::api::FountainAgents, crate::api::ApiError> {
    fountain::agents_answer(app, q).await
}

#[cfg(not(feature = "labs"))]
pub async fn fountain_agents(
    _: &crate::server::App,
    _: arugula_proto::api::FountainQuery,
) -> Result<arugula_proto::api::FountainAgents, crate::api::ApiError> {
    Err(crate::api::ApiError(axum::http::StatusCode::NOT_FOUND, not_built("Fountain")))
}

/// `POST /api/guests`'s answer: an ssh invite, or in a build without Labs
/// the 501 the route gave every method.
#[cfg(feature = "labs")]
pub async fn guest_mint(
    app: &Arc<App>,
    req: arugula_proto::api::GuestInviteRequest,
) -> Result<arugula_proto::api::GuestInvite, crate::api::ApiError> {
    guest_ssh::mint_invite(app, req).await
}

#[cfg(not(feature = "labs"))]
pub async fn guest_mint(
    _: &Arc<App>,
    _: arugula_proto::api::GuestInviteRequest,
) -> Result<arugula_proto::api::GuestInvite, crate::api::ApiError> {
    Err(guests_absent())
}

/// `GET /api/guests`'s answer (as `guest_mint`).
#[cfg(feature = "labs")]
pub fn guest_list(app: &App) -> Result<Vec<arugula_proto::api::GuestInvite>, crate::api::ApiError> {
    Ok(guest_ssh::list_invites(app))
}

#[cfg(not(feature = "labs"))]
pub fn guest_list(_: &App) -> Result<Vec<arugula_proto::api::GuestInvite>, crate::api::ApiError> {
    Err(guests_absent())
}

/// `DELETE /api/guests/{id}`'s answer (as `guest_mint`).
#[cfg(feature = "labs")]
pub async fn guest_revoke(app: &App, id: u32) -> Result<(), crate::api::ApiError> {
    guest_ssh::revoke_invite(app, id).await
}

#[cfg(not(feature = "labs"))]
pub async fn guest_revoke(_: &App, _: u32) -> Result<(), crate::api::ApiError> {
    Err(guests_absent())
}

/// What a build without Labs answers to `/api/guests`.
#[cfg(not(feature = "labs"))]
fn guests_absent() -> crate::api::ApiError {
    crate::api::ApiError(axum::http::StatusCode::NOT_IMPLEMENTED, not_built("Guest ssh"))
}

/// Studio's and the sandboxes' operations (#578): each calls its Labs
/// function here. The routes are Labs' (`apps/routes.rs`, `resident.rs`), so a
/// build without Labs has none: its twins are never reached over HTTP, where a
/// path no router has is the 404 it always was, and say the same if they were.
#[cfg(feature = "labs")]
pub use apps::routes::{studio_apps, studio_follow, studio_login, studio_logout, studio_status};
#[cfg(feature = "labs")]
pub use resident::{
    Failed as SandboxFailed, demote as sandbox_demote, list as sandboxes, make_resident as sandbox_promote,
};

#[cfg(not(feature = "labs"))]
mod studio_off {
    use arugula_proto::api::{Empty, StudioApps, StudioLoggedIn, StudioLoginRequest, StudioStatus};
    use axum::http::StatusCode;

    use super::not_built;
    use crate::{api::ApiError, server::App};

    fn gone<T>() -> Result<T, ApiError> {
        Err(ApiError(StatusCode::NOT_FOUND, not_built("Studio")))
    }

    pub async fn studio_status() -> Result<StudioStatus, ApiError> {
        gone()
    }

    pub async fn studio_login(_: StudioLoginRequest) -> Result<StudioLoggedIn, ApiError> {
        gone()
    }

    pub async fn studio_logout() -> Result<Empty, ApiError> {
        gone()
    }

    pub async fn studio_apps(_: &App) -> Result<StudioApps, ApiError> {
        gone()
    }

    pub async fn studio_follow(_: &str, _: Option<&str>) -> Result<Empty, ApiError> {
        gone()
    }
}

#[cfg(not(feature = "labs"))]
pub use studio_off::{studio_apps, studio_follow, studio_login, studio_logout, studio_status};

#[cfg(not(feature = "labs"))]
mod sandboxes_off {
    use arugula_proto::hosts::{Demoted, Host, PromoteRequest, SandboxList};
    use axum::http::StatusCode;

    use super::not_built;
    use crate::server::App;

    pub type SandboxFailed = (StatusCode, String);

    fn gone<T>() -> Result<T, SandboxFailed> {
        Err((StatusCode::NOT_FOUND, not_built("Sandboxes")))
    }

    pub async fn sandboxes(_: &App) -> Result<SandboxList, SandboxFailed> {
        gone()
    }

    pub async fn sandbox_promote(_: &App, _: &str, _: PromoteRequest) -> Result<Host, SandboxFailed> {
        gone()
    }

    pub async fn sandbox_demote(_: &App, _: &str) -> Result<Demoted, SandboxFailed> {
        gone()
    }
}

#[cfg(not(feature = "labs"))]
pub use sandboxes_off::{SandboxFailed, sandbox_demote, sandbox_promote, sandboxes};

/// Whether chat is in this build: `Ok`, or the refusal for a request that
/// needs it.
#[cfg(feature = "labs")]
pub fn chat() -> Result<(), String> {
    Ok(())
}

#[cfg(not(feature = "labs"))]
pub fn chat() -> Result<(), String> {
    Err(not_built("Chat"))
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

/// #619: which decision and run made each hunk of a file in a workspace
/// member's *Changes*, from the workspace's chant: [`file_why`] reads it,
/// and [`FileWhy::hunk_json`] answers for a hunk's added lines.
#[cfg(feature = "labs")]
pub use workspace::why::{Why as FileWhy, read as file_why};

#[cfg(not(feature = "labs"))]
pub struct FileWhy;

#[cfg(not(feature = "labs"))]
impl FileWhy {
    pub fn hunk_json(&self, _added: &[u32]) -> Option<Value> {
        None
    }
}

#[cfg(not(feature = "labs"))]
pub async fn file_why(
    _ctx: &BlockCtx,
    _root: &str,
    _chant: Option<&str>,
    _file: &str,
    _at: Option<&str>,
) -> Result<FileWhy, String> {
    Err(not_built("Which decision and run made a hunk"))
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

/// A recipe (a Claude Code subagent file, M76) worn on this host by a block
/// working in `cwd`, as [`wear`] wears a Fountain agent. Needs the `agents`
/// flag.
#[cfg(feature = "labs")]
pub async fn wear_recipe(
    runner: &crate::review::Runner,
    state_dir: &Path,
    cwd: &Path,
    name: &str,
) -> Result<Worn, String> {
    if !on(state_dir, arugula_proto::flags::AGENTS) {
        return Err("agent recipes are in Labs: `arugulad flags agents on` turns them on here".into());
    }
    agents::wear::wear(runner, cwd, name).await
}

#[cfg(not(feature = "labs"))]
pub async fn wear_recipe(
    _runner: &crate::review::Runner,
    _state_dir: &Path,
    _cwd: &Path,
    _name: &str,
) -> Result<Worn, String> {
    Err(not_built("Agent recipes"))
}

/// The twins of what [`Worn`] is, for a build without Labs: nothing is ever
/// worn, so an agent block's `worn` stays `None`.
#[cfg(not(feature = "labs"))]
mod absent {
    use std::sync::Arc;

    use serde_json::Value;

    use crate::{review::Runner, server::App};

    /// Uninhabited: no build without Labs makes a daemon resident in a
    /// sandbox, so there are never binaries to copy in.
    #[derive(Debug, Clone)]
    pub enum Binaries {}

    /// The guest invites of a build without Labs: none, ever. It still sits
    /// on the control link's text channel, so its routes watch is kept open
    /// (holding nothing): a closed one would end the link's loop.
    #[derive(Default)]
    pub struct Guests {
        routes_out: tokio::sync::watch::Sender<Option<String>>,
    }

    impl Guests {
        pub fn run(self: &Arc<Self>, _app: &Arc<App>) {}

        /// Nothing to tell control about routes.
        pub fn routes_messages(&self) -> tokio::sync::watch::Receiver<Option<String>> {
            self.routes_out.subscribe()
        }

        /// Never ours, so the text goes on to the forge.
        pub fn heard_from_control(&self, _text: &str) -> bool {
            false
        }

        /// Dropped: control has no guest to hand through.
        pub fn serve_relayed(self: &Arc<Self>, _app: &Arc<App>, _stream: tokio::io::DuplexStream) {}
    }

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

        pub fn permission_mode(&self) -> Option<&str> {
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

/// Adds what makes a daemon a home daemon for VMs: its provider's sandboxes
/// (making a daemon resident in one), and the provider tunnel to the
/// resident daemons in them. And the owner's guest ssh invites
/// (`/api/guests`), which a build without Labs answers with 501 and "Guest
/// ssh isn't in this build", so `arugula share --guest` says so.
#[cfg(feature = "labs")]
pub fn home_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.merge(resident::routes()).merge(provider_tunnel::routes()).merge(guest_ssh::routes())
}

#[cfg(not(feature = "labs"))]
pub fn home_routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    use axum::{Json, http::StatusCode, response::IntoResponse, routing::any};

    async fn no_guests() -> axum::response::Response {
        (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({ "error": not_built("Guest ssh") }))).into_response()
    }
    r.route("/api/guests", any(no_guests)).route("/api/guests/{id}", any(no_guests))
}

/// Control's ssh jump host for guests, as its `/control.json` describes it.
#[cfg(feature = "labs")]
pub use guest_ssh::Jump as GuestJump;

/// The invites for guests with only OpenSSH (M65), and the ssh server for
/// them. Always made, so it has a twin that holds nothing.
#[cfg(feature = "labs")]
pub use guest_ssh::Guests;

#[cfg(not(feature = "labs"))]
pub use absent::Guests;

/// Opens the guest invites (`given`: `--guest-ssh` or `--guest-ssh-host`
/// wasn't left at its default). A build without Labs has none: flags given are
/// ignored, with a warning.
#[cfg(feature = "labs")]
pub fn open_guests(
    state_dir: &Path,
    listen: Option<std::net::SocketAddr>,
    host: Option<String>,
    _given: bool,
) -> Arc<Guests> {
    Guests::open(state_dir, listen, host)
}

#[cfg(not(feature = "labs"))]
pub fn open_guests(
    _state_dir: &Path,
    _listen: Option<std::net::SocketAddr>,
    _host: Option<String>,
    given: bool,
) -> Arc<Guests> {
    if given {
        tracing::warn!("{}", not_built("Guest ssh"));
    }
    Arc::new(Guests::default())
}

/// The sandbox provider VM panes get their machines from, if there is one:
/// the Sprites API (wisp, or Fly's) at `url`, with the token in
/// `token_file`. A build without Labs has none: a provider given
/// (`given`: the flags weren't left at their defaults) is ignored, with a
/// warning.
#[cfg(feature = "labs")]
pub fn open_provider(url: &str, token_file: &Path, _given: bool) -> Option<Arc<dyn Provider>> {
    sprites::Sprites::open(url, token_file).map(|p| Arc::new(p) as Arc<dyn Provider>)
}

#[cfg(not(feature = "labs"))]
pub fn open_provider(url: &str, _token_file: &Path, given: bool) -> Option<Arc<dyn Provider>> {
    if given {
        tracing::warn!(url, "{}", not_built("VMs"));
    }
    None
}

/// The static binaries to copy into a sandbox when making a daemon resident
/// there.
#[cfg(feature = "labs")]
pub use resident::Binaries;

#[cfg(not(feature = "labs"))]
pub use absent::Binaries;

/// The static binaries in `dir`, if they are there.
#[cfg(feature = "labs")]
pub fn binaries(dir: PathBuf) -> Option<Binaries> {
    dir.join("arugulad").exists().then_some(Binaries { dir })
}

#[cfg(not(feature = "labs"))]
pub fn binaries(_dir: PathBuf) -> Option<Binaries> {
    None
}

/// `arugulad install --tailnet`: joins a tailnet with a userspace tailscaled
/// and keeps the daemon running there (for machines without systemd).
#[cfg(all(feature = "labs", unix))]
pub fn tailnet_install(cmd: crate::args::Command) -> anyhow::Result<()> {
    let crate::args::Command::Install {
        tailnet: Some(authkey),
        home,
        join,
        owner,
        hostname,
        port,
        no_serve,
        daemon_args,
        ..
    } = cmd
    else {
        anyhow::bail!("not a tailnet install")
    };
    sandbox::install(sandbox::TailnetOpts { authkey, hostname, home, join, owner, port, no_serve, daemon_args })
}

#[cfg(all(not(feature = "labs"), unix))]
pub fn tailnet_install(_cmd: crate::args::Command) -> anyhow::Result<()> {
    anyhow::bail!(not_built("A tailnet sandbox"))
}

/// `arugulad sandbox`: keeps tailscaled and the daemon running, as
/// `install --tailnet` set them up.
#[cfg(all(feature = "labs", unix))]
pub fn supervise_sandbox() -> anyhow::Result<()> {
    sandbox::supervise()
}

#[cfg(all(not(feature = "labs"), unix))]
pub fn supervise_sandbox() -> anyhow::Result<()> {
    anyhow::bail!(not_built("The sandbox supervisor"))
}

#[cfg(not(unix))]
pub fn supervise_sandbox() -> anyhow::Result<()> {
    anyhow::bail!("the sandbox supervisor is for Linux boxes")
}
