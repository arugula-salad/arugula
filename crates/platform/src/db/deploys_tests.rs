use super::*;
use crate::db::migrate::Dialect;

fn project(db: &Db, id: &str) {
    db.c()
        .execute(
            "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES (?1, ?1, ?1, FALSE, 1)",
            params![id],
        )
        .unwrap();
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn commit(sha: &str) -> Source {
    Source { kind: SourceKind::Commit, reference: sha.into(), place: "arugula-salad/arugula".into() }
}

fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn production(db: &Db, project_id: &str) -> Environment {
    project(db, project_id);
    db.create_environment(project_id, "production", "chant-fly", &strings(&["FLY_API_TOKEN"]), 1).unwrap()
}

fn plan(db: &Db, env: &str, sha: &str, by: &Actor, at: u64) -> Deploy {
    let source = commit(sha);
    db.plan_deploy(&NewDeploy { environment: env, source: &source, plan_digest: &digest(sha) }, by, at)
        .unwrap()
        .unwrap()
}

fn record(db: &Db, d: &str, kind: EventKind, by: &Actor, at: u64) -> std::result::Result<Recorded, Refusal> {
    db.record_deploy(d, kind, by, at, json!({})).unwrap()
}

fn jake() -> Actor {
    Actor::person("jake")
}

/// Jake's device's verified decision on `d` as it is now: tests stand in
/// for M218's verifier.
fn decision(db: &Db, d: &str, decision: Decision) -> VerifiedApproval {
    let row = db.deploy(d).unwrap().unwrap();
    let planned = db.deploy_history(d).unwrap().into_iter().find(|e| e.kind == "planned").unwrap();
    VerifiedApproval::unchecked_for_tests(
        d,
        &row.plan_digest,
        &planned.hash,
        &digest("op"),
        "jake",
        "dev_jake",
        decision,
        99,
        "5167",
    )
}

fn approve(db: &Db, d: &str, at: u64) -> std::result::Result<Recorded, Refusal> {
    db.record_approval(&decision(db, d, Decision::Once), at).unwrap()
}

/// Planned, asked for and approved by Jake's device.
fn approved(db: &Db, env: &str, sha: &str, at: u64) -> Deploy {
    let d = plan(db, env, sha, &jake(), at);
    record(db, &d.id, EventKind::Requested, &jake(), at).unwrap();
    approve(db, &d.id, at).unwrap().deploy
}

/// Drop both of `deploy_events`' triggers: what someone with the database
/// can do.
fn drop_triggers(db: &Db) {
    let c = db.c();
    for t in ["deploy_events_append_only", "deploy_events_kept"] {
        c.execute_batch(&match c.dialect() {
            Dialect::Sqlite => format!("DROP TRIGGER {t}"),
            Dialect::Postgres => format!("DROP TRIGGER {t} ON deploy_events"),
        })
        .unwrap();
    }
}

fn kinds(events: &[Event]) -> Vec<&str> {
    events.iter().map(|e| e.kind.as_str()).collect()
}

/// An environment's name is its project's own; its driver is a known one;
/// its secrets are names, sorted.
#[test]
fn environments_are_named_in_their_project() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    assert!(env.id.starts_with("env_") && env.id.len() == 36, "{}", env.id);
    assert_eq!((env.name.as_str(), env.driver.as_str()), ("production", "chant-fly"));
    let again = db.create_environment("prj_a", "production", "apps-platform", &[], 2).unwrap_err();
    assert!(again.is::<crate::db::Taken>(), "{again:#}");
    project(&db, "prj_b");
    db.create_environment("prj_b", "production", "apps-platform", &[], 2).unwrap();
    assert!(db.create_environment("prj_a", "Prod", "chant-fly", &[], 2).is_err(), "a name is lowercase");
    assert!(db.create_environment("prj_a", "staging", "planter", &[], 2).is_err(), "planter waits for step 8");
    assert!(db.create_environment("prj_a", "staging", "chant-fly", &strings(&["fly token"]), 2).is_err());

    let env = db.set_environment_secrets(&env.id, &strings(&["ZED", "FLY_API_TOKEN", "ZED"]), 3).unwrap().unwrap();
    assert_eq!(env.secret_names, strings(&["FLY_API_TOKEN", "ZED"]));
    assert_eq!((env.created_at, env.updated_at), (1, 3));
    assert_eq!(db.environment(&env.id).unwrap().unwrap(), env);
    assert_eq!(db.environments("prj_a").unwrap(), vec![env]);
    assert!(db.set_environment_secrets("env_none", &[], 3).unwrap().is_none());
}

/// A deploy goes planned, pending, approved, running, succeeded, and each
/// step is an event in its environment's log, by whom it was.
#[test]
fn a_deploy_moves_through_s37s_states() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let d = plan(&db, &env.id, "abc123", &Actor::agent("jake", "geek"), 10);
    assert!(d.id.starts_with("dep_") && d.id.len() == 36, "{}", d.id);
    assert_eq!((d.state, d.planned_seq, d.run.as_deref()), (State::Planned, 1, None));
    assert_eq!((d.planned_by.as_str(), d.planned_by_agent.as_deref()), ("jake", Some("geek")));
    // An agent may ask for approval.
    record(&db, &d.id, EventKind::Requested, &Actor::agent("jake", "geek"), 11).unwrap();
    assert_eq!(db.may_approve(&d.id).unwrap(), Ok(()));
    approve(&db, &d.id, 12).unwrap();
    assert_eq!(db.may_approve(&d.id).unwrap(), Err(Refusal::State(State::Approved)), "approved once");
    let started = record(&db, &d.id, EventKind::Started, &Actor::run("run_1"), 13).unwrap();
    assert_eq!((started.deploy.state, started.deploy.run.as_deref()), (State::Running, Some("run_1")));
    assert_eq!(started.events[0].detail_json().unwrap(), json!({ "run": "run_1" }));
    assert_eq!(
        record(&db, &d.id, EventKind::Succeeded, &Actor::run("run_2"), 14),
        Err(Refusal::NotItsRun),
        "another run can't finish it"
    );
    let done = record(&db, &d.id, EventKind::Succeeded, &Actor::run("run_1"), 14).unwrap();
    assert_eq!((done.deploy.state, done.deploy.updated_at), (State::Succeeded, 14));
    assert_eq!(db.deploy(&d.id).unwrap().unwrap(), done.deploy);
    let log = db.deploy_events(&env.id).unwrap();
    assert_eq!(kinds(&log), ["planned", "requested", "approved", "started", "succeeded"]);
    let actors: Vec<_> = log.iter().map(|e| Actor::parse(&e.actor).unwrap()).collect();
    assert_eq!(
        actors,
        [Actor::agent("jake", "geek"), Actor::agent("jake", "geek"), jake(), Actor::run("run_1"), Actor::run("run_1")]
    );
    assert_eq!(
        log[0].detail_json().unwrap(),
        json!({ "source": { "kind": "commit", "ref": "abc123", "where": "arugula-salad/arugula" }, "plan": digest("abc123") })
    );
    assert_eq!(db.deploy_history(&d.id).unwrap(), log);
    // Done is done.
    assert_eq!(record(&db, &d.id, EventKind::Canceled, &jake(), 15), Err(Refusal::State(State::Succeeded)));
    assert_eq!(record(&db, "dep_none", EventKind::Canceled, &jake(), 15), Err(Refusal::Gone));
    assert!(
        db.plan_deploy(
            &NewDeploy { environment: "env_none", source: &commit("a"), plan_digest: &digest("a") },
            &jake(),
            1
        )
        .unwrap()
        .is_none()
    );
}

/// Agents plan and ask, and never approve or deny; only runs apply; only
/// the platform expires.
#[test]
fn agents_never_approve() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let agent = Actor::agent("jake", "geek");
    let d = plan(&db, &env.id, "abc", &agent, 1);
    record(&db, &d.id, EventKind::Requested, &agent, 2).unwrap();
    assert_eq!(record(&db, &d.id, EventKind::Approved, &agent, 3), Err(Refusal::Agent));
    assert_eq!(record(&db, &d.id, EventKind::Denied, &agent, 3), Err(Refusal::Agent));
    for by in [jake(), Actor::run("run_1"), Actor::Platform] {
        assert_eq!(record(&db, &d.id, EventKind::Approved, &by, 3), Err(Refusal::Unverified), "{by:?}");
        assert_eq!(record(&db, &d.id, EventKind::Denied, &by, 3), Err(Refusal::Unverified), "{by:?}");
    }
    assert_eq!(record(&db, &d.id, EventKind::Expired, &jake(), 3), Err(Refusal::Actor));
    assert_eq!(record(&db, &d.id, EventKind::Started, &jake(), 3), Err(Refusal::Actor));
    assert_eq!(record(&db, &d.id, EventKind::Staled, &Actor::Platform, 3), Err(Refusal::Actor));
    assert_eq!(db.deploy(&d.id).unwrap().unwrap().state, State::Pending, "nothing changed");
    assert_eq!(kinds(&db.deploy_events(&env.id).unwrap()), ["planned", "requested"]);
    // An owner's device denies; denied is the end.
    db.record_approval(&decision(&db, &d.id, Decision::Deny), 4).unwrap().unwrap();
    assert_eq!(approve(&db, &d.id, 5), Err(Refusal::State(State::Denied)));
    assert!(
        db.plan_deploy(
            &NewDeploy { environment: &env.id, source: &commit("b"), plan_digest: &digest("b") },
            &Actor::Platform,
            6
        )
        .is_err()
    );
}

/// The stale rule: a deploy planned before another apply to its
/// environment can't be approved or started, and goes stale when it
/// happens. A failed apply counts, and so does one that finishes after
/// the plan. Other environments aren't touched.
#[test]
fn the_stale_rule() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let staging = db.create_environment("prj_a", "staging", "chant-fly", &[], 1).unwrap();
    let a = approved(&db, &env.id, "aaa", 1);
    let b = plan(&db, &env.id, "bbb", &jake(), 2);
    record(&db, &b.id, EventKind::Requested, &jake(), 2).unwrap();
    let c = approved(&db, &env.id, "ccc", 3);
    let elsewhere = plan(&db, &staging.id, "sss", &jake(), 3);
    record(&db, &elsewhere.id, EventKind::Requested, &jake(), 3).unwrap();
    assert_eq!(db.may_approve(&b.id).unwrap(), Ok(()));

    // A starts: B (pending) and C (approved) were planned before it.
    let started = record(&db, &a.id, EventKind::Started, &Actor::run("run_a"), 4).unwrap();
    assert_eq!(kinds(&started.events), ["started", "staled", "staled"]);
    let at = started.events[0].seq;
    assert_eq!(started.events[1].deploy, b.id);
    assert_eq!(started.events[2].deploy, c.id);
    assert_eq!(started.events[1].detail_json().unwrap(), json!({ "after": a.id, "seq": at }));
    assert_eq!(Actor::parse(&started.events[1].actor), Some(Actor::Platform));
    for d in [&b, &c] {
        assert_eq!(db.deploy(&d.id).unwrap().unwrap().state, State::Stale);
    }
    let why = Refusal::Stale { seq: at, deploy: a.id.clone() };
    assert_eq!(db.may_approve(&b.id).unwrap(), Err(why.clone()));
    assert_eq!(approve(&db, &b.id, 5), Err(why.clone()));
    assert_eq!(record(&db, &c.id, EventKind::Started, &Actor::run("run_c"), 5), Err(why.clone()));
    // Stale is the end, as denied is.
    assert_eq!(record(&db, &b.id, EventKind::Canceled, &jake(), 5), Err(Refusal::State(State::Stale)));
    // Another environment's deploy isn't stale.
    assert_eq!(db.may_approve(&elsewhere.id).unwrap(), Ok(()));

    // The rule is the log's, not the row's: a row put back to pending, and
    // its copy of its plan's seq moved past the apply, is still refused.
    db.c().execute("UPDATE deploys SET state = 'pending', planned_seq = 999 WHERE id = ?1", params![b.id]).unwrap();
    assert_eq!(db.may_approve(&b.id).unwrap(), Err(why.clone()));
    assert_eq!(approve(&db, &b.id, 5), Err(why));
    db.c()
        .execute("UPDATE deploys SET state = 'stale', planned_seq = ?2 WHERE id = ?1", params![b.id, b.planned_seq])
        .unwrap();

    // D is planned while A is being applied: it may be approved, but not
    // started while A runs, and A's end makes it stale (A may have changed
    // the environment after D's plan).
    let d = approved(&db, &env.id, "ddd", 6);
    assert_eq!(
        record(&db, &d.id, EventKind::Started, &Actor::run("run_d"), 6),
        Err(Refusal::Busy { deploy: a.id.clone() })
    );
    let failed = record(&db, &a.id, EventKind::Failed, &Actor::run("run_a"), 7).unwrap();
    assert_eq!(kinds(&failed.events), ["failed", "staled"]);
    assert_eq!(
        record(&db, &d.id, EventKind::Started, &Actor::run("run_d"), 8),
        Err(Refusal::Stale { seq: failed.events[0].seq, deploy: a.id.clone() }),
        "a failed apply counts"
    );

    // E, planned after every apply, goes through, and its own apply
    // doesn't make it stale.
    let e = approved(&db, &env.id, "eee", 9);
    record(&db, &e.id, EventKind::Started, &Actor::run("run_e"), 10).unwrap();
    record(&db, &e.id, EventKind::Succeeded, &Actor::run("run_e"), 11).unwrap();
    assert_eq!(db.deploy(&e.id).unwrap().unwrap().state, State::Succeeded);
    assert_eq!(db.deploys(&env.id, 2).unwrap().iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), [&e.id, &d.id]);
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(db.deploy_events(&env.id).unwrap().len() as u64));
}

/// The log is a hash chain per environment: it verifies, it can't be
/// updated in place, and a row changed behind the platform's back (the
/// trigger dropped) is found at the first bad link: a changed field, a
/// forged hash (the next link breaks), a removed row.
#[test]
fn the_hash_chain_finds_a_tampered_event() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let other = db.create_environment("prj_a", "staging", "chant-fly", &[], 1).unwrap();
    let a = approved(&db, &env.id, "aaa", 1);
    plan(&db, &other.id, "sss", &jake(), 1);
    record(&db, &a.id, EventKind::Started, &Actor::run("run_a"), 2).unwrap();
    record(&db, &a.id, EventKind::Succeeded, &Actor::run("run_a"), 3).unwrap();
    let log = db.deploy_events(&env.id).unwrap();
    assert_eq!(log.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3, 4, 5], "its own seq, from 1");
    assert_eq!(log[0].prev_hash, GENESIS);
    assert!(log.windows(2).all(|w| w[1].prev_hash == w[0].hash));
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(5));
    assert_eq!(db.verify_deploy_log(&other.id).unwrap(), Ok(1), "each environment its own chain");
    assert_eq!(verify_chain(&other.id, &log), Err(BadLink { seq: 1, why: Broken::Environment }));

    // The platform can't update one.
    let c = db.c();
    let refused = c.execute("UPDATE deploy_events SET at = 0 WHERE environment_id = ?1", params![env.id]);
    assert!(format!("{:#}", refused.unwrap_err()).contains("append-only"));
    // Nor delete one while its environment is there.
    let refused = c.execute("DELETE FROM deploy_events WHERE environment_id = ?1", params![env.id]);
    assert!(format!("{:#}", refused.unwrap_err()).contains("append-only"));
    drop(c);
    // Someone with the database can: drop the triggers.
    drop_triggers(&db);
    let c = db.c();
    let tamper = |sql: &str| {
        c.execute(sql, params![env.id]).unwrap();
    };
    // Who approved, changed.
    tamper("UPDATE deploy_events SET actor = 'person mallory' WHERE environment_id = ?1 AND seq = 3");
    drop(c);
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Err(BadLink { seq: 3, why: Broken::Hash }));
    // Its hash forged to match: the next event's link breaks.
    let mut forged = db.deploy_events(&env.id).unwrap()[2].clone();
    forged.hash = forged.digest().unwrap();
    let c = db.c();
    c.execute("UPDATE deploy_events SET hash = ?2 WHERE environment_id = ?1 AND seq = 3", params![env.id, forged.hash])
        .unwrap();
    drop(c);
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Err(BadLink { seq: 4, why: Broken::Prev }));
    // Put back, then one removed.
    let c = db.c();
    c.execute(
        "UPDATE deploy_events SET actor = ?2, hash = ?3 WHERE environment_id = ?1 AND seq = 3",
        params![env.id, log[2].actor, log[2].hash],
    )
    .unwrap();
    drop(c);
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(5));
    db.c().execute("DELETE FROM deploy_events WHERE environment_id = ?1 AND seq = 2", params![env.id]).unwrap();
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Err(BadLink { seq: 2, why: Broken::Seq { found: 3 } }));
    assert_eq!(db.verify_deploy_log(&other.id).unwrap(), Ok(1), "the other environment's chain holds");
    // The detail, too, and a field with a newline can't be read one way.
    let mut e = log[0].clone();
    e.detail = e.detail.replace("aaa", "bbb");
    assert_eq!(verify_chain(&env.id, &[e]), Err(BadLink { seq: 1, why: Broken::Hash }));
    let mut e = log[0].clone();
    e.actor = "person jake\nat 1".into();
    assert_eq!(verify_chain(&env.id, &[e]), Err(BadLink { seq: 1, why: Broken::Unreadable }));
}

/// An event's canonical bytes and hash, pinned: another verifier (the
/// phone, an export's reader) must get the same.
#[test]
fn an_events_canonical_form_is_pinned() {
    let e = Event {
        environment: "env_1".into(),
        seq: 1,
        deploy: "dep_1".into(),
        kind: "planned".into(),
        actor: Actor::agent("acct", "geek").encode(),
        at: 1_700_000_000_000,
        detail: r#"{"plan":"ff"}"#.into(),
        prev_hash: GENESIS.into(),
        hash: String::new(),
    };
    assert_eq!(
        e.canonical().unwrap(),
        format!(
            "arugula deploy event v1\nprev {GENESIS}\nenvironment env_1\nseq 1\ndeploy dep_1\nkind planned\n\
             actor agent acct geek\nat 1700000000000\ndetail eyJwbGFuIjoiZmYifQ==\n"
        )
    );
    assert_eq!(e.digest().unwrap(), "5771e7fb68963ef115a17ee48691dbf38fb4edaf93281e30093a90a936e1569d"); // gitleaks:allow (a test hash)
    for a in [jake(), Actor::agent("a", "m"), Actor::run("run_1"), Actor::Platform] {
        assert_eq!(Actor::parse(&a.encode()), Some(a));
    }
    for bad in ["", "person", "person a b", "agent a", "platform x", "robot a"] {
        assert_eq!(Actor::parse(bad), None, "{bad:?}");
    }
}

/// Plans from two of the platform's machines at once (Postgres) get the
/// seqs 1 to n, each once, and the chain holds.
#[test]
fn concurrent_plans_keep_one_chain() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let second = db.second();
    let handles: Vec<&Db> = std::iter::once(&db).chain(second.as_ref()).collect();
    let made: Vec<Deploy> = std::thread::scope(|s| {
        let threads: Vec<_> = (0..40)
            .map(|i| {
                let (db, env) = (handles[i % handles.len()], &env.id);
                s.spawn(move || plan(db, env, &format!("sha{i}"), &jake(), i as u64))
            })
            .collect();
        threads.into_iter().map(|t| t.join().unwrap()).collect()
    });
    let mut seqs: Vec<u64> = made.iter().map(|d| d.planned_seq).collect();
    seqs.sort();
    assert_eq!(seqs, (1..=40).collect::<Vec<u64>>());
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(40));
}

/// Deleting an account takes the environments, deploys and logs of the
/// projects that go with it. Elsewhere its deploys stop naming it, and
/// its events stay as written, so the chain still verifies.
#[test]
fn deleting_an_account_keeps_the_chain() {
    let db = Db::test();
    let mine = db.personal_project("alice", 1).unwrap();
    let own = db.create_environment(&mine, "production", "chant-fly", &[], 1).unwrap();
    plan(&db, &own.id, "aaa", &Actor::person("alice"), 1);
    let team = production(&db, "prj_team");
    let d = plan(&db, &team.id, "bbb", &Actor::agent("alice", "alices-box"), 2);
    record(&db, &d.id, EventKind::Requested, &Actor::person("alice"), 3).unwrap();
    let gone = Deleted { account: "alice".into(), machines: vec!["alices-box".into()], disbanded: vec![] };
    for _ in 0..2 {
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        crate::db::projects::erase_account(&tx, &gone).unwrap();
        erase_account(&tx, &gone).unwrap();
        tx.commit().unwrap();
    }
    assert!(db.environment(&own.id).unwrap().is_none(), "its personal project's environments go");
    assert!(db.deploy_events(&own.id).unwrap().is_empty());
    let d = db.deploy(&d.id).unwrap().unwrap();
    assert_eq!((d.planned_by.as_str(), d.planned_by_agent.as_deref(), d.state), ("", None, State::Pending));
    assert_eq!(db.verify_deploy_log(&team.id).unwrap(), Ok(2));
    assert_eq!(db.mentions("alice"), ["deploy_events.actor"], "only the log keeps its id");
}

/// Events cut off the end of the log leave a chain that verifies, so the
/// deploys' rows are checked against their events: a state, a plan's
/// seq, a run they don't bear out, or an event with no deploy.
#[test]
fn a_log_cut_short_shows_in_the_rows() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let a = approved(&db, &env.id, "aaa", 1);
    record(&db, &a.id, EventKind::Started, &Actor::run("run_a"), 2).unwrap();
    record(&db, &a.id, EventKind::Succeeded, &Actor::run("run_a"), 3).unwrap();
    let b = plan(&db, &env.id, "bbb", &jake(), 4);
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(6));
    drop_triggers(&db);
    let bad =
        |seq, deploy: &Deploy, field| Err(BadLink { seq, why: Broken::Deploy { deploy: deploy.id.clone(), field } });
    let sql = |q: &str| {
        db.c().execute(q, params![env.id]).unwrap();
    };
    // B's plan cut off: B's row has no planned event.
    sql("DELETE FROM deploy_events WHERE environment_id = ?1 AND seq = 6");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), bad(6, &b, DeployField::Planned));
    // And A's success: A's row says more than its events.
    sql("DELETE FROM deploys WHERE environment_id = ?1 AND state = 'planned'");
    sql("DELETE FROM deploy_events WHERE environment_id = ?1 AND seq = 5");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), bad(4, &a, DeployField::State));
    sql("UPDATE deploys SET state = 'running' WHERE environment_id = ?1");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(4), "the row and its events agree again");
    // Its run, or its plan's seq, changed on the row.
    sql("UPDATE deploys SET run_id = 'run_x' WHERE environment_id = ?1");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), bad(4, &a, DeployField::Run));
    sql("UPDATE deploys SET run_id = 'run_a', planned_seq = 2 WHERE environment_id = ?1");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), bad(2, &a, DeployField::Planned));
    // Its row gone: its events name a deploy that isn't there.
    sql("DELETE FROM deploys WHERE environment_id = ?1");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), bad(1, &a, DeployField::Missing));
}

/// An approval is recorded only from a verified one, and only for the
/// deploy as it is: its plan and its `planned` event. It goes in the event
/// whole, as the owner's account.
#[test]
fn only_a_verified_approval_approves() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let d = plan(&db, &env.id, "aaa", &jake(), 1);
    assert_eq!(approve(&db, &d.id, 2), Err(Refusal::State(State::Planned)), "nobody asked yet");
    record(&db, &d.id, EventKind::Requested, &jake(), 2).unwrap();
    let other = plan(&db, &env.id, "bbb", &jake(), 3);
    let planned = |id: &str| db.deploy_history(id).unwrap()[0].hash.clone();
    let row = db.deploy(&d.id).unwrap().unwrap();
    let made = |plan: &str, planned: &str| {
        VerifiedApproval::unchecked_for_tests(
            &d.id,
            plan,
            planned,
            "0p",
            "jake",
            "dev_jake",
            Decision::Once,
            99,
            "5167",
        )
    };
    assert_eq!(
        db.record_approval(&made(&digest("bbb"), &planned(&d.id)), 4).unwrap(),
        Err(Refusal::Mismatch(Mismatch::Plan))
    );
    assert_eq!(
        db.record_approval(&made(&row.plan_digest, &planned(&other.id)), 4).unwrap(),
        Err(Refusal::Mismatch(Mismatch::Planned))
    );
    assert_eq!(db.deploy(&d.id).unwrap().unwrap().state, State::Pending, "nothing changed");
    let ok = made(&row.plan_digest, &planned(&d.id));
    let done = db.record_approval(&ok, 5).unwrap().unwrap();
    assert_eq!(done.deploy.state, State::Approved);
    let e = &done.events[0];
    assert_eq!((e.kind.as_str(), Actor::parse(&e.actor)), ("approved", Some(jake())));
    assert_eq!(e.detail_json().unwrap(), ok.detail());
    assert_eq!(
        ok.detail(),
        json!({ "decision": "once", "plan": row.plan_digest, "planned": planned(&d.id), "op": "0p",
                "device": "dev_jake", "expires": 99, "signature": "5167" })
    );
    assert_eq!(db.record_approval(&ok, 6).unwrap(), Err(Refusal::State(State::Approved)), "once");
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(4));
}

/// The platform can't fail a running deploy (its run may still be
/// applying, and the busy check would let another start); the run's own
/// report can.
#[test]
fn only_its_run_fails_a_running_deploy() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let a = approved(&db, &env.id, "aaa", 1);
    record(&db, &a.id, EventKind::Started, &Actor::run("run_a"), 2).unwrap();
    let b = approved(&db, &env.id, "bbb", 3);
    assert_eq!(record(&db, &a.id, EventKind::Failed, &Actor::Platform, 4), Err(Refusal::Actor));
    assert_eq!(record(&db, &a.id, EventKind::Failed, &Actor::run("run_b"), 4), Err(Refusal::NotItsRun));
    assert_eq!(
        record(&db, &b.id, EventKind::Started, &Actor::run("run_b"), 4),
        Err(Refusal::Busy { deploy: a.id.clone() }),
        "still being applied"
    );
    let failed = record(&db, &a.id, EventKind::Failed, &Actor::run("run_a"), 5).unwrap();
    assert_eq!(failed.deploy.state, State::Failed);
}

/// An event's detail is small.
#[test]
fn details_are_capped() {
    let db = Db::test();
    let env = production(&db, "prj_a");
    let d = plan(&db, &env.id, "aaa", &jake(), 1);
    let big = json!({ "note": "x".repeat(MAX_DETAIL) });
    let e = db.record_deploy(&d.id, EventKind::Requested, &jake(), 2, big).unwrap_err();
    assert!(format!("{e:#}").contains("at most"), "{e:#}");
    assert_eq!(db.deploy(&d.id).unwrap().unwrap().state, State::Planned);
    record(&db, &d.id, EventKind::Requested, &jake(), 2).unwrap();
}
