use arugula_e2e::DeviceKeys;
use serde_json::json;

use super::*;
use crate::db::Taken;

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

/// An S37 operation for `daemon`.
fn op(daemon: &str) -> String {
    format!("arugula op v1\ndaemon {daemon}\nkind run\nsession -\npane - -\ncwd -\nargv bWFrZQ==\n")
}

/// A hosted daemon, its key, and the operation its runs run.
struct Daemon {
    keys: DeviceKeys,
    id: String,
    op: String,
}

impl Daemon {
    fn new() -> Self {
        let keys = DeviceKeys::generate();
        let id = keys.id();
        let op = op(&id);
        Self { keys, id, op }
    }

    fn key(&self) -> String {
        hex::encode(self.keys.sign_public())
    }

    fn new_run<'a>(&'a self, project: &'a str, source: &'a serde_json::Value, grant: Option<&'a str>) -> NewRun<'a> {
        NewRun {
            project,
            workspace: Some("hw_1"),
            trigger: TriggerKind::Manual,
            source,
            op: &self.op,
            started_by: "a1",
            started_by_agent: None,
            grant,
        }
    }

    fn create(&self, db: &Db, grant: Option<&str>) -> Run {
        db.create_run(&self.new_run("prj_a", &json!({}), grant), 10).unwrap().unwrap()
    }

    /// This daemon's report on `run`, signed and checked, as event `seq`.
    fn signed(&self, run: &str, seq: u64, from: State, to: State, at: u64, detail: &str) -> DaemonReport {
        let ev = arugula_e2e::run::Event { run, seq, from: Some(from), to, at, detail, daemon: &self.id };
        DaemonReport::verify(&ev, &self.key(), &ev.sign(&self.keys)).unwrap()
    }

    /// Move `run` from `from` to `to` as this daemon reports it, as its
    /// next event.
    fn report(&self, db: &Db, run: &str, from: State, to: State, at: u64) -> Result<(Run, Event), Refused> {
        let seq = db.run(run).unwrap().map_or(1, |r| r.last_seq + 1);
        let detail = match to {
            State::Succeeded => r#"{"exit":0}"#,
            State::Failed if from == State::Running => r#"{"exit":2}"#,
            _ => "{}",
        };
        db.move_run(run, &Move::Daemon(&self.signed(run, seq, from, to, at, detail))).unwrap()
    }

    /// An owner's approval of this daemon's operation.
    fn approval(&self) -> Approval {
        Approval::checked("apv_1", &self.id, &hex::encode(Sha256::digest(self.op.as_bytes())), "owner-phone")
    }

    fn approve(&self, db: &Db, run: &str, from: State, at: u64) -> Result<(Run, Event), Refused> {
        db.move_run(run, &Move::Approved { from, at, approval: &self.approval() }).unwrap()
    }
}

fn control(db: &Db, run: &str, from: State, to: State, at: u64) -> Result<(Run, Event), Refused> {
    let detail = if to == State::Failed { r#"{"reason":"the workspace is gone"}"# } else { "{}" };
    db.move_run(run, &Move::Control { from, to, at, detail }).unwrap()
}

/// Move `run` from `from` to `to` the way that state is reported: the
/// daemon for what only it reports and for a running run's end, an
/// approval for `approved`, else control.
fn step(db: &Db, d: &Daemon, run: &str, from: State, to: State, at: u64) -> Result<(Run, Event), Refused> {
    match to {
        State::Approved => d.approve(db, run, from, at),
        State::Failed if from == State::Running => d.report(db, run, from, to, at),
        to if to.daemon_only() => d.report(db, run, from, to, at),
        _ => control(db, run, from, to, at),
    }
}

/// A run with a standing grant, brought to `state` the shortest way.
fn run_in(db: &Db, d: &Daemon, state: State) -> Run {
    use State::*;
    let run = d.create(db, Some("grt_1"));
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
                let exit = match (from, to) {
                    (State::Running, State::Succeeded) => Some(0),
                    (State::Running, State::Failed) => Some(2),
                    _ => None,
                };
                assert_eq!(after.exit_code, exit, "{from} to {to}");
                match ev.signed() {
                    Some(signed) => {
                        assert!(to.daemon_only() || from == State::Running, "{from} to {to}");
                        assert!(signed.verify(&d.key(), ev.sig.as_deref().unwrap()), "{from} to {to}");
                    }
                    None => assert!(!to.daemon_only() && ev.sig.is_none()),
                }
                if to == State::Approved {
                    let detail: serde_json::Value = serde_json::from_str(&ev.detail).unwrap();
                    assert_eq!(detail, json!({ "approval": "apv_1", "op": run.op_hash, "by": "owner-phone" }));
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

/// Running and succeeded only from the run's own daemon; approved only
/// with an approval of the run's operation; denied and canceled only from
/// control; expired and failed from either.
#[test]
fn each_state_has_its_reporters() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    for (from, to) in [(State::Approved, State::Running), (State::Running, State::Succeeded)] {
        let run = run_in(&db, &d, from);
        assert_eq!(control(&db, &run.id, from, to, 50).unwrap_err(), Refused::Reporter, "{to}");
        let detail = if to == State::Succeeded { r#"{"exit":0}"# } else { "{}" };
        // Another daemon's report, signed and checked, but not the run's.
        let other = Daemon::new().signed(&run.id, run.last_seq + 1, from, to, 50, detail);
        assert_eq!(db.move_run(&run.id, &Move::Daemon(&other)).unwrap().unwrap_err(), Refused::OtherDaemon);
        // The run's daemon's report about another run.
        let elsewhere = d.signed("run_other", run.last_seq + 1, from, to, 50, detail);
        assert_eq!(db.move_run(&run.id, &Move::Daemon(&elsewhere)).unwrap().unwrap_err(), Refused::OtherDaemon);
        assert_eq!(db.run(&run.id).unwrap().unwrap(), run);
    }
    for to in [State::Denied, State::Canceled] {
        let run = d.create(&db, None);
        assert_eq!(d.report(&db, &run.id, State::Pending, to, 50).unwrap_err(), Refused::Reporter, "{to}");
    }
    let run = d.create(&db, None);
    assert_eq!(control(&db, &run.id, State::Pending, State::Approved, 50).unwrap_err(), Refused::Reporter);
    assert_eq!(control(&db, &run.id, State::Pending, State::Running, 50).unwrap_err(), Refused::Reporter);
    assert_eq!(d.report(&db, &run.id, State::Pending, State::Expired, 50).unwrap().0.state, State::Expired);
    let run = run_in(&db, &d, State::Running);
    let (failed, ev) = control(&db, &run.id, State::Running, State::Failed, 50).unwrap();
    assert_eq!((failed.exit_code, ev.daemon), (None, None), "control's failure has a reason, not an exit code");
    let run = d.create(&db, None);
    assert_eq!(control(&db, &run.id, State::Pending, State::Failed, 50).unwrap().0.exit_code, None);
}

/// An approval of another operation, or for another daemon, approves
/// nothing.
#[test]
fn an_approval_must_be_of_the_runs_operation() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let run = d.create(&db, None);
    let other_op = op(&d.id).replace("bWFrZQ==", "cm0=");
    for wrong in [
        Approval::checked("apv_2", &d.id, &hex::encode(Sha256::digest(other_op.as_bytes())), "owner-phone"),
        Approval::checked("apv_3", "another-daemon", &run.op_hash, "owner-phone"),
    ] {
        let m = Move::Approved { from: State::Pending, at: 50, approval: &wrong };
        assert_eq!(db.move_run(&run.id, &m).unwrap().unwrap_err(), Refused::Approval);
    }
    assert_eq!(db.run(&run.id).unwrap().unwrap(), run);
    assert_eq!(d.approve(&db, &run.id, State::Pending, 51).unwrap().0.state, State::Approved);
}

/// Straight to running needs the standing grant; an approved run doesn't.
#[test]
fn pending_to_running_needs_a_grant() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let run = d.create(&db, None);
    assert_eq!(d.report(&db, &run.id, State::Pending, State::Running, 50).unwrap_err(), Refused::NoGrant);
    assert_eq!(db.run_events(&run.id).unwrap().len(), 1);
    d.approve(&db, &run.id, State::Pending, 51).unwrap();
    let (now, _) = d.report(&db, &run.id, State::Approved, State::Running, 52).unwrap();
    assert_eq!((now.state, now.started_at), (State::Running, Some(52)));
}

/// A move from a state the run has left, a report that isn't next (a
/// replay), a signature that doesn't hold, a detail that doesn't fit: each
/// refused, nothing written.
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
    let stored = &db.run_events(&run.id).unwrap()[1];
    let replay = DaemonReport::verify(&stored.signed().unwrap(), &d.key(), stored.sig.as_deref().unwrap()).unwrap();
    assert_eq!(
        db.move_run(&run.id, &Move::Daemon(&replay)).unwrap().unwrap_err(),
        Refused::Moved { now: State::Running }
    );
    let ahead = d.signed(&run.id, 2, State::Running, State::Succeeded, 60, r#"{"exit":0}"#);
    assert_eq!(db.move_run(&run.id, &Move::Daemon(&ahead)).unwrap().unwrap_err(), Refused::Seq { next: 3 });
    unchanged(&run);

    // A report whose signature doesn't hold never becomes one.
    let ev = arugula_e2e::run::Event {
        run: &run.id,
        seq: 3,
        from: Some(State::Running),
        to: State::Succeeded,
        at: 61,
        detail: r#"{"exit":0}"#,
        daemon: &d.id,
    };
    let sig = ev.sign(&d.keys);
    assert!(DaemonReport::verify(&ev, &Daemon::new().key(), &sig).is_none(), "another key");
    assert!(DaemonReport::verify(&arugula_e2e::run::Event { at: 62, ..ev }, &d.key(), &sig).is_none(), "changed");
    assert!(DaemonReport::verify(&arugula_e2e::run::Event { from: None, ..ev }, &d.key(), &sig).is_none());

    // Exit codes: the daemon's, and only on a running run's end.
    for (to, detail) in [
        (State::Succeeded, r#"{"exit":1}"#),
        (State::Succeeded, "{}"),
        (State::Failed, r#"{"exit":0}"#),
        (State::Failed, r#"{"exit":"2"}"#),
        (State::Failed, "[]"),
        (State::Failed, "not json"),
    ] {
        let r = d.signed(&run.id, 3, State::Running, to, 61, detail);
        assert!(matches!(db.move_run(&run.id, &Move::Daemon(&r)).unwrap(), Err(Refused::Detail(_))), "{detail}");
    }
    for detail in [r#"{"exit":2,"reason":"x"}"#, "{}", r#"{"reason":""}"#, "[]"] {
        let m = Move::Control { from: State::Running, to: State::Failed, at: 61, detail };
        assert!(matches!(db.move_run(&run.id, &m).unwrap(), Err(Refused::Detail(_))), "{detail}");
    }
    let long = format!(r#"{{"reason":"{}"}}"#, "x".repeat(MAX_DETAIL));
    let m = Move::Control { from: State::Running, to: State::Failed, at: 61, detail: &long };
    assert!(matches!(db.move_run(&run.id, &m).unwrap(), Err(Refused::Detail(_))));
    unchanged(&run);
    // Failing the check at the start never ran, so it has no exit code.
    for from in [State::Pending, State::Approved] {
        let other = run_in(&db, &d, from);
        let r = d.signed(&other.id, other.last_seq + 1, from, State::Failed, 61, r#"{"exit":2}"#);
        assert!(matches!(db.move_run(&other.id, &Move::Daemon(&r)).unwrap(), Err(Refused::Detail(_))), "{from}");
        let r = d.signed(&other.id, other.last_seq + 1, from, State::Failed, 61, r#"{"check":"op"}"#);
        assert_eq!(db.move_run(&other.id, &Move::Daemon(&r)).unwrap().unwrap().0.exit_code, None);
    }

    // What a daemon wrote stays as written, so its signature still holds.
    let detail = r#"{ "exit": 0,  "note": "ok" }"#;
    let (_, ev) = db
        .move_run(&run.id, &Move::Daemon(&d.signed(&run.id, 3, State::Running, State::Succeeded, 62, detail)))
        .unwrap()
        .unwrap();
    assert_eq!(ev.detail, detail);
    assert!(ev.signed().unwrap().verify(&d.key(), ev.sig.as_deref().unwrap()));
}

/// Two reporters moving one run at once (on Postgres, from two of the
/// platform's machines): one wins, the other is told what it is now.
#[test]
fn one_of_two_racing_moves_wins() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let second = db.second();
    let handles: Vec<&Db> = std::iter::once(&db).chain(second.as_ref()).collect();
    for round in 0..10 {
        let run = d.create(&db, None);
        let got: Vec<_> = std::thread::scope(|s| {
            [State::Canceled, State::Approved]
                .into_iter()
                .enumerate()
                .map(|(i, to)| {
                    let (db, id, d) = (handles[i % handles.len()], run.id.clone(), &d);
                    s.spawn(move || step(db, d, &id, State::Pending, to, round))
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

/// No one changes an event, or deletes one while its run is there, not
/// even with SQL of their own. Deleting the run first lets its events go.
#[test]
fn events_are_append_only() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let run = d.create(&db, None);
    let refused = |sql: &str| {
        let e = db.c().execute(sql, params![run.id]).unwrap_err();
        let e = format!("{e:#}");
        assert!(e.contains("append-only"), "{e}");
    };
    refused("UPDATE run_events SET to_state = 'succeeded' WHERE run_id = ?1");
    refused("DELETE FROM run_events WHERE run_id = ?1");
    assert_eq!(db.run_events(&run.id).unwrap()[0].to, State::Pending);
    // The same insert twice is refused by the key.
    let ev = db.run_events(&run.id).unwrap().remove(0);
    assert!(add_event(&db.c(), &ev).is_err());
    db.c().execute("DELETE FROM runs WHERE id = ?1", params![run.id]).unwrap();
    db.c().execute("DELETE FROM run_events WHERE run_id = ?1", params![run.id]).unwrap();
    assert!(db.run_events(&run.id).unwrap().is_empty());
}

#[test]
fn a_run_is_made_pending_with_its_operation_hashed() {
    let db = Db::test();
    team_project(&db, "prj_a");
    let d = Daemon::new();
    let source = json!({ "schedule": "sch_1" });
    let new =
        NewRun { trigger: TriggerKind::Schedule, started_by: "sch_1", ..d.new_run("prj_a", &source, Some("grt_1")) };
    let run = db.create_run(&new, 10).unwrap().unwrap();
    assert!(run.id.starts_with("run_") && run.id.len() == 36, "{}", run.id);
    assert_eq!(run.op_hash, hex::encode(Sha256::digest(d.op.as_bytes())));
    assert_eq!(run.daemon, d.id, "the daemon its operation names");
    assert_eq!(
        (run.state, run.trigger, &run.source, run.workspace.as_deref(), run.grant.as_deref()),
        (State::Pending, TriggerKind::Schedule, &source, Some("hw_1"), Some("grt_1"))
    );
    assert_eq!((run.last_seq, run.created_at, run.started_at, run.ended_at, run.exit_code), (1, 10, None, None, None));
    assert_eq!(db.run(&run.id).unwrap().unwrap(), run);
    let ev = db.run_events(&run.id).unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!((ev[0].from, ev[0].to, ev[0].daemon.as_ref()), (None, State::Pending, None));

    assert!(db.create_run(&d.new_run("prj_gone", &json!({}), None), 10).unwrap().is_none());
    for bad in ["rm -rf /", "arugula op v1\nkind run\n", "arugula op v1\ndaemon \n", "arugula op v1\ndaemon a b\n"] {
        assert!(db.create_run(&NewRun { op: bad, ..d.new_run("prj_a", &json!({}), None) }, 10).is_err(), "{bad}");
    }
    assert!(db.create_run(&d.new_run("prj_a", &json!([1]), None), 10).is_err());
    assert!(db.run("run_nope").unwrap().is_none());

    let later = db.create_run(&d.new_run("prj_a", &json!({}), None), 20).unwrap().unwrap();
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
    let run = Daemon::new().create(&db, None);
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
        .create_run(&NewRun { started_by: "alice", ..d.new_run("prj_p", &json!({}), Some("grt_1")) }, 10)
        .unwrap()
        .unwrap();
    d.report(&db, &mine.id, State::Pending, State::Running, 11).unwrap();
    db.add_run_artifact(&mine.id, "a", 1, "sha256:aa", "k", 12).unwrap();
    let theirs = db
        .create_run(
            &NewRun { started_by: "alice", started_by_agent: Some("geek"), ..d.new_run("prj_t", &json!({}), None) },
            10,
        )
        .unwrap()
        .unwrap();
    let scheduled = db
        .create_run(
            &NewRun { trigger: TriggerKind::Schedule, started_by: "sch_1", ..d.new_run("prj_t", &json!({}), None) },
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
    assert_eq!(db.run_events(&theirs.id).unwrap().len(), 1, "the runs that stay keep their history");
    assert_eq!(db.run(&scheduled.id).unwrap().unwrap().started_by, "sch_1");
    assert_eq!(db.mentions("alice"), Vec::<String>::new());
}
