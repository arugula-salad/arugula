//! Per-project envelope keys (M209), on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, on Postgres.

use super::*;
use crate::db::sql::Value;

const OLD: &str = "0101010101010101010101010101010101010101010101010101010101010101"; // gitleaks:allow
const NEW: &str = "0202020202020202020202020202020202020202020202020202020202020202"; // gitleaks:allow

fn master(hex: &str) -> SecretCipher {
    SecretCipher::new(hex).unwrap()
}

/// Two projects' cloud secrets, a hosted workspace's and a project's own
/// inference key, sealed as they were before M209: with the master key
/// itself.
fn before_m209(db: &Db, master: &SecretCipher) {
    let c = db.c();
    for (account, project, name, value) in
        [("alice", "prj_a", "GITHUB_TOKEN", "ghp_alice"), ("bob", "prj_b", "NPM_TOKEN", "npm_bob")]
    {
        c.execute(
            "INSERT INTO cloud_secrets (account, name, sealed, project_id) VALUES (?1, ?2, ?3, ?4)",
            params![account, name, master.seal(account, name, value).unwrap(), project],
        )
        .unwrap();
    }
    c.execute(
        "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, created_at, updated_at)
         VALUES ('hw_1', 'prj_b', 'dev', '{}', 'h', 'ready', 1, 1)",
        params![],
    )
    .unwrap();
    c.execute(
        "INSERT INTO hosted_workspace_secrets (workspace_id, name, store, sealed, updated_at)
         VALUES ('hw_1', 'DB_URL', 'sealed', ?1, 1), ('hw_1', 'ANTHROPIC_KEY', 'broker', NULL, 1)",
        params![master.seal("hw_1", "DB_URL", "postgres://b").unwrap()],
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
    for (account, name) in [("alice", "GITHUB_TOKEN"), ("bob", "NPM_TOKEN")] {
        let (project, sealed) = db.cloud_secret(account, name).unwrap().unwrap();
        out.push(open_cloud(&db.c(), master, account, name, project.as_deref(), &sealed).unwrap());
    }
    let sealed = db.hosted_secret_sealed("hw_1", "DB_URL").unwrap().unwrap();
    out.push(open_hosted(&db.c(), master, "prj_b", "hw_1", "DB_URL", &sealed).unwrap());
    for (provider, sealed) in db.sealed_inference_keys("prj_a").unwrap() {
        out.push(open_inference(&db.c(), master, "prj_a", &provider, &sealed).unwrap());
    }
    out
}

const VALUES: [&str; 4] = ["ghp_alice", "npm_bob", "postgres://b", "sk-ant-a"];

fn key_versions(db: &Db) -> Vec<i64> {
    db.c()
        .query_map(
            "SELECT key_version FROM cloud_secrets UNION ALL
             SELECT key_version FROM hosted_workspace_secrets WHERE sealed IS NOT NULL UNION ALL
             SELECT key_version FROM project_inference_keys",
            params![],
            |r| r.get(0),
        )
        .unwrap()
}

fn sealed_bytes(db: &Db) -> Vec<Vec<u8>> {
    db.c()
        .query_map(
            "SELECT sealed FROM cloud_secrets UNION ALL
             SELECT sealed FROM hosted_workspace_secrets WHERE sealed IS NOT NULL UNION ALL
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
}

/// Making a project's key twice keeps the first; a new key seals with
/// the project's latest version.
#[test]
fn a_project_has_one_key_until_it_is_rotated() {
    let db = Db::test();
    let m = master(OLD);
    let c = db.c();
    let first = current_key(&c, &m, "prj_a").unwrap();
    assert_eq!(current_key(&c, &m, "prj_a").unwrap(), first);
    assert_eq!(first.0, 1);
    let master_id: String =
        c.query_row("SELECT master_id FROM project_keys WHERE project_id = 'prj_a'", params![], |r| r.get(0)).unwrap();
    assert_eq!(master_id, m.id());
    assert!(data_key(&c, &master(NEW), "prj_a", 1).is_err(), "another master key doesn't open it");
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
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 4, by_master: 4, failed: vec![] });

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
        let s = seal_value(&tx, &m, "prj_b", &cloud_name("bob", "NPM_TOKEN"), "npm_bob").unwrap();
        tx.execute(
            "UPDATE cloud_secrets SET sealed = ?1, key_version = ?2 WHERE account = 'bob'",
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
    assert_eq!(check(&db.c(), &m).unwrap(), Checked { opened: 4, by_master: 0, failed: vec![] });
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
    let c = db.c();
    c.execute(
        "INSERT INTO cloud_secrets (account, name, sealed, project_id) VALUES ('carol', 'T', ?1, 'prj_c')",
        params![master(NEW).seal("carol", "T", "other-key").unwrap()],
    )
    .unwrap();
    drop(c);
    let r = reseal(&db, &m, usize::MAX).unwrap();
    assert_eq!(r.resealed, 4);
    assert_eq!(r.left, ["cloud_secrets carol T"]);
    let (_, carol) = db.cloud_secret("carol", "T").unwrap().unwrap();
    assert_eq!(carol.key_version, MASTER);
    assert_eq!(master(NEW).open("carol", "T", &carol.bytes).unwrap(), "other-key", "never lost");

    // Changed under it: the newer value wins.
    let store = &STORES[0];
    let row = store.read(&db.c(), true).unwrap().pop().unwrap();
    let newer = Sealed { key_version: MASTER, bytes: master(NEW).seal("carol", "T", "newer").unwrap() };
    db.put_cloud_secret("carol", "prj_c", "T", &newer).unwrap();
    assert!(!reseal_row(&db, &master(NEW), store, &row).unwrap());
    assert_eq!(db.cloud_secret("carol", "T").unwrap().unwrap().1, newer);
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
    assert_eq!(r, Rotated { resealed: 2, rewrapped: 2, already: 0 });
    let new = master(NEW);
    assert_eq!(values(&db, &new), VALUES);
    assert_eq!(
        open(&db, &new, "prj_a", &environment_name("env_1", "FLY_API_TOKEN")).unwrap().as_deref(),
        Some("fly-a")
    );
    assert_eq!(check(&db.c(), &new).unwrap(), Checked { opened: 5, by_master: 0, failed: vec![] });
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
    assert_eq!(rotate(&db, &third).unwrap(), Rotated { resealed: 0, rewrapped: 2, already: 0 });
    assert_eq!(sealed_bytes(&db), before);
    assert_eq!(rotate(&db, &third).unwrap(), Rotated { resealed: 0, rewrapped: 0, already: 2 }, "again: nothing");
    assert_eq!(values(&db, &third.without_previous()), VALUES);
}

/// A rotation with a project key that neither key wrapped re-wraps the
/// rest and says which.
#[test]
fn rotation_names_keys_it_cant_rewrap() {
    let db = Db::test();
    seal(&db, &master(OLD), "prj_a", "A", "a").unwrap();
    seal(&db, &master("04".repeat(32).as_str()), "prj_z", "Z", "z").unwrap();
    let e = rotate(&db, &master(NEW).with_previous(Some(OLD)).unwrap()).unwrap_err();
    assert!(e.to_string().contains("prj_z version 1"), "{e}");
    assert_eq!(open(&db, &master(NEW), "prj_a", "A").unwrap().as_deref(), Some("a"));
}

/// Deleting an account takes its personal project's keys and secrets.
#[test]
fn a_deleted_accounts_project_keys_go() {
    let db = Db::test();
    let m = master(OLD);
    let c = db.c();
    c.execute(
        "INSERT INTO projects (id, account_id, name, is_default, created_at) VALUES ('prj_a', 'alice', 'Personal', TRUE, 1),
             ('prj_b', 'bob', 'Personal', TRUE, 1)",
        params![],
    )
    .unwrap();
    drop(c);
    seal(&db, &m, "prj_a", "A", "a").unwrap();
    seal(&db, &m, "prj_b", "B", "b").unwrap();
    let d = crate::Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
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
