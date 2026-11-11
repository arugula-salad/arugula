//! Labs' flags (#347): whether this build has Labs, and which of its flags
//! the machine has on. In the base, so what asks (the API, MCP, the forge
//! blocks, agents) doesn't name Labs itself, which is on its way to a crate
//! of its own (#232). Labs re-exports these as it always had them.

use std::path::Path;

use arugula_proto::{BlockType, flags::On};

/// Whether this build has Labs.
pub const BUILT: bool = cfg!(feature = "labs");

pub use arugula_mux::labs::{flag, not_built};

/// The Labs flags that are on here: none in a build without Labs, else
/// what the machine's state directory says (read on each call).
pub fn flags(state_dir: &Path) -> On {
    if BUILT { arugula_proto::flags::on(state_dir) } else { On::none() }
}

/// Whether `flag` is on here: this build has Labs, and the machine has
/// turned the flag on.
pub fn on(state_dir: &Path, flag: &str) -> bool {
    flags(state_dir).has(flag)
}

/// The flag a new block of `kind` needs (#347), for a route to refuse it
/// before it does anything else (an app block asks the studio where its box
/// is first). The block types check it again when made.
pub fn block_flag(kind: BlockType) -> Option<&'static str> {
    use arugula_proto::flags;
    match kind {
        BlockType::Agents => Some(flags::AGENT_DELEGATION),
        BlockType::App => Some(flags::STUDIO),
        BlockType::Workspace => Some(flags::CHANT_WORKSPACES),
        _ => None,
    }
}

/// Whether a forge block of `provider` may be made or opened here. GitHub's
/// always may; Forgejo's and GitLab's need a build with Labs' code (the
/// default one). They left Labs' flags at launch (native #523).
pub fn forge_allowed(provider: arugula_proto::forge::Provider, _state_dir: &Path) -> Result<(), String> {
    use arugula_proto::forge::Provider;
    let name = match provider {
        Provider::Github => return Ok(()),
        Provider::Forgejo => "Forgejo",
        Provider::Gitlab => "GitLab",
    };
    if BUILT { Ok(()) } else { Err(not_built(&format!("A {name} block"))) }
}
