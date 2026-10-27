//! Runs' dispatch to hosted machines (M212): a run is ready only for the
//! hosted machine its operation names, and only once its job has checked
//! that; a person's machine gets nothing and the run fails, saying so; the
//! machine's signed reports move it once, a retry is told it was kept, and
//! anyone else's report is refused; a paused box is woken; a run nobody
//! takes fails. On SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::SeqCst},
    },
    time::Duration,
};

use arugula_control_wire::work::{Report, Reported, TAKES_RUN};
use arugula_e2e::DeviceKeys;
use serde_json::json;

use super::*;
use crate::{
    chat_tests::TestHub,
    db::{
        runs::{NewRun, TriggerKind},
        sql::{Sql as _, params},
    },
    jobs::Worker,
};

/// A hosted machine: its keys, and what the hub says of it.
pub(crate) struct Machine {
    pub(crate) keys: DeviceKeys,
    pub(crate) m: HostedMachine,
}

impl Machine {
    pub(crate) fn new(project: &str, workspace: &str) -> Self {
        let keys = DeviceKeys::generate();
        let m = HostedMachine {
            project: project.into(),
            workspace: workspace.into(),
            machine: keys.id(),
            sign: hex::encode(keys.sign_public()),
        };
        Self { keys, m }
    }

    /// Its report of `run` moving `from` `to`, as event `seq`.
    pub(crate) fn report(&self, run: &str, seq: u64, from: State, to: State, detail: &str) -> Report {
        let ev = arugula_e2e::run::Event { run, seq, from: Some(from), to, at: 50, detail, daemon: &self.m.machine };
        Report {
            run: run.into(),
            seq,
            from: from.as_str().into(),
            to: to.as_str().into(),
            at: 50,
            detail: detail.into(),
            sig: ev.sign(&self.keys),
        }
    }
}

pub(crate) struct World {
    pub(crate) app: Arc<Platform>,
    pub(crate) project: String,
    pub(crate) clock: Arc<AtomicU64>,
    pub(crate) worker: Worker,
}

impl World {
    pub(crate) fn new() -> Self {
        let hub = Arc::new(TestHub::default());
        let p = Platform::new(Default::default(), Db::test(), hub, reqwest::Client::new(), Default::default());
        let app = Arc::new(p);
        let project = app.db.team_project("t1", "t1", 0).unwrap();
        let clock = Arc::new(AtomicU64::new(1_000));
        let read = clock.clone();
        let worker = Worker::new()
            .with(Dispatch { app: app.clone() })
            .clock(Clock::new(move || read.load(SeqCst)))
            .heartbeat(Duration::from_secs(60));
        Self { app, project, clock, worker }
    }

    /// A hosted workspace whose box enrolled as `m`, in `state`.
    pub(crate) fn workspace(&self, id: &str, m: &Machine, state: &str) {
        let made =
            self.app.db.add_hosted_workspace(id, &self.project, id, &Default::default(), "alice", 10, 1).unwrap();
        assert!(matches!(made, crate::db::hosted::Added::Made(_)));
        self.app
            .db
            .c()
            .execute(
                "UPDATE hosted_workspaces SET state = ?2, machine = ?3 WHERE id = ?1",
                params![id, state, m.m.machine],
            )
            .unwrap();
    }

    /// A run of `echo hi` on `daemon`, in workspace `ws`, under a grant
    /// (or not).
    pub(crate) fn run(&self, daemon: &str, ws: &str, grant: Option<&str>) -> String {
        let op =
            format!("arugula op v1\ndaemon {daemon}\nkind run\nsession -\npane - -\ncwd -\nargv ZWNobw==\nargv aGk=\n");
        let new = NewRun {
            project: &self.project,
            workspace: Some(ws),
            trigger: TriggerKind::Manual,
            source: &json!({}),
            op: &op,
            started_by: "alice",
            started_by_agent: None,
            grant,
        };
        self.app.db.create_run(&new, 10).unwrap().unwrap().id
    }

    /// Every step due now.
    pub(crate) async fn settle(&self) {
        for _ in 0..20 {
            if !self.worker.tick(&self.app.db).await.unwrap() {
                return;
            }
        }
        panic!("the worker never settled");
    }

    pub(crate) fn at(&self, ms: u64) {
        self.clock.store(ms, SeqCst);
    }

    pub(crate) async fn ready(&self, m: &Machine) -> Vec<String> {
        work(&self.app, &m.m, &[TAKES_RUN.into()]).await.unwrap().items.into_iter().map(|i| i.run).collect()
    }

    pub(crate) fn state(&self, run: &str) -> State {
        self.app.db.run(run).unwrap().unwrap().state
    }

    pub(crate) fn job(&self, run: &str) -> String {
        self.app.db.platform_job(run).unwrap().unwrap().state
    }
}

#[tokio::test]
async fn a_run_is_ready_only_for_its_hosted_machine() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    // Not until its job has looked.
    assert!(w.ready(&m).await.is_empty());
    assert!(crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap());
    assert!(!crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap(), "one job a run");
    w.settle().await;
    let items = work(&w.app, &m.m, &[TAKES_RUN.into()]).await.unwrap().items;
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!((item.run.as_str(), item.workspace.as_str(), item.state.as_str()), (run.as_str(), "hw_1", "pending"));
    assert_eq!(item.last_seq, 1);
    let r = w.app.db.run(&run).unwrap().unwrap();
    assert_eq!((&item.op, &item.op_hash), (&r.op, &r.op_hash));
    // Asked again, it comes again: the machine's ledger runs it once.
    assert_eq!(w.ready(&m).await, std::slice::from_ref(&run));
    // Nobody else gets it: another machine in the workspace's place,
    // another workspace, a machine that doesn't say it takes runs.
    let other = Machine::new(&w.project, "hw_1");
    assert!(w.ready(&other).await.is_empty());
    let mut elsewhere = m.m.clone();
    elsewhere.workspace = "hw_2".into();
    assert!(work(&w.app, &elsewhere, &[TAKES_RUN.into()]).await.unwrap().items.is_empty());
    assert!(work(&w.app, &m.m, &[]).await.unwrap().items.is_empty());

    // A pending run with no standing grant waits for an approval (M218).
    let unapproved = w.run(&m.m.machine, "hw_1", None);
    crate::db::runs::dispatch(&w.app.db.c(), &unapproved, 1_000).unwrap();
    w.settle().await;
    assert_eq!(w.ready(&m).await, std::slice::from_ref(&run));
    assert_eq!(w.state(&unapproved), State::Pending);
}

/// A run whose operation names a machine that isn't its workspace's
/// hosted machine (a person's own) never reaches it: it fails, saying so.
#[tokio::test]
async fn a_personal_machine_gets_no_work() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let mine = DeviceKeys::generate().id();
    let run = w.run(&mine, "hw_1", Some("g1"));
    let nowhere = w.run(&mine, "hw_none", Some("g1"));
    for r in [&run, &nowhere] {
        crate::db::runs::dispatch(&w.app.db.c(), r, 1_000).unwrap();
    }
    w.settle().await;
    for r in [&run, &nowhere] {
        assert_eq!(w.state(r), State::Failed);
        assert_eq!(w.job(r), "done");
        let events = w.app.db.run_events(r).unwrap();
        let last = events.last().unwrap();
        assert!(last.detail.contains("not_hosted"), "{}", last.detail);
        assert_eq!(last.daemon, None, "control's");
    }
    assert!(w.ready(&m).await.is_empty());
    // The person's machine, even if it could ask, isn't the workspace's.
    let personal =
        HostedMachine { project: w.project.clone(), workspace: "hw_1".into(), machine: mine, sign: String::new() };
    assert!(work(&w.app, &personal, &[TAKES_RUN.into()]).await.unwrap().items.is_empty());
}

#[tokio::test]
async fn a_machines_signed_reports_move_its_run_once() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let other = Machine::new(&w.project, "hw_2");
    w.workspace("hw_2", &other, "ready");
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap();
    w.settle().await;

    let running = m.report(&run, 2, State::Pending, State::Running, r#"{"pane":3,"epoch":1}"#);
    // Not its own signature; another hosted machine's, about this run.
    let mut forged = running.clone();
    forged.sig = other.report(&run, 2, State::Pending, State::Running, &running.detail).sig;
    assert!(matches!(report(&w.app, &m.m, &forged).await.unwrap(), Reported::Refused { .. }));
    let theirs = other.report(&run, 2, State::Pending, State::Running, "{}");
    assert!(matches!(report(&w.app, &other.m, &theirs).await.unwrap(), Reported::Refused { .. }));
    // Not the next event.
    let late = m.report(&run, 5, State::Pending, State::Running, "{}");
    assert_eq!(
        report(&w.app, &m.m, &late).await.unwrap(),
        Reported::Refused { why: "the run's next event is 2".into(), now: None, next: Some(2) }
    );
    assert_eq!(w.state(&run), State::Pending);

    assert_eq!(report(&w.app, &m.m, &running).await.unwrap(), Reported::Taken);
    assert_eq!(report(&w.app, &m.m, &running).await.unwrap(), Reported::Already, "a retry");
    assert_eq!(w.state(&run), State::Running);
    assert!(w.ready(&m).await.is_empty(), "it started: no more deliveries");
    w.settle().await;
    assert_eq!(w.job(&run), "pending", "the job follows it while it runs");

    let done = m.report(&run, 3, State::Running, State::Succeeded, r#"{"exit":0}"#);
    assert_eq!(report(&w.app, &m.m, &done).await.unwrap(), Reported::Taken);
    let r = w.app.db.run(&run).unwrap().unwrap();
    assert_eq!((r.state, r.exit_code), (State::Succeeded, Some(0)));
    // Its events carry the machine's signatures, which anyone can check.
    let events = w.app.db.run_events(&run).unwrap();
    for e in &events[1..] {
        let signed = e.signed().unwrap();
        assert!(signed.verify(&m.m.sign, e.sig.as_deref().unwrap()));
    }
    w.at(1_000 + WAITING_MS + RUNNING_MS);
    w.settle().await;
    assert_eq!(w.job(&run), "done");

    // A report after it ended: refused, saying where it is.
    let again = m.report(&run, 4, State::Running, State::Failed, r#"{"exit":1}"#);
    assert!(matches!(
        report(&w.app, &m.m, &again).await.unwrap(),
        Reported::Refused { now: Some(s), .. } if s == "succeeded"
    ));
}

#[tokio::test]
async fn a_waiting_run_wakes_its_box_and_fails_if_nobody_takes_it() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "paused");
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap();
    w.settle().await;
    // M182's wake, asked beside the step.
    let mut asked = None;
    for _ in 0..100 {
        asked = w.app.db.hosted_workspace(&w.project, "hw_1").unwrap().unwrap().asked;
        if asked.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(asked.as_deref(), Some("wake"));
    // Ready for it, but its box never asks.
    assert_eq!(w.app.db.ready_runs(&w.project, "hw_1", &m.m.machine, 10).unwrap(), std::slice::from_ref(&run));

    w.at(1_000 + TAKE_MS + WAITING_MS + 1);
    w.settle().await;
    assert_eq!(w.state(&run), State::Failed);
    assert_eq!(w.job(&run), "done");
    let last = w.app.db.run_events(&run).unwrap().pop().unwrap();
    assert!(last.detail.contains("not_taken"), "{}", last.detail);
    assert!(w.ready(&m).await.is_empty());
}

/// A box deleted while its run runs: the run fails, and the job ends.
#[tokio::test]
async fn a_run_whose_box_went_fails() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap();
    w.settle().await;
    let running = m.report(&run, 2, State::Pending, State::Running, "{}");
    assert_eq!(report(&w.app, &m.m, &running).await.unwrap(), Reported::Taken);
    w.app.db.c().execute("UPDATE hosted_workspaces SET state = 'deleting' WHERE id = 'hw_1'", params![]).unwrap();
    w.at(1_000 + RUNNING_MS + WAITING_MS);
    w.settle().await;
    assert_eq!(w.state(&run), State::Failed);
    assert_eq!(w.job(&run), "done");
}

/// C4: every pull carries the project's revokes, ready runs or not, so a
/// machine stops a run going under a grant revoked with `running stop`.
#[tokio::test]
async fn every_pull_carries_the_projects_revokes() {
    use arugula_e2e::{
        hosted::{Delegation, HostedKey},
        project_grant::{GrantRevoke, ProjectGrant, Running},
    };
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let owner = DeviceKeys::generate();
    let mut d = Delegation {
        project: w.project.clone(),
        owner: "team:t1".into(),
        key: HostedKey::from_seed(&[1; 32]).public(),
        at: 1,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(&owner);
    let now = arugula_e2e::now_ms();
    let mut g = ProjectGrant::new(&d, now, Some(now + 86_400_000));
    g.sign_with(&owner);
    w.app.db.add_run_grant(&g, "alice", now).unwrap();
    assert!(work(&w.app, &m.m, &[TAKES_RUN.into()]).await.unwrap().revokes.is_empty());
    let r = GrantRevoke::new(&w.project, &g.delegation, Running::Stop, now + 1, &owner);
    w.app.db.revoke_run_grants(&r, now + 1).unwrap();
    let got = work(&w.app, &m.m, &[TAKES_RUN.into()]).await.unwrap();
    assert!(got.items.is_empty(), "no runs ready");
    assert_eq!(got.revokes, [r]);
}

/// C9: a run its machine keeps asking for (busy with another run) isn't
/// failed as never taken; one nobody asked for in [`TAKE_MS`] is. A pull
/// never overwrites a step's checkpoint while the step holds the job.
#[tokio::test]
async fn a_run_its_machine_keeps_pulling_isnt_failed_as_not_taken() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let pulled = w.run(&m.m.machine, "hw_1", Some("g1"));
    let ignored = w.run(&m.m.machine, "hw_1", Some("g1"));
    for r in [&pulled, &ignored] {
        crate::db::runs::dispatch(&w.app.db.c(), r, 1_000).unwrap();
    }
    w.settle().await;
    // Pulled now and then, as a busy machine does; the other never.
    let mut at = 1_000;
    while at < 1_000 + 2 * TAKE_MS {
        at += TAKE_MS / 3;
        w.app.db.note_pulled(&pulled, at).unwrap();
        w.at(at + 1);
        w.settle().await;
    }
    assert_eq!(w.state(&pulled), State::Pending);
    assert_eq!(w.state(&ignored), State::Failed);
    let last = w.app.db.run_events(&ignored).unwrap().pop().unwrap();
    assert!(last.detail.contains("not_taken"), "{}", last.detail);
    // Its checkpoint says when it was last pulled, and when it was ready.
    let cp: Checkpoint =
        serde_json::from_value(w.app.db.platform_job(&pulled).unwrap().unwrap().checkpoint.unwrap()).unwrap();
    assert_eq!((cp.ready_at, cp.pulled_at), (1_000, Some(at)));
    // Held by a step: a pull leaves the checkpoint alone.
    let held = w.app.db.claim_job(&[crate::db::jobs::JobKind::Run], at + WAITING_MS + 10).unwrap();
    assert!(held.is_some());
    let before = w.app.db.platform_job(&pulled).unwrap().unwrap().checkpoint;
    w.app.db.note_pulled(&pulled, at + WAITING_MS + 20).unwrap();
    assert_eq!(w.app.db.platform_job(&pulled).unwrap().unwrap().checkpoint, before);
}

/// A `ready` box whose daemon hasn't asked for its run in [`QUIET_MS`] may
/// have paused itself: the job wakes it through M182's `hosted::wake`. One
/// whose daemon keeps pulling isn't woken.
#[tokio::test]
async fn a_quiet_ready_box_is_woken_and_a_pulling_one_isnt() {
    let w = World::new();
    let m = Machine::new(&w.project, "hw_1");
    w.workspace("hw_1", &m, "ready");
    let run = w.run(&m.m.machine, "hw_1", Some("g1"));
    crate::db::runs::dispatch(&w.app.db.c(), &run, 1_000).unwrap();
    w.settle().await;
    let asked = |w: &World| w.app.db.hosted_workspace(&w.project, "hw_1").unwrap().unwrap().asked;
    w.app.db.note_pulled(&run, 1_000 + QUIET_MS).unwrap();
    w.at(1_000 + QUIET_MS + WAITING_MS);
    w.settle().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(asked(&w), None, "it's pulling");
    w.at(1_000 + 3 * QUIET_MS);
    w.settle().await;
    let mut got = None;
    for _ in 0..100 {
        got = asked(&w);
        if got.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(got.as_deref(), Some("wake"));
}

/// A pulled prompt says who wrote it by login (the "from" note's), else by
/// the name they chose, and so does an action (its "Answered by"); GitHub's
/// text and an account the hub doesn't know are left as they are.
#[test]
fn a_pulled_prompt_names_its_author() {
    use arugula_control_wire::work::{SessionAction, SessionPrompt, SessionWork};
    let p = |author: &str, source: &str| SessionPrompt {
        id: format!("spr_{author}"),
        author: author.into(),
        author_name: String::new(),
        source: source.into(),
        association: None,
        text: "hi".into(),
        at: 1,
    };
    let mut sessions = vec![SessionWork {
        session: "ses_1".into(),
        project: "prj_1".into(),
        workspace: "hw_1".into(),
        asker: "acct_jake".into(),
        repo: None,
        branch: "arugula/1-x".into(),
        state: "queued".into(),
        n: 0,
        prompts: vec![
            p("acct_jake", "member"),
            p("acct_ann", "member"),
            p("acct_gone", "member"),
            p("github:o", "github"),
        ],
        actions: ["acct_jake", "acct_ann", "acct_gone"]
            .into_iter()
            .map(|by| SessionAction {
                id: format!("sac_{by}"),
                by: by.into(),
                by_name: String::new(),
                kind: "answer".into(),
                request: None,
                answer: None,
                option: None,
                reason: None,
                at: 1,
            })
            .collect(),
        finish: None,
        harness: None,
        review: false,
        model: None,
    }];
    let account = |id: &str, login: &str, name: &str| {
        (id.to_owned(), crate::hub::Account { id: id.into(), login: login.into(), name: name.into() })
    };
    let accounts = [account("acct_jake", "jhgaylor", "Jake"), account("acct_ann", "", "Ann")].into_iter().collect();
    super::name_authors(&mut sessions, &accounts);
    let named: Vec<&str> = sessions[0].prompts.iter().map(|p| p.who()).collect();
    assert_eq!(named, ["jhgaylor", "Ann", "acct_gone", "github:o"]);
    // An action's asker, for the box's "Answered by" note, the same way.
    let by: Vec<&str> = sessions[0].actions.iter().map(|a| a.who()).collect();
    assert_eq!(by, ["jhgaylor", "Ann", "acct_gone"]);
}
