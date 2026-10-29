//! What starts runs, in storage (M214): a trigger's key makes one run and
//! queues one `run` job however often it's asked; a tick is won once; a
//! rule is found only for a linked repo of its project; deleting an
//! account takes what went with its projects. On SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, on Postgres.

// Also the chat crate's tests' (#314, the `testing` feature): its helpers
// are `pub`, and only its own tests use the rest.
#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use base64::Engine as _;
use serde_json::json;

use super::*;
use crate::db::{
    Taken,
    forge::{Installation, LinkedRepo},
    runs::State,
};

/// Project `prj_a` (a team's) with hosted workspace `hw_a`, enrolled as
/// machine `dev_a`, and `hw_b`, not enrolled yet.
pub fn seed(db: &Db) {
    db.c()
        .execute_batch(
            "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES ('prj_a', 't', 'a', FALSE, 1);
             INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, created_by, created_at, updated_at)
                 VALUES ('hw_a', 'prj_a', 'dev', '{}', 'h', 'ready', 'dev_a', 'alice', 1, 1),
                        ('hw_b', 'prj_a', 'new', '{}', 'h', 'creating', NULL, 'alice', 1, 1);",
        )
        .unwrap();
}

pub fn cmd(argv: &[&str]) -> Operation {
    Operation { argv: argv.iter().map(|a| a.to_string()).collect(), ..Default::default() }
}

/// `run` jobs queued for `run`.
pub fn run_jobs(db: &Db, run: &str) -> u64 {
    db.c()
        .query_row("SELECT count(*) FROM platform_jobs WHERE kind = 'run' AND id = ?1", params![run], |r| r.get(0))
        .unwrap()
}

pub fn runs_in(db: &Db, project: &str) -> Vec<Run> {
    db.project_runs(project, 100).unwrap()
}

fn start<'a>(spec: &'a RunSpec, source: &'a Value, key: Option<&'a str>) -> Start<'a> {
    Start {
        project: &spec.project,
        workspace: &spec.workspace,
        op: &spec.op,
        trigger: TriggerKind::Event,
        source,
        started_by: "rru_1",
        started_by_agent: None,
        key,
    }
}

/// An operation is S37's lines, naming the machine; a command and an
/// instruction each check.
#[test]
fn operations_are_s37s() {
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    let op = Operation { directory: Some("/work/app".into()), ..cmd(&["just", "test"]) };
    assert_eq!(
        op.encode("dev_a"),
        format!(
            "arugula op v1\ndaemon dev_a\nkind run\nsession -\npane - -\ncwd {}\nargv {}\nargv {}\n",
            b64("/work/app"),
            b64("just"),
            b64("test")
        )
    );
    let agent = Operation { instruction: Some("triage the failure".into()), ..Default::default() };
    assert_eq!(agent.kind(), "agent");
    assert!(agent.encode("d").ends_with(&format!("cwd -\ntext {}\nagent -\n", b64("triage the failure"))));
    assert!(op.check().is_ok() && agent.check().is_ok());
    for bad in [
        Operation::default(),
        Operation { instruction: Some("x".into()), ..cmd(&["ls"]) },
        Operation { instruction: Some("  ".into()), ..Default::default() },
        Operation { agent: Some("reviewer".into()), ..cmd(&["ls"]) },
        Operation { directory: Some("relative".into()), ..cmd(&["ls"]) },
        Operation { directory: Some("/a\nb".into()), ..cmd(&["ls"]) },
        cmd(&[""]),
        cmd(&["a\0b"]),
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
}

/// A trigger's key makes one run, `pending`, with one `run` job, however
/// often it's asked; a start without a key makes a run each time. The
/// run's operation names the workspace's machine.
#[test]
fn a_key_makes_one_run_and_one_dispatch() {
    let db = Db::test();
    seed(&db);
    let spec = db.add_run_spec("prj_a", "test", "hw_a", &cmd(&["just", "test"]), "alice", 5).unwrap();
    assert_eq!((spec.kind.as_str(), spec.workspace.as_str()), ("run", "hw_a"));
    let source = json!({ "rule": "rru_1" });
    let Started::Made(run) = db.start_run(&start(&spec, &source, Some("rule:rru_1:pr:7:5")), 10).unwrap() else {
        panic!("made")
    };
    assert_eq!((run.state, run.trigger, run.daemon.as_str()), (State::Pending, TriggerKind::Event, "dev_a"));
    assert_eq!((run.workspace.as_deref(), run.started_by.as_str()), (Some("hw_a"), "rru_1"));
    assert_eq!(run.op, spec.op.encode("dev_a"));
    let again = db.start_run(&start(&spec, &source, Some("rule:rru_1:pr:7:5")), 11).unwrap();
    assert_eq!(again, Started::Already(run.clone()));
    assert_eq!(runs_in(&db, "prj_a").len(), 1);
    assert_eq!(run_jobs(&db, &run.id), 1, "dispatched once");
    let job = db.platform_job(&run.id).unwrap().unwrap();
    assert_eq!((job.kind.as_str(), job.payload, job.next_at), ("run", json!({ "run": run.id }), 10));
    assert_eq!(db.run_events(&run.id).unwrap().len(), 1);

    for at in [20, 21] {
        assert!(matches!(db.start_run(&start(&spec, &source, None), at).unwrap(), Started::Made(_)));
    }
    assert_eq!(runs_in(&db, "prj_a").len(), 3, "by hand, a run each time");
}

/// No run on a workspace with no machine yet, another project's, or one
/// that's gone.
#[test]
fn no_run_without_a_machine() {
    let db = Db::test();
    seed(&db);
    let spec = db.add_run_spec("prj_a", "test", "hw_b", &cmd(&["ls"]), "alice", 5).unwrap();
    let source = json!({});
    assert_eq!(db.start_run(&start(&spec, &source, None), 10).unwrap(), Started::NoMachine);
    let elsewhere = RunSpec { project: "prj_z".into(), workspace: "hw_a".into(), ..spec.clone() };
    assert_eq!(db.start_run(&start(&elsewhere, &source, None), 10).unwrap(), Started::NoWorkspace);
    db.c().execute("UPDATE hosted_workspaces SET deleted_at = 9 WHERE id = 'hw_a'", params![]).unwrap();
    let gone = RunSpec { workspace: "hw_a".into(), ..spec };
    assert_eq!(db.start_run(&start(&gone, &source, None), 10).unwrap(), Started::NoWorkspace);
    assert!(runs_in(&db, "prj_a").is_empty());
}

/// Specs: a name once per project, found by id or name, changed and
/// deleted with their schedules (and jobs) and rules.
#[test]
fn specs_by_name() {
    let db = Db::test();
    seed(&db);
    let spec = db.add_run_spec("prj_a", "test", "hw_a", &cmd(&["ls"]), "alice", 5).unwrap();
    let taken = db.add_run_spec("prj_a", "test", "hw_a", &cmd(&["ls"]), "alice", 5).unwrap_err();
    assert!(taken.is::<Taken>(), "{taken:#}");
    assert_eq!(db.run_spec("prj_a", "test").unwrap().unwrap().id, spec.id);
    assert_eq!(db.run_spec("prj_a", &spec.id).unwrap().unwrap().name, "test");
    assert!(db.run_spec("prj_z", "test").unwrap().is_none());
    let other = db.add_run_spec("prj_a", "lint", "hw_a", &cmd(&["ls"]), "alice", 5).unwrap();
    assert!(db.update_run_spec(&other.id, Some("test"), None, None, "bob", 6).unwrap_err().is::<Taken>());
    let agent =
        Operation { instruction: Some("fix the build".into()), agent: Some("fixer".into()), ..Default::default() };
    let changed = db.update_run_spec(&spec.id, Some("triage"), None, Some(&agent), "bob", 7).unwrap().unwrap();
    assert_eq!((changed.name.as_str(), changed.kind.as_str(), &changed.op), ("triage", "agent", &agent));
    assert_eq!((changed.updated_by.as_deref(), changed.updated_at, changed.created_at), (Some("bob"), 7, 5));
    let renamed = db.update_run_spec(&spec.id, None, Some("hw_b"), None, "bob", 8).unwrap().unwrap();
    assert_eq!((renamed.workspace.as_str(), &renamed.op), ("hw_b", &agent), "the operation stays");

    let s = db.add_run_schedule(&changed, "0 * * * *", "UTC", 3_600_000, "alice", 9).unwrap();
    db.add_run_rule(&changed, RuleEvent::Push, None, Some("main"), false, 20, "alice", 9).unwrap();
    assert_eq!(db.platform_job(&s.id).unwrap().unwrap().next_at, 3_600_000);
    assert!(db.delete_run_spec(&spec.id).unwrap());
    assert!(db.run_schedule(&s.id).unwrap().is_none() && db.platform_job(&s.id).unwrap().is_none());
    assert!(db.spec_rules(&spec.id).unwrap().is_empty());
    assert_eq!(db.run_specs("prj_a").unwrap().len(), 1);
}

/// A tick is won once: firing it again from the same read is `Moved`,
/// and so is firing a schedule turned off since.
#[test]
fn a_tick_fires_once() {
    let db = Db::test();
    seed(&db);
    let spec = db.add_run_spec("prj_a", "nightly", "hw_a", &cmd(&["just", "e2e"]), "alice", 5).unwrap();
    let s = db.add_run_schedule(&spec, "0 3 * * *", "UTC", 1_000, "alice", 5).unwrap();
    let fired = db.under_tx(|tx| fire(tx, &s, 2_000, None, 1_500));
    let Fired::Run(run) = fired else { panic!("{fired:?}") };
    assert_eq!((run.trigger, run.started_by.as_str()), (TriggerKind::Schedule, s.id.as_str()));
    assert_eq!(run.source, json!({ "schedule": s.id, "spec": spec.id, "tick": 1_000 }));
    assert_eq!(db.under_tx(|tx| fire(tx, &s, 2_000, None, 1_600)), Fired::Moved);
    let now = db.run_schedule(&s.id).unwrap().unwrap();
    assert_eq!((now.next_at, now.last_fired_at, now.last_run.as_deref()), (2_000, Some(1_000), Some(run.id.as_str())));
    assert_eq!((runs_in(&db, "prj_a").len(), run_jobs(&db, &run.id)), (1, 1));

    // Off: its next tick isn't won.
    let off = db.set_run_schedule(&s.id, "0 3 * * *", "UTC", false, 2_000, 1_700).unwrap().unwrap();
    assert_eq!(db.under_tx(|tx| fire(tx, &off, 3_000, None, 2_000)), Fired::Moved);
    // On, on a workspace with no machine: the tick passes, saying why.
    db.update_run_spec(&spec.id, None, Some("hw_b"), None, "alice", 2_000).unwrap();
    let on = db.set_run_schedule(&s.id, "0 3 * * *", "UTC", true, 2_000, 2_000).unwrap().unwrap();
    assert_eq!(
        db.under_tx(|tx| fire(tx, &on, 3_000, None, 2_000)),
        Fired::Skipped("its cloud workspace has no machine yet")
    );
    let after = db.run_schedule(&s.id).unwrap().unwrap();
    assert_eq!((after.next_at, after.last_error.as_deref()), (3_000, Some("its cloud workspace has no machine yet")));
    assert_eq!(runs_in(&db, "prj_a").len(), 1);
}

/// Changing a schedule makes its job due at the new tick, and queues it
/// again if it had ended.
#[test]
fn a_changed_schedule_requeues_its_job() {
    let db = Db::test();
    seed(&db);
    let spec = db.add_run_spec("prj_a", "nightly", "hw_a", &cmd(&["ls"]), "alice", 5).unwrap();
    let s = db.add_run_schedule(&spec, "0 3 * * *", "UTC", 1_000, "alice", 5).unwrap();
    db.c()
        .execute("UPDATE platform_jobs SET state = 'failed', attempts = 8, error = 'x' WHERE id = ?1", params![s.id])
        .unwrap();
    db.set_run_schedule(&s.id, "0 4 * * *", "UTC", true, 5_000, 10).unwrap().unwrap();
    let job = db.platform_job(&s.id).unwrap().unwrap();
    assert_eq!((job.state.as_str(), job.next_at, job.attempts, job.error), ("pending", 5_000, 0, None));
    assert_eq!(db.run_schedule(&s.id).unwrap().unwrap().cron, "0 4 * * *");
}

pub fn link(db: &Db, repo: u64, name: &str, project: &str) {
    let l = LinkedRepo {
        repo_id: repo,
        full_name: name.into(),
        private: true,
        project_id: project.into(),
        installation_id: 9,
        linked_by: Some("alice".into()),
        linked_at: 0,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    let i = Installation {
        id: 9,
        account_id: 500,
        login: "acme".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    db.link_repo(&l, &i, "alice", 1).unwrap();
}

/// A repo's rules: on, in the project it's linked to, naming it (in any
/// case) or no repo, on the event asked.
#[test]
fn rules_for_a_linked_repo() {
    let db = Db::test();
    seed(&db);
    link(&db, 77, "acme/app", "prj_a");
    link(&db, 78, "acme/other", "prj_z");
    let spec = db.add_run_spec("prj_a", "ci", "hw_a", &cmd(&["just", "test"]), "alice", 5).unwrap();
    let any = db.add_run_rule(&spec, RuleEvent::Push, None, Some("main"), false, 20, "alice", 5).unwrap();
    let named = db.add_run_rule(&spec, RuleEvent::Push, Some("Acme/App"), None, false, 20, "alice", 6).unwrap();
    db.add_run_rule(&spec, RuleEvent::Push, Some("acme/elsewhere"), None, false, 20, "alice", 7).unwrap();
    db.add_run_rule(&spec, RuleEvent::PrOpened, None, None, false, 20, "alice", 8).unwrap();
    let off = db.add_run_rule(&spec, RuleEvent::Push, None, None, false, 20, "alice", 9).unwrap();
    db.set_run_rule(&off.id, Some(false), None, None, 10).unwrap().unwrap();
    let ids = |repo, e| db.rules_for(repo, e).unwrap().into_iter().map(|(r, _)| r.id).collect::<Vec<_>>();
    assert_eq!(ids(77, RuleEvent::Push), [any.id.clone(), named.id]);
    assert_eq!(db.rules_for(77, RuleEvent::Push).unwrap()[0].1, spec);
    assert!(ids(78, RuleEvent::Push).is_empty(), "another project's repo");
    assert!(ids(79, RuleEvent::Push).is_empty(), "not linked");
    assert_eq!(ids(77, RuleEvent::CheckFailed).len(), 0);
    db.unlink_repo("prj_a", "acme/app", 20).unwrap().unwrap();
    assert!(ids(77, RuleEvent::Push).is_empty(), "unlinked");
}

/// Deleting an account takes the specs of the projects that went with it,
/// their schedules, jobs and rules, and its name from what stays.
#[test]
fn deleting_an_account() {
    let db = Db::test();
    seed(&db);
    db.c()
        .execute_batch(
            "INSERT INTO projects (id, account_id, name, is_default, created_at) VALUES ('prj_p', 'bob', 'p', TRUE, 1)",
        )
        .unwrap();
    let kept = db.add_run_spec("prj_a", "ci", "hw_a", &cmd(&["ls"]), "bob", 5).unwrap();
    let ks = db.add_run_schedule(&kept, "0 3 * * *", "UTC", 1_000, "bob", 5).unwrap();
    db.add_run_rule(&kept, RuleEvent::Push, None, None, false, 20, "bob", 5).unwrap();
    db.c()
        .execute(
            "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, created_at, updated_at)
             VALUES ('hw_x', 'prj_p', 'x', '{}', 'h', 'ready', 'dev_x', 1, 1)",
            params![],
        )
        .unwrap();
    let mine = db.add_run_spec("prj_p", "mine", "hw_x", &cmd(&["ls"]), "bob", 5).unwrap();
    let Started::Made(gone_run) = db.start_run(&start(&mine, &json!({}), None), 6).unwrap() else { panic!() };
    let Started::Made(kept_run) = db.start_run(&start(&kept, &json!({}), None), 6).unwrap() else { panic!() };
    let ms = db.add_run_schedule(&mine, "0 3 * * *", "UTC", 1_000, "bob", 5).unwrap();
    db.add_run_rule(&mine, RuleEvent::Push, None, None, false, 20, "bob", 5).unwrap();
    let mut c = db.c();
    let tx = c.transaction().unwrap();
    tx.execute("DELETE FROM projects WHERE id = 'prj_p'", params![]).unwrap();
    runs::erase_account(&tx, &Deleted { account: "bob".into(), machines: vec![], disbanded: vec![] }).unwrap();
    erase_account(&tx, &Deleted { account: "bob".into(), machines: vec![], disbanded: vec![] }).unwrap();
    tx.commit().unwrap();
    drop(c);
    assert!(db.run_spec("prj_p", "mine").unwrap().is_none());
    assert!(db.run_schedule(&ms.id).unwrap().is_none() && db.platform_job(&ms.id).unwrap().is_none());
    assert!(db.spec_rules(&mine.id).unwrap().is_empty());
    let k = db.run_spec("prj_a", "ci").unwrap().unwrap();
    assert_eq!((k.created_by, k.updated_by), (None, None));
    assert_eq!(db.run_schedule(&ks.id).unwrap().unwrap().created_by, None);
    assert_eq!(db.spec_rules(&kept.id).unwrap()[0].created_by, None);
    assert!(db.platform_job(&ks.id).unwrap().is_some());
    assert_eq!((run_jobs(&db, &gone_run.id), run_jobs(&db, &kept_run.id)), (0, 1), "a gone run's job goes");
}

impl Db {
    /// `f` in a transaction of its own, committed.
    fn under_tx<T>(&self, f: impl FnOnce(&super::super::sql::Tx<'_, '_>) -> Result<T>) -> T {
        let mut c = self.c();
        let tx = c.transaction().unwrap();
        let out = f(&tx).unwrap();
        tx.commit().unwrap();
        out
    }
}

/// A run a trigger makes runs under the project's live standing grant
/// (M211); with none, it names none and waits for an approval.
#[test]
fn a_run_names_the_live_grant() {
    let db = Db::test();
    seed(&db);
    let spec = db.add_run_spec("prj_a", "test", "hw_a", &cmd(&["just", "test"]), "alice", 5).unwrap();
    let source = json!({});
    let Started::Made(none) = db.start_run(&start(&spec, &source, Some("k1")), 10).unwrap() else { panic!() };
    assert_eq!(none.grant, None);
    let g = crate::db::run_grants::tests::grant("prj_a", &arugula_e2e::DeviceKeys::generate(), 10);
    db.add_run_grant(&g, "alice", 20).unwrap();
    let Started::Made(under) = db.start_run(&start(&spec, &source, Some("k2")), 30).unwrap() else { panic!() };
    assert_eq!(under.grant.as_deref(), Some(g.id.as_str()));
    let other = RunSpec { project: "prj_z".into(), ..spec.clone() };
    assert_eq!(db.start_run(&start(&other, &source, None), 30).unwrap(), Started::NoWorkspace, "another project's");
}
