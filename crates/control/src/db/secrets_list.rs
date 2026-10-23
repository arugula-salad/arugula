//! The list of deploy environments whose secrets the platform keeps
//! (`crates/control/secrets.toml`, M209), beside the table manifest. Code
//! names an environment's secret with `arugula_platform::environment_secret!`,
//! which reads this file at compile time and doesn't compile for a secret
//! it doesn't list. These tests keep the file in the shape that reader
//! reads, and keep the macro the only way to make one.

use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    environment: Vec<Environment>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Environment {
    name: String,
    #[allow(dead_code)]
    what: String,
    secrets: Vec<String>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every listed secret has a good name, and the compile-time reader finds
/// each one where the TOML says it is, and no other.
#[test]
fn the_list_reads_the_same_at_compile_time() {
    let text = std::fs::read_to_string(root().join("crates/control/secrets.toml")).unwrap();
    assert_eq!(text, arugula_platform::keys::SECRETS_TOML, "the file the macro read");
    let list: List = toml::from_str(&text).unwrap_or_else(|e| panic!("crates/control/secrets.toml: {e}"));
    let mut names = std::collections::BTreeSet::new();
    for e in &list.environment {
        assert!(names.insert(&e.name), "secrets.toml lists {} twice", e.name);
        assert!(!e.name.is_empty() && !e.name.contains('"'), "a bad environment name: {:?}", e.name);
        for s in &e.secrets {
            assert!(arugula_platform::secrets::valid_name(s), "{}: {s} is a reserved or bad name", e.name);
            assert!(arugula_platform::keys::is_listed(&e.name, s), "{}: {s} isn't read at compile time", e.name);
        }
        assert!(!arugula_platform::keys::is_listed(&e.name, "NOT_LISTED_ANYWHERE"));
    }
}

/// Nothing but the macro makes an `EnvironmentSecret`.
#[test]
fn only_the_macro_makes_one() {
    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.ends_with("target") && !p.ends_with("node_modules") {
                    sources(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    sources(&root().join("crates"), &mut files);
    for f in files {
        if f.ends_with("platform/src/keys.rs") || f.ends_with("db/secrets_list.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(!text.contains("EnvironmentSecret::listed("), "{}: use environment_secret!", f.display());
    }
}
