//! The migration runner, and the match test (#1159): a new database built
//! from migrations has the schema production's becomes when it's adopted.

use rusqlite::Connection;

use super::{Dialect, Report, SQLITE, Shape, TABLE, run, run_set, run_to, tables};

/// Production's platform tables, as control v0.53.2 made them.
const PRODUCTION: &str = include_str!("snapshots/production-v0.53.2.sql");
/// The platform's tables as main made them just before M142.
const MAIN: &str = include_str!("snapshots/main-0cd3465c.sql");
/// An older release's, before projects (M129).
const V0_50: &str = include_str!("snapshots/release-v0.50.0.sql");

fn latest() -> u32 {
    Dialect::Sqlite.migrations().last().unwrap().version
}

fn fresh() -> Connection {
    let mut c = Connection::open_in_memory().unwrap();
    run(&mut c).unwrap();
    c
}

fn shape(c: &Connection) -> Shape {
    Shape::of(c, &tables(c).unwrap()).unwrap()
}

fn sql_of(c: &Connection, name: &str) -> String {
    c.query_row("SELECT sql FROM sqlite_master WHERE name = ?1", [name], |r| r.get(0)).unwrap()
}

/// A database as an older build left it: a hub table beside the platform's
/// `snapshot`, with a row or two in it.
fn made_before(snapshot: &str) -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE accounts (id TEXT PRIMARY KEY); INSERT INTO accounts VALUES ('alice');").unwrap();
    c.execute_batch(snapshot).unwrap();
    c.execute_batch(
        "INSERT INTO chat_rooms (id, scope, kind, created_by, created) VALUES ('r1', 'team:t1', 'channel', 'alice', 1);
         INSERT INTO chat_messages (room, seq, author, at, text) VALUES ('r1', 1, 'alice', 1, 'the kettle is on');
         INSERT INTO cloud_jobs (id, body) VALUES ('run_1', '{}');",
    )
    .unwrap();
    c
}

/// What production's own file looks like: each column a probe added over
/// time is last, in the order they ran (`ALTER TABLE ... ADD COLUMN`
/// rewrites the stored `CREATE TABLE`), and the tables M114 and M115
/// rebuilt went through a `_v2` and a rename, which quotes their name.
fn age(c: &Connection) {
    c.execute_batch(
        "DROP INDEX cloud_workspaces_project; DROP INDEX chat_rooms_project; DROP INDEX cloud_secrets_project;",
    )
    .unwrap();
    for (table, column, decl) in [
        ("chat_members", "from_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("chat_messages", "imported_id", "INTEGER"),
        ("cloud_workspaces", "settings_applied", "INTEGER NOT NULL DEFAULT 1"),
        ("cloud_workspaces", "vault_secrets", "TEXT"),
        ("cloud_workspaces", "rebuild", "TEXT"),
        ("cloud_workspaces", "rebuild_error", "TEXT"),
        ("cloud_workspaces", "rebuilds", "INTEGER NOT NULL DEFAULT 0"),
        ("cloud_workspaces", "project_id", "TEXT"),
        ("chat_rooms", "project_id", "TEXT"),
        ("cloud_secrets", "project_id", "TEXT"),
        ("cloud_workspaces", "setup_step", "TEXT"),
        ("cloud_tabs", "relaunch", "TEXT"),
    ] {
        c.execute_batch(&format!(
            "ALTER TABLE {table} DROP COLUMN {column}; ALTER TABLE {table} ADD COLUMN {column} {decl};"
        ))
        .unwrap();
    }
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS cloud_workspaces_project ON cloud_workspaces (project_id);
         CREATE INDEX IF NOT EXISTS chat_rooms_project ON chat_rooms (project_id);
         CREATE INDEX IF NOT EXISTS cloud_secrets_project ON cloud_secrets (project_id);",
    )
    .unwrap();
    for table in ["cloud_jobs", "cloud_requests", "cloud_notifications", "cloud_events"] {
        let made = sql_of(c, table).replacen(table, &format!("{table}_v2"), 1);
        c.execute_batch(&format!(
            "BEGIN;
             {made};
             INSERT INTO {table}_v2 SELECT * FROM {table};
             DROP TABLE {table};
             ALTER TABLE {table}_v2 RENAME TO {table};
             COMMIT;"
        ))
        .unwrap();
    }
}

fn recorded(c: &Connection) -> Vec<(u32, bool)> {
    let mut q = c.prepare("SELECT version, adopted FROM platform_migrations ORDER BY version").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

fn additive(c: &Connection) -> Vec<(u32, bool)> {
    let mut q = c.prepare("SELECT version, additive FROM platform_migrations ORDER BY version").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

fn count(c: &Connection, sql: &str) -> u64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn a_new_database_runs_every_migration_once() {
    let mut c = Connection::open_in_memory().unwrap();
    let report = run(&mut c).unwrap();
    assert_eq!(report, Report { ran: (1..=latest()).collect(), ..Default::default() });
    assert_eq!(recorded(&c), (1..=latest()).map(|v| (v, false)).collect::<Vec<_>>());
    assert_eq!(additive(&c), SQLITE.iter().map(|m| (m.version, m.additive)).collect::<Vec<_>>());
    assert_eq!(run(&mut c).unwrap(), Report::default(), "nothing left to run");
}

/// The match test. Production's schema, as v0.53.2 made it and as its file
/// really is after years of probes, and main's from just before M142, are
/// each adopted at the version they are, run the rest, and end up exactly
/// where a new database does. Their rows and the hub's table stay.
#[test]
fn production_is_adopted_and_matches_a_new_database() {
    let new = fresh();
    let want = shape(&new);
    let names = tables(&new).unwrap();
    for (name, snapshot, aged, at) in
        [("v0.53.2", PRODUCTION, false, 1), ("v0.53.2, aged", PRODUCTION, true, 1), ("main", MAIN, false, 2)]
    {
        let mut c = made_before(snapshot);
        if aged {
            age(&c);
            assert!(sql_of(&c, "cloud_jobs").contains("\"cloud_jobs\""), "the rename's quotes");
            let last: String = c
                .query_row(
                    "SELECT name FROM pragma_table_info('cloud_workspaces') ORDER BY cid DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(last, "setup_step", "a probe's column, last");
        }
        let report = run(&mut c).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(report, Report { adopted: at, upgraded: false, ran: (at + 1..=latest()).collect() }, "{name}");
        let have = Shape::of(&c, &names).unwrap();
        assert_eq!(have.diff(&want), Vec::<String>::new(), "{name}");
        assert_eq!(have, want, "{name}");
        let adopted: Vec<(u32, bool)> = (1..=latest()).map(|v| (v, v <= at)).collect();
        assert_eq!(recorded(&c), adopted, "{name}");
        let n = |sql: &str| -> u64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(n("SELECT count(*) FROM accounts"), 1, "{name}: the hub's table");
        assert_eq!(n("SELECT count(*) FROM chat_fts WHERE chat_fts MATCH 'kettle'"), 1, "{name}: search");
        assert_eq!(n("SELECT count(*) FROM sqlite_master WHERE name LIKE 'cloud%'"), 0, "{name}: 0030 dropped them");
    }
}

/// The checked-in snapshots are what the migrations make at the version
/// production and main were at, so the match test can't pass by both
/// drifting together.
#[test]
fn the_snapshots_are_the_migrations_at_their_versions() {
    for (snapshot, at) in [(PRODUCTION, 1), (MAIN, 2)] {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(snapshot).unwrap();
        let mut m = Connection::open_in_memory().unwrap();
        run_to(&mut m, at).unwrap();
        assert_eq!(shape(&c).diff(&shape(&m)), Vec::<String>::new(), "version {at}");
    }
}

/// An older release's tables, as a self-hosted control that skipped
/// releases has them: the schema code from before migrations brings them
/// to main's, they're adopted there, and the rest runs. Rows stay.
#[test]
fn an_older_release_is_upgraded_then_adopted() {
    let new = fresh();
    let want = shape(&new);
    let names = tables(&new).unwrap();
    let v0_50 = made_before(V0_50);
    v0_50.execute_batch("INSERT INTO cloud_secrets (account, name, sealed) VALUES ('alice', 'A', x'00')").unwrap();
    // Older still: v0.50.0 without the columns its releases added late.
    let older = made_before(V0_50);
    for (table, column) in [
        ("chat_members", "from_ms"),
        ("chat_messages", "imported_id"),
        ("cloud_workspaces", "settings_applied"),
        ("cloud_workspaces", "vault_secrets"),
        ("cloud_workspaces", "rebuild"),
        ("cloud_workspaces", "rebuild_error"),
        ("cloud_workspaces", "rebuilds"),
        ("cloud_workspaces", "setup_step"),
        ("cloud_tabs", "relaunch"),
    ] {
        older.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN {column}")).unwrap();
    }
    // Production from before a probe it has since run.
    let unstepped = made_before(PRODUCTION);
    unstepped.execute_batch("ALTER TABLE cloud_workspaces DROP COLUMN setup_step").unwrap();
    // Cloud jobs keyed on their run, from before Cloud workspaces (M114).
    let keyed_on_run = made_before(PRODUCTION);
    keyed_on_run
        .execute_batch(
            "DROP TABLE cloud_jobs;
             CREATE TABLE cloud_jobs (run TEXT PRIMARY KEY, body TEXT NOT NULL, cancel INTEGER NOT NULL DEFAULT 0,
                 lease TEXT, lease_until INTEGER NOT NULL DEFAULT 0);
             INSERT INTO cloud_jobs (run, body) VALUES ('run_1', '{}');",
        )
        .unwrap();
    for (name, mut c) in
        [("v0.50.0", v0_50), ("older", older), ("unstepped", unstepped), ("keyed on run", keyed_on_run)]
    {
        let report = run(&mut c).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(report, Report { adopted: 2, upgraded: true, ran: (3..=latest()).collect() }, "{name}");
        let have = Shape::of(&c, &names).unwrap();
        assert_eq!(have.diff(&want), Vec::<String>::new(), "{name}");
        assert_eq!(count(&c, "SELECT count(*) FROM accounts"), 1, "{name}: the hub's table");
        assert_eq!(count(&c, "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH 'kettle'"), 1, "{name}: search");
        assert_eq!(count(&c, "SELECT count(*) FROM sqlite_master WHERE name LIKE 'cloud%'"), 0, "{name}: dropped");
        assert_eq!(run(&mut c).unwrap(), Report::default(), "{name}: once");
    }
}

/// Tables that match no migration even after the old schema code: the
/// runner says what's wrong and changes nothing.
#[test]
fn a_half_matching_schema_is_refused() {
    // A column no probe ever added.
    let missing_column = made_before(PRODUCTION);
    missing_column.execute_batch("ALTER TABLE billing DROP COLUMN seat_item").unwrap();
    let some_tables = Connection::open_in_memory().unwrap();
    some_tables.execute_batch("CREATE TABLE billing (owner TEXT PRIMARY KEY, status TEXT NOT NULL)").unwrap();
    let different_index = made_before(MAIN);
    different_index
        .execute_batch("DROP INDEX grants_scope; CREATE INDEX grants_scope ON grants (scope_id, scope_kind)")
        .unwrap();
    for (mut c, says) in [
        (missing_column, vec!["0002_hub_feed.sql", "billing: column seat_item is missing (TEXT)"]),
        (some_tables, vec!["billing: column customer is missing"]),
        (different_index, vec!["0002_hub_feed.sql", "grants: index grants_scope is `CREATE INDEX grants_scope ON"]),
    ] {
        let before = shape(&c);
        let e = format!("{:#}", run(&mut c).unwrap_err());
        assert!(e.contains("even after the schema code from before migrations"), "{e}");
        for s in says {
            assert!(e.contains(s), "{s:?} in:\n{e}");
        }
        assert_eq!(shape(&c), before, "nothing changed");
        let made: bool =
            c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1)", [TABLE], |r| r.get(0)).unwrap();
        assert!(!made, "not even the runner's table");
    }
}

/// An older build (a rollback) starts on a database past it when every
/// migration it doesn't know is additive, and leaves them be; one from
/// before the last migration that isn't additive refuses.
#[test]
fn an_older_build_starts_past_additive_migrations() {
    let mut c = fresh();
    let floor = SQLITE.iter().rev().find(|m| !m.additive).unwrap().version;
    for known in 1..latest() {
        let report = run_set(&mut c, &SQLITE[..known as usize], u32::MAX);
        if known < floor {
            assert!(report.is_err(), "a build that knows up to {known}, before {floor}");
        } else {
            assert_eq!(report.unwrap(), Report::default(), "a build that knows up to {known}");
        }
    }
    assert_eq!(recorded(&c).len(), latest() as usize);
    // A later additive migration, from a build newer than this one.
    c.execute(
        "INSERT INTO platform_migrations (version, name, checksum, applied_at, additive) VALUES (?1, 'later', '', 0, 1)",
        [latest() + 1],
    )
    .unwrap();
    assert_eq!(run(&mut c).unwrap(), Report::default());
}

/// ... and refuses when one it doesn't know isn't additive.
#[test]
fn an_older_build_refuses_a_migration_that_isnt_additive() {
    let mut c = fresh();
    c.execute("UPDATE platform_migrations SET additive = 0 WHERE version = 3", []).unwrap();
    let e = format!("{:#}", run_set(&mut c, &SQLITE[..1], u32::MAX).unwrap_err());
    assert!(e.contains("knows only up to 1, and migration 3 isn't additive"), "{e}");
    assert!(run(&mut c).is_ok(), "a build that knows it");
}

/// A database at a version this build doesn't know, which isn't additive,
/// or whose record of a migration doesn't match the file: refused.
#[test]
fn a_database_ahead_or_edited_is_refused() {
    let mut ahead = fresh();
    ahead
        .execute(
            "INSERT INTO platform_migrations (version, name, checksum, applied_at) VALUES (?1, 'later', '', 0)",
            [latest() + 1],
        )
        .unwrap();
    let e = format!("{:#}", run(&mut ahead).unwrap_err());
    let says = format!(
        "at migration {0} and this build knows only up to {1}, and migration {0} isn't",
        latest() + 1,
        latest()
    );
    assert!(e.contains(&says), "{e}");

    let mut edited = fresh();
    edited.execute("UPDATE platform_migrations SET checksum = 'x' WHERE version = 1", []).unwrap();
    let e = format!("{:#}", run(&mut edited).unwrap_err());
    assert!(e.contains("0001_baseline.sql has changed"), "{e}");
}

/// A version is a file, `NNNN_name.sql`, numbered from 1 with no gaps, and
/// every file is listed; Postgres's set (M143) has the same versions and
/// names as SQLite's.
#[test]
fn migrations_are_their_numbered_files() {
    let files = |d: Dialect| -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(d.dir());
        let mut out: Vec<String> = std::fs::read_dir(dir)
            .map(|r| r.map(|e| e.unwrap().file_name().into_string().unwrap()).collect())
            .unwrap_or_default();
        out.retain(|f| f.ends_with(".sql"));
        out.sort();
        out
    };
    for d in [Dialect::Sqlite, Dialect::Postgres] {
        let set = d.migrations();
        assert_eq!(set.iter().map(|m| m.file()).collect::<Vec<_>>(), files(d), "{d:?}");
        assert!(set.iter().enumerate().all(|(i, m)| m.version as usize == i + 1), "{d:?}: numbered from 1");
    }
    for (pg, lite) in Dialect::Postgres.migrations().iter().zip(Dialect::Sqlite.migrations()) {
        assert_eq!((pg.version, pg.name), (lite.version, lite.name));
    }
}

#[test]
fn normalising_ignores_comments_quotes_and_spacing() {
    assert_eq!(
        super::normalise(
            "CREATE TABLE \"t\" (\n    a TEXT NOT NULL DEFAULT 'x  -- y', -- a note\n    CHECK (a IN ('p', 'q'))\n);"
        ),
        "CREATE TABLE t(a TEXT NOT NULL DEFAULT 'x  -- y',CHECK(a IN('p','q')))"
    );
    assert_eq!(
        super::normalise("CREATE INDEX i ON t(a)\n    WHERE b IS NULL"),
        "CREATE INDEX i ON t(a) WHERE b IS NULL"
    );
    assert_eq!(super::checks("CREATE TABLE t(a,CHECK((a)!=(b)),CHECK(c))"), ["CHECK((a)!=(b))", "CHECK(c)"]);
}

/// Postgres's tables are SQLite's (M143): the same tables, each with the
/// same columns, each as nullable, but for the columns only Postgres has
/// (chat's and issues' `search`). Runs when
/// `ARUGULA_TEST_POSTGRES_URL` names a Postgres.
#[test]
fn postgres_has_sqlites_tables() {
    use crate::db::{
        Db, Sql as _,
        sql::{Value, params},
    };
    let pg = Db::test();
    if pg.dialect() != Dialect::Postgres {
        return;
    }
    let only_postgres = ["chat_messages.search", "issues.search"];
    let columns = |db: &Db, sql: &str| -> Vec<(String, bool)> {
        db.c()
            .query_map(sql, params![], |r| {
                let column = format!("{}.{}", r.get::<String>(0)?, r.get::<String>(1)?);
                let null = r.get::<Value>(2)? == Value::Int(0);
                Ok((column, null))
            })
            .unwrap()
            .into_iter()
            .filter(|(c, _)| !only_postgres.contains(&c.as_str()))
            .collect()
    };
    let mut lite = columns(
        &Db::memory(),
        "SELECT m.name, p.name, CASE WHEN p.\"notnull\" = 1 OR p.pk > 0 THEN 1 ELSE 0 END
         FROM sqlite_master m, pragma_table_info(m.name) p
         WHERE m.type = 'table' AND m.name NOT LIKE 'chat_fts%' AND m.name NOT LIKE 'issues_fts%'",
    );
    let mut post = columns(
        &pg,
        "SELECT table_name::text, column_name::text, CASE WHEN is_nullable = 'NO' THEN 1 ELSE 0 END
         FROM information_schema.columns WHERE table_schema = current_schema()",
    );
    lite.sort();
    post.sort();
    let only =
        |a: &[(String, bool)], b: &[(String, bool)]| a.iter().filter(|c| !b.contains(c)).cloned().collect::<Vec<_>>();
    assert_eq!((only(&post, &lite), only(&lite, &post)), (vec![], vec![]), "(only Postgres's, only SQLite's)");
}

/// Channel members (0048, #118): every channel there was becomes public,
/// with every member of its team's latest roster in it, so nobody loses a
/// room; #general is the one everyone who joins later joins too; DMs and
/// other teams' rosters are left alone.
#[test]
fn existing_channels_become_public_with_the_whole_team_in() {
    let mut c = Connection::open_in_memory().unwrap();
    run_to(&mut c, 46).unwrap();
    c.execute_batch(
        "INSERT INTO team_members (team_id, account_id, role, name, roster_version)
             VALUES ('t', 'a', 'owner', 'a', 1), ('t', 'b', 'editor', 'b', 1), ('u', 'z', 'owner', 'z', 1);
         INSERT INTO chat_rooms (id, scope, kind, name, created_by, created) VALUES
             ('ch_general', 'team:t', 'channel', 'general', 'a', 1),
             ('ch_random', 'team:t', 'channel', 'random', 'b', 2),
             ('dm_ab', 'team:t', 'dm', '', 'a', 3),
             ('ch_u', 'team:u', 'channel', 'ops', 'z', 4);
         INSERT INTO chat_members (room, account) VALUES ('dm_ab', 'a'), ('dm_ab', 'b');",
    )
    .unwrap();
    run(&mut c).unwrap();
    let members = |room: &str| -> Vec<String> {
        let mut q = c.prepare("SELECT account FROM chat_members WHERE room = ?1 ORDER BY account").unwrap();
        q.query_map([room], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(members("ch_general"), ["a", "b"]);
    assert_eq!(members("ch_random"), ["a", "b"]);
    assert_eq!(members("ch_u"), ["z"]);
    assert_eq!(members("dm_ab"), ["a", "b"], "a DM keeps its two");
    assert_eq!(count(&c, "SELECT count(*) FROM chat_rooms WHERE private != 0"), 0, "all public");
    assert_eq!(count(&c, "SELECT count(*) FROM chat_rooms WHERE auto_join != 0 AND name = 'general'"), 1);
    assert_eq!(count(&c, "SELECT count(*) FROM chat_rooms WHERE auto_join != 0"), 1);
    assert_eq!(count(&c, "SELECT count(*) FROM chat_rooms WHERE dm_key IS NOT NULL"), 0, "old DMs have no key");
}
