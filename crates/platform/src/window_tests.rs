use std::path::{Path, PathBuf};

use super::*;
use crate::db::Sql as _;

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
    // Parents first: every table a declared foreign key names comes before
    // the table that names it. Only declared keys bind the copy's order: a
    // table a migration made again (0049 made hosted_workspaces again for
    // its CHECK) moves to the end, after tables that name its ids without a
    // key, such as hosted_workspace_secrets.
    let db = crate::db::Db::memory();
    let at = |n: &str| t.iter().position(|x| x == n);
    for child in &t {
        let parents: Vec<String> = db
            .sqlite()
            .prepare(&format!("SELECT \"table\" FROM pragma_foreign_key_list('{child}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for parent in parents {
            assert!(at(&parent) < at(child), "{parent} before {child}");
        }
    }
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
    assert_eq!(count(&platform, "SELECT rowid FROM issues_fts WHERE issues_fts MATCH 'canary'"), 3, "issues' too");
}

/// Rowids survive (a delete left a gap).
#[test]
fn rowids_are_kept() {
    let d = Dir::new();
    let (hub, platform) = (d.at("control.db"), d.at("platform.db"));
    hub_file(&hub);
    copy(&hub, &platform, false).unwrap();
    let rows = |p: &Path| -> Vec<(i64, String)> {
        let c = read_only(p).unwrap();
        let mut q = c.prepare("SELECT rowid, reader FROM hub_feed ORDER BY rowid").unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    assert_eq!(rows(&platform), rows(&hub));
    assert_eq!(rows(&hub).iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["projects", "chat"]);
    assert_eq!(rows(&hub)[0].0, 2, "gone's rowid was 1, and stays a gap");
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
    let opened = crate::keys::Checked { opened: 1, by_master: 1, ..Default::default() };
    assert_eq!(secrets_open(&hub, SAMPLE_KEY, None).unwrap(), opened);
    let wrong = "02".repeat(32);
    let failed = vec![("hw_1".into(), "GITHUB_TOKEN".into())];
    assert_eq!(secrets_open(&hub, &wrong, None).unwrap(), crate::keys::Checked { failed, ..Default::default() });
}

/// The second window's copy (M145), to Postgres and back: every count and
/// portable checksum the same on both sides, read back alike by
/// `checksums`; Postgres's identities moved past the copied rows; search
/// and secrets work there; and what Postgres can't keep as it is (a real
/// in an integer column) refused, with nothing written.
#[test]
fn a_copy_to_postgres_and_back_matches() {
    let pg = crate::db::Db::test();
    let Some(url) = pg.test_url() else { return eprintln!("skipped: needs ARUGULA_TEST_POSTGRES_URL") };
    let d = Dir::new();
    let hub = d.at("control.db");
    hub_file(&hub);
    let (file, to) = (Place::File(hub.clone()), Place::Postgres(url));
    let refused = portable::copy(&file, &to, false).unwrap_err();
    assert!(format!("{refused:#}").contains("writing hub_feed"), "{refused:#}");
    let n: u64 =
        pg.c().query_row("SELECT count(*) FROM chat_messages", crate::db::sql::params![], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "nothing was written");
    rusqlite::Connection::open(&hub).unwrap().execute("UPDATE hub_feed SET seq = 2 WHERE seq = 2.5", []).unwrap();

    let r = portable::copy(&file, &to, false).unwrap();
    assert!(r.tables.iter().all(|t| t.matches()), "{}", r.render(&file, &to));
    assert_eq!(r.rows(), checksums(&hub).unwrap().iter().map(|t| t.1).sum::<u64>());
    assert!(r.render(&file, &to).contains("every count and checksum matches"));
    assert!(!r.render(&file, &to).contains(":m144@") && !to.to_string().contains("m144"), "no password printed");
    assert_eq!(portable::checksums(&file).unwrap(), portable::checksums(&to).unwrap());
    let id: i64 = pg
        .c()
        .query_row(
            "INSERT INTO chat_messages (room, seq, author, at, text) VALUES ('r1', 4, 'acct-a', 30, 'after') RETURNING id",
            crate::db::sql::params![],
            |r| r.get(0),
        )
        .unwrap();
    assert!(id > 40, "the identity went on past the copied ids: {id}");
    let words = [crate::db::chat::Word { text: "rehearsal".into(), prefix: false }];
    let search = crate::db::chat::Search { words: &words, ..Default::default() };
    let found = pg.chat_search(&["r1".into()], &search, 10).unwrap();
    assert_eq!(found.len(), 2, "search finds the copied words");
    let words = [crate::db::chat::Word { text: "canary".into(), prefix: false }];
    assert_eq!(pg.search_issues("prj_t", &words, 10).unwrap().len(), 1, "and the copied issues");
    assert_eq!(portable::secrets_open(&to, SAMPLE_KEY, None).unwrap().failed, vec![]);
    let again = portable::copy(&file, &to, false).unwrap_err();
    assert!(again.to_string().contains("already holds"), "{again}");

    // And back, for a rollback after the window: a new file.
    let back = Place::File(d.at("back.db"));
    let r = portable::copy(&to, &back, false).unwrap();
    assert_eq!(r.fts_rows, Some(5), "chat_fts rebuilt there");
    assert_eq!(portable::checksums(&back).unwrap(), portable::checksums(&to).unwrap());
    assert_eq!(count(&d.at("back.db"), "SELECT rowid FROM issues_fts WHERE issues_fts MATCH 'canary'"), 3);
}

/// `migrations` reads what a database has applied without running
/// anything (M145's drill and nightly dump): a file this build made has
/// nothing to run; an edited migration is refused; a database that has
/// never seen the platform is refused.
#[test]
fn migrations_reads_what_a_file_applied() {
    let d = Dir::new();
    let file = d.at("platform.db");
    drop(crate::db::Db::open(&file).unwrap());
    let at = Place::File(file.clone());
    let m = portable::migrations(&at).unwrap();
    let known = crate::db::migrate::Dialect::Sqlite.migrations().last().unwrap().version;
    assert_eq!((m.known, m.pending.clone(), m.ahead.clone()), (known, vec![], vec![]));
    assert_eq!(m.applied.len() as u32, known);
    assert!(m.render(&at).contains("nothing to run"), "{}", m.render(&at));

    rusqlite::Connection::open(&file)
        .unwrap()
        .execute("UPDATE platform_migrations SET checksum = 'edited' WHERE version = 1", [])
        .unwrap();
    let e = portable::migrations(&at).unwrap_err();
    assert!(format!("{e:#}").contains("has changed since it ran"), "{e:#}");

    let empty = d.at("empty.db");
    rusqlite::Connection::open(&empty).unwrap().execute_batch("CREATE TABLE t (a)").unwrap();
    let e = portable::migrations(&Place::File(empty)).unwrap_err();
    assert!(e.to_string().contains("never started on it"), "{e}");
}

/// The read-only tools never migrate Postgres: a database behind this
/// build is refused by `checksums` and `secrets-check`, `migrations` says
/// what this build would run, and nothing ran (the nightly dump reads the
/// live cluster with them, M145).
#[test]
fn the_read_only_tools_never_migrate_postgres() {
    let pg = crate::db::Db::test();
    let Some(url) = pg.test_url() else { return eprintln!("skipped: needs ARUGULA_TEST_POSTGRES_URL") };
    let at = Place::Postgres(url);
    let last = crate::db::migrate::Dialect::Postgres.migrations().last().unwrap().version;
    assert!(portable::migrations(&at).unwrap().pending.is_empty());
    assert!(portable::checksums(&at).is_ok());

    // As if the database were a migration behind this build.
    pg.c().execute("DELETE FROM platform_migrations WHERE version = ?1", crate::db::sql::params![last]).unwrap();
    let m = portable::migrations(&at).unwrap();
    assert_eq!(m.pending, vec![last]);
    assert!(m.render(&at).contains("would run"), "{}", m.render(&at));
    for e in [portable::checksums(&at).unwrap_err(), portable::secrets_open(&at, SAMPLE_KEY, None).unwrap_err()] {
        assert!(e.to_string().contains("this tool only reads"), "{e}");
    }
    let n: i64 = pg
        .c()
        .query_row("SELECT count(*) FROM platform_migrations WHERE version = ?1", crate::db::sql::params![last], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 0, "nothing was migrated");
}

/// `checksums --snapshot` reads in a snapshot another session exported:
/// a row written after it doesn't count, as it wouldn't in a
/// `pg_dump --snapshot` of it (the nightly dump's record, M145).
#[test]
fn checksums_in_an_exported_snapshot() {
    let pg = crate::db::Db::test();
    let Some(url) = pg.test_url() else { return eprintln!("skipped: needs ARUGULA_TEST_POSTGRES_URL") };
    let d = Dir::new();
    let hub = d.at("control.db");
    hub_file(&hub);
    rusqlite::Connection::open(&hub).unwrap().execute("UPDATE hub_feed SET seq = 2 WHERE seq = 2.5", []).unwrap();
    let at = Place::Postgres(url);
    portable::copy(&Place::File(hub), &at, false).unwrap();
    let before = portable::checksums(&at).unwrap();

    let holder = pg.c();
    holder.execute_batch("BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY").unwrap();
    let id: String = holder.query_row("SELECT pg_export_snapshot()", crate::db::sql::params![], |r| r.get(0)).unwrap();
    pg.c()
        .execute(
            "INSERT INTO chat_messages (room, seq, author, at, text) VALUES ('r1', 4, 'acct-a', 30, 'after the snapshot')",
            crate::db::sql::params![],
        )
        .unwrap();
    assert_ne!(portable::checksums(&at).unwrap(), before, "the new row counts outside the snapshot");
    assert_eq!(portable::checksums_in(&at, Some(&id)).unwrap(), before, "and not inside it");
    holder.execute_batch("COMMIT").unwrap();

    let e = portable::checksums_in(&at, Some("1'; DROP TABLE chat_rooms; --")).unwrap_err();
    assert!(e.to_string().contains("isn't a snapshot id"), "{e}");
    let e = portable::checksums_in(&Place::File(d.at("control.db")), Some(&id)).unwrap_err();
    assert!(e.to_string().contains("Postgres's"), "{e}");
}

/// A Postgres URL can be named without typing it (M145's window): `@FILE`
/// reads it from a file, `env:NAME` from the environment; neither prints
/// its password.
#[test]
fn a_url_comes_from_a_file_or_the_environment() {
    let d = Dir::new();
    let file = d.at("pg.url");
    std::fs::write(&file, "postgres://fly-user:hunter2@direct.example/fly-db?sslmode=disable\n").unwrap();
    let at: Place = format!("@{}", file.display()).parse().unwrap();
    assert_eq!(at, Place::Postgres("postgres://fly-user:hunter2@direct.example/fly-db?sslmode=disable".into()));
    assert!(!at.to_string().contains("hunter2"), "{at}");
    std::fs::write(&file, "not a url").unwrap();
    assert!(format!("@{}", file.display()).parse::<Place>().unwrap_err().contains("doesn't hold"));
    assert!("@/nowhere/pg.url".parse::<Place>().is_err());
    assert!("env:ARUGULA_WINDOW_TEST_NO_SUCH_VARIABLE".parse::<Place>().unwrap_err().contains("neither"));
    assert_eq!("/data/platform.db".parse::<Place>().unwrap(), Place::File("/data/platform.db".into()));
}

/// `previews` (M187): the operator lists what's public, and takes one
/// down at once by its URL, host or id.
#[test]
fn previews_lists_and_takes_down() {
    let d = Dir::new();
    let file = d.at("platform.db");
    let id = "0123456789abcdef0123456789abcdef";
    {
        let db = crate::db::Db::open(&file).unwrap();
        db.add_hosted_workspace("hw_1", "prj_a", "dev", &Default::default(), "alice", 5, 0).unwrap();
        let now = arugula_e2e::now_ms();
        db.make_port_public("hw_1", 5173, "alice", id, now + 60_000, now).unwrap();
        db.make_port_public("hw_1", 3000, "bob", "ffffffffffffffffffffffffffffffff", now + 60_000, now).unwrap();
    }
    let at = Place::File(file);
    let (taken, list) = portable::previews(&at, None).unwrap();
    assert!(!taken);
    assert_eq!(list.len(), 2);
    let (taken, list) = portable::previews(&at, Some(&format!("https://{id}.arugula.site/some/page"))).unwrap();
    assert!(taken);
    assert_eq!(list.iter().map(|p| p.port).collect::<Vec<_>>(), [3000]);
    let (taken, _) = portable::previews(&at, Some(id)).unwrap();
    assert!(!taken, "already down");
}
