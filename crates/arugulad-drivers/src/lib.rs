//! arugulad-drivers: the deploy drivers the daemon's binary runs as
//! programs of their own, as a deploy's run user (#232). Each is a
//! subcommand (`arugulad _planter …`) that `arugulad-base`'s `main` hands
//! over to before anything else of the daemon starts; the work runner
//! (`arugulad_base::work`) starts them as such. The binary calls
//! [`register`].
//!
//! - `apps_platform.rs`: the apps-platform driver's apply (M222).
//! - `planter.rs`: planter's drivers (M233), on `apps_platform`'s git.
//! - `arugula_release.rs`: Arugula's own release (M221).

pub mod apps_platform;
pub mod arugula_release;
pub mod planter;

// What the code here names as `crate::…`, as it did when it was the
// daemon's.
use arugulad_base::{roots, task, work};

/// Adds the drivers' subcommands to the daemon. Call it before
/// `arugulad_base::main`.
pub fn register() {
    use arugulad_base::subcommands::register;
    register("_apps-platform", apps_platform::cli);
    register("_planter", planter::cli);
    register("_arugula-release", arugula_release::cli);
}

/// The temp directories the drivers' tests work in: theirs are Unix only.
#[cfg(all(test, unix))]
mod hosted {
    use std::path::PathBuf;

    /// A fresh directory for `name` in this test process.
    pub fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-hosted-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
