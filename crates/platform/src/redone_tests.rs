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
    let hub = Arc::new(TestHub::default());
    hub.github("a", 4242, "fern-gh");
    hub.github("b", 5151, "alder-gh");
    hub.github("c", 6161, "c-gh");
    hub.github("x", 3131, "x-gh");
    hub.team("t", &[("b", TeamRole::Owner), ("c", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.secret_key = Some(Arc::new(crate::secrets::SecretCipher::new(&"07".repeat(32)).unwrap()));
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
        dry_run,
    }
}

fn line<'a>(r: &'a Report, tenant: &str) -> &'a TenantLine {
    r.tenants.iter().find(|t| t.tenant.id == tenant).unwrap()
}

fn token_outcome<'a>(r: &'a Report, id: &str) -> &'a TokenOutcome {
    &r.tenants.iter().flat_map(|t| &t.tokens).find(|k| k.token.id == id).unwrap().outcome
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
    // It sets a secret, by name, that it can't read back.
    let r = tool(&base, &token, "set_secret", json!({ "name": "API_KEY", "value": "hunter2-m235" })).await;
    assert_eq!(ok(&r)["set"], json!(true));
    let master = app.secret_key.clone().unwrap();
    assert_eq!(crate::keys::open(&app.db, &master, &fern, "project/API_KEY").unwrap().as_deref(), Some("hunter2-m235"));
    // It plans a deploy, and asks: a gets the card.
    let (st, env) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{fern}/environments"),
        Some(json!({ "name": "fern-site", "driver": "fake", "secret_names": ["APP_TOKEN"] })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
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
        dry_run: true,
    };
    let r = import(&app, &backup, &o).await.unwrap();
    assert!(matches!(line(&r, "alder").outcome, TenantOutcome::Refused { .. }), "{:?}", line(&r, "alder"));
    let _: Value = serde_json::to_value(&r).unwrap();
}
