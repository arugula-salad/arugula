//! studio as a client (M236) through the platform's routes: an account's
//! switch before a client sees its projects, apps kept by project (a
//! teammate opens an app they didn't add; a viewer can't keep one; a member
//! who left can't open it a moment later), the secrets an owner lets a
//! client read and every read counted, and what unlinking and deleting an
//! account take. On SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::TestHub,
    hosted::broker::{
        Config,
        clients::{self, Clients},
    },
    secrets::SecretCipher,
};

const STUDIO: &str = "studio-client-credential-test-0123456789"; // gitleaks:allow: a test value
const HUD: &str = "hud-client-credential-test-0123456789"; // gitleaks:allow: a test value
const STRIPE: &str = "sk_test_stripe-value-0123456789"; // gitleaks:allow: a test value

struct Rig {
    app: Arc<Platform>,
    hub: Arc<TestHub>,
    base: String,
    /// Team t1's project: alice owns it, bob edits it, vic views it. dana
    /// owns t2's.
    t1: String,
    t2: String,
}

async fn rig() -> Rig {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    hub.team("t2", &[("dana", TeamRole::Owner)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    let registered = clients::parse(&[
        format!("studio:{}:20:payers", hex::encode(clients::digest(STUDIO))),
        format!("hud:{}", hex::encode(clients::digest(HUD))),
    ])
    .unwrap();
    let cipher = Arc::new(SecretCipher::new(&"01".repeat(32)).unwrap());
    p.hosted = crate::hosted::Config {
        cipher: Some(cipher.clone()),
        broker: Config { clients: Clients::new(registered), ..Config::new(None, "", 20.0, 4, &[], &[]).unwrap() },
        ..Default::default()
    };
    p.secret_key = Some(cipher);
    let app = Arc::new(p);
    let t1 = app.db.team_project("t1", "t1", 0).unwrap();
    let t2 = app.db.team_project("t2", "t2", 0).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    Rig { app, hub, base, t1, t2 }
}

impl Rig {
    /// As a client by its credential, for `account` when it names one.
    async fn client(
        &self,
        credential: &str,
        account: Option<&str>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut req = reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .bearer_auth(credential)
            .header("origin", "https://evil.example");
        if let Some(a) = account {
            req = req.header("arugula-account", a);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn studio(&self, account: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.client(STUDIO, Some(account), method, path, body).await
    }

    /// As a person on the platform page.
    async fn page(&self, account: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("cookie", format!("ilg_session=session-{account}"))
            .header("origin", "https://control.example");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    /// Studio reads STRIPE for `app`, as its egress broker does.
    async fn read(&self, app: &str) -> (u16, Value) {
        self.client(STUDIO, None, "GET", &format!("/api/client/apps/{app}/secrets/STRIPE"), None).await
    }

    async fn let_see(&self, account: &str) {
        let (s, v) = self.page(account, "PUT", "/api/linked-products/studio/projects", None).await;
        assert_eq!((s, &v["projects"]), (200, &json!(true)), "{v}");
    }
}

fn reason(v: &Value) -> &str {
    v["reason"].as_str().unwrap_or_default()
}

/// Nothing until the account lets the client see its projects: then its
/// projects, its role in each and their members, as the page shows them.
#[tokio::test]
async fn a_client_sees_projects_only_once_the_account_lets_it() {
    let r = rig().await;
    let (s, v) = r.studio("alice", "GET", "/api/client/projects", None).await;
    assert_eq!((s, reason(&v)), (403, "consent"), "{v}");
    assert!(v["error"].as_str().unwrap().contains("Linked products"), "it says where to turn it on: {v}");
    for path in ["/api/client/apps", "/api/client/apps/app-1"] {
        assert_eq!(reason(&r.studio("alice", "GET", path, None).await.1), "consent", "{path}");
    }
    let put = r.studio("alice", "PUT", "/api/client/apps/app-1", Some(json!({ "project": r.t1, "title": "x" }))).await;
    assert_eq!(reason(&put.1), "consent");

    // A wrong credential, no account, and a session with no client.
    let (s, v) = r.client("not-a-credential", Some("alice"), "GET", "/api/client/projects", None).await;
    assert_eq!((s, reason(&v)), (401, "client"));
    let (s, v) = r.client(STUDIO, None, "GET", "/api/client/projects", None).await;
    assert_eq!((s, reason(&v)), (400, "account"));
    let (s, _) = r.page("alice", "GET", "/api/client/projects", None).await;
    assert_eq!(s, 401, "a session isn't a client");

    // Only a registered client's switch.
    assert_eq!(r.page("alice", "PUT", "/api/linked-products/nobody/projects", None).await.0, 404);
    r.let_see("alice").await;
    let (s, v) = r.studio("alice", "GET", "/api/client/projects", None).await;
    assert_eq!(s, 200, "{v}");
    let projects = v["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 2, "{v}");
    assert_eq!((projects[0]["personal"].as_bool(), projects[0]["role"].as_str()), (Some(true), Some("owner")));
    assert_eq!((projects[1]["id"].as_str(), projects[1]["role"].as_str()), (Some(r.t1.as_str()), Some("owner")));
    let members: Vec<(&str, &str)> = projects[1]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["account"].as_str().unwrap(), m["role"].as_str().unwrap()))
        .collect();
    assert_eq!(members, [("alice", "owner"), ("bob", "editor"), ("vic", "viewer")]);
    // Another client of alice's isn't let.
    assert_eq!(reason(&r.client(HUD, Some("alice"), "GET", "/api/client/projects", None).await.1), "consent");
    // Bob hasn't let it: studio can't act for him because alice did.
    assert_eq!(reason(&r.studio("bob", "GET", "/api/client/projects", None).await.1), "consent");

    // Turned off: refused again at once.
    assert_eq!(r.page("alice", "DELETE", "/api/linked-products/studio/projects", None).await.0, 200);
    assert_eq!(reason(&r.studio("alice", "GET", "/api/client/projects", None).await.1), "consent");
}

/// An app is its project's members': a teammate opens one they didn't
/// add, a viewer sees it but can't keep one, someone outside the project
/// can't, and a member who left can't open it a moment later.
#[tokio::test]
async fn apps_are_kept_by_project_and_a_teammate_opens_one() {
    let r = rig().await;
    for a in ["alice", "bob", "vic", "dana"] {
        r.let_see(a).await;
    }
    let (s, v) = r
        .studio("alice", "PUT", "/api/client/apps/app-1", Some(json!({ "project": r.t1, "title": "Pinboard\u{7}" })))
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        (v["project"].as_str(), v["title"].as_str(), v["role"].as_str()),
        (Some(r.t1.as_str()), Some("Pinboard"), Some("owner"))
    );
    assert_eq!(v["added_by"], "alice");

    // Bob, who didn't add it, sees it and opens it as an editor.
    let (s, v) = r.studio("bob", "GET", "/api/client/apps", None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["apps"][0]["app"], "app-1");
    assert_eq!((v["apps"][0]["role"].as_str(), v["apps"][0]["project_name"].as_str()), (Some("editor"), Some("t1")));
    let (s, v) = r.studio("bob", "GET", "/api/client/apps/app-1", None).await;
    assert_eq!((s, v["role"].as_str()), (200, Some("editor")), "{v}");
    // Vic views it; dana isn't in its project.
    assert_eq!(r.studio("vic", "GET", "/api/client/apps/app-1", None).await.1["role"], "viewer");
    let (s, v) = r.studio("dana", "GET", "/api/client/apps/app-1", None).await;
    assert_eq!((s, reason(&v)), (403, "not_member"));
    assert_eq!(r.studio("dana", "GET", "/api/client/apps", None).await.1["apps"], json!([]));
    let (s, v) = r.studio("dana", "GET", "/api/client/apps/app-9", None).await;
    assert_eq!((s, reason(&v)), (404, "no_app"));

    // A viewer can't keep an app in the project, nor move one out of it.
    let (s, v) = r.studio("vic", "PUT", "/api/client/apps/app-2", Some(json!({ "project": r.t1 }))).await;
    assert_eq!((s, reason(&v)), (403, "role"));
    let (s, v) = r.studio("vic", "PUT", "/api/client/apps/app-1", Some(json!({ "project": "personal" }))).await;
    assert_eq!((s, reason(&v)), (403, "role"));
    // Dana can't take it into her team: she isn't in the one it leaves.
    let (s, v) = r.studio("dana", "PUT", "/api/client/apps/app-1", Some(json!({ "project": r.t2 }))).await;
    assert_eq!((s, reason(&v)), (403, "not_member"));
    // Nor put one in a project she isn't in.
    let (s, v) = r.studio("dana", "PUT", "/api/client/apps/app-3", Some(json!({ "project": r.t1 }))).await;
    assert_eq!((s, reason(&v)), (403, "not_member"));
    // A bad id.
    assert_eq!(r.studio("alice", "GET", "/api/client/apps/a%2Fb", None).await.0, 400);

    // Hud's apps are hud's: it doesn't see studio's.
    assert_eq!(r.page("alice", "PUT", "/api/linked-products/hud/projects", None).await.0, 200);
    assert_eq!(reason(&r.client(HUD, Some("alice"), "GET", "/api/client/apps/app-1", None).await.1), "no_app");

    // The project's members see it in its settings.
    let (s, v) = r.page("vic", "GET", &format!("/api/projects/{}/clients", r.t1), None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["apps"][0]["app"].as_str(), v["apps"][0]["client"].as_str()), (Some("app-1"), Some("studio")));
    assert!(v.get("grants").is_none(), "grants are for owners: {v}");

    // Bob leaves the team: the next open is refused, read fresh.
    r.hub.team("t1", &[("alice", TeamRole::Owner), ("vic", TeamRole::Viewer)]);
    let (s, v) = r.studio("bob", "GET", "/api/client/apps/app-1", None).await;
    assert_eq!((s, reason(&v)), (403, "not_member"), "{v}");

    // Alice moves it to her own project, then it's hers alone; a new title
    // without moving it keeps who put it there.
    let (s, v) =
        r.studio("alice", "PUT", "/api/client/apps/app-1", Some(json!({ "project": "personal", "title": "" }))).await;
    assert_eq!((s, v["personal"].as_bool(), v["title"].as_str()), (200, Some(true), Some("Pinboard")), "{v}");
    assert_eq!(reason(&r.studio("vic", "GET", "/api/client/apps/app-1", None).await.1), "not_member");
    let (_, v) = r
        .studio("alice", "PUT", "/api/client/apps/app-1", Some(json!({ "project": "personal", "title": "Board" })))
        .await;
    assert_eq!((v["title"].as_str(), v["added_by"].as_str()), (Some("Board"), Some("alice")));

    // The client forgets it (retired), with no account.
    let (s, v) = r.client(STUDIO, None, "DELETE", "/api/client/apps/app-1", None).await;
    assert_eq!((s, &v["removed"]), (200, &json!(true)));
    assert_eq!(reason(&r.studio("alice", "GET", "/api/client/apps/app-1", None).await.1), "no_app");
}

/// A project's owner sets a secret and lets studio read it by name; studio
/// reads it for its app there and nowhere else; each read is counted and
/// kept as an event; revoking it, or clearing the secret, stops the next.
#[tokio::test]
async fn a_client_reads_a_project_secret_only_by_an_owners_grant() {
    let r = rig().await;
    for a in ["alice", "dana"] {
        r.let_see(a).await;
    }
    r.studio("alice", "PUT", "/api/client/apps/app-1", Some(json!({ "project": r.t1 }))).await;
    r.studio("dana", "PUT", "/api/client/apps/app-2", Some(json!({ "project": r.t2 }))).await;
    let t1 = format!("/api/projects/{}", r.t1);

    // Only an owner sets one, and only a valid name.
    let (s, _) = r.page("bob", "PUT", &format!("{t1}/secrets/STRIPE"), Some(json!({ "value": STRIPE }))).await;
    assert_eq!(s, 403, "an editor doesn't");
    assert_eq!(r.page("alice", "PUT", &format!("{t1}/secrets/ARUGULA_X"), Some(json!({ "value": "v" }))).await.0, 400);
    assert_eq!(r.page("alice", "PUT", &format!("{t1}/secrets/STRIPE"), Some(json!({ "value": "" }))).await.0, 400);
    let (s, v) = r.page("alice", "PUT", &format!("{t1}/secrets/STRIPE"), Some(json!({ "value": STRIPE }))).await;
    assert_eq!((s, &v["set"]), (200, &json!(true)), "{v}");
    let (_, v) = r.page("alice", "GET", &format!("{t1}/secrets"), None).await;
    assert_eq!(v["secrets"][0]["name"], "STRIPE");
    assert!(!v.to_string().contains(STRIPE), "never shown");

    // Not granted: refused, with no account needed to say so.
    let (s, v) = r.read("app-1").await;
    assert_eq!((s, reason(&v)), (403, "no_grant"), "{v}");
    assert!(!v.to_string().contains(STRIPE));

    // An editor can't grant; an owner grants only a secret that's set.
    assert_eq!(r.page("bob", "PUT", &format!("{t1}/clients/studio/secrets/STRIPE"), None).await.0, 403);
    assert_eq!(r.page("alice", "PUT", &format!("{t1}/clients/studio/secrets/OTHER"), None).await.0, 404);
    assert_eq!(r.page("alice", "PUT", &format!("{t1}/clients/nobody/secrets/STRIPE"), None).await.0, 404);
    let (s, v) = r.page("alice", "PUT", &format!("{t1}/clients/studio/secrets/STRIPE"), None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["grants"][0]["name"].as_str(), v["grants"][0]["reads"].as_u64()), (Some("STRIPE"), Some(0)));

    let (s, v) = r.read("app-1").await;
    assert_eq!((s, v["value"].as_str()), (200, Some(STRIPE)), "{v}");
    // Not for an app in another project, nor for another client, nor
    // another name, nor an app the client doesn't keep.
    assert_eq!(reason(&r.read("app-2").await.1), "no_grant");
    assert_eq!(reason(&r.client(HUD, None, "GET", "/api/client/apps/app-1/secrets/STRIPE", None).await.1), "no_app");
    assert_eq!(
        reason(&r.client(STUDIO, None, "GET", "/api/client/apps/app-1/secrets/OTHER", None).await.1),
        "no_grant"
    );
    assert_eq!(reason(&r.read("app-9").await.1), "no_app");
    r.read("app-1").await;

    // The owners see the reads and the events; no value.
    let (_, v) = r.page("alice", "GET", &format!("{t1}/clients"), None).await;
    assert_eq!(v["grants"][0]["reads"].as_u64(), Some(2), "{v}");
    let kinds: Vec<&str> = v["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["read", "read", "granted"], "{v}");
    assert_eq!((v["events"][0]["app"].as_str(), v["events"][2]["by"].as_str()), (Some("app-1"), Some("alice")));
    assert!(!v.to_string().contains(STRIPE));

    // Revoked: the next read is refused.
    assert_eq!(r.page("alice", "DELETE", &format!("{t1}/clients/studio/secrets/STRIPE"), None).await.0, 200);
    assert_eq!(reason(&r.read("app-1").await.1), "no_grant");
    // Granted again, then the secret cleared: its grants end with it.
    r.page("alice", "PUT", &format!("{t1}/clients/studio/secrets/STRIPE"), None).await;
    assert_eq!(r.read("app-1").await.0, 200);
    let (s, v) = r.page("alice", "DELETE", &format!("{t1}/secrets/STRIPE"), None).await;
    assert_eq!((s, v["grants_ended"].as_u64()), (200, Some(1)), "{v}");
    assert_eq!(reason(&r.read("app-1").await.1), "no_grant");
    let (_, v) = r.page("alice", "GET", &format!("{t1}/clients"), None).await;
    assert_eq!(v["grants"], json!([]));
    assert_eq!(v["events"][0]["kind"], "revoked");
    // The values are nowhere in the database in the clear.
    assert!(r.app.db.mentions(STRIPE).is_empty(), "sealed");
}

/// Unlinking the product on the platform page ends what the account let
/// it do here too; deleting the account forgets it everywhere.
#[tokio::test]
async fn unlinking_ends_the_switches_and_an_account_goes_with_its_apps() {
    let r = rig().await;
    r.hub.product("alice", "studio", "u-alice");
    r.let_see("alice").await;
    r.app.db.set_consent("alice", "studio", true, 1).unwrap();
    let (_, v) = r.page("alice", "GET", "/api/linked-products", None).await;
    assert_eq!(
        (v["links"][0]["client"].as_str(), v["links"][0]["projects"].as_bool()),
        (Some("studio"), Some(true)),
        "{v}"
    );
    r.studio("alice", "PUT", "/api/client/apps/app-1", Some(json!({ "project": "personal" }))).await;
    r.studio("alice", "PUT", "/api/client/apps/app-2", Some(json!({ "project": r.t1 }))).await;

    let (s, _) = r.page("alice", "POST", "/api/linked-products/studio/unlink", None).await;
    assert_eq!(s, 200);
    assert_eq!(reason(&r.studio("alice", "GET", "/api/client/apps", None).await.1), "consent");
    assert!(!r.app.db.consented("alice", "studio").unwrap(), "Claude too");
    assert_eq!(r.page("alice", "POST", "/api/linked-products/studio/unlink", None).await.0, 404, "nothing left");

    // Alice's account is deleted: her own project's app goes, t1's stays, by nobody.
    let deleted = crate::Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
    crate::PlatformPort::account_deleted(&*r.app, &deleted).await.unwrap();
    assert!(r.app.db.client_app("studio", "app-1").unwrap().is_none());
    let kept = r.app.db.client_app("studio", "app-2").unwrap().unwrap();
    assert_eq!((kept.project.as_str(), kept.added_by), (r.t1.as_str(), None));
    assert!(r.app.db.client_consents("alice", crate::clients::Scope::Projects).unwrap().is_empty());
}
