//! Labs' surface (#232): what the rest of the daemon asks of Labs, as one
//! trait, [`Hooks`], which Labs registers ([`install`]) before the daemon
//! starts. Each default is what a build without Labs does: nothing, or
//! [`not_built`]. So no caller names Labs or needs a `cfg` of its own to
//! call it, and Labs can become a crate of its own (`arugulad-labs`) that
//! the base never depends on. (Labs' flags are [`crate::lab_flags`].)

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use arugula_proto::BlockType;
use axum::Router;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::{
    agent::Worn,
    block::{Block, BlockCtx, BlockKind, BlockKinds},
    lab_flags::not_built,
    mcp::{
        results::ForgeSignIn,
        tools::{Call, DelegateArgs, ListTeamAgentsArgs, OpenAppArgs, Out},
    },
    provider::Provider,
    review::Runner,
    server::App,
};

/// Where the guest ssh server listens unless `--guest-ssh` says otherwise.
/// (The flag is in every build, so the default is too.)
pub const GUEST_SSH_LISTEN: &str = "0.0.0.0:7684";

/// What Labs is given as the daemon starts, before the app is made.
pub struct Start {
    pub state_dir: PathBuf,
    /// Where the guest ssh server listens (`--guest-ssh`), if it does.
    pub guest_listen: Option<SocketAddr>,
    /// The address guests are told to ssh to (`--guest-ssh-host`).
    pub guest_host: Option<String>,
    /// `--guest-ssh` or `--guest-ssh-host` wasn't left at its default.
    pub guest_given: bool,
    /// Where the static binaries a resident daemon runs are.
    pub static_dir: PathBuf,
}

/// Which decision and run made each hunk of a file (#619), as Labs reads it
/// from a workspace's chant.
pub trait FileWhy: Send + Sync {
    /// The why of a hunk with these added lines, as the diff block's state
    /// carries it.
    fn hunk_json(&self, added: &[u32]) -> Option<Value>;
}

/// Labs, as the daemon calls it. Every method has the default a build
/// without Labs has.
pub trait Hooks: Send + Sync + 'static {
    // ------------------------------------------------------------ start-up

    /// `arugulad install --tailnet`: joins a tailnet with a userspace
    /// tailscaled and keeps the daemon running there.
    #[cfg(unix)]
    fn tailnet_install(&self, _cmd: crate::args::Command) -> anyhow::Result<()> {
        anyhow::bail!(not_built("A tailnet sandbox"))
    }

    /// `arugulad sandbox`: keeps tailscaled and the daemon running, as
    /// `install --tailnet` set them up.
    fn supervise_sandbox(&self) -> anyhow::Result<()> {
        if cfg!(unix) {
            anyhow::bail!(not_built("The sandbox supervisor"))
        }
        anyhow::bail!("the sandbox supervisor is for Linux boxes")
    }

    /// The sandbox provider VM panes get their machines from: the Sprites
    /// API at `url`, with the token in `token_file`. `given`: the flags
    /// weren't left at their defaults.
    fn open_provider(&self, url: &str, _token_file: &Path, given: bool) -> Option<Arc<dyn Provider>> {
        if given {
            tracing::warn!(url, "{}", not_built("VMs"));
        }
        None
    }

    /// The sandbox provider when it's loam: its API at `url`, with a
    /// project key in `key_file`.
    fn open_loam(&self, url: &str, _key_file: &Path) -> Option<Arc<dyn Provider>> {
        tracing::warn!(url, "{}", not_built("VMs"));
        None
    }

    /// Sets up the studio token store (M35). `given`: `--studio-file` was.
    fn install_studio(&self, file: PathBuf, given: bool) {
        if given {
            tracing::warn!(file = %file.display(), "{}", not_built("Studio"));
        }
    }

    /// Opens what Labs keeps for the daemon's life (the guest invites, the
    /// static binaries), before the app is made.
    fn open(&self, start: Start) {
        if start.guest_given {
            tracing::warn!("{}", not_built("Guest ssh"));
        }
    }

    /// Registers Labs' block types. `invite` is what the agent catalog's
    /// card sends an invite with.
    fn kinds(&self, kinds: &mut BlockKinds, _invite: &crate::invite::Hook) {
        kinds.add(BlockType::Agents, NotBuilt("The agent catalog"));
        kinds.add(BlockType::App, NotBuilt("An app block"));
        kinds.add(BlockType::Workspace, NotBuilt("A chant workspace"));
    }

    /// The app is made: start what runs beside it (the guest ssh server,
    /// the agents' tasks a restart interrupted).
    fn start(&self, _app: &Arc<App>) {}

    /// The daemon is stopping. Blocks.
    fn at_exit(&self) {}

    // ------------------------------------------------------------ routes

    /// Adds Labs' routes to the API's.
    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r
    }

    /// Adds what makes a daemon a home daemon for VMs (its provider's
    /// sandboxes, the tunnel to resident daemons) and the owner's guest ssh
    /// invites (`/api/guests`, which a build without Labs answers with 501
    /// and "Guest ssh isn't in this build", so `arugula share --guest` says
    /// so).
    fn home_routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        use axum::{Json, http::StatusCode, response::IntoResponse, routing::any};

        async fn no_guests() -> axum::response::Response {
            (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({ "error": not_built("Guest ssh") }))).into_response()
        }
        r.route("/api/guests", any(no_guests)).route("/api/guests/{id}", any(no_guests))
    }

    /// Adds the routes outside the API's authorization that are trusted by
    /// a signature: Forgejo's and GitLab's webhooks.
    fn hook_routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r
    }

    /// Whether `path` is one of [`Self::hook_routes`]', which the guard
    /// lets through to be checked by its signature.
    fn is_hook(&self, _path: &str) -> bool {
        false
    }

    /// The path a webhook for `provider` is made to deliver to.
    #[cfg(feature = "forge")]
    fn hook_path(&self, provider: crate::forge::model::Provider) -> Result<&'static str, String> {
        Err(not_built(&format!("A {provider:?} webhook")))
    }

    /// A Forgejo block's `tea` login and adapter (`forge::TeaAsk`).
    #[cfg(feature = "forge")]
    fn tea_connect<'a>(
        &'a self,
        _runner: Runner,
        _ask: crate::forge::TeaAsk,
    ) -> BoxFuture<'a, Result<crate::forge::TeaFound, crate::forge::TeaMissed>> {
        Box::pin(async { Err(crate::forge::TeaMissed { candidates: None, error: not_built("A Forgejo block") }) })
    }

    /// A Forgejo block's `login {name}`: that `tea` login and its adapter.
    #[cfg(feature = "forge")]
    fn tea_login<'a>(
        &'a self,
        _runner: Runner,
        _provider: crate::forge::model::Provider,
        _name: &'a str,
    ) -> BoxFuture<'a, Result<crate::forge::TeaFound, String>> {
        Box::pin(async { Err(not_built("A Forgejo login")) })
    }

    /// A GitLab block's adapter (glab's token for `host`, or anonymous and
    /// read-only) and the login it reads as.
    #[cfg(feature = "forge")]
    #[allow(clippy::type_complexity)]
    fn gitlab_connect<'a>(
        &'a self,
        _runner: Runner,
        _host: &'a str,
        _api: &'a str,
    ) -> BoxFuture<'a, Result<(Arc<dyn crate::forge::Adapter>, Option<String>), String>> {
        Box::pin(async { Err(not_built("A GitLab block")) })
    }

    // ------------------------------------------------------------ control

    /// Whether this machine offers agents to its teams' members (#399).
    fn offers_agents(&self, _state_dir: &Path) -> bool {
        false
    }

    /// What Labs tells control on the link's text channel (guest routes).
    fn control_out(&self) -> Option<tokio::sync::watch::Receiver<Option<String>>> {
        None
    }

    /// Whether `text` from control was Labs' (else it goes on to the forge).
    fn heard_from_control(&self, _text: &str) -> bool {
        false
    }

    /// A raw stream control opened of `kind` (a guest's ssh connection
    /// through its jump host): dropped unless Labs serves it.
    fn serve_raw(&self, _app: &Arc<App>, _kind: &[u8], _stream: tokio::io::DuplexStream) {}

    // ------------------------------------------------------------ blocks

    /// Whether the user is logged in to a studio: `GET /api/host`'s
    /// `features.studio`.
    fn studio_here(&self) -> bool {
        false
    }

    /// A studio app block's config (M35) from `{app}`: the box and studio
    /// filled in from studio's list when not given.
    fn app_config<'a>(&'a self, _c: &'a Value) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Err(not_built("An app block")) })
    }

    /// #619: which decision and run made each hunk of `file` in a workspace
    /// member's *Changes*, from the workspace's chant.
    fn file_why<'a>(
        &'a self,
        _ctx: &'a BlockCtx,
        _root: &'a str,
        _chant: Option<&'a str>,
        _file: &'a str,
        _at: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Box<dyn FileWhy>, String>> {
        Box::pin(async { Err(not_built("Which decision and run made a hunk")) })
    }

    /// A recipe (a Claude Code subagent file, M76) worn on this host by a
    /// block working in `cwd`.
    fn wear_recipe<'a>(
        &'a self,
        _runner: &'a Runner,
        _state_dir: &'a Path,
        _cwd: &'a Path,
        _name: &'a str,
    ) -> BoxFuture<'a, Result<Arc<dyn Worn>, String>> {
        Box::pin(async { Err(not_built("Agent recipes")) })
    }

    // ------------------------------------------------------------ MCP

    /// `show {kind: "app"}`: a studio app, or the user's apps.
    fn open_app<'a>(&'a self, _call: &'a Call<'a>, _a: OpenAppArgs) -> BoxFuture<'a, Out> {
        Box::pin(async { Err(not_built("An app block")) })
    }

    /// Delegating (M78): a task for another machine's agent.
    fn delegate<'a>(&'a self, _call: &'a Call<'a>, _kind: &'a str, _a: DelegateArgs) -> BoxFuture<'a, Out> {
        Box::pin(async { Err(not_built("Delegating")) })
    }

    /// The team's agent catalog (M77).
    fn team_agents<'a>(&'a self, _call: &'a Call<'a>, _a: ListTeamAgentsArgs) -> BoxFuture<'a, Out> {
        Box::pin(async { Err(not_built("The agent catalog")) })
    }

    /// Labs' forges and the user's logins for them, for MCP's
    /// `capabilities` (#229).
    fn forge_logins<'a>(&'a self, _runner: &'a Runner, _state_dir: &'a Path) -> BoxFuture<'a, Vec<ForgeSignIn>> {
        Box::pin(async { Vec::new() })
    }
}

/// A block type this build doesn't have: it says so ([`not_built`]).
struct NotBuilt(&'static str);

impl BlockKind for NotBuilt {
    fn create(&self, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
        Err(not_built(self.0))
    }
}

/// A build without Labs: every default.
struct Absent;

impl Hooks for Absent {}

static LABS: OnceLock<Box<dyn Hooks>> = OnceLock::new();

/// Puts Labs in this daemon (once, before `main`).
pub fn install(labs: Box<dyn Hooks>) {
    if LABS.set(labs).is_err() {
        tracing::warn!("Labs was registered twice");
    }
}

/// Labs, or a build without it.
pub fn labs() -> &'static dyn Hooks {
    LABS.get().map_or(&Absent, |l| l.as_ref())
}
