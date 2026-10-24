use std::path::{Path, PathBuf};

use super::*;
use crate::window::{checksums, copy, drop_command, sample, tables, window_era, window_output};

/// The first window's `window-hub.txt`, as the hub printed it
/// (2026-10-21, #1158).
const WINDOW_HUB: &str = "copied the platform's tables
  from /data/control.db
  to   /tmp/window.db

table                    rows from   rows to  sha256 from       sha256 to
billing                          0         0  e3b0c44298fc1c14  e3b0c44298fc1c14  ok
reported                         0         0  e3b0c44298fc1c14  e3b0c44298fc1c14  ok
chat_rooms                      13        13  b601ae5e2ab25958  b601ae5e2ab25958  ok
chat_members                     3         3  c780b01406a7a248  c780b01406a7a248  ok
chat_messages                   26        26  3fa739de993760dc  3fa739de993760dc  ok
chat_reads                       3         3  a96e268069585885  a96e268069585885  ok
chat_readers                    62        62  18a7992b86695880  18a7992b86695880  ok
cloud_agents                     0         0  e3b0c44298fc1c14  e3b0c44298fc1c14  ok
cloud_runs                       1         1  1c8a62f0cf9fb188  1c8a62f0cf9fb188  ok
cloud_secrets                    0         0  e3b0c44298fc1c14  e3b0c44298fc1c14  ok
cloud_run_secrets                0         0  e3b0c44298fc1c14  e3b0c44298fc1c14  ok
cloud_jobs                       5         5  4064a392c671f3a8  4064a392c671f3a8  ok
cloud_requests                   6         6  948d2396969fe84c  948d2396969fe84c  ok
cloud_notifications              6         6  936cc3fa1f9ea1d7  936cc3fa1f9ea1d7  ok
cloud_events                   864       864  8cd02a0aba0e5b17  8cd02a0aba0e5b17  ok
cloud_workspaces                 5         5  ccf80aaed5bc0ddb  ccf80aaed5bc0ddb  ok
cloud_workspace_specs            3         3  0fcf7c589e936524  0fcf7c589e936524  ok
cloud_tabs                       8         8  bb1957ac81cc420c  bb1957ac81cc420c  ok
projects                         4         4  305c80985a163d5b  305c80985a163d5b  ok
team_members                     9         9  c4f459cf2cf9e841  c4f459cf2cf9e841  ok
grants                          14        14  4a8bbc697cdff677  4a8bbc697cdff677  ok
hub_feed                         1         1  4250b2775b951222  4250b2775b951222  ok
chat_fts                                  26  rebuilt from chat_messages, integrity-check ok

22 tables, 1033 rows: every count and checksum matches
";

/// A directory of its own, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let d = std::env::temp_dir().join(format!("arugula-drop-{}", hex::encode(arugula_e2e::random::<8>())));
        std::fs::create_dir_all(&d).unwrap();
        Self(d)
    }
    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const OUT: (&str, &str) = ("http://arugula-platform.internal:7700", "the test's");

/// The hub's file as the first window left it, with tables of the hub's
/// own (a foreign key, a trigger and a view among them), and the window's
/// output for it in `window-hub.txt`.
fn hub_after_window(d: &Dir) -> (PathBuf, PathBuf) {
    let (hub, expect) = (d.at("control.db"), d.at("window-hub.txt"));
    arugula_control_common::sqlite::open(&hub)
        .unwrap()
        .execute_batch(
            "CREATE TABLE accounts (id TEXT PRIMARY KEY, created INTEGER NOT NULL);
             CREATE TABLE sessions (id TEXT PRIMARY KEY, account TEXT NOT NULL REFERENCES accounts(id));
             CREATE INDEX sessions_account ON sessions (account);
             CREATE TABLE audit (what TEXT);
             CREATE TRIGGER accounts_audit AFTER DELETE ON accounts BEGIN INSERT INTO audit VALUES (old.id); END;
             CREATE VIEW live_sessions AS SELECT s.id FROM sessions s JOIN accounts a ON a.id = s.account;
             INSERT INTO accounts VALUES ('acct-a', 1), ('acct-b', 2);
             INSERT INTO sessions VALUES ('s1', 'acct-a');",
        )
        .unwrap();
    window_era(&hub);
    std::fs::write(&expect, window_output(&hub)).unwrap();
    (hub, expect)
}

/// Every table, index, trigger and view in the file.
fn schema(path: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open(path).unwrap();
    c.prepare("SELECT type || ' ' || name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY 1")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn count(path: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(path).unwrap().query_row(sql, [], |r| r.get(0)).unwrap()
}

/// The window's real output: 22 tables, the first 16 hex digits of each
/// checksum, its own total; `checksums`' output too; and an output cut
/// short, or one that didn't match, proves nothing.
#[test]
fn the_windows_output_is_read() {
    let e = parse_expected(WINDOW_HUB).unwrap();
    assert_eq!(e.len(), 22);
    assert_eq!(e.iter().map(|e| e.rows).sum::<u64>(), 1033);
    assert_eq!(e[14], Expected { name: "cloud_events".into(), rows: 864, sum: "8cd02a0aba0e5b17".into() });
    assert!(e.iter().all(|e| e.name != "chat_fts"));
    assert_eq!(e.iter().filter(|e| e.name.starts_with("cloud_")).count(), 11);

    let sums = format!(
        "the platform's tables in /data/control.db (read-only)\n\ntable rows  sha256\nhub_feed 1 {}\n\n1 tables, 1 rows\n",
        "4250b2775b951222".repeat(4)
    );
    let e = parse_expected(&sums).unwrap();
    assert_eq!(e[0].sum.len(), 64);

    let cut: String = WINDOW_HUB.lines().filter(|l| !l.starts_with("grants")).map(|l| format!("{l}\n")).collect();
    let err = parse_expected(&cut).unwrap_err().to_string();
    assert!(err.contains("cut short"), "{err}");
    let differs = WINDOW_HUB.replace(
        "hub_feed                         1         1  4250b2775b951222  4250b2775b951222  ok",
        "hub_feed                         1         2  4250b2775b951222  0000000000000000  DIFFERS",
    );
    let err = parse_expected(&differs).unwrap_err().to_string();
    assert!(err.contains("didn't match"), "{err}");
    assert!(parse_expected("nothing here\n").is_err());
}

/// The whole run: without --yes it says what it would drop and writes
/// nothing; with it, the window's 22 tables go (Cloud's, which this build
/// doesn't make, among them), with `chat_fts`, its shadow tables, every
/// trigger and index of theirs and `platform_migrations`; the hub's own
/// tables, rows, trigger and view stay. A second run refuses and changes
/// nothing, and the rollback's copy back still fills the file.
#[test]
fn it_drops_the_copies_and_nothing_else() {
    let d = Dir::new();
    let (hub, expect) = hub_after_window(&d);
    let current = tables();
    assert!(!current.iter().any(|t| t.starts_with("cloud_")), "this build doesn't make Cloud's tables");
    let before = schema(&hub);
    assert!(before.contains(&"table cloud_events".into()) && before.contains(&"table chat_fts_data".into()));

    let e = drop_command(&hub, &expect, false, OUT).unwrap_err().to_string();
    assert!(e.contains("would drop") && e.contains("pass --yes"), "{e}");
    assert!(e.contains("cloud_events                     3  the window's copy, unchanged"), "{e}");
    assert_eq!(schema(&hub), before, "a check writes nothing");

    let out = drop_command(&hub, &expect, true, OUT).unwrap();
    assert!(out.contains("24 tables, 24 rows, in one transaction; the hub's 3 tables are as they were"), "{out}");
    assert!(out.contains("chat_fts                         2  the platform's index"), "{out}");
    assert!(out.contains("platform_migrations              3  the platform's migrations"), "{out}");
    assert_eq!(
        schema(&hub),
        [
            "index sessions_account",
            "table accounts",
            "table audit",
            "table sessions",
            "trigger accounts_audit",
            "view live_sessions"
        ]
    );
    assert_eq!(count(&hub, "SELECT count(*) FROM accounts"), 2);
    assert_eq!(count(&hub, "SELECT count(*) FROM live_sessions"), 1);

    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("gone already"), "{e}");
    assert_eq!(schema(&hub).len(), 6);

    // Rolling back to in process starts with copying the platform back.
    let platform = d.at("platform.db");
    sample(&crate::db::Db::open(&platform).unwrap());
    copy(&platform, &hub, false).unwrap();
    assert_eq!(checksums(&hub).unwrap(), checksums(&platform).unwrap());
    assert_eq!(count(&hub, "SELECT count(*) FROM accounts"), 2);
}

/// A copy something wrote since the window, or one that's missing, is
/// refused, and nothing is dropped.
#[test]
fn a_changed_copy_is_refused() {
    let d = Dir::new();
    let (hub, expect) = hub_after_window(&d);
    let before = schema(&hub);
    rusqlite::Connection::open(&hub)
        .unwrap()
        .execute("UPDATE cloud_events SET body = '[]' WHERE cursor = 2", [])
        .unwrap();
    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("something wrote") && e.contains("cloud_events"), "{e}");
    assert_eq!(schema(&hub), before);

    let d = Dir::new();
    let (hub, expect) = hub_after_window(&d);
    rusqlite::Connection::open(&hub).unwrap().execute("DELETE FROM grants", []).unwrap();
    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("grants") && e.contains("rows now"), "{e}");

    rusqlite::Connection::open(&hub).unwrap().execute("DROP TABLE cloud_tabs", []).unwrap();
    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("lacks the window's cloud_tabs"), "{e}");
}

/// With the platform in process, its tables are the live ones.
#[test]
fn a_hub_running_the_platform_in_process_is_refused() {
    let d = Dir::new();
    let (hub, expect) = hub_after_window(&d);
    let before = schema(&hub);
    for at in ["", "in-process"] {
        let e = drop_command(&hub, &expect, true, (at, "the running server's (pid 1)")).unwrap_err().to_string();
        assert!(e.contains("runs the platform in process"), "{e}");
    }
    assert!(out_of_process("https://arugula-platform.flycast:7700"));
    assert_eq!(schema(&hub), before);
}

/// What isn't the platform's is never dropped: a hub table `--expect`
/// names, or one the hub's schema still points at; a platform table the
/// window didn't copy goes only while it's empty.
#[test]
fn only_the_platforms_tables_go() {
    let d = Dir::new();
    let (hub, expect) = hub_after_window(&d);
    let before = schema(&hub);
    let mut c = rusqlite::Connection::open(&hub).unwrap();
    let (n, sum) = super::sum(&c, "main", "accounts").unwrap();
    let forged = d.at("forged.txt");
    std::fs::write(&forged, format!("table rows sha256\naccounts {n} {sum}\n")).unwrap();
    let e = drop_command(&hub, &forged, true, OUT).unwrap_err().to_string();
    assert!(e.contains("no platform migration ever made accounts"), "{e}");

    c.execute_batch("CREATE VIEW room_names AS SELECT name FROM chat_rooms").unwrap();
    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("view room_names names chat_rooms"), "{e}");
    c.execute_batch("DROP VIEW room_names; CREATE TABLE pins (room TEXT REFERENCES chat_rooms(id))").unwrap();
    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("table pins has a foreign key to chat_rooms"), "{e}");
    c.execute_batch("DROP TABLE pins").unwrap();
    assert_eq!(schema(&hub), before);

    // A later migration's table, which the window didn't copy.
    let tx = c.transaction().unwrap();
    tx.execute_batch(include_str!("../migrations/sqlite/0004_chat_events.sql")).unwrap();
    tx.execute("INSERT INTO chat_events (seq, kind, body, at) VALUES (1, 'message', '{}', 1)", []).unwrap();
    tx.commit().unwrap();
    let e = drop_command(&hub, &expect, true, OUT).unwrap_err().to_string();
    assert!(e.contains("rows in platform tables the window didn't copy: chat_events (1)"), "{e}");
    c.execute("DELETE FROM chat_events", []).unwrap();
    let out = drop_command(&hub, &expect, true, OUT).unwrap();
    assert!(out.contains("chat_events                      0  this build's platform table, empty"), "{out}");
    assert_eq!(schema(&hub).len(), 6);
}
