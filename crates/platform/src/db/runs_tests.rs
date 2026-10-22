use arugula_e2e::DeviceKeys;
use serde_json::json;

use super::*;
use crate::db::Taken;

const OP: &str = "arugula op v1\ndaemon d1\nkind run\nsession -\npane - -\ncwd -\nargv bWFrZQ==\n";

fn team_project(db: &Db, id: &str) {
    db.c()
        .execute(
            "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES (?1, ?1, ?1, FALSE, 1)",
            params![id],
        )
        .unwrap();
}

fn personal_project(db: &Db, id: &str, account: &str) {
    db.c()
        .execute(
            "INSERT INTO projects (id, account_id, name, is_default, created_at) VALUES (?1, ?2, ?1, FALSE, 1)",
            params![id, account],
        )
        .unwrap();
}

fn new_run<'a>(project: &'a str, source: &'a serde_json::Value, grant: Option<&'a str>) -> NewRun<'a> {
    NewRun {
        project,
        workspace: Some("hw_1"),
        trigger: TriggerKind::Manual,
        source,
        op: OP,
        started_by: "a1",
        started_by_agent: None,
        grant,
    }
}

fn create(db: &Db, grant: Option<&str>) -> Run {
    db.create_run(&new_run("prj_a", &json!({}), grant), 10).unwrap().unwrap()
}

/// The detail a move to `to` carries: an exit code on the two that end a
/// process.
fn detail(to: State) -> &'static str {
    match to {
        State::Succeeded => r#"{"exit":0}"#,
        State::Failed => r#"{"exit":2}"#,
        _ => "{}",
    }
}

/// The hosted daemon running the runs, and its key.
struct Daemon {
    keys: DeviceKeys,
    id: String,
}

impl Daemon {
    fn new() -> Self {
        let keys = DeviceKeys::generate();
        let id = keys.id();
        Self { keys, id }
    }

    fn key(&self) -> String {
        hex::encode(self.keys.sign_public())
    }

    /// Move `run` from `from` to `to` as this daemon reports it, signed.
    fn report(&self, db: &Db, run: &str, from: State, to: State, at: u64) -> Result<(Run, Event), Refused> {
        let seq = db.run(run).unwrap().map_or(1, |r| r.last_seq + 1);
        let d = detail(to);
        let signed = arugula_e2e::run::Event { run, seq, from: Some(from), to, at, detail: d, daemon: &self.id };
        let sig = signed.sign(&self.keys);
        db.move_run(run, &Move { from, to, at, detail: d, by: Reporter::Daemon { id: &self.id, seq, sig: &sig } })
            .unwrap()
    }
}

fn control(db: &Db, run: &str, from: State, to: State, at: u64) -> Result<(Run, Event), Refused> {
    db.move_run(run, &Move { from, to, at, detail: detail(to), by: Reporter::Control }).unwrap()
}

/// Move `run` from `from` to `to` with a reporter that may: the daemon
/// for what only it reports, else control.
fn step(db: &Db, d: &Daemon, run: &str, from: State, to: State, at: u64) -> Result<(Run, Event), Refused> {
    if to.daemon_only() { d.report(db, run, from, to, at) } else { control(db, run, from, to, at) }
}

/// A run with a standing grant, brought to `state` the shortest way.
fn run_in(db: &Db, d: &Daemon, state: State) -> Run {
    use State::*;
    let run = create(db, Some("grt_1"));
    let path: &[State] = match state {
        Pending => &[],
        Approved | Denied | Canceled | Expired | Running => &[state],
        Succeeded | Failed => &[Running, state],
    };
    let mut from = Pending;
    for (i, to) in path.iter().enumerate() {
        step(db, d, &run.id, from, *to, 20 + i as u64).unwrap();
        from = *to;
    }
    db.run(&run.id).unwrap().unwrap()
}

/// From each of the eight states to each of the eight, a run moves when
/// S37 lets it and is refused, unchanged, when S37 doesn't (M210's
/// done-when). Each move writes one event, numbered next, and a daemon's
/// carries a signature that verifies over what was stored.
#[test]
fn every_move_s37_allows_and_none_it_doesnt() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let mut allowed = 0;
    for from in State::ALL {
        for to in State::ALL {
            let run = run_in(&db, &d, from);
            assert_eq!(run.state, from);
            let before = db.run_events(&run.id).unwrap();
            let got = step(&db, &d, &run.id, from, to, 100);
            if from.may_become(to) {
                allowed += 1;
                let (after, ev) = got.unwrap_or_else(|e| panic!("{from} to {to}: {e}"));
                assert_eq!((after.state, after.last_seq), (to, run.last_seq + 1), "{from} to {to}");
                assert_eq!(db.run(&run.id).unwrap().unwrap(), after);
                let events = db.run_events(&run.id).unwrap();
                assert_eq!(events.len(), before.len() + 1);
                assert_eq!(events.last().unwrap(), &ev);
                assert_eq!((ev.seq, ev.from, ev.to, ev.at), (after.last_seq, Some(from), to, 100));
                assert_eq!(after.started_at.is_some(), to == State::Running || from == State::Running);
                assert_eq!(after.ended_at, to.terminal().then_some(100));
                assert_eq!(
                    after.exit_code,
                    (to == State::Succeeded).then_some(0).or((to == State::Failed).then_some(2))
                );
                match ev.signed() {
                    Some(signed) => {
                        assert!(to.daemon_only(), "{to}");
                        assert!(signed.verify(&d.key(), ev.sig.as_deref().unwrap()), "{from} to {to}");
                    }
                    None => assert!(!to.daemon_only() && ev.sig.is_none()),
                }
            } else {
                assert_eq!(got.unwrap_err(), Refused::Disallowed { from, to });
                assert_eq!(db.run(&run.id).unwrap().unwrap(), run, "{from} to {to} changed the run");
                assert_eq!(db.run_events(&run.id).unwrap(), before, "{from} to {to} wrote an event");
            }
        }
    }
    assert_eq!(allowed, 12, "S37's arrows, with failing the check at the start");
    // Each run's history is the moves it made, from its making.
    let run = run_in(&db, &d, State::Succeeded);
    let ev = db.run_events(&run.id).unwrap();
    let moves: Vec<_> = ev.iter().map(|e| (e.seq, e.from, e.to)).collect();
    assert_eq!(
        moves,
        [
            (1, None, State::Pending),
            (2, Some(State::Pending), State::Running),
            (3, Some(State::Running), State::Succeeded)
        ]
    );
    assert!(ev[0].daemon.is_none() && ev[1].daemon.as_deref() == Some(d.id.as_str()));
}

/// Expired and failed may come from control or from the daemon; the
/// rest from one only.
#[test]
fn each_state_has_its_reporters() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    for to in [State::Running, State::Succeeded] {
        let run = run_in(&db, &d, if to == State::Running { State::Approved } else { State::Running });
        assert_eq!(control(&db, &run.id, run.state, to, 50).unwrap_err(), Refused::Reporter, "{to}");
    }
    for to in [State::Approved, State::Denied, State::Canceled] {
        let run = create(&db, None);
        assert_eq!(d.report(&db, &run.id, State::Pending, to, 50).unwrap_err(), Refused::Reporter, "{to}");
    }
    let run = create(&db, None);
    assert_eq!(d.report(&db, &run.id, State::Pending, State::Expired, 50).unwrap().0.state, State::Expired);
    let run = run_in(&db, &d, State::Running);
    assert_eq!(control(&db, &run.id, State::Running, State::Failed, 50).unwrap().0.state, State::Failed);
    let run = create(&db, None);
    assert_eq!(control(&db, &run.id, State::Pending, State::Failed, 50).unwrap().0.exit_code, Some(2));
}

/// Straight to running needs the standing grant; an approved run doesn't.
#[test]
fn pending_to_running_needs_a_grant() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let run = create(&db, None);
    assert_eq!(d.report(&db, &run.id, State::Pending, State::Running, 50).unwrap_err(), Refused::NoGrant);
    assert_eq!(db.run_events(&run.id).unwrap().len(), 1);
    control(&db, &run.id, State::Pending, State::Approved, 51).unwrap();
    let (now, _) = d.report(&db, &run.id, State::Approved, State::Running, 52).unwrap();
    assert_eq!((now.state, now.started_at), (State::Running, Some(52)));
}

/// A move from a state the run has left, a daemon's report that isn't
/// next (a replay), a detail that doesn't fit: each refused, nothing
/// written.
#[test]
fn stale_replayed_and_malformed_moves_are_refused() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let run = run_in(&db, &d, State::Running);
    let unchanged = |run: &Run| {
        assert_eq!(&db.run(&run.id).unwrap().unwrap(), run);
        assert_eq!(db.run_events(&run.id).unwrap().len() as u64, run.last_seq);
    };

    assert_eq!(
        control(&db, &run.id, State::Pending, State::Canceled, 50).unwrap_err(),
        Refused::Moved { now: State::Running }
    );
    assert_eq!(control(&db, "run_nope", State::Pending, State::Canceled, 50).unwrap_err(), Refused::NoRun);
    unchanged(&run);

    // The report that started it, sent again: its seq is spent.
    let first = &db.run_events(&run.id).unwrap()[1];
    let replay = Move {
        from: State::Running,
        to: State::Succeeded,
        at: 60,
        detail: r#"{"exit":0}"#,
        by: Reporter::Daemon { id: &d.id, seq: first.seq, sig: first.sig.as_deref().unwrap() },
    };
    assert_eq!(db.move_run(&run.id, &replay).unwrap().unwrap_err(), Refused::Seq { next: 3 });
    let bad_id = Move { by: Reporter::Daemon { id: "d 1", seq: 3, sig: "00" }, ..replay };
    assert_eq!(db.move_run(&run.id, &bad_id).unwrap().unwrap_err(), Refused::Reporter);
    unchanged(&run);

    for (to, detail) in [
        (State::Succeeded, r#"{"exit":1}"#),
        (State::Succeeded, "{}"),
        (State::Failed, r#"{"exit":0}"#),
        (State::Failed, r#"{"exit":"2"}"#),
        (State::Failed, "[]"),
        (State::Failed, "not json"),
    ] {
        let m = Move { from: State::Running, to, at: 61, detail, by: Reporter::Control };
        assert!(matches!(db.move_run(&run.id, &m).unwrap(), Err(Refused::Detail(_) | Refused::Reporter)), "{detail}");
    }
    let long = format!(r#"{{"why":"{}"}}"#, "x".repeat(MAX_DETAIL));
    let m = Move { from: State::Running, to: State::Failed, at: 61, detail: &long, by: Reporter::Control };
    assert!(matches!(db.move_run(&run.id, &m).unwrap(), Err(Refused::Detail(_))));
    let pending = create(&db, None);
    let m = Move { from: State::Pending, to: State::Canceled, at: 61, detail: r#"{"exit":0}"#, by: Reporter::Control };
    assert!(matches!(db.move_run(&pending.id, &m).unwrap(), Err(Refused::Detail(_))));
    unchanged(&run);

    // What a daemon wrote stays as written, so its signature still holds.
    let detail = r#"{ "exit": 0,  "note": "ok" }"#;
    let signed = arugula_e2e::run::Event {
        run: &run.id,
        seq: 3,
        from: Some(State::Running),
        to: State::Succeeded,
        at: 62,
        detail,
        daemon: &d.id,
    };
    let sig = signed.sign(&d.keys);
    let m = Move {
        from: State::Running,
        to: State::Succeeded,
        at: 62,
        detail,
        by: Reporter::Daemon { id: &d.id, seq: 3, sig: &sig },
    };
    let (_, ev) = db.move_run(&run.id, &m).unwrap().unwrap();
    assert_eq!(ev.detail, detail);
    assert!(ev.signed().unwrap().verify(&d.key(), &sig));
}

/// Two reporters moving one run at once (on Postgres, from two of the
/// platform's machines): one wins, the other is told what it is now.
#[test]
fn one_of_two_racing_moves_wins() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let second = db.second();
    let handles: Vec<&Db> = std::iter::once(&db).chain(second.as_ref()).collect();
    for round in 0..10 {
        let run = create(&db, None);
        let got: Vec<_> = std::thread::scope(|s| {
            [State::Canceled, State::Approved]
                .into_iter()
                .enumerate()
                .map(|(i, to)| {
                    let (db, id) = (handles[i % handles.len()], run.id.clone());
                    s.spawn(move || control(db, &id, State::Pending, to, round))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect()
        });
        let won: Vec<_> = got.iter().filter_map(|g| g.as_ref().ok()).collect();
        assert_eq!(won.len(), 1, "{got:?}");
        let lost = got.iter().find_map(|g| g.as_ref().err()).unwrap();
        assert_eq!(lost, &Refused::Moved { now: won[0].0.state });
        assert_eq!(db.run_events(&run.id).unwrap().len(), 2);
    }
}

/// No one changes an event, not even with SQL of their own.
#[test]
fn events_are_append_only() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let run = create(&db, None);
    let e = db.c().execute("UPDATE run_events SET to_state = 'succeeded' WHERE run_id = ?1", params![run.id]);
    let e = format!("{:#}", e.unwrap_err());
    assert!(e.contains("append-only"), "{e}");
    assert_eq!(db.run_events(&run.id).unwrap()[0].to, State::Pending);
    // The same insert twice is refused by the key.
    let ev = db.run_events(&run.id).unwrap().remove(0);
    assert!(add_event(&db.c(), &ev).is_err());
}

#[test]
fn a_run_is_made_pending_with_its_operation_hashed() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let source = json!({ "schedule": "sch_1" });
    let new =
        NewRun { trigger: TriggerKind::Schedule, started_by: "sch_1", ..new_run("prj_a", &source, Some("grt_1")) };
    let run = db.create_run(&new, 10).unwrap().unwrap();
    assert!(run.id.starts_with("run_") && run.id.len() == 36, "{}", run.id);
    assert_eq!(run.op_hash, hex::encode(Sha256::digest(OP.as_bytes())));
    assert_eq!(
        (run.state, run.trigger, &run.source, run.workspace.as_deref(), run.grant.as_deref()),
        (State::Pending, TriggerKind::Schedule, &source, Some("hw_1"), Some("grt_1"))
    );
    assert_eq!((run.last_seq, run.created_at, run.started_at, run.ended_at, run.exit_code), (1, 10, None, None, None));
    assert_eq!(db.run(&run.id).unwrap().unwrap(), run);
    let ev = db.run_events(&run.id).unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!((ev[0].from, ev[0].to, ev[0].daemon.as_ref()), (None, State::Pending, None));

    assert!(db.create_run(&new_run("prj_gone", &json!({}), None), 10).unwrap().is_none());
    assert!(db.create_run(&NewRun { op: "rm -rf /", ..new_run("prj_a", &json!({}), None) }, 10).is_err());
    assert!(db.create_run(&new_run("prj_a", &json!([1]), None), 10).is_err());
    assert!(db.run("run_nope").unwrap().is_none());

    let later = db.create_run(&new_run("prj_a", &json!({}), None), 20).unwrap().unwrap();
    let ids: Vec<_> = db.project_runs("prj_a", 10).unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, [later.id.clone(), run.id.clone()]);
    assert_eq!(db.project_runs("prj_a", 1).unwrap().len(), 1);

    assert!(db.link_chant_ledger(&run.id, "chant:release/42").unwrap());
    assert!(!db.link_chant_ledger(&run.id, "chant:release/43").unwrap(), "once");
    assert!(!db.link_chant_ledger("run_nope", "x").unwrap());
    assert_eq!(db.run(&run.id).unwrap().unwrap().chant_ledger.as_deref(), Some("chant:release/42"));
}

#[test]
fn artifacts_are_kept_by_name() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let run = create(&db, None);
    let a =
        db.add_run_artifact(&run.id, "report.html", 1234, "sha256:aa", "prj_a/run/report.html", 30).unwrap().unwrap();
    db.add_run_artifact(&run.id, "log.txt", u64::from(u32::MAX) + 1, "sha256:bb", "prj_a/run/log.txt", 31).unwrap();
    assert!(db.add_run_artifact(&run.id, "report.html", 1, "sha256:cc", "k", 32).unwrap_err().is::<Taken>());
    assert!(db.add_run_artifact("run_nope", "x", 1, "sha256:cc", "k", 32).unwrap().is_none());
    let got = db.run_artifacts(&run.id).unwrap();
    assert_eq!(got.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["log.txt", "report.html"]);
    assert_eq!(got[1], a);
    assert_eq!(got[0].size, u64::from(u32::MAX) + 1);
}

/// Deleting an account takes its projects' runs, with their events and
/// artifacts, and its name off the runs that stay.
#[test]
fn deleting_an_account_takes_its_runs_and_its_name() {
    let db = Db::test();
    team_project(&db, "prj_t");
    personal_project(&db, "prj_p", "alice");
    let d = Daemon::new();
    let mine = db
        .create_run(&NewRun { started_by: "alice", ..new_run("prj_p", &json!({}), Some("grt_1")) }, 10)
        .unwrap()
        .unwrap();
    d.report(&db, &mine.id, State::Pending, State::Running, 11).unwrap();
    db.add_run_artifact(&mine.id, "a", 1, "sha256:aa", "k", 12).unwrap();
    let theirs = db
        .create_run(
            &NewRun { started_by: "alice", started_by_agent: Some("geek"), ..new_run("prj_t", &json!({}), None) },
            10,
        )
        .unwrap()
        .unwrap();
    let scheduled = db
        .create_run(
            &NewRun { trigger: TriggerKind::Schedule, started_by: "sch_1", ..new_run("prj_t", &json!({}), None) },
            10,
        )
        .unwrap()
        .unwrap();
    let gone = Deleted { account: "alice".into(), machines: vec!["geek".into()], disbanded: vec![] };
    {
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        super::super::projects::erase_account(&tx, &gone).unwrap();
        erase_account(&tx, &gone).unwrap();
        erase_account(&tx, &gone).unwrap();
        tx.commit().unwrap();
    }
    assert!(db.run(&mine.id).unwrap().is_none());
    assert!(db.run_events(&mine.id).unwrap().is_empty());
    assert!(db.run_artifacts(&mine.id).unwrap().is_empty());
    let kept = db.run(&theirs.id).unwrap().unwrap();
    assert_eq!((kept.started_by.as_str(), kept.started_by_agent.as_deref()), ("", None));
    assert_eq!(db.run(&scheduled.id).unwrap().unwrap().started_by, "sch_1");
    assert_eq!(db.mentions("alice"), Vec::<String>::new());
}
