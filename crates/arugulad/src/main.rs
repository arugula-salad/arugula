//! arugulad: owns the terminals; clients attach over WebSocket. The daemon
//! is `arugulad-base` (crates/daemon) and the feature crates on it
//! (`arugulad-<area>`, #232): this binary registers the features it's built
//! with, gives the daemon the release it is and starts it.

#[cfg(test)]
mod whole_tests;

/// The version, findable in the binary's bytes: the testnet tests read it
/// from a box's static build they can't run here (#259).
#[used]
static VERSION_MARK: &str = concat!("\0arugula-version=", env!("CARGO_PKG_VERSION"), "\0");

/// Puts every feature crate this build has in the daemon. Once.
fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // Docs (crates/arugulad-docs).
        arugulad_docs::register();
        // The deploy drivers, programs of their own (crates/arugulad-drivers).
        arugulad_drivers::register();
        // The diff and file blocks (crates/arugulad-review).
        arugulad_review::register();
        // Inviting a person into a session (crates/arugulad-invite).
        arugulad_invite::register();
        // The GitHub forge, which Labs' forges are built on
        // (crates/arugulad-forge).
        #[cfg(feature = "forge")]
        arugulad_forge::register();
        // The editor server and Claude Code's IDE (crates/arugulad-editor).
        #[cfg(feature = "editor")]
        arugulad_editor::register();
        // Labs (crates/arugulad-labs).
        #[cfg(feature = "labs")]
        arugulad_labs::register();
    });
}

fn main() -> anyhow::Result<()> {
    register();
    arugulad_base::main(env!("CARGO_PKG_VERSION"))
}
