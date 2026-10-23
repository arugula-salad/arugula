//! Public previews (M187) through the platform's own routes: who may make
//! a port public, renew it and turn it off, what the hub and the machine
//! are told, and a control with previews off. Runs on SQLite and, with
//! `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_e2e::{now_ms, team::TeamRole};
use serde_json::json;

use super::{
    Config, EnrollBy,
    previews::PUBLIC_FOR_MS,
    tests::{Fake, make},
};
use crate::{
    Platform, PlatformPort,
    chat_tests::{TestHub, call},
};

/// As `tests::serve`, with public previews on `previews.example`.
async fn serve(domain: Option<&str>) -> (Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let mut p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    p.compute = Some(Arc::new(Fake::default()));
    p.hosted = Config {
        template: Some("tpl".into()),
        enroll: EnrollBy::Sandbox,
        accounts: vec!["alice".into(), "bob".into()],
        preview_domain: domain.map(Into::into),
        ..Default::default()
    };
    let app = Arc::new(p);
    let project = app.db.team_project("t1", "t1", 0).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (app, base, project)
}

/// An editor makes a port public: it gets an unguessable host under the
/// preview domain, says who and until when (7 days), and the hub and the
/// machine both hear of it. Renewing keeps the host; off ends it at once;
/// a viewer and an outsider can't change it.
#[tokio::test]
async fn an_editor_makes_a_port_public() {
    let (app, base, project) = serve(Some("previews.example")).await;
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let port = format!("/api/projects/{project}/workspaces/{id}/ports/5173/public");

    assert_eq!(call(&base, "vic", "PUT", &port, None).await.0, 403, "a viewer");
    assert_eq!(call(&base, "olga", "PUT", &port, None).await.0, 404, "not a member");
    let (status, p) = call(&base, "bob", "PUT", &port, Some(json!({ "renew": true }))).await;
    assert_eq!(status, 409, "renewing a port that isn't public: {p}");

    let before = now_ms();
    let (status, p) = call(&base, "bob", "PUT", &port, None).await;
    assert_eq!(status, 200, "{p}");
    let url = p["url"].as_str().unwrap().to_owned();
    let host = url.strip_prefix("https://").and_then(|h| h.strip_suffix(".previews.example")).unwrap();
    assert!(host.len() == 32 && host.bytes().all(|b| b.is_ascii_hexdigit()), "{url}");
    assert!(!url.contains(&id[3..]), "not the workspace's id");
    assert_eq!(p["public_by"], "bob");
    let until = p["public_until"].as_u64().unwrap();
    assert!(until >= before + PUBLIC_FOR_MS && until <= now_ms() + PUBLIC_FOR_MS);

    // Every member sees it, with who made it public.
    let (status, listed) =
        call(&base, "vic", "GET", &format!("/api/projects/{project}/workspaces/{id}/ports"), None).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["previews"], true);
    assert_eq!(listed["ports"][0]["port"], 5173);
    assert_eq!(listed["ports"][0]["public"], true);
    assert_eq!(listed["ports"][0]["url"], url.as_str());
    assert_eq!(listed["ports"][0]["public_by"], "bob");

    // The hub finds it by its host label; the machine by its workspace.
    let found = app.public_preview(host).await.unwrap().unwrap();
    assert_eq!((found.workspace.as_str(), found.port, found.project.as_str()), (id.as_str(), 5173, project.as_str()));
    assert_eq!(app.public_ports(&id).await.unwrap(), [(5173, host.to_owned())]);
    assert_eq!(app.public_preview("not-a-preview").await.unwrap(), None);

    // Renewed by alice: same host, her name.
    let (status, p) = call(&base, "alice", "PUT", &port, Some(json!({ "renew": true }))).await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["url"], url.as_str());
    assert_eq!(p["public_by"], "alice");

    assert_eq!(call(&base, "vic", "DELETE", &port, None).await.0, 403);
    let (status, off) = call(&base, "bob", "DELETE", &port, None).await;
    assert_eq!((status, off["was_public"].clone()), (200, json!(true)));
    assert_eq!(app.public_preview(host).await.unwrap(), None);
    assert!(app.public_ports(&id).await.unwrap().is_empty());

    assert_eq!(
        call(&base, "bob", "PUT", &format!("/api/projects/{project}/workspaces/{id}/ports/0/public"), None).await.0,
        400
    );
}

/// Without ARUGULA_PREVIEW_DOMAIN nothing can be made public, and a row
/// that says public (from before it was turned off) isn't served.
#[tokio::test]
async fn previews_off() {
    let (app, base, project) = serve(None).await;
    let (_, w) = make(&base, "alice", &project, "dev").await;
    let id = w["id"].as_str().unwrap().to_owned();
    let port = format!("/api/projects/{project}/workspaces/{id}/ports/5173/public");
    assert_eq!(call(&base, "alice", "PUT", &port, None).await.0, 404);
    let hex = "0123456789abcdef0123456789abcdef";
    app.db.make_port_public(&id, 5173, "alice", hex, now_ms() + 60_000, now_ms()).unwrap();
    assert_eq!(app.public_preview(hex).await.unwrap(), None);
    assert!(app.public_ports(&id).await.unwrap().is_empty());
    let (status, listed) =
        call(&base, "alice", "GET", &format!("/api/projects/{project}/workspaces/{id}/ports"), None).await;
    assert_eq!((status, listed["previews"].clone()), (200, json!(false)));
}
