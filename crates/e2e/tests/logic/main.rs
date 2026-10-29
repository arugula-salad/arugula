//! The example files for the logic the hosted server shares with the daemon
//! (#220): `contract/examples/logic/<module>/<name>.json`, input to output,
//! refusals included, so a second implementation can be checked against
//! them with no Rust toolchain.
//!
//! Each module here makes its files ([`harness::Spec`]) from fixed keys and
//! fixed times, and runs a case's input through `arugula_e2e` to get its
//! output. The tests hold the files to the code:
//! - [`the_files_are_what_the_code_makes`]: every file, made again, is the
//!   one checked in, byte for byte. `ARUGULA_WRITE_EXAMPLES=1` writes them
//!   instead, when a change is on purpose.
//! - [`every_case_runs_from_its_file_alone`]: every file on disk, read
//!   back, runs case by case from its JSON alone (no generator), and gives
//!   the output the file says. A file nothing here runs fails, and so does
//!   an unknown field, a missing `about` or a label no object has.
//! - [`a_changed_or_malformed_file_fails`]: and that those do fail.
//!
//! The format, for whoever reads the files: `contract/examples/logic/README.md`.

mod harness;

mod cert;
mod deploy;
mod drivers;
mod hosted;
mod run;
mod team;

use harness::Spec;
use serde_json::{Value, json};

/// Every file, by module.
const MODULES: &[&[Spec]] = &[cert::FILES, team::FILES, deploy::FILES, drivers::FILES, run::FILES, hosted::FILES];

fn specs() -> impl Iterator<Item = &'static Spec> {
    MODULES.iter().flat_map(|m| m.iter())
}

fn spec_for(module: &str, name: &str) -> Option<&'static Spec> {
    specs().find(|s| s.module == module && s.name == name)
}

#[test]
fn the_files_are_what_the_code_makes() {
    let write = std::env::var_os("ARUGULA_WRITE_EXAMPLES").is_some();
    let mut stale = vec![];
    for spec in specs() {
        let path = spec.path();
        let text = spec.example().text();
        if write {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(path.display().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "these example files aren't what the code makes now (ARUGULA_WRITE_EXAMPLES=1 writes them, if the change is \
         on purpose):\n{}",
        stale.join("\n")
    );
}

/// Every `.json` file under the directory, by module and name.
fn on_disk() -> Vec<(String, String, std::path::PathBuf)> {
    let mut out = vec![];
    for module in std::fs::read_dir(harness::dir()).unwrap() {
        let module = module.unwrap().path();
        if !module.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&module).unwrap() {
            let path = file.unwrap().path();
            let m = module.file_name().unwrap().to_str().unwrap().to_owned();
            let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            assert!(path.extension().is_some_and(|x| x == "json"), "{}: not an example file", path.display());
            out.push((m, name, path));
        }
    }
    out
}

#[test]
fn every_case_runs_from_its_file_alone() {
    let files = on_disk();
    for (m, name, path) in &files {
        let spec = spec_for(m, name).unwrap_or_else(|| panic!("{}: no runner here knows this file", path.display()));
        let text = std::fs::read_to_string(path).unwrap();
        harness::run_file(&text, spec.run).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
    }
    assert_eq!(files.len(), specs().count(), "a file the generator makes is missing on disk");
}

/// A changed output, an unknown field, a missing `about` or object, and a
/// broken file each fail.
#[test]
fn a_changed_or_malformed_file_fails() {
    for (m, name, path) in on_disk() {
        let Some(spec) = spec_for(&m, &name) else { continue };
        let text = std::fs::read_to_string(&path).unwrap();
        let good: Value = serde_json::from_str(&text).unwrap();
        let fails = |f: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            f(&mut v);
            harness::run_file(&v.to_string(), spec.run).is_err()
        };
        let what = format!("{m}/{name}");
        assert!(fails(&|v| v["cases"][0]["output"] = json!({ "changed": true })), "{what}: output");
        assert!(fails(&|v| v["cases"][0]["input"]["unknown"] = json!(1)), "{what}: an unknown field");
        assert!(fails(&|v| v["cases"][0]["about"] = json!("")), "{what}: no about");
        assert!(fails(&|v| v["about"] = json!("")), "{what}: no file about");
        assert!(fails(&|v| v["extra"] = json!(1)), "{what}: an unknown top-level field");
        assert!(fails(&|v| v["cases"] = json!([])), "{what}: no cases");
        assert!(harness::run_file(&text[..text.len() / 2], spec.run).is_err(), "{what}: cut short");
        let first_names_objects = good["cases"][0]["input"].to_string().contains('"')
            && good["objects"]
                .as_object()
                .is_some_and(|o| o.keys().any(|k| good["cases"][0]["input"].to_string().contains(&format!("\"{k}\""))));
        if first_names_objects {
            assert!(fails(&|v| v["objects"] = json!({})), "{what}: its objects gone");
        }
    }
}
