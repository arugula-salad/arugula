//! planter's drivers' platform half (M233): only planter's App mints for
//! them, and what their runs report is checked by shape before it's kept.

use arugula_e2e::deploy::planter as p;
use serde_json::json;

use super::*;
use crate::{
    db::{
        deploys::{SourceKind, State},
        runs::{State as RunState, TriggerKind},
    },
    drivers::driver,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn env(name: &str, settings: &[(&str, &str)]) -> Environment {
    let given = settings.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    Environment {
        id: "env_1".into(),
        project: "prj_1".into(),
        name: "production".into(),
        driver: name.into(),
        secret_names: vec![],
        settings: arugula_e2e::drivers::settings(name, &given).unwrap(),
        created_at: 1,
        updated_at: 1,
    }
}

fn run(exit: Option<i64>) -> Run {
    Run {
        id: "run_1".into(),
        project: "prj_1".into(),
        workspace: None,
        trigger: TriggerKind::Manual,
        source: json!({}),
        op: String::new(),
        op_hash: String::new(),
        daemon: "d".into(),
        state: RunState::Succeeded,
        started_by: "a".into(),
        started_by_agent: None,
        grant: None,
        last_seq: 3,
        created_at: 1,
        started_at: None,
        ended_at: None,
        exit_code: exit,
        chant_ledger: None,
    }
}

fn deploy(kind: SourceKind, reference: &str, place: &str) -> Deploy {
    Deploy {
        id: "dep_1".into(),
        environment: "env_1".into(),
        source: Source { kind, reference: reference.into(), place: place.into() },
        plan_digest: "d".into(),
        state: State::Running,
        planned_seq: 1,
        run: Some("run_1".into()),
        planned_by: "a".into(),
        planned_by_agent: None,
        subject: None,
        expected_version: None,
        created_at: 1,
        updated_at: 1,
    }
}

/// Every token planter's drivers' runs get is planter's App's, for one
/// named repository or the deploy's own (linked) one, with no more than its
/// step needs; none is the platform's own App's, and none writes what the
/// plan doesn't say.
#[test]
fn planters_drivers_mint_only_from_planters_app() {
    use crate::{drivers::mint::PlanterRepo, forge::DeployScope};
    type Mints = Vec<(&'static str, PlanterRepo, DeployScope)>;
    let want: [(&str, Mints); 4] = [
        (p::PROD, vec![(p::APP_TOKEN, PlanterRepo::Source, DeployScope::Scaffold)]),
        (
            p::GO_LIVE,
            vec![
                (p::POINTERS_TOKEN, PlanterRepo::Named("arugula-salad/planter-apps"), DeployScope::Contents),
                (p::SOURCE_TOKEN, PlanterRepo::Source, DeployScope::Read),
            ],
        ),
        (
            p::SHIP,
            vec![
                (p::SHIPPED_TOKEN, PlanterRepo::Named("arugula-salad/shipped"), DeployScope::Contents),
                (p::POINTERS_TOKEN, PlanterRepo::Named("arugula-salad/planter-apps"), DeployScope::Contents),
            ],
        ),
        (p::PROMOTE, vec![(p::SHIPPED_TOKEN, PlanterRepo::Named("arugula-salad/shipped"), DeployScope::Pulls)]),
    ];
    for (name, mints) in want {
        let settings: &[(&str, &str)] = if [p::SHIP, p::PROMOTE].contains(&name) { &[("app", "hello")] } else { &[] };
        let d = driver(name).unwrap_or_else(|| panic!("{name} is a driver"));
        let got: Vec<_> = d
            .mints(&env(name, settings))
            .into_iter()
            .map(|m| match m {
                Mint::Planter { name, repo, scope } => (name, repo, scope),
                other => panic!("{name}: {other:?} isn't planter's App's"),
            })
            .collect();
        assert_eq!(got, mints, "{name}");
        // What the box is handed is what the driver takes: these tokens only.
        let mut names: Vec<&str> = got.iter().map(|(n, ..)| *n).collect();
        names.sort_unstable();
        let mut takes = arugula_e2e::deploy::driver_secrets(name).unwrap().to_vec();
        takes.sort_unstable();
        assert_eq!(names, takes, "{name}");
        assert!(crate::db::deploys::DRIVERS.contains(&name));
    }
    assert!(driver(p::GO_LIVE).unwrap().rolls_back());
    for n in [p::PROD, p::SHIP, p::PROMOTE] {
        assert!(!driver(n).unwrap().rolls_back(), "{n}");
    }
}

/// What a run reports is kept only in the shape the plan allows: a pull
/// request of the repository it named, a full commit id; anything else
/// fails the deploy and is never kept (a page opens these links).
#[test]
fn what_a_run_reports_is_checked_by_shape() {
    let prod = driver(p::PROD).unwrap();
    let d = deploy(SourceKind::Commit, SHA, "arugula-salad/ocarina");
    let e = env(p::PROD, &[]);
    let good = json!({ "pull": "https://github.com/arugula-salad/ocarina/pull/12", "commit": "a".repeat(40) });
    assert_eq!(prod.verify(&e, &d, &run(Some(0)), &good), Verdict::Healthy);
    assert_eq!(prod.kept(&e, &d, &good)["pull"], "https://github.com/arugula-salad/ocarina/pull/12");
    for bad in [
        json!({ "pull": "https://github.com/arugula-salad/other/pull/12" }),
        json!({ "pull": "https://github.com/arugula-salad/ocarina/pull/012" }),
        json!({ "pull": "javascript:alert(1)//https://github.com/arugula-salad/ocarina/pull/1" }),
        json!({ "pull": "https://github.com/arugula-salad/ocarina/pull/1#x" }),
        json!({}),
    ] {
        assert!(matches!(prod.verify(&e, &d, &run(Some(0)), &bad), Verdict::Unhealthy(_)), "{bad}");
        assert!(!prod.kept(&e, &d, &bad).contains_key("pull"), "{bad}");
    }
    assert!(matches!(prod.verify(&e, &d, &run(Some(2)), &good), Verdict::Unhealthy(_)));

    let ship = driver(p::SHIP).unwrap();
    let d = deploy(SourceKind::Artifact, &format!("sha256:{}", "c".repeat(64)), p::ARTIFACT_WHERE);
    let e = env(p::SHIP, &[("app", "hello")]);
    assert_eq!(ship.verify(&e, &d, &run(Some(0)), &json!({ "commit": "b".repeat(40) })), Verdict::Healthy);
    assert!(matches!(ship.verify(&e, &d, &run(Some(0)), &json!({ "commit": "main" })), Verdict::Unhealthy(_)));

    let promote = driver(p::PROMOTE).unwrap();
    let d = deploy(SourceKind::Commit, SHA, p::SHIPPED_REPO);
    let e = env(p::PROMOTE, &[("app", "hello")]);
    let pr = json!({ "pull": "https://github.com/arugula-salad/shipped/pull/3" });
    assert_eq!(promote.verify(&e, &d, &run(Some(0)), &pr), Verdict::Healthy);
    let elsewhere = json!({ "pull": "https://github.com/arugula-salad/hello/pull/3" });
    assert!(matches!(promote.verify(&e, &d, &run(Some(0)), &elsewhere), Verdict::Unhealthy(_)));

    let live = driver(p::GO_LIVE).unwrap();
    let d = deploy(SourceKind::Commit, SHA, "arugula-salad/ocarina");
    assert_eq!(live.verify(&env(p::GO_LIVE, &[]), &d, &run(Some(0)), &json!(null)), Verdict::Healthy);
}
