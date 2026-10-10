//! The GitHub forge (`forge/`: pull requests and issues as blocks, their
//! live updates through control), behind the cargo feature `forge`. It's on
//! by default; `--no-default-features` leaves it out, and Labs' GitLab and
//! Forgejo need it, so `labs` turns it on.
//!
//! **The pattern** is Labs' (see `labs/mod.rs`): `forge/` is declared in
//! `main.rs` under the feature, and nothing outside it names it but this
//! file. What the rest of the daemon calls is below, each with a twin under
//! `#[cfg(not(feature = "forge"))]` that does nothing or says [`not_built`],
//! so no caller needs a `cfg` of its own. A build without the feature has
//! no forge blocks (a saved one says so when it's opened), no forge tools in
//! MCP's list and no live updates, and control's `forge.*` messages go
//! unheard. Everything in `arugula-proto` stays, so the protocol is the same.
//!
//! Tests that need the forge are `#[cfg(feature = "forge")]`.

#[cfg(not(feature = "forge"))]
use std::sync::Arc;

use arugula_proto::BlockType;
#[cfg(not(feature = "forge"))]
use serde_json::Value;

use crate::block::BlockKinds;
#[cfg(not(feature = "forge"))]
use crate::block::{Block, BlockCtx, BlockKind};

/// Whether this build has the forge.
pub const BUILT: bool = cfg!(feature = "forge");

/// What a request that needs the forge, in a build without it, answers.
#[cfg(not(feature = "forge"))]
pub fn not_built(what: &str) -> String {
    format!("{what} isn't in this build (built without the forge feature)")
}

/// Registers the forge block type.
#[cfg(feature = "forge")]
pub fn kinds(kinds: &mut BlockKinds) {
    kinds.add(BlockType::Forge, crate::block::Make(crate::forge::ForgeBlock::create));
}

/// A kind this build doesn't have: it says so ([`not_built`]).
#[cfg(not(feature = "forge"))]
struct NotBuilt(&'static str);

#[cfg(not(feature = "forge"))]
impl BlockKind for NotBuilt {
    fn create(&self, _ctx: BlockCtx, _config: Value) -> Result<Arc<dyn Block>, String> {
        Err(not_built(self.0))
    }
}

#[cfg(not(feature = "forge"))]
pub fn kinds(kinds: &mut BlockKinds) {
    kinds.add(BlockType::Forge, NotBuilt("A forge block"));
}

/// At start: where the forge keeps hooks' secrets, control (for GitHub), and
/// the URLs this daemon is reached at.
#[cfg(feature = "forge")]
pub fn start(
    state_dir: std::path::PathBuf,
    control: Option<&std::sync::Arc<crate::control::Control>>,
    urls: Vec<String>,
) {
    crate::forge::live::init(state_dir, control, urls);
}

#[cfg(not(feature = "forge"))]
pub fn start(_state_dir: std::path::PathBuf, _control: Option<&Arc<crate::control::Control>>, _urls: Vec<String>) {}

/// A forge block's config, made whole from a link, `OWNER/REPO#N`, or `N` in
/// a clone.
#[cfg(feature = "forge")]
pub async fn open_config(c: &serde_json::Value, state_dir: &std::path::Path) -> Result<serde_json::Value, String> {
    crate::forge::open_config(c, state_dir).await
}

#[cfg(not(feature = "forge"))]
pub async fn open_config(_c: &Value, _state_dir: &std::path::Path) -> Result<Value, String> {
    Err(not_built("A forge block"))
}

/// A text control's socket sent that may be the forge's (a poke or a
/// heartbeat).
#[cfg(feature = "forge")]
pub fn from_control(text: &str) {
    crate::forge::live::from_control(text);
}

#[cfg(not(feature = "forge"))]
pub fn from_control(_text: &str) {}

/// The `forge.watch` messages for control's socket.
#[cfg(feature = "forge")]
pub fn watch_messages() -> Option<tokio::sync::watch::Receiver<Option<String>>> {
    crate::forge::live::watch_messages()
}

#[cfg(not(feature = "forge"))]
pub fn watch_messages() -> Option<tokio::sync::watch::Receiver<Option<String>>> {
    None
}
