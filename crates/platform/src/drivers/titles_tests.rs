//! A deploy's title and the version a release expects (#333).

use arugula_e2e::drivers::arugula_release::Lane;

use super::titles::{SUBJECT_MAX, expected_version, subject_of, title};
use crate::db::deploys::{Environment, Source, SourceKind};

fn env(driver: &str) -> Environment {
    Environment {
        id: "env_1".into(),
        project: "p".into(),
        name: "production".into(),
        driver: driver.into(),
        secret_names: vec![],
        settings: Default::default(),
        created_at: 1,
        updated_at: 1,
    }
}

fn commit() -> Source {
    Source {
        kind: SourceKind::Commit,
        reference: "b1faaa04c0ffee00".repeat(2).chars().take(40).collect(),
        place: "a/b".into(),
    }
}

#[test]
fn a_title_says_what_is_deployed_and_never_a_certain_version() {
    let (rel, other) = (env("arugula-release"), env("chant-fly"));
    let t = |e, s, v| title(e, &commit(), s, v);
    assert_eq!(
        t(&rel, Some("Docs: the Arugula look"), Some("0.63.9")),
        "Release 0.63.9 (expected) · b1faaa04 · Docs: the Arugula look"
    );
    assert_eq!(t(&rel, None, Some("0.63.9")), "Release 0.63.9 (expected) · b1faaa04");
    // A release whose version wasn't read is a deploy like another.
    assert_eq!(t(&rel, Some("Fix"), None), "Deploy b1faaa04 to production · Fix");
    assert_eq!(t(&other, Some("Fix"), Some("9.9.9")), "Deploy b1faaa04 to production · Fix");
    assert_eq!(t(&other, None, None), "Deploy to production");
}

#[test]
fn the_expected_version_is_what_release_bump_makes() {
    let toml =
        |v: &str| format!("[package]\nname = \"x\"\nversion = \"9.9.9\"\n\n[workspace.package]\nversion = \"{v}\"\n");
    // The higher of the commit's and main's, then one bump up.
    assert_eq!(expected_version(Lane::Daemon, &toml("0.63.8"), &toml("0.63.9"), "patch").as_deref(), Some("0.63.10"));
    assert_eq!(expected_version(Lane::Daemon, &toml("0.64.0"), &toml("0.63.9"), "minor").as_deref(), Some("0.65.0"));
    assert_eq!(expected_version(Lane::Daemon, &toml("0.63.8"), &toml("0.63.9"), "major").as_deref(), Some("1.0.0"));
    // The app's lane reads the desktop crate's [package].
    assert_eq!(expected_version(Lane::App, &toml("0.1.0"), &toml("0.1.0"), "patch").as_deref(), Some("9.9.10"));
    assert_eq!(expected_version(Lane::Daemon, "no version", &toml("0.63.9"), "patch"), None);
}

#[test]
fn a_subject_is_the_first_line_cut_short() {
    assert_eq!(subject_of("  Docs: look \n\nbody").as_deref(), Some("Docs: look"));
    assert_eq!(subject_of("\n").as_deref(), None);
    let long = subject_of(&"w".repeat(300)).unwrap();
    assert_eq!(long.chars().count(), SUBJECT_MAX);
    assert!(long.ends_with('…'));
}
