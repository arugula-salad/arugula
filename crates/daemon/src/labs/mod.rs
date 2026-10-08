//! Labs: the features that are not part of the core (Fountain, studio apps
//! with hud, chant workspaces, VMs with their sandboxes, guest ssh, huddles
//! and chat, and the Forgejo and GitLab forges: everything listed in
//! #452 to #457 is here), behind the cargo feature `labs`. The feature is on by default, so a release build has all of it;
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
    forge::model::Provider as ForgeProvider,
    provider::{Begin, Exec, ExecEvent, Provider},
    server::App,
    shellenv::ShellEnv,
};

#[cfg(feature = "labs")]
pub mod apps;
// Huddles (M63): who is in each voice call on a session.
#[cfg(feature = "labs")]
pub mod calls;
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
// VMs: the machines panes run on, and the sandboxes they come from.
#[cfg(feature = "labs")]
pub mod machine;
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
// Chat (M61): the threads on panes and sessions. The mux's handling of
// both is `mux/thread_ops.rs` and `mux/call_ops.rs`, which need Daemon's
// private fields; `mux/labs_off.rs` has their twins.
#[cfg(feature = "labs")]
pub mod threads;
#[cfg(feature = "labs")]
pub mod workspace;

/// Where the guest ssh server listens unless `--guest-ssh` says otherwise.
/// (The flag is in every build, so the default is too.)
pub const GUEST_SSH_LISTEN: &str = "0.0.0.0:7684";

/// The raw stream kind control opens for a guest through its jump host.
pub const GUEST_STREAM_KIND: &[u8] = b"ssh-guest";

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

/// Whether a forge block of `provider` may be made or opened here. GitHub's
/// always may; Forgejo's and GitLab's are Labs, so they need this build to
/// have it and the machine to have turned it on (the `labs` file).
pub fn forge_allowed(provider: ForgeProvider, state_dir: &Path) -> Result<(), String> {
    let name = match provider {
        ForgeProvider::Github => return Ok(()),
        ForgeProvider::Forgejo => "Forgejo",
        ForgeProvider::Gitlab => "GitLab",
    };
    if enabled(state_dir) {
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
#[cfg(feature = "labs")]
pub fn hook_path(provider: ForgeProvider) -> Result<&'static str, String> {
    Ok(forge_live::path(provider))
}

#[cfg(not(feature = "labs"))]
pub fn hook_path(provider: ForgeProvider) -> Result<&'static str, String> {
    Err(not_built(&format!("A {provider:?} webhook")))
}

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

/// Threads on panes and sessions, and huddles on sessions. Always made, so
/// each has a twin that holds nothing and touches nothing on disk: a build
/// without Labs never opens, writes or deletes the state dir's `threads/`,
/// so a later build with Labs finds it as it was.
#[cfg(feature = "labs")]
pub use calls::Calls;
#[cfg(feature = "labs")]
pub use threads::Threads;

#[cfg(not(feature = "labs"))]
pub use absent::{Calls, Threads};

/// Whether `token` (an `@name` in a message) names this person: see
/// `threads::names`. Nobody is named where there is no chat.
#[cfg(feature = "labs")]
pub use threads::names as thread_names;

#[cfg(not(feature = "labs"))]
pub fn thread_names(_token: &str, _id: &str, _name: &str) -> bool {
    false
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

/// The twins of what [`Worn`] is, for a build without Labs: nothing is ever
/// worn, so an agent block's `worn` stays `None`.
#[cfg(not(feature = "labs"))]
mod absent {
    use std::{path::Path, sync::Arc};

    use arugula_proto::{CallMember, ClientId, SessionId, ThreadMsg, ThreadTarget};
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

    /// Threads of a build without Labs: none, and nothing read from or
    /// written to the state dir.
    pub struct Threads;

    impl Threads {
        pub fn open(_root: &Path) -> Self {
            Threads
        }

        pub fn get(&self, _target: ThreadTarget) -> &[ThreadMsg] {
            &[]
        }

        pub fn targets(&self) -> impl Iterator<Item = ThreadTarget> + '_ {
            std::iter::empty()
        }

        pub fn mark_read(&mut self, _who: &str, _target: ThreadTarget, _upto: u64) -> bool {
            false
        }

        pub fn save(&mut self) {}
    }

    /// Huddles of a build without Labs: none.
    #[derive(Default)]
    pub struct Calls;

    impl Calls {
        pub fn leave_all(&mut self, _client: ClientId) -> bool {
            false
        }

        pub fn retain(&mut self, _keep: impl FnMut(SessionId, &CallMember) -> bool) -> bool {
            false
        }
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

/// Why a request that needs VM panes can't have them: `why` when there is
/// no provider here, and that this build has no VMs when it was built without
/// Labs.
#[cfg(feature = "labs")]
pub fn vms_unavailable(why: &str) -> String {
    why.to_owned()
}

#[cfg(not(feature = "labs"))]
pub fn vms_unavailable(_why: &str) -> String {
    "VMs aren't in this build (built without labs)".to_owned()
}

/// Starts (or resumes) a session on `sprite`, a VM pane's terminal. There is
/// no pane on a machine without a provider, which a build without Labs
/// never has; if it is asked anyway, the session is lost at once.
#[cfg(feature = "labs")]
pub fn machine_start(
    rt: &tokio::runtime::Handle,
    provider: Arc<dyn Provider>,
    sprite: String,
    begin: Begin,
    size: (u16, u16),
    sink: impl Fn(ExecEvent) -> bool + Send + 'static,
) -> Exec {
    machine::start(rt, provider, sprite, begin, size, sink)
}

#[cfg(not(feature = "labs"))]
pub fn machine_start(
    _rt: &tokio::runtime::Handle,
    _provider: Arc<dyn Provider>,
    _sprite: String,
    _begin: Begin,
    _size: (u16, u16),
    sink: impl Fn(ExecEvent) -> bool + Send + 'static,
) -> Exec {
    sink(ExecEvent::Lost { machine_gone: true });
    Exec { tx: tokio::sync::mpsc::unbounded_channel().0 }
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
