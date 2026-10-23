//! The Infisical projection (M235) against a fake Infisical that answers
//! only what `fixtures/infisical/answers.json` captured (Infisical's API
//! reference and redone's tests, its README says): it logs in once, makes
//! the folder, creates or else overwrites, never reads a value or deletes;
//! a dry run writes nothing; a folder that's a cluster app's, or another
//! project's, is refused; and no value is ever logged.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use super::*;
use crate::{
    chat_tests::TestHub,
    db::{Db, redone::Imported},
};

/// One request the fake got.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub auth: Option<String>,
    pub body: Value,
}

/// A fake Infisical: its folders and secrets (`folder → key → value`),
/// and every request it got.
#[derive(Default)]
pub struct Fake {
    pub folders: Mutex<Vec<String>>,
    pub secrets: Mutex<std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>>,
    pub seen: Mutex<Vec<Seen>>,
    /// The client secret it takes.
    pub secret: String,
}

fn answers() -> Value {
    serde_json::from_str(include_str!("../fixtures/infisical/answers.json")).unwrap()
}

/// A captured answer, its `{…}`s filled from `fill`.
fn answer(name: &str, fill: &[(&str, &str)]) -> Response {
    let a = &answers()[name];
    let mut body = a["body"].to_string();
    for (k, v) in fill {
        body = body.replace(&format!("{{{k}}}"), &v.replace('"', "\\\""));
    }
    let status = StatusCode::from_u16(a["status"].as_u64().unwrap() as u16).unwrap();
    (status, Json(serde_json::from_str::<Value>(&body).unwrap())).into_response()
}

async fn serve(State(f): State<Arc<Fake>>, req: Request) -> Response {
    let method = req.method().to_string();
    let uri = req.uri().clone();
    let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_owned);
    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let path = uri.path().to_owned();
    f.seen.lock().unwrap().push(Seen {
        method: method.clone(),
        path: uri.to_string(),
        auth: auth.clone(),
        body: body.clone(),
    });
    if path == "/api/v1/auth/universal-auth/login" {
        return if body["clientSecret"].as_str() == Some(&f.secret) {
            answer("login", &[])
        } else {
            answer("login_refused", &[])
        };
    }
    if auth.as_deref() != Some("Bearer fake-infisical-access-token") {
        return answer("login_refused", &[]);
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/folders") => {
            let mut a = answers()["folders"]["body"].clone();
            let template = a["folders"][0].clone();
            a["folders"] = f
                .folders
                .lock()
                .unwrap()
                .iter()
                .map(|n| {
                    let mut x = template.clone();
                    x["name"] = json!(n);
                    x["path"] = json!(format!("/{n}"));
                    x
                })
                .collect();
            (StatusCode::OK, Json(a)).into_response()
        }
        ("POST", "/api/v1/folders") => {
            let name = body["name"].as_str().unwrap().to_owned();
            let mut folders = f.folders.lock().unwrap();
            if folders.contains(&name) {
                return answer("folder_exists", &[]);
            }
            folders.push(name.clone());
            answer("folder_created", &[("name", &name)])
        }
        (m @ ("POST" | "PATCH"), p) if p.starts_with("/api/v3/secrets/raw/") => {
            let key = p.trim_start_matches("/api/v3/secrets/raw/").to_owned();
            let folder = body["secretPath"].as_str().unwrap().trim_start_matches('/').to_owned();
            let value = body["secretValue"].as_str().unwrap().to_owned();
            let mut all = f.secrets.lock().unwrap();
            let there = all.get(&folder).is_some_and(|s| s.contains_key(&key));
            if m == "POST" && there {
                return answer("secret_exists", &[]);
            }
            all.entry(folder).or_default().insert(key.clone(), value.clone());
            let project = body["workspaceId"].as_str().unwrap().to_owned();
            let env = body["environment"].as_str().unwrap().to_owned();
            answer("secret_written", &[("key", &key), ("value", &value), ("project", &project), ("environment", &env)])
        }
        _ => answer("forbidden", &[]),
    }
}

/// A fake Infisical with these folders, taking `secret`: it and its URL.
pub async fn fake(folders: &[&str], secret: &str) -> (Arc<Fake>, String) {
    let f = Arc::new(Fake {
        folders: Mutex::new(folders.iter().map(|s| (*s).to_owned()).collect()),
        secret: secret.to_owned(),
        ..Default::default()
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let app = Router::new().fallback(serve).with_state(f.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (f, url)
}

fn platform(mode: Mode, writer: Option<Writer>) -> Platform {
    let mut p = Platform::new(
        crate::Config::default(),
        Db::test(),
        Arc::new(TestHub::default()),
        reqwest::Client::new(),
        Default::default(),
    );
    p.projection = Projection::new(mode, writer);
    p
}

fn writer(url: &str, secret: &str) -> Writer {
    Writer::new(reqwest::Client::new(), url, "proj-planter-apps", "prod", "client-id", secret)
}

fn ask<'a>(project: &'a str, app: &'a str, name: &'a str, value: &'a str) -> Ask<'a> {
    Ask { project, app, name, value, by_account: Some("a"), by_agent: Some("agt_1") }
}

#[tokio::test]
async fn the_writer_logs_in_once_makes_the_folder_and_overwrites() {
    let (f, url) = fake(&["beds"], "s3cret").await;
    let w = writer(&url, "s3cret");
    assert_eq!(w.put("fern-site", "API_KEY", "v1").await.unwrap(), Put::Written);
    assert_eq!(w.put("fern-site", "API_KEY", "v2").await.unwrap(), Put::Written);
    assert_eq!(w.put("fern-site", "OTHER", "v3").await.unwrap(), Put::Written);
    assert!(w.folder_exists("beds").await.unwrap());
    assert!(w.folder_exists("fern-site").await.unwrap());
    assert!(!w.folder_exists("fresh").await.unwrap());
    let seen = f.seen.lock().unwrap().clone();
    assert_eq!(seen.iter().filter(|s| s.path.contains("login")).count(), 1, "{seen:?}");
    let writes: Vec<String> =
        seen.iter().filter(|s| s.path.starts_with("/api/v3/")).map(|s| format!("{} {}", s.method, s.path)).collect();
    assert_eq!(
        writes,
        [
            "POST /api/v3/secrets/raw/API_KEY",
            "POST /api/v3/secrets/raw/API_KEY",
            "PATCH /api/v3/secrets/raw/API_KEY",
            "POST /api/v3/secrets/raw/OTHER"
        ]
    );
    // redone's request, field for field.
    let patch = seen.iter().find(|s| s.method == "PATCH").unwrap();
    assert_eq!(
        patch.body,
        json!({ "workspaceId": "proj-planter-apps", "environment": "prod", "secretPath": "/fern-site",
                "secretValue": "v2", "type": "shared" })
    );
    let folder = seen.iter().find(|s| s.method == "POST" && s.path == "/api/v1/folders").unwrap();
    assert_eq!(
        folder.body,
        json!({ "workspaceId": "proj-planter-apps", "environment": "prod", "path": "/", "name": "fern-site" })
    );
    assert!(
        seen.iter()
            .filter(|s| !s.path.contains("login"))
            .all(|s| s.auth.as_deref() == Some("Bearer fake-infisical-access-token"))
    );
    // Never a read of a value, never a delete.
    assert!(seen.iter().all(|s| s.method != "DELETE" && !s.path.contains("viewSecretValue=true")), "{seen:?}");
    assert!(seen.iter().filter(|s| s.method == "GET").all(|s| s.path.starts_with("/api/v1/folders?")));
    assert_eq!(f.secrets.lock().unwrap()["fern-site"]["API_KEY"], "v2");

    // A wrong client secret: refused, and the secret isn't in the error.
    let bad = writer(&url, "not-it");
    let e = format!("{:#}", bad.put("fern-site", "API_KEY", "v9").await.unwrap_err());
    assert!(e.contains("401") && e.contains("Invalid credentials") && !e.contains("not-it"), "{e}");
}

#[tokio::test]
async fn off_writes_and_logs_nothing_and_a_dry_run_writes_nothing() {
    let (f, url) = fake(&["beds"], "s3cret").await;
    let off = platform(Mode::Off, Some(writer(&url, "s3cret")));
    assert!(project(&off, &ask("prj_1", "fern-site", "API_KEY", "v1")).await.unwrap().is_none());
    assert!(off.db.projections("prj_1", 10).unwrap().is_empty());
    assert!(f.seen.lock().unwrap().is_empty());

    // Dry, with no credentials: nothing asked of Infisical at all.
    let dry = platform(Mode::DryRun, None);
    let r = project(&dry, &ask("prj_1", "fern-site", "API_KEY", "v1")).await.unwrap().unwrap();
    assert_eq!((r.mode.as_str(), r.outcome.as_str()), ("dry_run", "would_write"));
    assert!(r.detail.contains("wasn't checked"), "{r:?}");

    // Dry, with them: the folder check only (a read), no write.
    let dry = platform(Mode::DryRun, Some(writer(&url, "s3cret")));
    let r = project(&dry, &ask("prj_1", "fern-site", "API_KEY", "v1")).await.unwrap().unwrap();
    assert_eq!(r.outcome, "would_write", "{r:?}");
    let r = project(&dry, &ask("prj_1", "beds", "API_KEY", "v1")).await.unwrap().unwrap();
    assert_eq!(r.outcome, "refused", "{r:?}");
    let seen = f.seen.lock().unwrap().clone();
    assert!(seen.iter().all(|s| s.method == "GET" || s.path.contains("login")), "{seen:?}");
    assert!(f.secrets.lock().unwrap().is_empty());
    assert_eq!(f.folders.lock().unwrap().clone(), ["beds"]);
}

#[tokio::test]
async fn on_writes_only_into_the_projects_own_folder_and_never_logs_a_value() {
    let (f, url) = fake(&["beds", "fern-old"], "s3cret").await;
    let p = platform(Mode::On, Some(writer(&url, "s3cret")));
    // fern-old was redone's app, carried over to prj_1: its folder is there,
    // and it's prj_1's.
    p.db.record_redone_import(&Imported {
        kind: "app".into(),
        redone_id: "fern-old".into(),
        tenant: "fern".into(),
        project: "prj_1".into(),
        account: None,
        platform_id: None,
        at: 1,
    })
    .unwrap();

    let value = "hunter2-m235";
    let r = project(&p, &ask("prj_1", "fern-site", "API_KEY", value)).await.unwrap().unwrap();
    assert_eq!((r.mode.as_str(), r.outcome.as_str()), ("write", "written"), "{r:?}");
    let r = project(&p, &ask("prj_1", "fern-site", "API_KEY", "again")).await.unwrap().unwrap();
    assert_eq!(r.outcome, "written", "{r:?}");
    let r = project(&p, &ask("prj_1", "fern-old", "DB_URL", value)).await.unwrap().unwrap();
    assert_eq!(r.outcome, "written", "{r:?}");
    assert_eq!(f.secrets.lock().unwrap()["fern-site"]["API_KEY"], "again");
    assert_eq!(f.secrets.lock().unwrap()["fern-old"]["DB_URL"], value);

    // BREAK: a cluster app's folder, another project's, a bad name.
    let r = project(&p, &ask("prj_1", "beds", "LOBBY_SECRET", value)).await.unwrap().unwrap();
    assert_eq!(r.outcome, "refused", "{r:?}");
    assert!(r.detail.contains("cluster app"), "{r:?}");
    let r = project(&p, &ask("prj_2", "fern-site", "API_KEY", value)).await.unwrap().unwrap();
    assert_eq!(r.outcome, "refused", "{r:?}");
    assert!(r.detail.contains("another project"), "{r:?}");
    let r = project(&p, &ask("prj_2", "fern-old", "API_KEY", value)).await.unwrap().unwrap();
    assert_eq!(r.outcome, "refused", "{r:?}");
    for (app, key) in [("planter-x", "A"), ("Fern", "A"), ("../x", "A"), ("fern-site", "1BAD"), ("fern-site", "A-B")] {
        let r = project(&p, &ask("prj_1", app, key, value)).await.unwrap().unwrap();
        assert_eq!(r.outcome, "refused", "{app}/{key}: {r:?}");
    }
    assert!(!f.secrets.lock().unwrap().contains_key("beds"));

    // Infisical down: failed, logged, and no value in it.
    let down = platform(Mode::On, Some(writer("http://127.0.0.1:9", "s3cret")));
    let r = project(&down, &ask("prj_9", "fern-new", "API_KEY", value)).await.unwrap().unwrap();
    assert_eq!(r.outcome, "failed", "{r:?}");

    let mut rows = p.db.projections("prj_1", 50).unwrap();
    rows.extend(p.db.projections("prj_2", 50).unwrap());
    rows.extend(down.db.projections("prj_9", 50).unwrap());
    assert!(rows.len() >= 10, "{rows:?}");
    let dump = format!("{rows:?}");
    assert!(!dump.contains(value) && !dump.contains("again"), "{dump}");
    assert!(rows.iter().all(|r| r.by_account.as_deref() == Some("a") && r.by_agent.as_deref() == Some("agt_1")));
}

#[test]
fn modes_and_args() {
    assert_eq!(Mode::parse("off"), Some(Mode::Off));
    assert_eq!(Mode::parse(""), Some(Mode::Off));
    assert_eq!(Mode::parse("dry-run"), Some(Mode::DryRun));
    assert_eq!(Mode::parse("on"), Some(Mode::On));
    assert_eq!(Mode::parse("yes"), None);
    let http = reqwest::Client::new();
    let on = Args { projection: "on".into(), environment: "prod".into(), ..Default::default() };
    assert!(on.projection(&http).is_err(), "on needs the writer");
    let dry = Args { projection: "dry-run".into(), environment: "prod".into(), ..Default::default() };
    let d = dry.projection(&http).unwrap();
    assert!(d.mode == Mode::DryRun && d.writer.is_none());
    let full = Args {
        projection: "on".into(),
        url: Some("https://infisical.example/".into()),
        project_id: Some("p".into()),
        environment: "prod".into(),
        client_id: Some("id".into()),
        client_secret: Some("secret".into()),
    };
    let w = full.projection(&http).unwrap();
    assert!(w.mode == Mode::On && w.writer.is_some());
    assert!(!format!("{:?}", w.writer.unwrap()).contains("secret"));
    assert!(valid_app("fern-site") && !valid_app("planter-apps") && !valid_app("-x") && !valid_app(&"a".repeat(33)));
    assert!(valid_key("API_KEY") && valid_key("_x") && !valid_key("9A") && !valid_key(""));
}
