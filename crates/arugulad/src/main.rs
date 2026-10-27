//! arugulad: owns the terminals; clients attach over WebSocket. The daemon
//! is `arugulad-base` (crates/daemon) and the feature crates on it
//! (`arugulad-<area>`, #232): this binary registers the features it's built
//! with, gives the daemon the release it is and starts it.

/// The version, findable in the binary's bytes: the testnet tests read it
/// from a box's static build they can't run here (#259).
#[used]
static VERSION_MARK: &str = concat!("\0arugula-version=", env!("CARGO_PKG_VERSION"), "\0");

fn main() -> anyhow::Result<()> {
    // The editor server and Claude Code's IDE (crates/arugulad-editor).
    #[cfg(feature = "editor")]
    arugulad_editor::register();
    arugulad_base::main(env!("CARGO_PKG_VERSION"))
}
