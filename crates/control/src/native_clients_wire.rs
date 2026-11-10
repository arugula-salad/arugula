//! A native app (aru-131): a `browser` device paired by code, whose
//! signature counts only on routes that list `native`, with `native_clients`
//! on. Off, or for any other browser, nothing changes.

use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
use serde_json::{Value, json};

use crate::{
    clients::{PAGE_ONLY, native_may},
    routing_wire::{Control, control, person, session, v2},
};

/// A request signed by `keys`.
async fn signed(c: &Control, keys: &DeviceKeys, method: &str, path: &str, body: Value) -> (u16, Value) {
    let b = serde_json::to_vec(&body).unwrap();
    let r = c
        .http
        .request(method.parse().unwrap(), format!("{}{path}", c.base))
        .header("x-arugula-auth", v2(keys, method, path, if method == "GET" { b"" } else { &b }))
        .header("content-type", "application/json");
    let r = if method == "GET" { r } else { r.body(b) };
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// The join a device asks for, with its key's proof.
fn ask(keys: &DeviceKeys, kind: Kind, name: &str) -> (Value, Cert) {
    let cert = Cert::new(keys, "", kind, name);
    let ms = now_ms();
    let sig = hex::encode(keys.signature(arugula_e2e::cert::join_proof_body(&cert, ms).as_bytes()));
    (json!({ "cert": cert, "urls": [], "proof": { "ms": ms, "sig": sig } }), cert)
}

/// Pair a new app by code, as a signed-in browser approves one.
async fn pair(c: &Control, root: &DeviceKeys, account: &str, cookie: &str, name: &str) -> DeviceKeys {
    let keys = DeviceKeys::generate();
    let (body, _) = ask(&keys, Kind::Browser, name);
    let r = c.http.post(format!("{}/api/join", c.base)).json(&body).send().await.unwrap();
    assert_eq!(r.status(), 200, "the app asks to join");
    let joined: Value = r.json().await.unwrap();
    let code = joined["code"].as_str().unwrap().to_owned();
    let mut cert = Cert::new(&keys, account, Kind::Browser, name);
    cert.sign_with(root);
    let (st, v) =
        c.as_person(cookie, "POST", &format!("/api/joins/{code}/approve"), Some(json!({ "cert": cert }))).await;
    assert_eq!(st, 200, "the browser approves it: {v}");
    keys
}

/// A browser's own key, approved the ordinary way, and so not an app's.
fn page_browser(c: &Control, root: &DeviceKeys, account: &str) -> DeviceKeys {
    let keys = DeviceKeys::generate();
    let mut cert = Cert::new(&keys, account, Kind::Browser, "Safari on iPhone");
    cert.sign_with(root);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    keys
}

#[test]
fn rows_say_what_an_app_calls() {
    let get = axum::http::Method::GET;
    assert!(native_may(&get, "/api/me"));
    assert!(native_may(&get, "/api/relay/c/{id}"));
    assert!(native_may(&get, "/api/chat/sidebar"));
    assert!(!native_may(&get, "/api/me/passkeys"), "account security stays on the web");
    assert!(!native_may(&axum::http::Method::POST, "/api/devices"), "only a page enrolls a device");
    assert!(!native_may(&get, "/api/not-a-route"), "not listed: no");
}

/// Everything an app's route does for it with the flag on, and refuses.
#[tokio::test]
async fn a_paired_app_signs_only_its_own_routes() {
    let c = control(|a| a.cfg.native_clients = true).await;
    let root = person(&c.app, "jake", "a1");
    let cookie = session(&c.app, "a1");
    let app = pair(&c, &root, "a1", &cookie, "Arugula on iPhone").await;

    // A hub route, then a platform one through the gateway, as its person.
    let (st, v) = signed(&c, &app, "GET", "/api/me", Value::Null).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["account"], "a1");
    let (st, v) = signed(&c, &app, "GET", "/api/projects", Value::Null).await;
    assert_eq!(st, 200, "{v}");

    // What isn't listed: refused before its handler looks.
    for (method, path) in [("GET", "/api/me/passkeys"), ("POST", "/api/devices"), ("GET", "/api/me/sessions")] {
        let (st, v) = signed(&c, &app, method, path, json!({})).await;
        assert_eq!((st, v["reason"].as_str()), (401, Some(PAGE_ONLY)), "{method} {path}: {v}");
    }

    // A signature's used once.
    let path = "/api/me";
    let auth = v2(&app, "GET", path, b"");
    let once = || c.http.get(format!("{}{path}", c.base)).header("x-arugula-auth", &auth).send();
    assert_eq!(once().await.unwrap().status(), 200);
    assert_eq!(once().await.unwrap().status(), 401);

    // Removed: it signs nothing any more.
    c.app.db.add_revocation(&arugula_e2e::Revocation::new("a1", &app.id(), &root)).unwrap();
    let (st, _) = signed(&c, &app, "GET", "/api/me", Value::Null).await;
    assert_ne!(st, 200, "a removed app");
}

/// Every route routes.toml doesn't mark `native` refuses a paired app's
/// signature, before its handler looks. Those it marks don't refuse it so.
#[tokio::test]
async fn an_app_is_refused_on_every_route_that_isnt_its_own() {
    let c = control(|a| a.cfg.native_clients = true).await;
    let root = person(&c.app, "jake", "a1");
    let cookie = session(&c.app, "a1");
    let app = pair(&c, &root, "a1", &cookie, "Arugula on iPhone").await;
    let probe = |path: &str| {
        path.split('/')
            .map(|s| {
                if s.starts_with("{*") {
                    "probe/x"
                } else if s.starts_with('{') {
                    "probe"
                } else {
                    s
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    };
    let (mut refused, mut allowed) = (0, 0);
    for (method, path, mine) in crate::clients::native_rows() {
        let method = if method == "ANY" { "POST".to_owned() } else { method };
        let path = probe(&path);
        let body: &[u8] = if method == "GET" || method == "DELETE" { b"" } else { b"{}" };
        let mut req = c
            .http
            .request(method.parse().unwrap(), format!("{}{path}", c.base))
            .header("x-arugula-auth", v2(&app, &method, &path, body));
        if !body.is_empty() {
            req = req.header("content-type", "application/json").body(body.to_vec());
        }
        // A route that streams or waits may not answer at once: it wasn't
        // refused, which is all this asks.
        let (st, v) = match tokio::time::timeout(std::time::Duration::from_secs(5), req.send()).await {
            Ok(Ok(r)) => (r.status().as_u16(), r.json::<Value>().await.unwrap_or_default()),
            _ => (0, Value::Null),
        };
        if mine {
            allowed += 1;
            assert_ne!(v["reason"].as_str(), Some(PAGE_ONLY), "{method} {path} is the app's");
        } else {
            refused += 1;
            assert_eq!((st, v["reason"].as_str()), (401, Some(PAGE_ONLY)), "{method} {path}: {v}");
        }
    }
    assert!(allowed > 100 && refused > 200, "{allowed} allowed, {refused} refused");
}

/// A browser that didn't pair as an app signs nothing, flag or no flag: a
/// key taken from a page doesn't work from anywhere.
#[tokio::test]
async fn a_pages_key_signs_nothing() {
    let c = control(|a| a.cfg.native_clients = true).await;
    let root = person(&c.app, "jake", "a1");
    let page = page_browser(&c, &root, "a1");
    let (st, v) = signed(&c, &page, "GET", "/api/me", Value::Null).await;
    assert_eq!(st, 401, "{v}");
    let (st, v) = signed(&c, &root, "GET", "/api/me", Value::Null).await;
    assert_eq!(st, 401, "the account's first browser: {v}");
}

/// With the flag off nothing changes: an app can't pair, and one paired
/// while it was on signs nothing.
#[tokio::test]
async fn off_nothing_changes() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let keys = DeviceKeys::generate();
    let (body, _) = ask(&keys, Kind::Browser, "Arugula on iPhone");
    let r = c.http.post(format!("{}/api/join", c.base)).json(&body).send().await.unwrap();
    assert_eq!(r.status(), 400, "no pairing");

    let mut cert = Cert::new(&keys, "a1", Kind::Browser, "Arugula on iPhone");
    cert.sign_with(&root);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    c.app.db.put_native_device("a1", &keys.id(), now_ms()).unwrap();
    let (st, v) = signed(&c, &keys, "GET", "/api/me", Value::Null).await;
    assert_eq!(st, 401, "{v}");
    let unknown = DeviceKeys::generate();
    let (st2, v2_) = signed(&c, &unknown, "GET", "/api/me", Value::Null).await;
    assert_eq!((st, &v), (st2, &v2_), "the same answer as a key nobody knows");
}

/// An old desktop app's window can't pair as an app, or its name would read
/// as a window everywhere else (`Cert::from_app`, aru-131).
#[tokio::test]
async fn a_window_cant_pair() {
    let c = control(|a| a.cfg.native_clients = true).await;
    person(&c.app, "jake", "a1");
    let keys = DeviceKeys::generate();
    let (body, _) = ask(&keys, Kind::Browser, "Arugula app on geek");
    let r = c.http.post(format!("{}/api/join", c.base)).json(&body).send().await.unwrap();
    assert_eq!(r.status(), 400);
}

/// The CLI's rules don't change.
#[tokio::test]
async fn a_cli_is_as_it_was() {
    let c = control(|a| a.cfg.native_clients = true).await;
    let root = person(&c.app, "jake", "a1");
    let cli = DeviceKeys::generate();
    let mut cert = Cert::new(&cli, "a1", Kind::Cli, "Arugula CLI");
    cert.sign_with(&root);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    let (st, v) = signed(&c, &cli, "GET", "/api/me", Value::Null).await;
    assert_eq!((st, v["reason"].as_str()), (401, Some(PAGE_ONLY)), "/api/me is the page's and an app's, not a CLI's");
    let (st, _) = signed(&c, &cli, "GET", "/api/devices", Value::Null).await;
    assert_eq!(st, 200);
}
