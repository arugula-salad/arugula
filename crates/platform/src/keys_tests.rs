//! Per-project envelope keys (M209), on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, on Postgres.

use super::*;
use crate::db::sql::Value;

const OLD: &str = "0101010101010101010101010101010101010101010101010101010101010101"; // gitleaks:allow
const NEW: &str = "0202020202020202020202020202020202020202020202020202020202020202"; // gitleaks:allow

fn master(hex: &str) -> SecretCipher {
    SecretCipher::new(hex).unwrap()
}

/// Personal projects, as the platform makes them.
fn projects(db: &Db, ids: &[&str]) {
    for id in ids {
        db.c()
            .execute(
                "INSERT INTO projects (id, account_id, name, is_default, created_at) VALUES (?1, ?1, 'Personal', TRUE, 1)",
                params![id],
            )
            .unwrap();
    }
}

/// A hosted workspace in `project`.
fn workspace(db: &Db, id: &str, project: &str) {
    db.c()
        .execute(
            "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, created_at, updated_at)
             VALUES (?1, ?2, 'dev', '{}', 'h', 'ready', 1, 1)",
            params![id, project],
        )
        .unwrap();
}

/// A hosted workspace's secret, sealed as it was before M209: with the
/// master key itself.
fn sealed_by_master(db: &Db, master: &SecretCipher, workspace: &str, name: &str, value: &str) {
    db.c()
        .execute(
            "INSERT INTO hosted_workspace_secrets (workspace_id, name, store, sealed, updated_at)
             VALUES (?1, ?2, 'sealed', ?3, 1)",
            params![workspace, name, master.seal(workspace, name, value).unwrap()],
        )
        .unwrap();
}

/// Two projects' hosted workspaces' secrets and a project's own inference
/// key, sealed as they were before M209: with the master key itself.
fn before_m209(db: &Db, master: &SecretCipher) {
    projects(db, &["prj_a", "prj_b"]);
    workspace(db, "hw_a", "prj_a");
    workspace(db, "hw_1", "prj_b");
    sealed_by_master(db, master, "hw_a", "GITHUB_TOKEN", "ghp_alice");
    sealed_by_master(db, master, "hw_1", "NPM_TOKEN", "npm_bob");
    sealed_by_master(db, master, "hw_1", "DB_URL", "postgres://b");
    let c = db.c();
    c.execute(
        "INSERT INTO hosted_workspace_secrets (workspace_id, name, store, sealed, updated_at)
         VALUES ('hw_1', 'ANTHROPIC_KEY', 'broker', NULL, 1)",
        params![],
    )
    .unwrap();
    c.execute(
        "INSERT INTO project_inference_keys (project_id, provider, sealed, set_by, updated_at)
         VALUES ('prj_a', 'anthropic', ?1, 'alice', 1)",
        params![master.seal("prj_a", "inference:anthropic", "sk-ant-a").unwrap()],
    )
    .unwrap();
}

/// Every stored secret, opened as the platform opens it.
fn values(db: &Db, master: &SecretCipher) -> Vec<String> {
    let mut out = Vec::new();
    for (project, workspace, name) in
        [("prj_a", "hw_a", "GITHUB_TOKEN"), ("prj_b", "hw_1", "NPM_TOKEN"), ("prj_b", "hw_1", "DB_URL")]
    {
        let sealed = db.hosted_secret_sealed(workspace, name).unwrap().unwrap();
        out.push(open_hosted(&db.c(), master, project, workspace, name, &sealed).unwrap());
    }
    for (provider, sealed) in db.sealed_inference_keys("prj_a").unwrap() {
        out.push(open_inference(&db.c(), master, "prj_a", &provider, &sealed).unwrap());
    }
    out
}

const VALUES: [&str; 4] = ["ghp_alice", "npm_bob", "postgres://b", "sk-ant-a"];

fn key_versions(db: &Db) -> Vec<i64> {
    db.c()
        .query_map(
            "SELECT key_version FROM hosted_workspace_secrets WHERE sealed IS NOT NULL UNION ALL
             SELECT key_version FROM project_inference_keys",
            params![],
            |r| r.get(0),
        )
        .unwrap()
}

fn sealed_bytes(db: &Db) -> Vec<Vec<u8>> {
    db.c()
        .query_map(
            "SELECT sealed FROM hosted_workspace_secrets WHERE sealed IS NOT NULL UNION ALL
             SELECT sealed FROM project_inference_keys",
            params![],
            |r| r.get(0),
        )
        .map(|mut v: Vec<Vec<u8>>| {
            v.sort();
            v
        })
        .unwrap()
}

/// A project's key seals only its own secrets: another project's key
/// doesn't open them, under any name, and neither does the right key
/// under another project's name.
#[test]
fn a_projects_key_cant_open_another_projects_secrets() {
    let db = Db::test();
    let m = master(OLD);
    projects(&db, &["prj_a", "prj_b"]);
    seal(&db, &m, "prj_a", "environment/env_1/FLY_API_TOKEN", "fly-a").unwrap();
    seal(&db, &m, "prj_b", "environment/env_2/FLY_API_TOKEN", "fly-b").unwrap();
    assert_eq!(open(&db, &m, "prj_a", "environment/env_1/FLY_API_TOKEN").unwrap().as_deref(), Some("fly-a"));
    assert_eq!(open(&db, &m, "prj_b", "environment/env_2/FLY_API_TOKEN").unwrap().as_deref(), Some("fly-b"));
    assert_eq!(open(&db, &m, "prj_b", "environment/env_1/FLY_API_TOKEN").unwrap(), None);

    let a = seal_value(&db.c(), &m, "prj_a", "X", "only-a").unwrap();
    let c = db.c();
    let (key_a, key_b) = (data_key(&c, &m, "prj_a", 1).unwrap(), data_key(&c, &m, "prj_b", 1).unwrap());
    assert_ne!(key_a, key_b, "each project has a key of its own");
    // Project b's key, with a's binding: no.
    assert!(open_with(&cipher(&key_b), &aad("prj_a", "X").unwrap(), &a.bytes).is_err());
    // Through the API, under b: no.
    assert!(open_value(&c, &m, "prj_b", "X", &a).is_err());
    // A's key, but bound to another name or project: no.
    assert!(open_value(&c, &m, "prj_a", "Y", &a).is_err());
    assert!(open_with(&cipher(&key_a), &aad("prj_b", "X").unwrap(), &a.bytes).is_err());
    assert_eq!(open_value(&c, &m, "prj_a", "X", &a).unwrap(), "only-a");
    // Moving a's row to b is no way in either.
    c.execute("UPDATE project_secrets SET project_id = 'prj_b', name = 'moved' WHERE project_id = 'prj_a'", params![])
        .unwrap();
    drop(c);
    assert!(open(&db, &m, "prj_b", "moved").is_err());
    // No value is in the clear anywhere.
    assert!(db.mentions("fly-a").is_empty() && db.mentions("only-a").is_empty());
    assert!(remove(&db, "prj_b", "environment/env_2/FLY_API_TOKEN").unwrap());
    assert!(!remove(&db, "prj_b", "environment/env_2/FLY_API_TOKEN").unwrap());

    // Another table's sealed value, put in project_secrets under its own
    // name, doesn't open as one: the table is bound too.
    let hosted = seal_value(&db.c(), &m, "prj_a", &hosted_name("hw_a", "T"), "hosted-a").unwrap();
    db.c()
        .execute(
            "INSERT INTO project_secrets (project_id, name, key_version, sealed, updated_at) VALUES ('prj_a', ?1, ?2, ?3, 1)",
            params![hosted_name("hw_a", "T"), hosted.key_version, hosted.bytes],
        )
        .unwrap();
    assert!(open(&db, &m, "prj_a", &hosted_name("hw_a", "T")).is_err());
}

/// Making a project's key twice keeps the first; a new key seals with
/// the project's latest version.
#[test]
fn a_project_has_one_key_until_it_is_rotated() {
    let db = Db::test();
    let m = master(OLD);
    projects(&db, &["prj_a"]);
    let c = db.c();
    let first = current_key(&c, &m, "prj_a").unwrap();
    assert_eq!(current_key(&c, &m, "prj_a").unwrap(), first);
    assert_eq!(first.0, 1);
    let master_id: String =
        c.query_row("SELECT master_id FROM project_keys WHERE project_id = 'prj_a'", params![], |r| r.get(0)).unwrap();
    assert_eq!(master_id, m.id());
    assert!(data_key(&c, &master(NEW), "prj_a", 1).is_err(), "another master key doesn't open it");
    assert!(current_key(&c, &m, "prj_gone").is_err(), "a project that isn't there gets none");
    assert_eq!(wrapped_by(&c, m.id()).unwrap(), 1);
}

/// Re-sealing stops halfway and starts again without losing a secret:
/// every row opens at every step, a second run finishes the job, and a
/// third does nothing.
#[test]
fn reseal_is_idempotent_and_survives_stopping_halfway() {
    let db = Db::test();
    let m = master(OLD);
    before_m209(&db, &m);
    assert_eq!(values(&db, &m), VALUES);
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 4, by_master: 4, ..Default::default() });

    // Stopped after one row, as a crash between rows would.
    let first = reseal(&db, &m, 1).unwrap();
    assert_eq!(first.resealed, 1);
    assert_eq!(first.left.len(), 3);
    assert_eq!(values(&db, &m), VALUES, "both kinds open");
    assert_eq!(check(&db.c(), &m).unwrap().by_master, 3);

    // A crash inside a row: the project key got made, the row's write
    // never committed.
    {
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        let s = seal_value(&tx, &m, "prj_b", &hosted_name("hw_1", "NPM_TOKEN"), "npm_bob").unwrap();
        tx.execute(
            "UPDATE hosted_workspace_secrets SET sealed = ?1, key_version = ?2
             WHERE workspace_id = 'hw_1' AND name = 'NPM_TOKEN'",
            params![s.bytes, s.key_version],
        )
        .unwrap();
        // Dropped uncommitted.
    }
    assert_eq!(values(&db, &m), VALUES);

    let rest = reseal(&db, &m, usize::MAX).unwrap();
    assert_eq!(rest, Resealed { resealed: 3, left: vec![] });
    assert_eq!(key_versions(&db), [1, 1, 1, 1]);
    assert_eq!(values(&db, &m), VALUES);
    let after = sealed_bytes(&db);
    assert_eq!(reseal(&db, &m, usize::MAX).unwrap(), Resealed::default(), "nothing left to do");
    assert_eq!(sealed_bytes(&db), after, "and nothing rewritten");
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 4, ..Default::default() });
    // A broker secret has no value here, and stays that way.
    assert_eq!(db.hosted_secret_sealed("hw_1", "ANTHROPIC_KEY").unwrap(), None);
}

/// A row the master key can't open, or that has no project, is left as it
/// was, and the rest are re-sealed; one that changed while it was being
/// re-sealed isn't overwritten.
#[test]
fn reseal_leaves_what_it_cant_do() {
    let db = Db::test();
    let m = master(OLD);
    before_m209(&db, &m);
    projects(&db, &["prj_c"]);
    workspace(&db, "hw_c", "prj_c");
    sealed_by_master(&db, &master(NEW), "hw_c", "T", "other-key");
    let r = reseal(&db, &m, usize::MAX).unwrap();
    assert_eq!(r.resealed, 4);
    assert_eq!(r.left, ["hosted_workspace_secrets hw_c T"]);
    let carol = db.hosted_secret_sealed("hw_c", "T").unwrap().unwrap();
    assert_eq!(carol.key_version, MASTER);
    assert_eq!(master(NEW).open("hw_c", "T", &carol.bytes).unwrap(), "other-key", "never lost");

    // Changed under it: the newer value wins.
    let store = &STORES[0];
    let row = store.read(&db.c()).unwrap().into_iter().find(|r| r.a == "hw_c").unwrap();
    let newer = Sealed { key_version: MASTER, bytes: master(NEW).seal("hw_c", "T", "newer").unwrap() };
    db.set_hosted_secret("hw_c", "T", Some(&newer), 2).unwrap();
    assert!(!reseal_row(&db, &master(NEW), store, &row).unwrap());
    assert_eq!(db.hosted_secret_sealed("hw_c", "T").unwrap().unwrap(), newer);
}

/// Rotating the master key re-wraps every project key, leaves every
/// secret's bytes as they were, and every secret opens with the new key
/// alone. Run again, it does nothing; rows the old key sealed itself are
/// re-sealed first.
#[test]
fn rotation_rewraps_project_keys_and_every_secret_still_opens() {
    let db = Db::test();
    let old = master(OLD);
    before_m209(&db, &old);
    // Two of four are still sealed the old way when the rotation starts.
    reseal(&db, &old, 2).unwrap();
    seal(&db, &old, "prj_a", &environment_name("env_1", "FLY_API_TOKEN"), "fly-a").unwrap();
    assert!(rotate(&db, &master(NEW)).is_err(), "it needs the old key too");

    let both = master(NEW).with_previous(Some(OLD)).unwrap();
    let r = rotate(&db, &both).unwrap();
    // Re-sealing first re-wrapped the keys it sealed with; the rest
    // re-wrapped them all.
    assert_eq!((r.resealed, r.rewrapped + r.already, r.failed.len()), (2, 2, 0), "{r:?}");
    let new = master(NEW);
    assert_eq!(values(&db, &new), VALUES);
    assert_eq!(
        open(&db, &new, "prj_a", &environment_name("env_1", "FLY_API_TOKEN")).unwrap().as_deref(),
        Some("fly-a")
    );
    assert_eq!(check(&db.c(), &new).unwrap(), Checked { opened: 5, ..Default::default() });
    assert_eq!(check_with(&db.c(), NEW, Some(OLD)).unwrap().previous_wraps, Some(0), "the old key can go");
    let failed = check(&db.c(), &old).unwrap().failed;
    assert_eq!(failed.len(), 7, "the old key opens nothing now: {failed:?}");
    let master_ids: Vec<(String, Option<i64>)> = db
        .c()
        .query_map("SELECT master_id, rotated_at FROM project_keys", params![], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(master_ids.iter().all(|(id, at)| id == new.id() && at.is_some()));

    // Secrets untouched by a rotation.
    let before = sealed_bytes(&db);
    let third = master("03".repeat(32).as_str()).with_previous(Some(NEW)).unwrap();
    assert_eq!(check_with(&db.c(), NEW, Some(NEW)).unwrap().previous_wraps, Some(2));
    assert_eq!(rotate(&db, &third).unwrap(), Rotated { rewrapped: 2, ..Default::default() });
    assert_eq!(sealed_bytes(&db), before);
    assert_eq!(rotate(&db, &third).unwrap(), Rotated { already: 2, ..Default::default() }, "again: nothing");
    assert_eq!(values(&db, &third.without_previous()), VALUES);
}

/// A rotation with a project key that neither key wrapped re-wraps the
/// rest, and says, row by row, what it couldn't do.
#[test]
fn rotation_names_keys_it_cant_rewrap() {
    let db = Db::test();
    projects(&db, &["prj_a", "prj_y", "prj_z"]);
    seal(&db, &master(OLD), "prj_a", "A", "a").unwrap();
    seal(&db, &master(OLD), "prj_y", "Y", "y").unwrap();
    seal(&db, &master("04".repeat(32).as_str()), "prj_z", "Z", "z").unwrap();
    let r = rotate(&db, &master(NEW).with_previous(Some(OLD)).unwrap()).unwrap();
    assert_eq!(r.rewrapped, 2, "{r:?}");
    assert!(r.failed[0].contains("prj_z version 1") && r.failed[0].contains("neither key"), "{r:?}");
    assert!(r.failed.iter().any(|f| f.contains("prj_z Z")), "and it doesn't open after: {r:?}");
    assert_eq!(open(&db, &master(NEW), "prj_a", "A").unwrap().as_deref(), Some("a"));
    assert_eq!(open(&db, &master(NEW), "prj_y", "Y").unwrap().as_deref(), Some("y"));
}

/// A build from before M209 writing over a re-sealed row seals it with the
/// master key and leaves its version: it still opens, and the next re-seal
/// seals it with its project's key again.
#[test]
fn a_row_an_old_build_wrote_over_still_opens_and_is_resealed() {
    let db = Db::test();
    let m = master(OLD);
    before_m209(&db, &m);
    reseal(&db, &m, usize::MAX).unwrap();
    assert_eq!(key_versions(&db), [1, 1, 1, 1]);
    // The upserts as main had them before M209, which don't know key_version.
    let c = db.c();
    for (workspace, name, value) in [("hw_a", "GITHUB_TOKEN", "ghp_new"), ("hw_1", "DB_URL", "postgres://new")] {
        c.execute(
            "INSERT INTO hosted_workspace_secrets (workspace_id, name, store, sealed, updated_at) VALUES (?1, ?2, 'sealed', ?3, 2)
             ON CONFLICT (workspace_id, name) DO UPDATE SET store = excluded.store, sealed = excluded.sealed,
                 updated_at = excluded.updated_at",
            params![workspace, name, m.seal(workspace, name, value).unwrap()],
        )
        .unwrap();
    }
    c.execute(
        "INSERT INTO project_inference_keys (project_id, provider, sealed, set_by, updated_at) VALUES ('prj_a', 'anthropic', ?1, 'alice', 2)
         ON CONFLICT (project_id, provider) DO UPDATE SET sealed = excluded.sealed, set_by = excluded.set_by,
             updated_at = excluded.updated_at",
        params![m.seal("prj_a", "inference:anthropic", "sk-ant-new").unwrap()],
    )
    .unwrap();
    drop(c);
    assert_eq!(key_versions(&db), [1, 1, 1, 1], "they kept their version");
    let now = ["ghp_new", "npm_bob", "postgres://new", "sk-ant-new"];
    assert_eq!(values(&db, &m), now, "and still open");
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 4, by_master: 3, ..Default::default() });

    assert_eq!(reseal(&db, &m, usize::MAX).unwrap(), Resealed { resealed: 3, left: vec![] });
    assert_eq!(values(&db, &m), now);
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 4, ..Default::default() });
    assert_eq!(reseal(&db, &m, usize::MAX).unwrap(), Resealed::default());
}

/// Sealing once the master key's rotated re-wraps the project's key first,
/// so nothing sealed after needs the old key, even before `rotate-key`.
#[test]
fn sealing_after_the_switch_rewraps_the_projects_key_first() {
    let db = Db::test();
    projects(&db, &["prj_a"]);
    seal(&db, &master(OLD), "prj_a", "A", "a").unwrap();
    let switched = master(NEW).with_previous(Some(OLD)).unwrap();
    assert_eq!(wrapped_by(&db.c(), master(OLD).id()).unwrap(), 1);
    seal(&db, &switched, "prj_a", "B", "b").unwrap();
    assert_eq!(wrapped_by(&db.c(), master(OLD).id()).unwrap(), 0, "re-wrapped before it sealed");
    for (name, value) in [("A", "a"), ("B", "b")] {
        assert_eq!(open(&db, &master(NEW), "prj_a", name).unwrap().as_deref(), Some(value));
    }
}

/// The rotation's two steps across two machines: once every machine can
/// open with the next key, the one that switched first wraps with it and
/// the one that hasn't yet still opens what it wraps, and the other way.
#[test]
fn two_machines_mid_rotation_open_each_others_keys() {
    let db = Db::test();
    projects(&db, &["prj_a", "prj_b"]);
    let waiting = master(OLD).with_next(Some(NEW)).unwrap();
    let switched = master(NEW).with_previous(Some(OLD)).unwrap();
    seal(&db, &switched, "prj_a", "A", "by-switched").unwrap();
    seal(&db, &waiting, "prj_b", "B", "by-waiting").unwrap();
    assert_eq!(open(&db, &waiting, "prj_a", "A").unwrap().as_deref(), Some("by-switched"));
    assert_eq!(open(&db, &switched, "prj_b", "B").unwrap().as_deref(), Some("by-waiting"));
    // The machine that switched seals to b too: it re-wraps b's key, which
    // the waiting machine still opens.
    seal(&db, &switched, "prj_b", "C", "c").unwrap();
    assert_eq!(open(&db, &waiting, "prj_b", "B").unwrap().as_deref(), Some("by-waiting"));
    assert_eq!(open(&db, &waiting, "prj_b", "C").unwrap().as_deref(), Some("c"));
    // Without the next key, the waiting machine couldn't have.
    assert!(open(&db, &master(OLD), "prj_a", "A").is_err());
}

/// A project that's gone gets no key: its rows aren't re-sealed, and the
/// check doesn't count them.
#[test]
fn a_gone_projects_rows_are_left_alone() {
    let db = Db::test();
    let m = master(OLD);
    before_m209(&db, &m);
    db.c().execute("DELETE FROM projects WHERE id = 'prj_b'", params![]).unwrap();
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 2, by_master: 2, orphaned: 2, ..Default::default() });
    let r = reseal(&db, &m, usize::MAX).unwrap();
    assert_eq!(r.resealed, 2);
    assert_eq!(r.left, ["hosted_workspace_secrets hw_1 DB_URL", "hosted_workspace_secrets hw_1 NPM_TOKEN"]);
    assert!(seal_value(&db.c(), &m, "prj_b", "X", "x").is_err());
    let keys: Vec<String> = db.c().query_map("SELECT project_id FROM project_keys", params![], |r| r.get(0)).unwrap();
    assert_eq!(keys, ["prj_a"]);
    let r = rotate(&db, &master(NEW).with_previous(Some(OLD)).unwrap()).unwrap();
    assert!(r.failed.iter().all(|f| f.contains("still sealed with the master key itself")), "{r:?}");
    assert_eq!(r.rewrapped, 1);
}

/// The compile-time reader of `secrets.toml` finds a listed secret under
/// its own environment only.
#[test]
fn secrets_toml_is_read_at_compile_time() {
    let toml = br#"
[[environment]]
name = "arugula-release"
what = "x, \"quoted\""
secrets = ["FLY_API_TOKEN", "GH_TOKEN"]

[[environment]]
name = "other"
what = "y"
secrets = []
"#;
    assert!(listed_in(toml, b"arugula-release", b"FLY_API_TOKEN"));
    assert!(listed_in(toml, b"arugula-release", b"GH_TOKEN"));
    assert!(!listed_in(toml, b"arugula-release", b"FLY_API"));
    assert!(!listed_in(toml, b"arugula", b"FLY_API_TOKEN"));
    assert!(!listed_in(toml, b"other", b"FLY_API_TOKEN"));
    assert!(!is_listed("arugula-release", "NOT_LISTED_ANYWHERE"));
}

/// Deleting an account takes its personal project's keys and secrets.
#[test]
fn a_deleted_accounts_project_keys_go() {
    let db = Db::test();
    let m = master(OLD);
    projects(&db, &["prj_a", "prj_b"]);
    seal(&db, &m, "prj_a", "A", "a").unwrap();
    seal(&db, &m, "prj_b", "B", "b").unwrap();
    let d = crate::Deleted { account: "prj_a".into(), machines: vec![], disbanded: vec![] };
    crate::db::projects::erase_account(&db.c(), &d).unwrap();
    let left: Vec<Value> = db
        .c()
        .query_map(
            "SELECT project_id FROM project_keys UNION ALL SELECT project_id FROM project_secrets",
            params![],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, [Value::Text("prj_b".into()), Value::Text("prj_b".into())]);
}
