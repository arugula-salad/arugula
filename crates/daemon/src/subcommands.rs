//! Subcommands a feature crate adds (`arugulad _planter …`, #232): a
//! program of its own in the daemon's binary, which a deploy or a box runs
//! as another user. Nothing else of the daemon runs: `main` hands the rest
//! of the command line over before it logs or reads its flags. The binary
//! registers them before it calls `main`.

use std::sync::Mutex;

/// Runs with the arguments after its name, and exits.
pub type Run = fn(&[String]) -> !;

static ALL: Mutex<Vec<(&'static str, Run)>> = Mutex::new(Vec::new());

/// Adds `arugulad NAME …`. A second registration of a name changes nothing.
pub fn register(name: &'static str, run: Run) {
    let mut all = ALL.lock().unwrap();
    if !all.iter().any(|(n, _)| *n == name) {
        all.push((name, run));
    }
}

/// The subcommand `argv` names, if a crate registered it.
pub(crate) fn find(argv: &[String]) -> Option<Run> {
    let name = argv.get(1)?;
    ALL.lock().unwrap().iter().find(|(n, _)| n == name).map(|(_, r)| *r)
}
