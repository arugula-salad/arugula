//! Grants (M130) over HTTP, through the hub's gate to the platform, which
//! holds them since step 2 (#1150): a revoked grant refuses only its own
//! machine on the Cloud (hosted) workspace routes, a machine that just joined drives
//! at once, and one that leaves and joins again starts with a new grant.
//! The hub writes no grant: a machine's joining reaches the platform through
//! the change feed (`arugula_platform::projects::follow`), or the first time
//! it asks.

use std::sync::Arc;

use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
use arugula_platform::db::projects::CLOUD_DRIVE;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::{
    App,
    routing_wire::{daemon, v2},
};

/// The hub with its platform in process: accounts `a`, `b` and `c` with
/// sessions `session-a`…. Calls go through the hub's gate and
/// `hub_port::InProcess`, as they do in production.
fn cloud_app() -> Arc<App> {
    let app = App::for_tests("https://control.example");
    let now = now_ms();
    for id in ["a", "b", "c"] {
        app.db.account_for("test", id, id, id, now).unwrap();
        app.db.add_session(&crate::auth::hash(&format!("session-{id}")), id, now, now + 60_000).unwrap();
    }
    app.start()
}

/// The app served on a port of its own: its address.
async fn serve(app: Arc<App>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() });
    (base, server)
}

/// A machine's signed call: its status and answer.
async fn call(base: &str, keys: &DeviceKeys, method: &str, path: &str) -> (StatusCode, Value) {
    let r = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-arugula-auth", v2(keys, method, path, b""))
        .send()
        .await
        .unwrap();
    (r.status(), r.json::<Value>().await.unwrap_or_default())
}

/// A person's call, by their session.
async fn person(base: &str, account: &str, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("cookie", format!("ilg_session=session-{account}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status(), r.json::<Value>().await.unwrap_or_default())
}

/// Revoking one machine's grant refuses that machine on the Cloud
/// (hosted) workspace routes and leaves the owner's other machine alone; giving
/// it back lets it in again.
#[tokio::test]
async fn a_revoked_grant_refuses_only_its_own_machine() {
    let app = cloud_app();
    let (geek, mini) = (daemon(&app, "a", "geek"), daemon(&app, "a", "mini"));
    let (geek_id, mini_id) = (geek.id(), mini.id());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() });
    let client = reqwest::Client::new();
    let call = |keys: &arugula_e2e::DeviceKeys, method: &str, path: &str, body: Option<Value>| {
        let bytes = body.map(|b| serde_json::to_vec(&b).unwrap()).unwrap_or_default();
        let mut req = client
            .request(method.parse().unwrap(), format!("{base}{path}"))
            .header("x-arugula-auth", v2(keys, method, path, &bytes));
        if !bytes.is_empty() {
            req = req.header("content-type", "application/json").body(bytes);
        }
        async move {
            let r = req.send().await.unwrap();
            (r.status(), r.json::<Value>().await.unwrap_or_default())
        }
    };
    let person = |method: &str, path: &str, body: Option<Value>| {
        let mut req =
            client.request(method.parse().unwrap(), format!("{base}{path}")).header("cookie", "ilg_session=session-a");
        if let Some(b) = body {
            req = req.json(&b);
        }
        async move {
            let r = req.send().await.unwrap();
            (r.status(), r.json::<Value>().await.unwrap_or_default())
        }
    };

    // Both joined machines hold cloud.drive over the account.
    let (status, listed) = person("GET", "/api/grants", None).await;
    assert_eq!(status, StatusCode::OK);
    let grants = listed["grants"].as_array().unwrap();
    assert_eq!(grants.len(), 2, "{listed}");
    assert!(grants.iter().all(|g| g["power"] == CLOUD_DRIVE && g["live"] == true && g["scope"]["id"] == "a"));
    assert_eq!(grants[0]["subject"]["name"], "geek");
    let geeks = grants[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(call(&geek, "GET", "/api/workspaces", None).await.0, StatusCode::OK);

    // A machine can't manage grants, not even its own.
    assert_eq!(call(&geek, "GET", "/api/grants", None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&geek, "POST", &format!("/api/grants/{geeks}/revoke"), None).await.0, StatusCode::UNAUTHORIZED);

    let (status, revoked) = person("POST", &format!("/api/grants/{geeks}/revoke"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(revoked["live"], false);
    // What `cloud` and `read_cloud` call, from the revoked machine:
    // refused, naming it and how to restore it, before anything else is
    // looked at.
    let project = "prj_0123456789abcdef0123456789abcdef";
    for (method, path, body) in [
        ("GET", "/api/workspaces".to_owned(), None),
        ("GET", format!("/api/projects/{project}/workspaces"), None),
        ("POST", format!("/api/projects/{project}/workspaces"), Some(json!({"name":"box"}))),
        ("GET", format!("/api/projects/{project}/workspaces/hw_00"), None),
        ("POST", format!("/api/projects/{project}/workspaces/hw_00/rebuild"), None),
        ("GET", format!("/api/projects/{project}/transcripts"), None),
    ] {
        let (status, said) = call(&geek, method, &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {said}");
        let said = said["error"].as_str().unwrap_or_default();
        assert!(said.starts_with("geek can't drive") && said.contains("arugula grants restore geek"), "{said}");
    }
    // The owner's other machine, and the owner, still drive them.
    let (status, listed) = call(&mini, "GET", "/api/workspaces", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["workspaces"], json!([]));
    assert_eq!(person("GET", "/api/workspaces", None).await.0, StatusCode::OK);

    // Another account can't give it back, or give one for a machine
    // that isn't its own; the owner can.
    let give = json!({"daemon": geek_id, "power": CLOUD_DRIVE});
    let (status, _) = person("POST", "/api/grants", Some(json!({"daemon": "nope", "power": CLOUD_DRIVE}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = person("POST", "/api/grants", Some(json!({"daemon": geek_id, "power": "cloud.admin"}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let other = client
        .post(format!("{base}/api/grants"))
        .header("cookie", "ilg_session=session-b")
        .json(&give)
        .send()
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    let (status, given) = person("POST", "/api/grants", Some(give.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(given["live"] == true && given["id"] != geeks.as_str(), "a new grant: {given}");
    assert_eq!(person("POST", "/api/grants", Some(give)).await.1["id"], given["id"], "a live one is the answer");
    assert_eq!(call(&geek, "GET", "/api/workspaces", None).await.0, StatusCode::OK);
    // A machine's signature still covers its one request: the hub's
    // gate takes it once, for the platform's routes as for its own.
    let path = "/api/workspaces";
    let h = v2(&geek, "GET", path, b"");
    let ok = client.get(format!("{base}{path}")).header("x-arugula-auth", &h).send().await.unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let again = client.get(format!("{base}{path}")).header("x-arugula-auth", &h).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::UNAUTHORIZED);
    // The revoked one stays on the list.
    let (_, listed) = person("GET", "/api/grants", None).await;
    let states: Vec<_> =
        listed["grants"].as_array().unwrap().iter().map(|g| (g["subject"]["id"].clone(), g["live"].clone())).collect();
    assert_eq!(states, [(json!(geek_id), json!(true)), (json!(geek_id), json!(false)), (json!(mini_id), json!(true))]);
    server.abort();
}

/// The hub's join writes no grant; the platform gives it from the change
/// feed, as soon as it's read, whether or not the machine has asked.
#[tokio::test]
async fn a_joined_machine_gets_its_grant_from_the_feed() {
    let app = cloud_app();
    let p = app.platform.inner();
    arugula_platform::projects::follow(p).await.unwrap();
    let box_ = daemon(&app, "a", "box").id();
    assert!(p.db.grants("a").unwrap().is_empty(), "nothing yet: the hub doesn't write grants");
    arugula_platform::projects::follow(p).await.unwrap();
    let grants = p.db.grants("a").unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!((grants[0].subject_id.as_str(), grants[0].granted_by.as_str()), (box_.as_str(), "a"));
    assert!(grants[0].revoked_at.is_none());
    // Reading it again gives nothing twice.
    arugula_platform::projects::follow(p).await.unwrap();
    assert_eq!(p.db.grants("a").unwrap().len(), 1);
}

/// A machine whose grant was revoked, that leaves and joins again, starts
/// with a new grant: at once, before the platform's next feed tick (the
/// check reads the feed to its end before it refuses).
#[tokio::test]
async fn a_machine_that_joins_again_starts_with_a_new_grant() {
    let app = cloud_app();
    arugula_platform::projects::follow(app.platform.inner()).await.unwrap();
    let keys = daemon(&app, "a", "geek");
    let (base, server) = serve(app.clone()).await;
    assert_eq!(call(&base, &keys, "GET", "/api/workspaces").await.0, StatusCode::OK, "it drives at once");
    let (_, listed) = person(&base, "a", "GET", "/api/grants", None).await;
    let id = listed["grants"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(person(&base, "a", "POST", &format!("/api/grants/{id}/revoke"), None).await.0, StatusCode::OK);
    assert_eq!(call(&base, &keys, "GET", "/api/workspaces").await.0, StatusCode::FORBIDDEN);

    // It's removed, and joins again with the same key, before the
    // platform's feed tick.
    app.db.drop_daemon(&keys.id()).unwrap();
    let cert = Cert::new(&keys, "a", Kind::Daemon, "geek");
    app.db.put_device(&cert, true, now_ms()).unwrap();
    app.db.put_daemon("a", &cert.device, "geek", &[]).unwrap();
    assert_eq!(call(&base, &keys, "GET", "/api/workspaces").await.0, StatusCode::OK, "a new grant");
    let (_, listed) = person(&base, "a", "GET", "/api/grants", None).await;
    let grants = listed["grants"].as_array().unwrap();
    assert_eq!(grants.len(), 1, "the revoked one went when it left: {listed}");
    assert!(grants[0]["live"] == true && grants[0]["id"] != id.as_str());
    server.abort();
}

/// Read your writes (control-smoke's "the team upgraded"): a team made, or
/// a machine joined, in the hub is in the platform's very next answer,
/// though the platform had already answered for the account and no feed
/// tick has run. A second team and a second machine too, whose account the
/// cache already kept a list for. In process and out (the gateway).
#[tokio::test]
async fn the_platform_reads_the_hubs_writes_at_once() {
    use arugula_e2e::team::TeamRole;

    use crate::routing_wire::{member, roster};
    for remote in [false, true] {
        let (app, base, _keep) = if remote {
            let s = crate::gateway_wire::split().await;
            (s.hub, s.base, None)
        } else {
            let app = cloud_app();
            let (base, server) = serve(app.clone()).await;
            (app, base, Some(server))
        };
        let get = |path: &'static str| {
            let base = base.clone();
            async move {
                let (st, v) = person(&base, "a", "GET", path, None).await;
                assert_eq!(st, StatusCode::OK, "{path}: {v}");
                v
            }
        };
        let teams = |v: &Value| v["teams"].as_array().unwrap().len();
        let names = |v: &Value| {
            let mut n: Vec<String> =
                v["grants"].as_array().unwrap().iter().map(|g| g["subject"]["name"].as_str().unwrap().into()).collect();
            n.sort();
            n
        };
        assert_eq!(teams(&get("/api/billing").await), 0, "remote {remote}");
        assert_eq!(names(&get("/api/grants").await), Vec::<String>::new());

        let keys = DeviceKeys::generate();
        for (i, id) in ["0123456789abcdef", "fedcba9876543210"].into_iter().enumerate() {
            let r = roster(id, 1, vec![member("a", &keys, TeamRole::Owner)], &keys);
            let t = crate::db::Team {
                id: id.into(),
                name: format!("team {i}"),
                founder: "a".into(),
                founder_root: keys.id(),
                locked: false,
            };
            app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
            assert_eq!(teams(&get("/api/billing").await), i + 1, "team {i} made, remote {remote}");
        }
        daemon(&app, "a", "box-1");
        assert_eq!(names(&get("/api/grants").await), ["box-1"], "remote {remote}");
        daemon(&app, "a", "box-2");
        assert_eq!(names(&get("/api/grants").await), ["box-1", "box-2"], "remote {remote}");
    }
}
