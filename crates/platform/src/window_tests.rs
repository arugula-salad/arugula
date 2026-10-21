use std::path::{Path, PathBuf};

use super::*;

/// A directory of its own, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let d = std::env::temp_dir().join(format!("arugula-window-{}", hex::encode(arugula_e2e::random::<8>())));
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

/// A hub's file as production has it before the window: the hub's own
/// tables, and the platform's beside them with rows in, the in-process
/// platform's `platform_migrations` among them.
fn hub_file(path: &Path) {
    let hub = arugula_control_common::sqlite::open(path).unwrap();
    hub.execute_batch(
        "CREATE TABLE accounts (id TEXT PRIMARY KEY, root TEXT, created INTEGER NOT NULL);
         INSERT INTO accounts VALUES ('acct-a', NULL, 1), ('acct-b', NULL, 2);",
    )
    .unwrap();
    sample(&crate::db::Db::open(path).unwrap());
}

fn count(path: &Path, sql: &str) -> i64 {
    let c = read_only(path).unwrap();
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// Every platform table, and nothing else: the shadow tables and the
/// migrations' own table aren't copied, and no table counts on an
/// `AUTOINCREMENT` sequence the copy would have to carry.
#[test]
fn the_tables_are_the_platforms_own() {
    let t = tables();
    assert!(t.contains(&"chat_messages".to_owned()) && t.contains(&"hub_feed".to_owned()), "{t:?}");
    assert!(t.iter().all(|n| n != "platform_migrations" && !n.starts_with("chat_fts") && !n.starts_with("sqlite_")));
    assert!(
        t.iter().position(|n| n == "cloud_runs") < t.iter().position(|n| n == "cloud_run_secrets"),
        "parents first"
    );
    let db = crate::db::Db::memory();
    let auto: i64 = db
        .sqlite()
        .query_row("SELECT count(*) FROM sqlite_master WHERE sql LIKE '%AUTOINCREMENT%'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(auto, 0);
}

/// The window's copy: every row of every table, rowids kept, the index
/// rebuilt so search finds what it found, and the counts and checksums
/// equal on both sides. The target's migrations are its own.
#[test]
fn a_copy_moves_every_row_and_proves_it() {
    let d = Dir::new();
    let (hub, platform) = (d.at("control.db"), d.at("platform.db"));
    hub_file(&hub);
    let before = checksums(&hub).unwrap();
    let r = copy(&hub, &platform, false).unwrap();
    assert!(r.tables.iter().all(Table::matches));
    assert_eq!(r.tables.len(), tables().len());
    assert_eq!(r.fts_rows, 4);
    assert_eq!(r.rows(), before.iter().map(|t| t.1).sum::<u64>());
    let msgs = r.tables.iter().find(|t| t.name == "chat_messages").unwrap();
    assert_eq!((msgs.from_rows, msgs.to_rows), (4, 4));
    assert_eq!(checksums(&platform).unwrap(), before, "the copy's own checksums are the source's");
    assert_eq!(checksums(&hub).unwrap(), before, "the source is untouched");
    let text = r.render(&hub, &platform);
    assert!(text.contains("every count and checksum matches"), "{text}");
    assert!(text.contains("chat_messages"), "{text}");
    assert!(!text.contains("ghp_never_printed"));

    assert_eq!(
        count(&platform, "SELECT rowid FROM chat_fts WHERE chat_fts MATCH 'rehearsal' ORDER BY rowid LIMIT 1"),
        7
    );
    assert_eq!(count(&platform, "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH 'rehearsal'"), 2);
}

/// The rowids some queries order by survive (a delete left a gap).
#[test]
fn rowids_are_kept() {
    let d = Dir::new();
    let (hub, platform) = (d.at("control.db"), d.at("platform.db"));
    hub_file(&hub);
    copy(&hub, &platform, false).unwrap();
    let rows = |p: &Path| -> Vec<(i64, String)> {
        let c = read_only(p).unwrap();
        let mut q = c.prepare("SELECT rowid, conversation FROM cloud_notifications ORDER BY rowid").unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    assert_eq!(rows(&platform), rows(&hub));
    assert_eq!(rows(&hub).iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["c1", "c3"]);
    assert_eq!(rows(&hub)[0].0, 2, "c2's rowid was 1, and stays a gap");
}

/// `platform_migrations` isn't copied: the target's runner made the schema
/// and recorded its own migrations, none of them adopted.
#[test]
fn the_targets_migrations_are_its_own() {
    let d = Dir::new();
    let (hub, platform) = (d.at("control.db"), d.at("platform.db"));
    hub_file(&hub);
    copy(&hub, &platform, false).unwrap();
    let latest = crate::db::migrate::Dialect::Sqlite.migrations().len() as i64;
    assert_eq!(count(&platform, "SELECT count(*) FROM platform_migrations"), latest);
    assert_eq!(count(&platform, "SELECT count(*) FROM platform_migrations WHERE adopted = 1"), 0);
    // And it opens as the platform opens it, with nothing left to run.
    drop(crate::db::Db::open(&platform).unwrap());
}

/// A second copy into the same file is refused, saying what's there;
/// `--replace` empties it first, so nothing doubles.
#[test]
fn a_target_with_rows_is_refused_unless_replaced() {
    let d = Dir::new();
    let (hub, platform) = (d.at("control.db"), d.at("platform.db"));
    hub_file(&hub);
    copy(&hub, &platform, false).unwrap();
    let e = copy(&hub, &platform, false).unwrap_err().to_string();
    assert!(e.contains("already holds the platform's rows") && e.contains("chat_messages (4)"), "{e}");
    let r = copy(&hub, &platform, true).unwrap();
    assert!(r.replaced > 0);
    assert_eq!(checksums(&platform).unwrap(), checksums(&hub).unwrap());
    assert_eq!(count(&platform, "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH 'psst'"), 1);
}

/// The rollback: what the platform wrote after the window goes back into
/// the hub's file over its old copies, and the hub's own tables stay as
/// they were.
#[test]
fn copying_back_replaces_the_hubs_old_copies() {
    let d = Dir::new();
    let (hub, platform) = (d.at("control.db"), d.at("platform.db"));
    hub_file(&hub);
    copy(&hub, &platform, false).unwrap();
    {
        let db = crate::db::Db::open(&platform).unwrap();
        db.sqlite()
            .execute_batch(
                "INSERT INTO chat_messages (id, room, seq, author, at, text) VALUES (41, 'r2', 2, 'acct-a', 50, 'after the window');
                 UPDATE chat_rooms SET last_seq = 2 WHERE id = 'r2';",
            )
            .unwrap();
    }
    let e = copy(&platform, &hub, false).unwrap_err().to_string();
    assert!(e.contains("--replace"), "{e}");
    copy(&platform, &hub, true).unwrap();
    assert_eq!(checksums(&hub).unwrap(), checksums(&platform).unwrap());
    assert_eq!(count(&hub, "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH 'window'"), 1);
    assert_eq!(count(&hub, "SELECT count(*) FROM accounts"), 2, "the hub's own table is left alone");
}

/// What it won't copy from or into.
#[test]
fn it_refuses_what_isnt_a_copy() {
    let d = Dir::new();
    let hub = d.at("control.db");
    hub_file(&hub);
    let e = copy(&hub, &hub, true).unwrap_err().to_string();
    assert!(e.contains("same file"), "{e}");
    let e = copy(&d.at("nothing.db"), &d.at("out.db"), false).unwrap_err().to_string();
    assert!(e.contains("isn't there"), "{e}");
    // A hub's file the platform was never opened on.
    let bare = d.at("bare.db");
    arugula_control_common::sqlite::open(&bare).unwrap().execute_batch("CREATE TABLE accounts (id TEXT)").unwrap();
    let e = copy(&bare, &d.at("out.db"), false).unwrap_err().to_string();
    assert!(e.contains("chat_messages: not in the source"), "{e}");
    // A source a column short (a build behind this one's migrations).
    let old = d.at("old.db");
    hub_file(&old);
    arugula_control_common::sqlite::open(&old)
        .unwrap()
        .execute_batch("ALTER TABLE chat_readers DROP COLUMN at")
        .unwrap();
    let out = d.at("out2.db");
    let e = copy(&old, &out, false).unwrap_err().to_string();
    assert!(e.contains("chat_readers: columns differ"), "{e}");
    assert_eq!(count(&out, "SELECT count(*) FROM chat_rooms"), 0, "nothing was copied");
}

/// The checksum: the same rows in another order, or columns in another
/// order, sum the same; any value, or its type, changing changes it.
#[test]
fn the_checksum_sees_values_not_order() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE a (x, y); INSERT INTO a (rowid, x, y) VALUES (1, 'p', 1), (2, 'q', 2.0);
         CREATE TABLE b (y, x); INSERT INTO b (rowid, y, x) VALUES (2, 2.0, 'q'), (1, 1, 'p');
         CREATE TABLE c (x, y); INSERT INTO c (rowid, x, y) VALUES (1, 'p', 1), (2, 'q', 2);
         CREATE TABLE e (x, y); INSERT INTO e (rowid, x, y) VALUES (1, 'p', 1), (3, 'q', 2.0);",
    )
    .unwrap();
    let s = |t: &str| sum(&c, "main", t).unwrap();
    assert_eq!(s("a"), s("b"));
    assert_eq!(s("a").0, 2);
    assert_ne!(s("a").1, s("c").1, "2.0 isn't 2");
    assert_ne!(s("a").1, s("e").1, "nor is another rowid");
}

/// The secrets check opens each sealed value with the key and says which
/// don't, never what they hold.
#[test]
fn secrets_open_only_with_their_key() {
    let d = Dir::new();
    let hub = d.at("control.db");
    hub_file(&hub);
    assert_eq!(secrets_open(&hub, SAMPLE_KEY).unwrap(), (1, vec![]));
    let wrong = "02".repeat(32);
    assert_eq!(secrets_open(&hub, &wrong).unwrap(), (0, vec![("acct-a".into(), "GITHUB_TOKEN".into())]));
}
