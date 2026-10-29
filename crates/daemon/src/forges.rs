//! The GitHub forge (pull requests and issues as blocks, their live updates
//! through control) is a crate of its own, `arugulad-forge`
//! (crates/arugulad-forge, #232), which the binary registers ([`install`])
//! when its `forge` feature is on, as it is by default. Labs' GitLab and
//! Forgejo are built on it, so the binary's `labs` feature turns it on.
//!
//! **The surface:** what the rest of the daemon calls is below. Each asks
//! the registered [`Forges`] or, with none, does nothing or says
//! [`not_built`], so no caller needs to know whether it's there. A build
//! without it has no forge blocks (a saved one says so when it's opened), no
//! forge tools in MCP's list and no live updates, and control's `forge.*`
//! messages go unheard. Everything in `arugula-proto` stays, so the protocol
//! is the same.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use arugula_control_wire::forge::{ForgeSend, OpAnswer};
use arugula_proto::{BlockType, forge::ForgeState};
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::{
    block::{Block, BlockCtx, BlockKind, BlockKinds},
    control::Control,
    review::Runner,
    server::App,
};

/// The forge crate, as the daemon calls it.
pub trait Forges: Send + Sync + 'static {
    /// Registers the forge block type.
    fn kinds(&self, kinds: &mut BlockKinds);
    /// At start: where the forge keeps hooks' secrets, control (for
    /// GitHub), and the URLs this daemon is reached at.
    fn start(&self, state_dir: PathBuf, control: Option<&Arc<Control>>, urls: Vec<String>);
    /// A forge block's config, made whole from a link, `OWNER/REPO#N`, or
    /// `N` in a clone.
    fn open_config<'a>(&'a self, c: &'a Value, state_dir: &'a Path) -> BoxFuture<'a, Result<Value, String>>;
    /// Whether `gh` has a login for `host` now.
    fn gh_signed_in<'a>(&'a self, runner: &'a Runner, host: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// A text control's socket sent that may be the forge's.
    fn heard(&self, text: &str);
    /// The `forge.watch` messages for control's socket.
    fn watch_messages(&self) -> Option<tokio::sync::watch::Receiver<Option<String>>>;
    /// Whether a forge block's item is on a GitHub repo linked to a project.
    fn linked<'a>(&'a self, st: &'a ForgeState) -> BoxFuture<'a, bool>;
    /// Control's `forge.send` op (M168).
    fn send<'a>(&'a self, app: &'a Arc<App>, s: ForgeSend) -> BoxFuture<'a, OpAnswer>;
}

static FORGES: OnceLock<Box<dyn Forges>> = OnceLock::new();

/// Puts the forge in this daemon (once, before `main`).
pub fn install(f: Box<dyn Forges>) {
    if FORGES.set(f).is_err() {
        tracing::warn!("the forge was registered twice");
    }
}

fn forges() -> Option<&'static dyn Forges> {
    FORGES.get().map(|f| f.as_ref())
}

/// Whether this daemon has the forge.
pub fn built() -> bool {
    forges().is_some()
}

/// What a request that needs the forge, in a build without it, answers.
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without the forge feature)")
}

/// A kind this build doesn't have: it says so ([`not_built`]).
struct NotBuilt(&'static str);

impl BlockKind for NotBuilt {
    fn create(&self, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
        Err(not_built(self.0))
    }
}

/// Registers the forge block type.
pub fn kinds(kinds: &mut BlockKinds) {
    match forges() {
        Some(f) => f.kinds(kinds),
        None => kinds.add(BlockType::Forge, NotBuilt("A forge block")),
    }
}

/// At start: where the forge keeps hooks' secrets, control (for GitHub), and
/// the URLs this daemon is reached at.
pub fn start(state_dir: PathBuf, control: Option<&Arc<Control>>, urls: Vec<String>) {
    if let Some(f) = forges() {
        f.start(state_dir, control, urls);
    }
}

/// A forge block's config, made whole from a link, `OWNER/REPO#N`, or `N` in
/// a clone.
pub async fn open_config(c: &Value, state_dir: &Path) -> Result<Value, String> {
    match forges() {
        Some(f) => f.open_config(c, state_dir).await,
        None => Err(not_built("A forge block")),
    }
}

/// Whether `gh` has a login for `host` now (asked afresh, not a block's
/// cached answer), for MCP's `capabilities` (#229). `Err`: why not.
pub async fn gh_signed_in(runner: &Runner, host: &str) -> Result<(), String> {
    match forges() {
        Some(f) => f.gh_signed_in(runner, host).await,
        None => Err(not_built("GitHub")),
    }
}

/// A text control's socket sent that may be the forge's (a poke or a
/// heartbeat).
pub fn from_control(text: &str) {
    if let Some(f) = forges() {
        f.heard(text);
    }
}

/// The `forge.watch` messages for control's socket.
pub fn watch_messages() -> Option<tokio::sync::watch::Receiver<Option<String>>> {
    forges()?.watch_messages()
}

/// Whether a forge block's item is on a GitHub repo linked to a project
/// for this machine's owner (M170's word on it, kept a few minutes): MCP's
/// `read_forge` reads it from the platform too then (M169).
pub async fn linked(st: &ForgeState) -> bool {
    match forges() {
        Some(f) => f.linked(st).await,
        None => false,
    }
}

/// `{"t": "ops"}` for control's socket (M168): the typed ops this machine
/// does when control asks: the run ops (M211), the hosted ones (Arugula
/// #209), and the forge's when it's built.
pub fn ops_message() -> tokio::sync::watch::Receiver<Option<String>> {
    let ops = arugula_control_wire::forge::Ops {
        ops: if built() {
            arugula_control_wire::forge::Ops::ours().ops
        } else {
            arugula_control_wire::forge::RUN_OPS
                .iter()
                .chain(arugula_control_wire::forge::HOSTED_OPS)
                .map(|s| (*s).to_owned())
                .collect()
        },
        ..Default::default()
    };
    tokio::sync::watch::channel(serde_json::to_string(&ops).ok()).1
}

/// A text control's socket sent, if it asks an op of this machine (M168):
/// whether it was one. The op runs on its own, and its answer goes to
/// `answers`.
pub fn op_from_control(app: &Arc<App>, text: &str, answers: &tokio::sync::mpsc::UnboundedSender<String>) -> bool {
    use arugula_control_wire::forge::{DaemonOp, OpAsk, OpDone};
    let Ok(ask) = serde_json::from_str::<OpAsk>(text) else { return false };
    let (app, answers) = (app.clone(), answers.clone());
    crate::task::spawn(async move {
        let answer = match ask.op {
            DaemonOp::ForgeSend(s) => match forges() {
                Some(f) => f.send(&app, s).await,
                None => OpAnswer::Unknown,
            },
            DaemonOp::RunStart(s) => crate::hosted::runs::start(&app, &s),
            DaemonOp::RunRevoke(r) => crate::hosted::runs::revoke(&app, &r),
            DaemonOp::HostedArchive(_) => crate::hosted::archive::flush(None).await,
            DaemonOp::HostedArchiveBlocks(b) => crate::hosted::archive::flush(Some(&b.blocks)).await,
        };
        let _ = answers.send(serde_json::to_string(&OpDone::new(ask.id, answer)).unwrap_or_default());
    });
    true
}
