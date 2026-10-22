//! Drivers (M219, M220): which an environment may name, and what their
//! platform half says.

use arugula_e2e::drivers::{CHANT_DIGEST, CHANT_ENV, IMAGE};
use serde_json::json;

use super::*;
use crate::db::{
    deploys::{SourceKind, State},
    runs::{State as RunState, TriggerKind},
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn image(n: char) -> String {
    format!("registry.fly.io/acme-web:deployment-1@sha256:{}", n.to_string().repeat(64))
}

fn ledger() -> String {
    format!("https://github.com/acme/app/blob/{}/github/releases.jsonl#L3", "f".repeat(40))
}

fn env(driver: &str) -> Environment {
    let settings = if driver == "chant-fly" {
        let given = [("fly_app", "acme-web"), ("chant_dir", "ops"), ("chant_env", "github")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        arugula_e2e::drivers::settings(driver, &given).unwrap()
    } else {
        Settings::new()
    };
    Environment {
        id: "env_1".into(),
        project: "prj_1".into(),
        name: "production".into(),
        driver: driver.into(),
        secret_names: vec![],
        settings,
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

fn source() -> Source {
    Source { kind: SourceKind::Commit, reference: SHA.into(), place: "acme/app".into() }
}

fn deploy(digest: &str) -> Deploy {
    Deploy {
        id: "dep_1".into(),
        environment: "env_1".into(),
        source: source(),
        plan_digest: digest.into(),
        state: State::Running,
        planned_seq: 1,
        run: Some("run_1".into()),
        planned_by: "a".into(),
        planned_by_agent: None,
        created_at: 1,
        updated_at: 1,
    }
}

/// What chant→Fly's box half reports when its apply went well.
fn reported() -> Value {
    json!({
        "chant_ledger": ledger(),
        "chant_record": { "component": "acme", "env": "github", "digest": "sha256:11", "gitSha": SHA },
        "fly": { "app": "acme-web", "image": image('2'), "healthy": true },
    })
}

#[test]
fn every_named_driver_plans_and_checks() {
    // Tests may name the fake one; a platform without the switch can't.
    assert_eq!(names(), ["chant-fly", "apps-platform", "fake"]);
    assert!(driver("planter").is_none(), "planter's waits for step 8");
    for name in names() {
        let d = driver(name).unwrap();
        assert_eq!(d.name(), name);
        let e = env(name);
        assert_eq!(d.settings(&e.settings).as_ref(), Ok(&e.settings), "{name}'s settings are its own");
        let p = d.plan(&e, &source());
        assert_eq!(p.digest, hex::encode(Sha256::digest(p.text.as_bytes())));
        assert!(
            p.text.contains(&format!("source commit {SHA}")) && p.text.contains("environment production"),
            "{}",
            p.text
        );
        assert!(p.text.contains(&format!("driver {name}\n")), "it names its driver: {}", p.text);
        let ok = if name == "chant-fly" { reported() } else { Value::Null };
        assert_eq!(d.verify(&e, &deploy(&p.digest), &run(Some(0)), &ok), Verdict::Healthy, "{name}");
        assert_eq!(
            d.verify(&e, &deploy(&p.digest), &run(Some(2)), &ok),
            Verdict::Unhealthy("the apply exited 2".into())
        );
        if name == fake::NAME {
            assert!(d.mints(&e).is_empty(), "the fake mints none");
        }
        assert!(!d.secret_names().is_empty(), "{name} names the secrets it takes");
        assert!(d.secret_names().iter().all(|n| arugula_e2e::deploy::secret_name_ok(n)), "none reserved");
        // A box hands its driver only the names on its list, minted ones
        // too (M222: the apps-platform driver's tokens).
        for m in d.mints(&env(name)) {
            assert!(d.secret_names().contains(&m.name()), "{name} mints {} it doesn't take", m.name());
        }
    }
    assert!(driver("fake").unwrap().rolls_back());
    assert!(driver("apps-platform").unwrap().rolls_back(), "M222's");
    assert_eq!(Mint::GithubRead { name: "GITHUB_TOKEN" }.name(), "GITHUB_TOKEN");
}

/// The apps-platform driver (M222): its plan names the list, the pointer's
/// path and the commit; it plans only a commit of one of the list's
/// organization's repositories; it mints a read token for the app's
/// repository and a write token for the list alone, unless the
/// environment holds a token of its own; and it rolls back.
#[test]
fn the_apps_platform_driver_plans_a_pin_and_mints_a_token_for_the_list_only() {
    let d = driver("apps-platform").unwrap();
    let sha = "c".repeat(40);
    let source = Source { kind: SourceKind::Commit, reference: sha.clone(), place: "arugula-salad/beds-smoke".into() };
    let p = d.plan(&env("apps-platform"), &source);
    for line in [
        "driver apps-platform\n".to_owned(),
        "repo arugula-salad/planter-apps (branch main)\n".to_owned(),
        "path apps/beds-smoke.yaml\n".to_owned(),
        format!("at commit {sha};"),
    ] {
        assert!(p.text.contains(&line), "{line:?} in {}", p.text);
    }
    assert_eq!(
        p.digest,
        arugula_e2e::deploy::plan_digest("apps-platform", "production", "commit", &sha, "arugula-salad/beds-smoke"),
        "the box recomputes the same plan"
    );
    assert!(d.refuses(&source).is_none());
    for bad in [
        Source { kind: SourceKind::Artifact, ..source.clone() },
        Source { place: "acme/app".into(), ..source.clone() },
        Source { reference: "abc".into(), ..source.clone() },
    ] {
        assert!(d.refuses(&bad).is_some(), "{bad:?}");
    }
    assert_eq!(
        d.mints(&env("apps-platform")),
        [
            Mint::GithubRead { name: "SOURCE_TOKEN" },
            Mint::GithubWrite { name: "APPS_PLATFORM_TOKEN", repo: "arugula-salad/planter-apps" }
        ]
    );
    let own = Environment { secret_names: vec!["APPS_PLATFORM_TOKEN".into()], ..env("apps-platform") };
    assert_eq!(d.mints(&own), [Mint::GithubRead { name: "SOURCE_TOKEN" }], "its own token, not a mint");
    assert!(d.rolls_back());
    assert!(driver("chant-fly").unwrap().refuses(&source).is_none(), "others plan any source");
}

/// chant→Fly's plan says what chant will do, with the settings it names;
/// its check wants Fly healthy on the app's image, this commit's record in
/// the settings' ledger environment, and a link to the deploy's own
/// repository; the log keeps them; the run links the entry.
#[test]
fn chant_fly_checks_what_its_box_reported() {
    let d = driver("chant-fly").unwrap();
    let e = env("chant-fly");
    let p = d.plan(&e, &source());
    for says in [
        "setting fly_app acme-web\n",
        "does: in ops, npm ci --ignore-scripts (without the tokens), then chant run release",
        "the repo's chant op release runs with FLY_API_TOKEN",
        "does: check Fly app acme-web is healthy",
    ] {
        assert!(p.text.contains(says), "{says:?} in {}", p.text);
    }
    assert!(d.settings(&Settings::new()).unwrap_err().contains("fly_app"));
    // Its GitHub token pushes chant's ledger: minted for the run, writing
    // the deploy's own repository only, unless the environment holds one.
    assert_eq!(d.mints(&e), [Mint::GithubWriteSource { name: "GITHUB_TOKEN" }]);
    assert_eq!(d.mints(&e)[0].writes(&source()), Some("acme/app"));
    let own = Environment { secret_names: vec!["GITHUB_TOKEN".into()], ..env("chant-fly") };
    assert!(d.mints(&own).is_empty(), "its own token, not a mint");
    assert!(p.text.contains("one the platform's GitHub App mints for this run that writes acme/app's contents only"));
    let dep = deploy(&p.digest);
    let ok = run(Some(0));
    assert_eq!(d.verify(&e, &dep, &ok, &reported()), Verdict::Healthy);
    let forged = |f: &dyn Fn(&mut Value)| {
        let mut r = reported();
        f(&mut r);
        d.verify(&e, &dep, &ok, &r)
    };
    for (what, f) in [
        ("isn't healthy", &(|r: &mut Value| r["fly"]["healthy"] = false.into()) as &dyn Fn(&mut Value)),
        ("by digest", &|r| r["fly"]["image"] = image('2').replace("acme-web", "acme-other").into()),
        ("by digest", &|r| r["fly"]["image"] = "registry.fly.io/acme-web:latest".into()),
        ("ledger environment", &|r| r["chant_record"]["env"] = "prod".into()),
        ("this commit's", &|r| r["chant_record"]["gitSha"] = "f".repeat(40).into()),
        ("chant's ledger", &|r| r["chant_ledger"] = "javascript:alert(1)".into()),
        ("chant's ledger", &|r| r["chant_ledger"] = ledger().replace("acme/app", "evil/app").into()),
        ("chant's ledger", &|r| r["chant_ledger"] = format!("chant:acme/app@{SHA}:github/releases.jsonl#L1").into()),
    ] {
        match forged(f) {
            Verdict::Unhealthy(why) => assert!(why.contains(what), "{what}: {why}"),
            v => panic!("{what}: {v:?}"),
        }
    }
    assert!(matches!(d.verify(&e, &dep, &ok, &Value::Null), Verdict::Unhealthy(_)), "nothing reported");
    let kept = d.kept(&e, &dep, &reported());
    assert_eq!(kept["image"], image('2'));
    assert_eq!(kept["chant_component"], "acme");
    assert_eq!(kept["chant_ledger"], ledger());
    assert!(!kept.contains_key("gitSha"));
    assert_eq!(d.ledger(&e, &dep, &reported()), Some(ledger()));
    let mut bad = reported();
    bad["chant_ledger"] = "https://evil.example/x".into();
    bad["fly"]["image"] = "x\ny".into();
    assert!(d.ledger(&e, &dep, &bad).is_none() && !d.kept(&e, &dep, &bad).contains_key("image"));
}

/// A roll back goes back to what a succeeded deploy's log kept: its image
/// and chant's record, all of them, in a plan of its own.
#[test]
fn chant_fly_rolls_back_to_what_a_deploy_kept() {
    let d = driver("chant-fly").unwrap();
    let e = env("chant-fly");
    assert!(d.rolls_back());
    let kept = Value::Object(d.kept(&e, &deploy(""), &reported()));
    let to = d.roll_back_to(&kept).expect("all four kept");
    assert_eq!(to[IMAGE], image('2'));
    assert_eq!((to[CHANT_ENV].as_str(), to[CHANT_DIGEST].as_str()), ("github", "sha256:11"));
    let mut partial = kept.clone();
    partial.as_object_mut().unwrap().remove(IMAGE);
    assert!(d.roll_back_to(&partial).is_none(), "no image kept: no roll back");
    let r = RollBack { deploy: format!("dep_{}", "b".repeat(32)), to };
    let back = d.plan_roll_back(&e, &source(), &r).unwrap();
    assert_ne!(back.digest, d.plan(&e, &source()).digest);
    assert!(back.text.contains(&format!("flyctl deploy --app acme-web --image {}", image('2'))), "{}", back.text);
    assert!(back.text.contains(&format!("roll back to {}\n", r.deploy)), "{}", back.text);
    assert!(driver("apps-platform").unwrap().plan_roll_back(&env("apps-platform"), &source(), &r).is_none());
}
