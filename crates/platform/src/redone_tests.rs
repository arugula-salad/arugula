//! Absorbing redone (M235): its backup reads as redone writes it; tenants
//! become projects by numeric GitHub id only, waiting for people who aren't
//! on Arugula yet; their `rdn_` tokens become agent grant tokens that work
//! on the platform's MCP endpoint as they are, and plan a deploy that only
//! an owner's device approves (the phone card); the approver keys, ntfy
//! topics and auto-approval aren't carried over; a second run changes
//! nothing, a revoke sticks, and the backup is never written.

use std::{path::PathBuf, sync::Arc};

use arugula_e2e::{deploy::Decision, team::TeamRole};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::*;
use crate::{
    agent_mcp_tests::{bearer, ok, refused, rpc, tool},
    chat_tests::{TestHub, call},
    db::{Db, sql::Sql as _, sql::params},
    deploys_tests::{devices, signed},
};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/redone-backup")
}

/// The made-up `rdn_` token behind a fixture token's hash.
fn raw(id: &str) -> String {
    let all = std::fs::read_to_string(dir().join("tokens.txt")).unwrap();
    all.lines().find_map(|l| l.strip_prefix(&format!("{id} "))).unwrap().to_owned()
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// a signs in with GitHub id 4242, b with 5151, c with 6161, x with 3131;
/// team t: b owns it, c edits, x views. The platform keeps secrets. Its
/// routes.
async fn platform() -> (Arc<TestHub>, Arc<Platform>, String) {
    platform_with(Default::default()).await
}

/// [`platform`], projecting secrets as `projection` says, with planter's
/// App (M233) at a fake GitHub that has it installed on `shipped` and
/// `planter-apps`, so ship deploys plan.
async fn platform_with(projection: crate::infisical::Projection) -> (Arc<TestHub>, Arc<Platform>, String) {
    use crate::forge::links_tests::{Shared, fake_github, granted};
    let gh: Shared = Arc::default();
    {
        let mut g = gh.lock().unwrap();
        for (repo, id) in [("arugula-salad/shipped", 2001), ("arugula-salad/planter-apps", 2002)] {
            g.installed.insert(repo.into(), (31, 500, "arugula-salad".into()));
            g.repos.insert(repo.into(), (id, false));
            let mut grants = granted();
            grants["contents"] = "write".into();
            g.grants.insert(repo.into(), grants);
        }
    }
    let api = fake_github(gh).await;
    let hub = Arc::new(TestHub::default());
    hub.github("a", 4242, "fern-gh");
    hub.github("b", 5151, "alder-gh");
    hub.github("c", 6161, "c-gh");
    hub.github("x", 3131, "x-gh");
    hub.team("t", &[("b", TeamRole::Owner), ("c", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.secret_key = Some(Arc::new(crate::secrets::SecretCipher::new(&"07".repeat(32)).unwrap()));
    p.projection = projection;
    // planter's drivers apply on hosted boxes: these accounts may use them.
    p.hosted = crate::hosted::Config { accounts: vec!["a".into(), "b".into()], ..Default::default() };
    p.deploy_app = Some(crate::forge::GithubApp::new(
        "4242".into(),
        "arugula-planter".into(),
        String::new(),
        &api,
        crate::forge::links_tests::throwaway_key(),
    ));
    let app = Arc::new(p);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (hub, app, base)
}

/// fern is 4242's; moss is two people with no team; alder is team t's,
/// led by b; newbie is 8888's, who isn't on Arugula; lonely is nobody's.
fn options(dry_run: bool) -> Options {
    Options {
        people: [
            ("fern".to_owned(), vec![4242]),
            ("moss".to_owned(), vec![4242, 6161]),
            ("alder".to_owned(), vec![5151, 6161, 9999]),
            ("newbie".to_owned(), vec![8888]),
        ]
        .into_iter()
        .collect(),
        teams: [("alder".to_owned(), "t".to_owned())].into_iter().collect(),
        driver: "fake".into(),
        merge: false,
        dry_run,
    }
}

/// [`options`] with planter's ship driver, as production imports.
fn ship_options(dry_run: bool) -> Options {
    Options { driver: SHIP_DRIVER.into(), ..options(dry_run) }
}

/// PLANTER_PROJECTS names `project`, as tests name it (M233).
fn allow(project: &str) {
    crate::drivers::planter::TEST_PROJECTS.lock().unwrap_or_else(|e| e.into_inner()).push(project.to_owned());
}

fn line<'a>(r: &'a Report, tenant: &str) -> &'a TenantLine {
    r.tenants.iter().find(|t| t.tenant.id == tenant).unwrap()
}

fn token_outcome<'a>(r: &'a Report, id: &str) -> &'a TokenOutcome {
    &r.tenants.iter().flat_map(|t| &t.tokens).find(|k| k.token.id == id).unwrap().outcome
}

fn app_outcome<'a>(r: &'a Report, name: &str) -> &'a AppOutcome {
    &r.tenants.iter().flat_map(|t| &t.apps).find(|a| a.app.name == name).unwrap().outcome
}

fn count(app: &Platform, table: &str) -> i64 {
    app.db.c().query_row(&format!("SELECT count(*) FROM {table}"), params![], |r| r.get(0)).unwrap()
}

#[test]
fn the_backup_reads_as_redone_writes_it() {
    let b = Backup::read(&dir()).unwrap();
    let ids: Vec<&str> = b.tenants.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["alder", "fern", "lonely", "moss", "newbie"]);
    let fern = b.tenants.iter().find(|t| t.id == "fern").unwrap();
    assert!(fern.ntfy && fern.auto_approve_updates);
    let toks: Vec<&str> = b.tokens.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(toks, ["tok_fern1", "tok_fern2", "tok_moss1", "tok_alder1", "tok_new1", "tok_ghost1"]);
    assert_eq!(b.tokens[0].hash, sha(raw("tok_fern1").as_bytes()));
    let site = b.apps.iter().find(|a| a.name == "fern-site").unwrap();
    assert_eq!((site.current.as_deref(), site.deploys), (Some("plan_fern2"), 2));
    assert_eq!(site.spec["health"], json!("/up"));
    let blog = b.apps.iter().find(|a| a.name == "fern-blog").unwrap();
    assert!(blog.current.is_none() && blog.spec.is_null());
    // Approver keys' hashes aren't read; tokens' hashes aren't shown.
    let shown = format!("{b:?}") + &serde_json::to_string(&b).unwrap();
    assert!(!shown.contains(&sha(b"rdn_approve_fern-test-key")), "approver hash read");
    assert!(!serde_json::to_string(&b).unwrap().contains(&b.tokens[0].hash), "token hash shown");
    assert!(Backup::read(&dir().join("nope")).is_err());
}

#[test]
fn people_are_numeric_github_ids_only() {
    assert_eq!(parse_people("fern=4242").unwrap(), ("fern".into(), vec![4242]));
    assert_eq!(parse_people("alder=5151, 6161").unwrap(), ("alder".into(), vec![5151, 6161]));
    for bad in ["fern=fern-gh", "fern=", "=4242", "fern=0", "fern", "fern=4242,jake"] {
        assert!(parse_people(bad).is_err(), "{bad}");
    }
    assert_eq!(parse_team("alder=t").unwrap(), ("alder".into(), "t".into()));
    assert!(parse_team("alder=").is_err());
    assert_eq!(token_name("fern laptop"), "redone: fern laptop");
    assert_eq!(token_name("\u{202e}evil\n"), "redone: evil");
    assert_eq!(token_name(&"x".repeat(100)).chars().count(), 64);
}

/// The done-when's part here: redone's tenants become projects, and a
/// redone agent with only its old token does redone's flow through the
/// platform's MCP endpoint (sets a secret, plans, asks), which an owner's
/// device approves on the phone path and nothing else can.
#[tokio::test]
async fn tenants_become_projects_and_redone_tokens_work_through_the_platform() {
    let (hub, app, base) = platform().await;
    let a = devices(&hub, "a");
    let state = std::fs::read(dir().join("state.json")).unwrap();
    let backup = Backup::read(&dir()).unwrap();

    // A dry run says what would happen, and writes nothing.
    let r = import(&app, &backup, &options(true)).await.unwrap();
    assert!(r.dry_run);
    assert!(matches!(line(&r, "fern").outcome, TenantOutcome::Project { personal: true, made: true, .. }));
    assert!(matches!(line(&r, "alder").outcome, TenantOutcome::Project { personal: false, .. }));
    assert!(matches!(line(&r, "moss").outcome, TenantOutcome::Refused { .. }), "{:?}", line(&r, "moss"));
    assert_eq!(line(&r, "newbie").outcome, TenantOutcome::WaitingSignIn { github_id: 8888 });
    assert_eq!(line(&r, "lonely").outcome, TenantOutcome::WaitingPeople);
    assert!(matches!(token_outcome(&r, "tok_fern1"), TokenOutcome::Carried { agent_token: None, .. }));
    assert_eq!(token_outcome(&r, "tok_new1"), &TokenOutcome::WaitingTenant);
    assert!(matches!(app_outcome(&r, "fern-site"), AppOutcome::Environment { environment: None, .. }));
    assert!(r.orphans.iter().any(|o| o.contains("tok_ghost1")), "{:?}", r.orphans);
    let text = r.render(&dir());
    assert!(text.contains("DRY RUN") && text.contains("nothing written"), "{text}");
    assert_eq!((count(&app, "redone_imports"), count(&app, "agent_tokens")), (0, 0));
    assert!(app.db.redone_personal_project("a").unwrap().is_none(), "a dry run made a project");

    // The run.
    let r = import(&app, &backup, &options(false)).await.unwrap();
    let TenantOutcome::Project { project: fern, personal: true, made: true } = line(&r, "fern").outcome.clone() else {
        panic!("{:?}", line(&r, "fern"))
    };
    assert_eq!(app.db.redone_personal_project("a").unwrap().as_deref(), Some(fern.as_str()));
    let TenantOutcome::Project { project: alder, personal: false, .. } = line(&r, "alder").outcome.clone() else {
        panic!("{:?}", line(&r, "alder"))
    };
    assert_eq!(app.db.team_default_project("t").unwrap().as_deref(), Some(alder.as_str()));
    let people: Vec<(u64, Option<&str>, Option<&str>)> =
        line(&r, "alder").people.iter().map(|p| (p.github_id, p.account.as_deref(), p.role.as_deref())).collect();
    assert_eq!(people, [(5151, Some("b"), Some("owner")), (6161, Some("c"), Some("editor")), (9999, None, None)]);
    let dropped = line(&r, "fern").dropped.join("\n");
    assert!(
        dropped.contains("approver key") && dropped.contains("ntfy") && dropped.contains("auto-approve"),
        "{dropped}"
    );
    let TokenOutcome::Carried { account, agent_token: Some(fern1) } = token_outcome(&r, "tok_fern1").clone() else {
        panic!("{:?}", token_outcome(&r, "tok_fern1"))
    };
    assert_eq!(account, "a");
    let TokenOutcome::Carried { agent_token: Some(fern2), .. } = token_outcome(&r, "tok_fern2").clone() else {
        panic!()
    };
    assert!(matches!(token_outcome(&r, "tok_alder1"), TokenOutcome::Carried { account, .. } if account == "b"));
    assert_eq!(token_outcome(&r, "tok_moss1"), &TokenOutcome::WaitingTenant);
    // Each app is an environment of its tenant's project now, by its name.
    let AppOutcome::Environment { environment: Some(site), .. } = app_outcome(&r, "fern-site").clone() else {
        panic!("{:?}", app_outcome(&r, "fern-site"))
    };
    let names = |p: &str| -> Vec<String> { app.db.environments(p).unwrap().into_iter().map(|e| e.name).collect() };
    assert_eq!(names(&fern), ["fern-blog", "fern-site"]);
    assert_eq!(names(&alder), ["alder-api"]);
    assert_eq!(app.db.environment(&site).unwrap().unwrap().driver, "fake");
    assert_eq!(count(&app, "agent_tokens"), 3);
    // Only redone's hash is kept, and no grant approves anything.
    let t = app.db.agent_token(&fern1).unwrap().unwrap();
    assert_eq!((t.imported_from.as_deref(), t.name.as_str()), (Some("redone"), "redone: fern laptop"));
    assert_eq!(app.db.agent_token_by_hash(&sha(raw("tok_fern1").as_bytes())).unwrap().unwrap().id, fern1);
    let powers: Vec<String> = app.db.agent_token_grants(&fern1, "a").unwrap().into_iter().map(|g| g.power).collect();
    assert_eq!(powers, ["deploys.plan", "secrets.write"]);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        hub.pushes().iter().any(|(to, m, _)| to == "a" && m["title"] == json!("Your redone token works on Arugula")),
        "{:?}",
        hub.pushes()
    );
    // a sees it on their page, carried over.
    let (_, listed) = call(&base, "a", "GET", "/api/agent-tokens", None).await;
    let mine = listed["tokens"].as_array().unwrap();
    assert!(mine.iter().any(|t| t["id"] == json!(fern1) && t["imported_from"] == json!("redone")), "{listed}");

    // redone's agent, with its old token, on the platform's endpoint.
    let token = raw("tok_fern1");
    let r = tool(&base, &token, "capabilities", json!({})).await;
    let caps = ok(&r).clone();
    assert_eq!(caps["projects"][0]["id"], json!(fern), "{caps}");
    assert_eq!(caps["agent"]["label"], json!("redone: fern laptop (MCP)"));
    assert_eq!(caps["approves"], json!(false));
    // It sets its app's secret, which it can't read back; a project-wide
    // secret is beyond a carried token.
    let r = tool(&base, &token, "set_secret", json!({ "app": "fern-site", "key": "API_KEY", "value": "hunter2-m235" }))
        .await;
    assert_eq!(ok(&r)["set"], json!(true));
    let master = app.secret_key.clone().unwrap();
    let opened = crate::keys::open(&app.db, &master, &fern, "app/fern-site/API_KEY").unwrap();
    assert_eq!(opened.as_deref(), Some("hunter2-m235"));
    let why = refused(&tool(&base, &token, "set_secret", json!({ "name": "API_KEY", "value": "x" })).await);
    assert!(why.contains("apps' secrets only"), "{why}");
    // It plans a deploy to its app, and asks: a gets the card.
    let sha1 = "cd".repeat(20);
    let r =
        tool(&base, &token, "plan_deploy", json!({ "environment": "fern-site", "commit": sha1, "repo": "fern/site" }))
            .await;
    let dep = ok(&r)["deploy"].clone();
    assert_eq!((&dep["state"], &dep["planned_by"]), (&json!("pending"), &json!("a")), "{dep}");
    let dep_id = dep["id"].as_str().unwrap().to_owned();
    let op = dep["operation"]["hash"].as_str().unwrap().to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(hub.pushes().iter().any(|(to, m, _)| to == "a" && m["deploy"] == json!(dep_id)), "{:?}", hub.pushes());

    // BREAK: nothing it holds approves. Not a tool, not a route with the
    // owner's own signature, not the tenant's old approver key.
    for name in ["approve_plan", "apply_plan_approved", "approve"] {
        let why = refused(&tool(&base, &token, name, json!({ "plan_id": dep_id })).await);
        assert!(why.contains("never approve"), "{name}: {why}");
    }
    let good = signed(&a.phone, "a", &dep_id, &op, Decision::Once);
    let st = bearer(&base, &token, "POST", &format!("/api/deploys/{dep_id}/approve"), Some(good.clone())).await;
    assert!(matches!(st.status().as_u16(), 401 | 403 | 404), "{}", st.status());
    for other in ["rdn_approve_fern-test-key".to_owned(), raw("tok_new1"), raw("tok_moss1"), raw("tok_ghost1")] {
        let (st, _) = rpc(&base, &other, "tools/list", json!({})).await;
        assert_eq!(st, 401, "{other}");
    }
    assert!(app.db.deploy_approvals(&dep_id).unwrap().is_empty());
    // a's phone approves it.
    let (st, d) = call(&base, "a", "POST", &format!("/api/deploys/{dep_id}/approve"), Some(good)).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    let r = tool(&base, &token, "read_deploy", json!({ "id": dep_id })).await;
    assert_eq!(ok(&r)["deploy"]["state"], json!("approved"));

    // Again: nothing changes.
    let tokens = count(&app, "agent_tokens");
    let grants = count(&app, "grants");
    let imports = count(&app, "redone_imports");
    let r = import(&app, &backup, &options(false)).await.unwrap();
    assert!(matches!(line(&r, "fern").outcome, TenantOutcome::ImportedBefore { .. }));
    assert!(matches!(token_outcome(&r, "tok_fern1"), TokenOutcome::ImportedBefore { .. }));
    assert!(matches!(app_outcome(&r, "fern-site"), AppOutcome::ImportedBefore { .. }));
    assert_eq!(r.tokens_carried(), 0);
    assert_eq!(
        (count(&app, "agent_tokens"), count(&app, "grants"), count(&app, "redone_imports")),
        (tokens, grants, imports)
    );

    // a revokes the other one: it's refused at once, and never carried over
    // again.
    let (st, _) = call(&base, "a", "POST", &format!("/api/agent-tokens/{fern2}/revoke"), None).await;
    assert_eq!(st, 200);
    let (st, _) = rpc(&base, &raw("tok_fern2"), "tools/list", json!({})).await;
    assert_eq!(st, 401);
    let r = import(&app, &backup, &options(false)).await.unwrap();
    assert!(matches!(token_outcome(&r, "tok_fern2"), TokenOutcome::ImportedBefore { .. }));
    assert!(app.db.agent_token(&fern2).unwrap().unwrap().revoked_at.is_some());

    // newbie signs in later: a later run picks them up.
    hub.github("n", 8888, "newbie-gh");
    let r = import(&app, &backup, &options(false)).await.unwrap();
    assert!(matches!(line(&r, "newbie").outcome, TenantOutcome::Project { personal: true, .. }));
    assert!(matches!(token_outcome(&r, "tok_new1"), TokenOutcome::Carried { account, .. } if account == "n"));
    let (st, _) = rpc(&base, &raw("tok_new1"), "tools/list", json!({})).await;
    assert_eq!(st, 200);

    // The backup was only read.
    assert_eq!(std::fs::read(dir().join("state.json")).unwrap(), state);
}

/// BREAK: a team's tenant whose first person only views the team isn't
/// carried over, and neither are its tokens; a person matched by their
/// GitHub id is never someone with that login.
#[tokio::test]
async fn a_tenant_goes_only_where_its_people_may_write() {
    let (hub, app, _) = platform().await;
    // z's login is 4242: nothing about it is matched.
    hub.github("z", 1, "4242");
    let backup = Backup::read(&dir()).unwrap();
    let o = Options {
        people: [("alder".to_owned(), vec![3131]), ("fern".to_owned(), vec![1])].into_iter().collect(),
        teams: [("alder".to_owned(), "t".to_owned())].into_iter().collect(),
        driver: "fake".into(),
        merge: false,
        dry_run: false,
    };
    let r = import(&app, &backup, &o).await.unwrap();
    assert_eq!(line(&r, "alder").outcome, TenantOutcome::NotOnTeam { team: "t".into() });
    assert_eq!(token_outcome(&r, "tok_alder1"), &TokenOutcome::WaitingTenant);
    // fern=1 is z (GitHub id 1), not a (whose id is 4242).
    assert!(matches!(token_outcome(&r, "tok_fern1"), TokenOutcome::Carried { account, .. } if account == "z"));
    let o = Options {
        people: [("alder".to_owned(), vec![5151])].into_iter().collect(),
        teams: [("alder".to_owned(), "no-such-team".to_owned())].into_iter().collect(),
        driver: "fake".into(),
        merge: false,
        dry_run: true,
    };
    let r = import(&app, &backup, &o).await.unwrap();
    assert!(matches!(line(&r, "alder").outcome, TenantOutcome::Refused { .. }), "{:?}", line(&r, "alder"));
    let _: Value = serde_json::to_value(&r).unwrap();
}

/// redone's tools on the platform, for a carried-over token only: an app
/// is an environment of the token's project, which the agent can't claim
/// (Arugula #55's hole: BREAK, a new name and another tenant's app are
/// refused, and nothing reaches Infisical for them); `set_secret {app,
/// key, value}` seals the app's secret and projects it into Infisical's
/// `/<app>/`; `get_plan` and `apply_plan` read a deploy and never approve
/// or apply it; an owner's phone approves it.
#[tokio::test]
async fn redones_tools_work_on_the_platform_and_never_claim_or_approve() {
    let (infisical, url) = crate::infisical::tests::fake(&["beds"], "s3cret").await;
    let writer =
        crate::infisical::Writer::new(reqwest::Client::new(), &url, "proj-planter-apps", "prod", "id", "s3cret");
    let (hub, app, base) =
        platform_with(crate::infisical::Projection::new(crate::infisical::Mode::On, Some(writer))).await;
    let a = devices(&hub, "a");
    import(&app, &Backup::read(&dir()).unwrap(), &ship_options(false)).await.unwrap();
    let fern = app.db.redone_personal_project("a").unwrap().unwrap();
    let alder = app.db.team_default_project("t").unwrap().unwrap();
    allow(&fern);
    allow(&alder);
    let token = raw("tok_fern1");

    // Its tools: the platform's, and redone's beside them. A token minted
    // on the page gets the platform's alone.
    let (_, listed) = rpc(&base, &token, "tools/list", json!({})).await;
    let names: Vec<String> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect();
    for n in [
        "whoami",
        "list_apps",
        "plan_deploy",
        "plan_rollback",
        "get_plan",
        "apply_plan",
        "app_status",
        "app_logs",
        "set_secret",
        "list_secrets",
        "promote",
        "capabilities",
        "read_deploy",
    ] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.contains("approv") || n.contains("deny")), "{names:?}");
    let (st, minted) = call(
        &base,
        "a",
        "POST",
        "/api/agent-tokens",
        Some(json!({ "name": "page", "projects": [fern], "powers": ["deploys.plan"] })),
    )
    .await;
    assert_eq!(st, 200, "{minted}");
    let (_, plain) = rpc(&base, minted["token"].as_str().unwrap(), "tools/list", json!({})).await;
    assert!(!plain.to_string().contains("\"whoami\""), "{plain}");
    let (_, init) = rpc(&base, &token, "initialize", json!({})).await;
    let says = init["result"]["instructions"].as_str().unwrap();
    assert!(says.contains("https://control.example/api/mcp/uploads") && says.contains("can't claim"), "{says}");

    let who = ok(&tool(&base, &token, "whoami", json!({})).await).clone();
    assert_eq!((&who["project"], &who["approves"]), (&json!(fern), &json!(false)), "{who}");
    let apps = ok(&tool(&base, &token, "list_apps", json!({})).await).clone();
    let apps: Vec<&str> = apps.as_array().unwrap().iter().map(|a| a["app"].as_str().unwrap()).collect();
    assert_eq!(apps, ["fern-blog", "fern-site"]);

    // A secret of the app's: sealed as the project's, and in Infisical's
    // /fern-site/.
    let value = "hunter2-m235-app";
    let r = tool(&base, &token, "set_secret", json!({ "app": "fern-site", "key": "API_KEY", "value": value })).await;
    assert_eq!((&ok(&r)["set"], &ok(&r)["projected"]), (&json!(true), &json!("written")), "{r}");
    let master = app.secret_key.clone().unwrap();
    assert_eq!(crate::keys::open(&app.db, &master, &fern, "app/fern-site/API_KEY").unwrap().as_deref(), Some(value));
    assert_eq!(infisical.secrets.lock().unwrap()["fern-site"]["API_KEY"], value);
    let r = tool(&base, &token, "list_secrets", json!({ "app": "fern-site" })).await;
    assert_eq!(ok(&r)["keys"], json!(["API_KEY"]));
    assert!(!r.to_string().contains(value));
    let (_, secrets) = call(&base, "a", "GET", &format!("/api/projects/{fern}/secrets"), None).await;
    assert!(
        secrets["secrets"].as_array().unwrap().iter().any(|s| s["name"] == json!("fern-site/API_KEY")),
        "{secrets}"
    );

    // BREAK (the review's 2): a carried token reaches the apps redone had,
    // not an environment its owner made since, by any tool.
    let (st, env) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{fern}/environments"),
        Some(json!({ "name": "flyapp", "driver": "chant-fly", "secret_names": ["FLY_API_TOKEN"],
                     "settings": { "fly_app": "fern-fly", "chant_env": "github" } })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
    let why =
        refused(&tool(&base, &token, "set_secret", json!({ "app": "flyapp", "key": "API_KEY", "value": value })).await);
    assert!(why.contains("has no app flyapp"), "{why}");
    let why = refused(
        &tool(
            &base,
            &token,
            "plan_deploy",
            json!({ "environment": "flyapp", "commit": "ab".repeat(20), "repo": "f/s" }),
        )
        .await,
    );
    assert!(why.contains("only to the apps redone had"), "{why}");
    assert!(app.db.deploys(env["id"].as_str().unwrap(), 10).unwrap().is_empty());

    // BREAK (the review's 1): the exact exploit. Team t's ship environment
    // holds `victim`; fern's project has an environment of its own called
    // victim. Nothing of fern's reaches Infisical's /victim/.
    let (st, _) = call(
        &base,
        "b",
        "POST",
        &format!("/api/projects/{alder}/environments"),
        Some(json!({ "name": "victim", "driver": "planter-ship", "settings": { "app": "victim" } })),
    )
    .await;
    assert_eq!(st, 200);
    let (st, _) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{fern}/environments"),
        Some(json!({ "name": "victim", "driver": "fake", "secret_names": ["APP_TOKEN"] })),
    )
    .await;
    assert_eq!(st, 200);
    let why =
        refused(&tool(&base, &token, "set_secret", json!({ "app": "victim", "key": "API_KEY", "value": value })).await);
    assert!(why.contains("has no app victim"), "{why}");
    assert!(!infisical.secrets.lock().unwrap().contains_key("victim"));

    // BREAK (the review's 3): another project, named outright, is refused
    // before anything of it is read: none of its apps' names come back.
    for (tool_name, args) in [
        ("set_secret", json!({ "project": alder, "app": "alder-api", "key": "K", "value": value })),
        ("plan_deploy", json!({ "project": alder, "app": "alder-api", "upload_id": "sha256:00" })),
        (
            "plan_deploy",
            json!({ "project": alder, "environment": "alder-api", "commit": "ab".repeat(20), "repo": "a/b" }),
        ),
    ] {
        let r = tool(&base, &token, tool_name, args).await;
        let why = refused(&r);
        assert!(!why.contains("alder-api") && !why.contains("victim"), "{tool_name}: {why}");
    }

    // BREAK (Arugula #55): no name is claimed by an agent's call. A name
    // the project has no app of, and another tenant's app, are refused, and
    // nothing of them reaches Infisical or the platform.
    for (app_name, key) in
        [("brand-new", "API_KEY"), ("alder-api", "API_KEY"), ("beds", "LOBBY_SECRET"), ("planter-x", "A")]
    {
        let why =
            refused(&tool(&base, &token, "set_secret", json!({ "app": app_name, "key": key, "value": value })).await);
        assert!(why.contains("no app") || why.contains("platform's names"), "{app_name}: {why}");
    }
    assert_eq!(infisical.secrets.lock().unwrap().keys().collect::<Vec<_>>(), ["fern-site"]);
    assert_eq!(app.db.environments(&fern).unwrap().len(), 4, "an agent's call made an environment");
    let why =
        refused(&tool(&base, &token, "plan_deploy", json!({ "app": "brand-new", "upload_id": "sha256:00" })).await);
    assert!(why.contains("no app brand-new"), "{why}");

    // redone's way to ship: the source uploaded by itself, then planned to
    // the app by its upload_id, and asked.
    // The platform checks it's gzipped; the deploy's box unpacks it (M233).
    let tgz: Vec<u8> = vec![0x1f, 0x8b, 8, 0, 2, 3, 5, 7, 11];
    let up = reqwest::Client::new()
        .put(format!("{base}/api/mcp/uploads"))
        .header("authorization", format!("Bearer {token}"))
        .body(tgz.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(up.status().as_u16(), 201);
    let up: Value = up.json().await.unwrap();
    let upload_id = up["upload_id"].as_str().unwrap().to_owned();
    assert_eq!(upload_id, format!("sha256:{}", sha(&tgz)));
    assert_eq!(app.db.deploy_artifact(&fern, &upload_id).unwrap().as_deref(), Some(&tgz[..]));
    // Too big: refused before it's read. Windows aborts the connection
    // under the bytes still being sent rather than let the client read the
    // 413, so there a request error is the refusal.
    let big: Vec<u8> = vec![0x1f, 0x8b].into_iter().chain(std::iter::repeat_n(0u8, 3 << 20)).collect();
    match reqwest::Client::new()
        .put(format!("{base}/api/mcp/uploads"))
        .header("authorization", format!("Bearer {token}"))
        .body(big)
        .send()
        .await
    {
        Ok(r) => assert_eq!(r.status().as_u16(), 413),
        Err(e) => assert!(cfg!(windows) && e.is_request(), "{e}"),
    }
    // Not gzipped, or with no token: refused.
    for (body, bearer_token, want) in
        [(b"not a tar".to_vec(), token.clone(), 422), (tgz.clone(), "rdn_nope".to_owned(), 401)]
    {
        let r = reqwest::Client::new()
            .put(format!("{base}/api/mcp/uploads"))
            .header("authorization", format!("Bearer {bearer_token}"))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), want);
    }
    let r = tool(&base, &token, "plan_deploy", json!({ "app": "fern-site", "upload_id": upload_id })).await;
    let shipped = ok(&r).clone();
    assert_eq!((&shipped["state"], &shipped["app"]), (&json!("pending"), &json!("fern-site")), "{shipped}");
    assert_eq!(shipped["deploy"]["source"]["ref"], json!(upload_id), "{shipped}");
    let why = refused(
        &tool(
            &base,
            &token,
            "plan_deploy",
            json!({ "app": "fern-site", "upload_id": format!("sha256:{}", "0".repeat(64)) }),
        )
        .await,
    );
    assert!(why.contains("no upload"), "{why}");

    // A deploy: get_plan and apply_plan read it, and never move it.
    let dep = shipped["deploy"].clone();
    let dep_id = dep["id"].as_str().unwrap().to_owned();
    let op = dep["operation"]["hash"].as_str().unwrap().to_owned();
    let got = ok(&tool(&base, &token, "get_plan", json!({ "plan_id": dep_id })).await).clone();
    assert_eq!((&got["state"], &got["app"]), (&json!("pending"), &json!("fern-site")), "{got}");
    let r = tool(&base, &token, "apply_plan", json!({ "plan_id": dep_id })).await;
    assert!(r["content"][0]["text"].as_str().unwrap().contains("waits for an owner"), "{r}");
    assert!(app.db.deploy_approvals(&dep_id).unwrap().is_empty());
    let (_, waiting) = call(&base, "a", "GET", "/api/deploys/waiting", None).await;
    assert!(waiting["deploys"].as_array().unwrap().iter().any(|d| d["id"] == json!(dep_id)), "{waiting}");
    let (st, d) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{dep_id}/approve"),
        Some(signed(&a.phone, "a", &dep_id, &op, Decision::Once)),
    )
    .await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    let got = ok(&tool(&base, &token, "get_plan", json!({ "plan_id": dep_id })).await).clone();
    assert_eq!(got["state"], json!("approved"));
    let r = tool(&base, &token, "apply_plan", json!({ "plan_id": dep_id })).await;
    assert!(r["content"][0]["text"].as_str().unwrap().contains("applies itself"), "{r}");
    let st = ok(&tool(&base, &token, "app_status", json!({ "app": "fern-site" })).await).clone();
    assert_eq!((&st["state"], &st["applying"]), (&json!("deploying"), &json!(dep_id)), "{st}");
    let why = refused(&tool(&base, &token, "plan_rollback", json!({ "app": "fern-site" })).await);
    assert!(why.contains("no earlier deploy"), "{why}");
    let why = refused(&tool(&base, &token, "promote", json!({ "app": "fern-site" })).await);
    assert!(why.contains("its owner"), "{why}");
    let r = tool(&base, &token, "app_logs", json!({ "app": "fern-blog" })).await;
    assert!(ok(&r)["log"].is_null(), "{r}");
    for name in ["approve_plan", "approve"] {
        let why = refused(&tool(&base, &token, name, json!({ "plan_id": dep_id })).await);
        assert!(why.contains("never approve"), "{name}: {why}");
    }
    // Every call is in its owner's list: the shim's names too.
    let t = app.db.agent_token_by_hash(&sha(token.as_bytes())).unwrap().unwrap();
    let calls = app.db.agent_calls("a", &t.id, 200).unwrap();
    for (tool_name, outcome) in
        [("set_secret", "ok"), ("set_secret", "failed"), ("get_plan", "ok"), ("approve", "refused")]
    {
        assert!(calls.iter().any(|c| c.tool == tool_name && c.outcome == outcome), "{tool_name}/{outcome}: {calls:?}");
    }
    assert!(!format!("{calls:?}").contains(value));
}

/// redone's apps become planter's ship environments (M233), with what
/// their live plan said, and their names the project's from the import on;
/// a name another project's ship environment has already is refused, not
/// taken over. The dry run says so too.
#[tokio::test]
async fn apps_become_ship_environments_and_their_names_the_projects() {
    let (_, app, base) = platform().await;
    let t = crate::projects::team_project(&app, "t").await.unwrap().unwrap();
    allow(&t);
    // b's team has a ship environment for fern-site already.
    let (st, env) = call(
        &base,
        "b",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({ "name": "fern-site", "driver": "planter-ship", "settings": { "app": "fern-site" } })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
    let backup = Backup::read(&dir()).unwrap();
    let o = |dry_run| Options {
        people: [("fern".to_owned(), vec![4242])].into_iter().collect(),
        teams: Default::default(),
        driver: SHIP_DRIVER.into(),
        merge: false,
        dry_run,
    };
    let r = import(&app, &backup, &o(true)).await.unwrap();
    assert!(
        matches!(app_outcome(&r, "fern-site"), AppOutcome::Refused { why } if why.contains("another project")),
        "{:?}",
        app_outcome(&r, "fern-site")
    );
    assert!(matches!(app_outcome(&r, "fern-blog"), AppOutcome::Environment { environment: None, .. }));
    let r = import(&app, &backup, &o(false)).await.unwrap();
    assert!(matches!(app_outcome(&r, "fern-site"), AppOutcome::Refused { .. }));
    // Its project isn't one PLANTER_PROJECTS names yet: the report says to
    // add it, and the environment and the name's claim are made anyway.
    let fern = app.db.redone_personal_project("a").unwrap().unwrap();
    assert_eq!(r.allowlist, std::slice::from_ref(&fern));
    assert!(r.render(&dir()).contains(&format!("PLANTER_PROJECTS names (M233): add {fern}")));
    assert_eq!(app.db.planter_app_owner("fern-blog").unwrap().as_deref(), Some(fern.as_str()));
    let AppOutcome::Environment { environment: Some(blog), driver } = app_outcome(&r, "fern-blog").clone() else {
        panic!("{:?}", app_outcome(&r, "fern-blog"))
    };
    assert_eq!(driver, "planter-ship");
    let blog = app.db.environment(&blog).unwrap().unwrap();
    assert_eq!(blog.settings.get("app").map(String::as_str), Some("fern-blog"));
    assert_eq!(blog.settings.get("port").map(String::as_str), Some("8080"));
    // Now nobody else takes fern-blog.
    let (st, why) = call(
        &base,
        "b",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({ "name": "fern-blog", "driver": "planter-ship", "settings": { "app": "fern-blog" } })),
    )
    .await;
    assert_eq!(st, 409, "{why}");
    // Its spec is the environment's: a fern-site that came over keeps its
    // health path, secrets and port.
    let spec = App {
        name: "x".into(),
        tenant: "fern".into(),
        current: Some("p".into()),
        spec: json!({ "port": 3000, "health": "/up", "secrets": ["B", "A"], "portEnv": "PORT" }),
        deploys: 1,
    };
    let s = app_settings(SHIP_DRIVER, &spec);
    assert_eq!(
        s.into_iter().collect::<Vec<_>>(),
        [
            ("app".to_owned(), "x".to_owned()),
            ("health".to_owned(), "/up".to_owned()),
            ("port".to_owned(), "3000".to_owned()),
            ("port_env".to_owned(), "PORT".to_owned()),
            ("secrets".to_owned(), "B,A".to_owned())
        ]
    );
    assert!(app_settings("fake", &spec).is_empty());
}

/// The review's 2: two tenants become one project only with `--merge`.
#[tokio::test]
async fn two_tenants_share_a_project_only_when_merged() {
    let (_, app, _) = platform().await;
    let backup = Backup::read(&dir()).unwrap();
    let o = |merge, dry_run| Options {
        people: [("fern".to_owned(), vec![4242]), ("moss".to_owned(), vec![4242])].into_iter().collect(),
        teams: Default::default(),
        driver: "fake".into(),
        merge,
        dry_run,
    };
    let r = import(&app, &backup, &o(false, true)).await.unwrap();
    assert!(matches!(line(&r, "fern").outcome, TenantOutcome::Project { .. }));
    assert!(
        matches!(&line(&r, "moss").outcome, TenantOutcome::Refused { why } if why.contains("--merge")),
        "{:?}",
        line(&r, "moss")
    );
    let r = import(&app, &backup, &o(false, false)).await.unwrap();
    assert!(matches!(&line(&r, "moss").outcome, TenantOutcome::Refused { why } if why.contains("--merge")));
    assert_eq!(token_outcome(&r, "tok_moss1"), &TokenOutcome::WaitingTenant);
    // A later run with --merge puts moss in fern's project.
    let r = import(&app, &backup, &o(true, false)).await.unwrap();
    let fern = app.db.redone_personal_project("a").unwrap().unwrap();
    assert!(matches!(&line(&r, "moss").outcome, TenantOutcome::Project { project, .. } if *project == fern));
    assert_eq!(app.db.redone_tenants_in(&fern).unwrap(), ["fern", "moss"]);
}

/// The review's 4: deleting an account keeps a tombstone of the tokens
/// carried over for it, so a later import never revives one, even for the
/// same person signing up again with the same GitHub id.
#[tokio::test]
async fn a_deleted_accounts_carried_tokens_are_never_carried_again() {
    use crate::PlatformPort as _;
    let (hub, app, base) = platform().await;
    let backup = Backup::read(&dir()).unwrap();
    import(&app, &backup, &options(false)).await.unwrap();
    let (st, _) = rpc(&base, &raw("tok_fern1"), "tools/list", json!({})).await;
    assert_eq!(st, 200);
    app.account_deleted(&crate::Deleted { account: "a".into(), machines: vec![], disbanded: vec![] }).await.unwrap();
    let i = app.db.redone_imported("token", "tok_fern1").unwrap().unwrap();
    assert!(i.account.is_none() && i.platform_id.is_some(), "{i:?}");
    let (st, _) = rpc(&base, &raw("tok_fern1"), "tools/list", json!({})).await;
    assert_eq!(st, 401);
    // The same person, a new account, the same GitHub id.
    hub.github("a2", 4242, "fern-gh");
    let mut o = options(false);
    o.people.retain(|t, _| t == "fern");
    let r = import(&app, &backup, &o).await.unwrap();
    assert!(matches!(token_outcome(&r, "tok_fern1"), TokenOutcome::ImportedBefore { .. }));
    assert!(matches!(token_outcome(&r, "tok_fern2"), TokenOutcome::ImportedBefore { .. }));
    let (st, _) = rpc(&base, &raw("tok_fern1"), "tools/list", json!({})).await;
    assert_eq!(st, 401);
    assert!(app.db.agent_token_by_hash(&sha(raw("tok_fern1").as_bytes())).unwrap().is_none());
}
