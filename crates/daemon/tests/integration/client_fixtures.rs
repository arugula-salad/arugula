//! The client fixtures (#200) replayed against this daemon: each fixture's
//! client side, sent to a fresh daemon, gets answers of the recorded shape
//! (`arugula_testkit::fixture::replay`). The current set
//! (`crates/proto/fixtures`) checks the clients being written now; the last
//! two releases' sets (`crates/proto/fixtures/releases/<version>`) check
//! that a daemon change doesn't break a client already released.

use std::path::Path;

use arugula_proto::fixture;
use arugula_testkit::fixture::{daemon, ready, replay};

/// Replay every fixture in `dir`, each against a daemon of its own; fail
/// with every fixture that didn't play.
fn replay_dir(set: &str, dir: &Path) {
    let fixtures = fixture::load_dir(dir).unwrap();
    assert!(!fixtures.is_empty(), "no fixtures in {}", dir.display());
    let mut failed = vec![];
    for f in &fixtures {
        let d = daemon(env!("CARGO_BIN_EXE_arugulad"), "fx").start();
        ready(&d);
        if let Err(e) = replay(&d, f) {
            failed.push(format!("{set}: {e}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn current_fixtures_play() {
    replay_dir("current", &fixture::dir());
}

#[test]
fn released_fixtures_still_play() {
    let sets = fixture::releases(2);
    if sets.is_empty() {
        eprintln!("no released fixture sets in crates/proto/fixtures/releases yet; nothing to replay");
        return;
    }
    for (version, dir) in sets {
        replay_dir(&version, &dir);
    }
}

/// The checked-in set covers what the SSH clients use, and says so.
#[test]
fn the_set_has_what_clients_use() {
    let names: Vec<String> =
        fixture::load_dir(&fixture::dir()).unwrap().into_iter().map(|f| f.header.fixture).collect();
    for want in ["attach", "run-capture", "events"] {
        assert!(names.iter().any(|n| n == want), "no {want}.jsonl in crates/proto/fixtures (`just record-fixtures`)");
    }
}
