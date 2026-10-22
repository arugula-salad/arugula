//! What only a browser's page does, over HTTP: a CLI's login can't sign
//! the desktop app in for itself, or make or approve a browser; a CLI or
//! the desktop app never certifies a browser, a phone or recovery codes;
//! the app's window joins as an `app` device, and never as an account's
//! first one.

use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
use serde_json::{Value, json};

use crate::{
    auth::{SESSION_COOKIE, Via, hash},
    clients::PAGE_ONLY,
    routing_wire::{Control, control, person, session, v2},
};

/// `by`'s certificate for new keys of `kind` in `account`.
fn signed(by: &DeviceKeys, account: &str, kind: Kind, name: &str) -> (DeviceKeys, Cert) {
    let k = DeviceKeys::generate();
    let mut c = Cert::new(&k, account, kind, name);
    c.sign_with(by);
    (k, c)
}

/// An approved device of `kind` that `by` signed.
fn device(c: &Control, by: &DeviceKeys, account: &str, kind: Kind, name: &str) -> DeviceKeys {
    let (k, cert) = signed(by, account, kind, name);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    k
}

/// A session cookie whose sign-in came `via`.
fn session_via(c: &Control, account: &str, via: Via) -> String {
    let cookie = session(&c.app, account);
    let token = cookie.strip_prefix(&format!("{SESSION_COOKIE}=")).unwrap();
    c.app.db.set_session_via(&hash(token), via.as_str()).unwrap();
    cookie
}

impl Control {
    /// A request signed by a CLI's key.
    async fn as_cli(&self, keys: &DeviceKeys, method: &str, path: &str, body: Value) -> (u16, Value) {
        let b = serde_json::to_vec(&body).unwrap();
        let r = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("x-arugula-auth", v2(keys, method, path, if method == "GET" { b"" } else { &b }))
            .header("content-type", "application/json");
        let r = if method == "GET" { r } else { r.body(b) };
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or_default())
    }
}

/// A CLI's login starts a desktop app sign-in and tries to allow it itself,
/// to redeem it for a session cookie: refused. The page allows it.
#[tokio::test]
async fn a_cli_cant_sign_the_app_in_for_itself() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let cli = device(&c, &root, "a1", Kind::Cli, "Arugula CLI");
    let ask =
        json!({ "name": "Arugula app on box", "challenge": hash("verifier"), "port": 50123, "features": ["no-code"] });
    let r = c.http.post(format!("{}/auth/app", c.base)).json(&ask).send().await.unwrap();
    let id = r.json::<Value>().await.unwrap()["ticket"].as_str().unwrap().to_owned();
    let allow = format!("/api/app-login/{id}/allow");
    let (st, v) = c.as_cli(&cli, "POST", &allow, json!({})).await;
    assert_eq!((st, v["reason"].as_str()), (401, Some(PAGE_ONLY)), "{v}");
    let (st, _) = c.as_cli(&cli, "GET", &format!("/api/app-login/{id}"), json!({})).await;
    assert_eq!(st, 401);
    // So there's no grant to redeem, with any Origin.
    let r = c
        .http
        .post(format!("{}/auth/app/{id}/redeem", c.base))
        .header("origin", &c.base)
        .json(&json!({ "grant": "00", "verifier": "verifier" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    // Nor does a CLI enroll, approve or add recovery codes.
    let (_, req) = signed(&root, "a1", Kind::Browser, "Chrome");
    for (path, body) in [
        ("/api/devices", json!({ "cert": req })),
        (&format!("/api/devices/{}/approve", req.device) as &str, json!({ "cert": req })),
        ("/api/recovery", json!({ "certs": [] })),
        ("/auth/passkey/register", json!({})),
    ] {
        let (st, v) = c.as_cli(&cli, "POST", path, body).await;
        assert_eq!((st, v["reason"].as_str()), (401, Some(PAGE_ONLY)), "{path}: {v}");
    }
    // The page allows it, by its cookie.
    let cookie = session(&c.app, "a1");
    let (st, v) = c.as_person(&cookie, "POST", &allow, Some(json!({}))).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["redirect"].as_str().unwrap().starts_with("http://127.0.0.1:50123/arugula-signin?"));
}

/// Only a browser approves a browser or recovery codes: a CLI's, the app's
/// and an app device from before the app had a kind of its own are
/// refused, though they approve apps, CLIs and machines.
#[tokio::test]
async fn only_a_browser_approves_a_browser() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let cli = device(&c, &root, "a1", Kind::Cli, "Arugula CLI");
    let app = device(&c, &root, "a1", Kind::App, "Arugula app on box");
    let old_app = device(&c, &root, "a1", Kind::Browser, "Arugula app on geek");
    let cookie = session(&c.app, "a1");

    // A browser asks; a CLI, the app or an old app device approving it is
    // refused, and its root approves it.
    let k = DeviceKeys::generate();
    let ask = Cert::new(&k, "a1", Kind::Browser, "Safari on iPhone");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": ask }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(false)), "{v}");
    let approve = format!("/api/devices/{}/approve", ask.device);
    for by in [&cli, &app, &old_app] {
        let mut cert = ask.clone();
        cert.sign_with(by);
        let (st, v) = c.as_person(&cookie, "POST", &approve, Some(json!({ "cert": cert }))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{v}");
    }
    let mut cert = ask.clone();
    cert.sign_with(&root);
    let (st, v) = c.as_person(&cookie, "POST", &approve, Some(json!({ "cert": cert }))).await;
    assert_eq!(st, 200, "{v}");

    // Presigned by the app: not approved at once, it waits like any other.
    let (_, presigned) = signed(&app, "a1", Kind::Browser, "Chrome on Mac");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": presigned }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(false)), "{v}");

    // Recovery codes, made by the app: refused.
    let (_, code) = signed(&app, "a1", Kind::Recovery, "recovery 1");
    let (st, v) = c.as_person(&cookie, "POST", "/api/recovery", Some(json!({ "certs": [code] }))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{v}");

    // The app approves another app, as the old app device does.
    for by in [&app, &old_app] {
        let (_, c2) = signed(by, "a1", Kind::App, "Arugula app on mini");
        let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": c2 }))).await;
        assert_eq!((st, &v["approved"]), (200, &json!(true)), "{v}");
    }
}

/// An account whose first device was the app, from before it had a kind,
/// still approves browsers with it: there's nothing else to.
#[tokio::test]
async fn an_old_app_that_is_the_root_still_approves() {
    let c = control(|_| {}).await;
    c.app.db.account_for("github", "gh-a1", "jake", "a1", now_ms()).unwrap();
    let rk = DeviceKeys::generate();
    let mut root = Cert::new(&rk, "a1", Kind::Browser, "Arugula app on jake-air");
    root.sign_with(&rk);
    c.app.db.put_device(&root, true, now_ms()).unwrap();
    let cookie = session(&c.app, "a1");
    let (_, b) = signed(&rk, "a1", Kind::Browser, "Firefox on Linux");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": b }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(true)), "{v}");
}

/// The desktop app's window, signed in by a browser's hand-over, joins as
/// an `app` device only, and never as an account's first.
#[tokio::test]
async fn the_apps_window_joins_as_an_app() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let cookie = session_via(&c, "a1", Via::App);
    let as_browser = Cert::new(&DeviceKeys::generate(), "a1", Kind::Browser, "Arugula app on box");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": as_browser }))).await;
    assert_eq!(st, 400, "{v}");
    let as_app = Cert::new(&DeviceKeys::generate(), "a1", Kind::App, "Arugula app on box");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": as_app }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(false)), "{v}");
    // Signed by the browser that allowed its sign-in: in at once.
    let (_, handed) = signed(&root, "a1", Kind::App, "Arugula app on box");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": handed }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(true)), "{v}");

    // An account with no devices yet: the app can't be its first.
    c.app.db.account_for("github", "gh-b1", "bo", "b1", now_ms()).unwrap();
    let cookie = session(&c.app, "b1");
    let k = DeviceKeys::generate();
    let mut first = Cert::new(&k, "b1", Kind::App, "Arugula app on box");
    first.sign_with(&k);
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": first }))).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("first_in_browser")), "{v}");
}
