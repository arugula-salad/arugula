//! Labs: the features that are not part of the core (studio apps with hud, chant workspaces, VMs with their sandboxes, guest ssh and chat,
//! and the Forgejo and GitLab forges: everything listed in
//! #452 to #457 is here), behind the cargo feature `labs`. The feature is on by default, so a release build has all of it;
//! `--no-default-features` leaves it out, for fast local builds, for agents
//! working on core, and for the CI job `core`. (The runtime switches, the
//! flags in the state dir, are separate: they decide what a machine
//! shows, and [`flags`](crate::lab_flags::flags) ANDs them with this build.)
//!
//! **The pattern.** Labs is this crate, `arugulad-labs` (#232), and the
//! daemon (`arugulad-base`) never names it. The daemon reaches Labs only
//! through its surface, the trait `lab_hooks::Hooks`, which [`register`]
//! installs (the binary calls it when its `labs` feature is on); each of its
//! defaults is what a build without Labs does (nothing, or "X isn't in this
//! build"). So the daemon never needs a `cfg` of its own to call Labs, and an
//! edit here rebuilds this crate and the binary, not the daemon. Its
//! operations are `Handle<Labs>`, routed with `op_of`.
//!
//! **What stays unconditional.** Everything in `arugula-proto` (block types,
//! agent kinds, request and answer types), so every client and daemon speaks
//! one protocol, and the argument structs of MCP's tools (their schemas are
//! filtered by the flags, not by the build). Only the daemon's handling is
//! gated.
//!
//! **Adding a feature.** Put its code in `src/<name>/`, declare it below,
//! and add what the rest of the daemon needs to `lab_hooks::Hooks`, with the
//! default a build without Labs has, and its body to [`Labs`]'s impl here:
//! a route set goes into `routes`, a block type into `kinds`, host info into
//! the host functions.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::BlockType;
use axum::Router;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::forge::model::Provider as ForgeProvider;
use crate::{
    block::{BlockCtx, BlockKinds},
    lab_hooks::{FileWhy, Hooks, Start},
    mcp::{
        results::ForgeSignIn,
        tools::{Call, DelegateArgs, ListTeamAgentsArgs, OpenAppArgs, Out},
    },
    provider::Provider,
    review::Runner,
    server::App,
};

// What the code here names as `crate::…`: the mux's modules and the daemon's,
// as it named them when it was the daemon's.
use arugula_mux::{acl, block, mux, pane, perm, provider, store, sys, sys::home};
use arugulad_base::{
    agent, api, browser, dial, e2e, forge, fs, gate, hostname, hosts, http1, invite, lab_flags, lab_hooks, mcp, ops,
    peer, ports, push, review, roots, server, sites, task,
};
// The tailnet sandbox's (Linux boxes).
#[cfg(unix)]
use {arugula_mux::procinfo, arugulad_base::args, arugulad_base::install, arugulad_base::tailscale};

// Agent recipes and their A2A cards (M76): the `agent-delegation` flag.
pub mod agents;
pub mod apps;
// Approving a gate where it came from: chant or hud.
pub mod gates;
// Forgejo and GitLab as forges (#457): their adapters, the `tea` logins
// Forgejo reads with, the webhooks they send, and what a forge block asks to
// reach them (`forges.rs`: the forge's `OtherForges`, `tea_connect` and the rest).
// GitHub is core's.
pub mod forge_live;
pub mod forgejo;
pub mod forges;
pub mod gitlab;
// Guest ssh: the russh server and its invites (M65).
pub mod guest_ssh;
// VMs: the machines panes run on, and the sandboxes they come from. The
// terminal on one is the core's (`arugula_mux::labs::machine`).
pub mod loam;
pub mod provider_tunnel;
pub mod resident;
// The tailnet sandbox supervisor: Linux boxes.
#[cfg(unix)]
pub mod sandbox;
pub mod sprites;
pub mod tea;
pub mod wearing;
pub mod workspace;

/// The raw stream kind control opens for a guest through its jump host.
pub const GUEST_STREAM_KIND: &[u8] = b"ssh-guest";

// The flags are the base's (`lab_flags.rs`), as Labs always had them.
#[allow(unused_imports)]
pub use crate::lab_flags::{BUILT, flag, not_built, on};

/// Labs, as the rest of the daemon calls it (`lab_hooks::Hooks`), and whose
/// operations are Labs' (`impl Handle<Labs> for StudioGet`).
pub struct Labs;

/// Puts Labs in the daemon. Call it before the daemon starts.
pub fn register() {
    crate::lab_hooks::install(Box::new(Labs));
    forge::others::install(Box::new(Labs));
}

/// A Labs block type that is made only where the machine has `flag` on
/// (#347). One saved while it was on is still brought back when the daemon
/// starts, as a forge block of Forgejo's is: turning a flag off hides and
/// refuses the feature, and doesn't throw away what was made with it.
struct Flagged<K> {
    flag: &'static str,
    kind: K,
}

impl<K: crate::block::BlockKind> crate::block::BlockKind for Flagged<K> {
    fn create(
        &self,
        ctx: crate::block::BlockCtx,
        config: serde_json::Value,
    ) -> Result<Arc<dyn crate::block::Block>, String> {
        if !ctx.restoring {
            flag(&ctx.state_dir, self.flag)?;
        }
        self.kind.create(ctx, config)
    }

    fn check(&self, config: &serde_json::Value) -> Result<(), String> {
        self.kind.check(config)
    }

    fn stored_text(&self, dir: &Path) -> Option<String> {
        self.kind.stored_text(dir)
    }
}

impl FileWhy for workspace::why::Why {
    fn hunk_json(&self, added: &[u32]) -> Option<Value> {
        workspace::why::Why::hunk_json(self, added)
    }
}

impl Hooks for Labs {
    #[cfg(unix)]
    fn tailnet_install(&self, cmd: crate::args::Command) -> anyhow::Result<()> {
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

    fn supervise_sandbox(&self) -> anyhow::Result<()> {
        #[cfg(unix)]
        return sandbox::supervise();
        #[cfg(not(unix))]
        anyhow::bail!("the sandbox supervisor is for Linux boxes")
    }

    fn open_provider(&self, url: &str, token_file: &Path, _given: bool) -> Option<Arc<dyn Provider>> {
        sprites::Sprites::open(url, token_file).map(|p| Arc::new(p) as Arc<dyn Provider>)
    }

    fn open_loam(&self, url: &str, key_file: &Path) -> Option<Arc<dyn Provider>> {
        loam::Loam::open(url, key_file).map(|p| Arc::new(p) as Arc<dyn Provider>)
    }

    fn install_studio(&self, file: PathBuf, _given: bool) {
        apps::studio::install(file);
    }

    fn open(&self, start: Start) {
        guest_ssh::open(&start.state_dir, start.guest_listen, start.guest_host);
        resident::find_binaries(start.static_dir);
    }

    /// Each is made only where the machine has its flag on ([`Flagged`]).
    fn kinds(&self, kinds: &mut BlockKinds, invite: &crate::invite::Hook) {
        use crate::block::Make;
        use arugula_proto::flags::{AGENT_DELEGATION, CHANT_WORKSPACES, STUDIO};
        kinds.add(
            BlockType::Agents,
            Flagged { flag: AGENT_DELEGATION, kind: agents::block::AgentsKind(invite.clone()) },
        );
        kinds.add(BlockType::App, Flagged { flag: STUDIO, kind: Make(apps::AppBlock::create) });
        kinds.add(BlockType::Workspace, Flagged { flag: CHANT_WORKSPACES, kind: Make(workspace::Workspace::create) });
    }

    fn start(&self, app: &Arc<App>) {
        if let Some(g) = guest_ssh::guests() {
            g.run(app);
        }
        // M78: follow again the tasks for this machine's agents that a
        // restart interrupted, and ask again for those waiting on the owner.
        if on(app.control.state_dir(), arugula_proto::flags::AGENT_DELEGATION) {
            agents::tasks::resume(app);
        }
    }

    /// Stop every behold a workspace block runs for its graph (#620), which
    /// is in a process group of its own.
    fn at_exit(&self) {
        workspace::stop_beholds();
    }

    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        agents::routes(apps::routes::routes(r))
    }

    fn home_routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r.merge(resident::routes()).merge(provider_tunnel::routes()).merge(guest_ssh::routes())
    }

    fn hook_routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        forge_live::routes(r)
    }

    fn is_hook(&self, path: &str) -> bool {
        forge_live::is_hook(path)
    }

    /// The `agent-delegation` flag is on and at least one recipe is offered.
    /// Until then no teammate's daemon gets in to a personal machine.
    fn offers_agents(&self, state_dir: &Path) -> bool {
        on(state_dir, arugula_proto::flags::AGENT_DELEGATION) && !agents::offers(state_dir).is_empty()
    }

    fn control_out(&self) -> Option<tokio::sync::watch::Receiver<Option<String>>> {
        guest_ssh::guests().map(|g| g.routes_messages())
    }

    fn heard_from_control(&self, text: &str) -> bool {
        guest_ssh::guests().is_some_and(|g| g.heard_from_control(text))
    }

    fn serve_raw(&self, app: &Arc<App>, kind: &[u8], stream: tokio::io::DuplexStream) {
        if kind == GUEST_STREAM_KIND
            && let Some(g) = guest_ssh::guests()
        {
            g.serve_relayed(app, stream);
        }
    }

    fn studio_here(&self) -> bool {
        apps::studio::get().and_then(|s| s.url()).is_some()
    }

    fn app_config<'a>(&'a self, c: &'a Value) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(apps::routes::app_config(c))
    }

    fn file_why<'a>(
        &'a self,
        ctx: &'a BlockCtx,
        root: &'a str,
        chant: Option<&'a str>,
        file: &'a str,
        at: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Box<dyn FileWhy>, String>> {
        Box::pin(async move {
            let why = workspace::why::read(ctx, root, chant, file, at).await?;
            Ok(Box::new(why) as Box<dyn FileWhy>)
        })
    }

    /// Needs the `agent-delegation` flag.
    fn wear_recipe<'a>(
        &'a self,
        runner: &'a Runner,
        state_dir: &'a Path,
        cwd: &'a Path,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Arc<dyn crate::agent::Worn>, String>> {
        Box::pin(async move {
            if !on(state_dir, arugula_proto::flags::AGENT_DELEGATION) {
                return Err("agent recipes are in Labs: `arugulad flags agent-delegation on` turns them on here".into());
            }
            let w = agents::wear::wear(runner, cwd, name).await?;
            Ok(Arc::new(w) as Arc<dyn crate::agent::Worn>)
        })
    }

    fn open_app<'a>(&'a self, call: &'a Call<'a>, a: OpenAppArgs) -> BoxFuture<'a, Out> {
        Box::pin(apps::tool::open_app(call, a))
    }

    fn delegate<'a>(&'a self, call: &'a Call<'a>, kind: &'a str, a: DelegateArgs) -> BoxFuture<'a, Out> {
        Box::pin(agents::tool::delegate(call, kind, a))
    }

    fn team_agents<'a>(&'a self, call: &'a Call<'a>, a: ListTeamAgentsArgs) -> BoxFuture<'a, Out> {
        Box::pin(agents::tool::team_agents(call, a))
    }

    /// `tea`'s Forgejo logins, and whether `glab` has gitlab.com's. None
    /// unless the `additional-forges` flag is on.
    fn forge_logins<'a>(&'a self, runner: &'a Runner, state_dir: &'a Path) -> BoxFuture<'a, Vec<ForgeSignIn>> {
        Box::pin(forge_logins(runner, state_dir))
    }
}

/// Forgejo and GitLab, the forges that are Labs' (`forges.rs`, #457).
impl forge::others::OtherForges for Labs {
    fn tea_connect<'a>(
        &'a self,
        runner: Runner,
        ask: forge::TeaAsk,
    ) -> BoxFuture<'a, Result<forge::TeaFound, forge::TeaMissed>> {
        Box::pin(forges::tea_connect(runner, ask))
    }

    fn tea_login<'a>(
        &'a self,
        runner: Runner,
        provider: ForgeProvider,
        name: &'a str,
    ) -> BoxFuture<'a, Result<forge::TeaFound, String>> {
        Box::pin(forges::tea_login(runner, provider, name))
    }

    fn gitlab_connect<'a>(
        &'a self,
        runner: Runner,
        host: &'a str,
        api: &'a str,
    ) -> BoxFuture<'a, Result<(Arc<dyn forge::Adapter>, Option<String>), String>> {
        Box::pin(async move { Ok(forges::gitlab_connect(runner, host, api).await) })
    }

    fn hook_path(&self, provider: ForgeProvider) -> Result<&'static str, String> {
        Ok(forge_live::path(provider))
    }
}

/// Labs' forges and the user's logins for them, for MCP's `capabilities`
/// (#229).
async fn forge_logins(runner: &Runner, state_dir: &Path) -> Vec<ForgeSignIn> {
    if !on(state_dir, arugula_proto::flags::ADDITIONAL_FORGES) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let tea = tea::Tea { runner: runner.clone() };
    match tea.logins().await {
        Ok(logins) if logins.is_empty() => out.push(ForgeSignIn {
            forge: "forgejo".into(),
            host: String::new(),
            signed_in: false,
            user: None,
            note: "tea has no logins: `tea login add`".into(),
        }),
        Ok(logins) => out.extend(logins.into_iter().map(|l| ForgeSignIn {
            forge: "forgejo".into(),
            host: crate::forge::login::url_host(&l.url).unwrap_or(l.url),
            signed_in: true,
            user: Some(l.user).filter(|u| !u.is_empty()),
            note: format!("tea login {}", l.name),
        })),
        Err(e) => out.push(ForgeSignIn {
            forge: "forgejo".into(),
            host: String::new(),
            signed_in: false,
            user: None,
            note: e,
        }),
    }
    let host = "gitlab.com";
    let (signed_in, note) = match gitlab::GlabToken::ask(runner, host).await {
        Ok(gitlab::Glab::Token(_)) => (true, "glab".to_owned()),
        Ok(gitlab::Glab::NoLogin) => (false, format!("glab has no login for {host}: `glab auth login`")),
        Ok(gitlab::Glab::Missing) => (false, "no glab here: install GitLab's CLI and `glab auth login`".into()),
        Err(e) => (false, e),
    };
    out.push(ForgeSignIn {
        forge: "gitlab".into(),
        host: host.into(),
        signed_in,
        user: None,
        note: format!("{note} (other GitLab hosts: whatever glab knows of each)"),
    });
    out
}
