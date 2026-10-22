//! Drivers (M219): which an environment may name, and what their platform
//! half says.

use super::*;
use crate::db::{
    deploys::{SourceKind, State},
    runs::{State as RunState, TriggerKind},
};

fn env(driver: &str) -> Environment {
    Environment {
        id: "env_1".into(),
        project: "prj_1".into(),
        name: "production".into(),
        driver: driver.into(),
        secret_names: vec![],
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
        source: serde_json::json!({}),
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

#[test]
fn every_named_driver_plans_and_checks() {
    // Tests may name the fake one; a platform without the switch can't.
    assert_eq!(names(), ["chant-fly", "apps-platform", "fake"]);
    assert!(driver("planter").is_none(), "planter's waits for step 8");
    let source = Source { kind: SourceKind::Commit, reference: "abc".into(), place: "acme/app".into() };
    for name in names() {
        let d = driver(name).unwrap();
        assert_eq!(d.name(), name);
        let p = d.plan(&env(name), &source);
        assert_eq!(p.digest, hex::encode(Sha256::digest(p.text.as_bytes())));
        assert!(p.text.contains("source commit abc") && p.text.contains("environment production"), "{}", p.text);
        assert!(p.text.contains(&format!("driver {name}\n")), "it names its driver: {}", p.text);
        let deploy = Deploy {
            id: "dep_1".into(),
            environment: "env_1".into(),
            source: source.clone(),
            plan_digest: p.digest.clone(),
            state: State::Running,
            planned_seq: 1,
            run: Some("run_1".into()),
            planned_by: "a".into(),
            planned_by_agent: None,
            created_at: 1,
            updated_at: 1,
        };
        assert_eq!(d.verify(&deploy, &run(Some(0))), Verdict::Healthy);
        assert_eq!(d.verify(&deploy, &run(Some(2))), Verdict::Unhealthy("the apply exited 2".into()));
        assert!(d.mints(&env(name)).is_empty(), "none mints yet: M220 brings Fly's");
        assert!(!d.secret_names().is_empty(), "{name} names the secrets it takes");
        assert!(d.secret_names().iter().all(|n| arugula_e2e::deploy::secret_name_ok(n)), "none reserved");
    }
    assert!(driver("fake").unwrap().rolls_back());
    assert_eq!(Mint::GithubRead { name: "GITHUB_TOKEN" }.name(), "GITHUB_TOKEN");
}
