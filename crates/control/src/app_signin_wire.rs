//! Signing in to the platform page (M152) over HTTP, with the platform in
//! process: a signed-in browser gets a code and then a session on the
//! platform's host with no prompt; a replayed code, another return address
//! or the wrong verifier fails; without a hub session control's page signs
//! in first; signing out on the hub ends the platform page's session at
//! once; and that session is good only from the platform page.

use std::{net::SocketAddr, sync::Arc};

use arugula_e2e::now_ms;
use base64::Engine as _;
use reqwest::{Method, header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{App, auth::hash, platform_host_wire::Files};

const CONTROL: &str = "http://control.test";
const APP: &str = "app.test";
const APP_ORIGIN: &str = "http://app.test";

struct Control {
    base: String,
    http: reqwest::Client,
    app: Arc<App>,
    _files: Files,
}

async fn control() -> Control {
    let platform = Files::page("platform");
    let mut app = App::for_tests(CONTROL);
    app.cfg.platform_host = crate::platform_host::parse(APP, CONTROL, &app.cfg.sites, None).unwrap();
    app.cfg.platform_page = arugula_platform::page::Page::new(Some(platform.0.clone()));
    let app = app.start();
    let now = now_ms();
    app.db.account_for("test", "a", "a", "a", now).unwrap();
    app.db.add_session(&hash("session-a"), "a", now, now + 3_600_000).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Control { base, http, app, _files: platform }
}

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A PKCE verifier and its S256 challenge.
fn pkce(seed: u8) -> (String, String) {
    let verifier = b64url(&[seed; 32]);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

const STATE: &str = "state-0123456789ab";
const RETURN: &str = "http://app.test/p/prj_00ff/chat";

impl Control {
    /// `GET /signin/platform` on control's host, with the hub session or
    /// without: its status and where it sends the browser.
    async fn start(&self, cookie: Option<&str>, return_to: &str, challenge: &str) -> (u16, String) {
        let q = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("return", return_to)
            .append_pair("challenge", challenge)
            .append_pair("state", STATE)
            .finish();
        let mut r = self.http.get(format!("{}/signin/platform?{q}", self.base)).header(header::HOST, "control.test");
        if let Some(c) = cookie {
            r = r.header(header::COOKIE, c);
        }
        let r = r.send().await.unwrap();
        let at = r.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        (r.status().as_u16(), at)
    }

    /// A code for `return_to`, from a signed-in browser.
    async fn code(&self, return_to: &str, challenge: &str) -> String {
        let (st, at) = self.start(Some("ilg_session=session-a"), return_to, challenge).await;
        assert_eq!(st, 303, "{at}");
        let (to, frag) = at.split_once('#').unwrap();
        assert_eq!(to, return_to);
        let code = frag.strip_prefix("code=").unwrap().split('&').next().unwrap().to_owned();
        assert!(frag.ends_with(&format!("&state={STATE}")), "{frag}");
        assert_eq!(code.len(), 64);
        code
    }

    /// `POST /api/app/session` on the platform's host: its status and the
    /// session cookie it set, if it did.
    async fn swap(&self, origin: &str, code: &str, verifier: &str, return_to: &str) -> (u16, Option<String>) {
        let r = self
            .http
            .post(format!("{}/api/app/session", self.base))
            .header(header::HOST, APP)
            .header(header::ORIGIN, origin)
            .json(&json!({ "code": code, "verifier": verifier, "return": return_to }))
            .send()
            .await
            .unwrap();
        let set = r.headers().get(header::SET_COOKIE).and_then(|v| v.to_str().ok()).map(str::to_owned);
        (r.status().as_u16(), set)
    }

    /// A session on the platform's host, the whole way.
    async fn signed_in(&self, seed: u8) -> String {
        let (v, c) = pkce(seed);
        let code = self.code(RETURN, &c).await;
        let (st, set) = self.swap(APP_ORIGIN, &code, &v, RETURN).await;
        assert_eq!(st, 204);
        set.unwrap().split(';').next().unwrap().to_owned()
    }

    async fn on_app(&self, method: Method, path: &str, cookie: &str, origin: Option<&str>) -> (u16, Value) {
        let mut r = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header(header::HOST, APP)
            .header(header::COOKIE, cookie);
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o).json(&json!({}));
        }
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or_default())
    }
}

/// Signed in on control's page: one redirect with a code, one swap, and the
/// platform's routes answer on its host as that account, with a cookie of
/// that host's alone.
#[tokio::test]
async fn a_signed_in_browser_gets_a_session_on_the_platform_host_with_no_prompt() {
    let c = control().await;
    let (v, ch) = pkce(1);
    let code = c.code(RETURN, &ch).await;
    let (st, set) = c.swap(APP_ORIGIN, &code, &v, RETURN).await;
    assert_eq!(st, 204);
    let set = set.unwrap();
    assert!(set.starts_with("arugula_app="), "{set}");
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax") && set.contains("Path=/"), "{set}");
    assert!(!set.to_ascii_lowercase().contains("domain="), "host-only: {set}");
    let cookie = set.split(';').next().unwrap();
    let (st, v) = c.on_app(Method::GET, "/api/projects", cookie, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["me"]["account"], "a");
    // The hub says where it is, for the page's sign-in and its links.
    let r = c.http.get(format!("{}/api/projects", c.base)).header(header::HOST, APP).send().await.unwrap();
    assert_eq!((r.status().as_u16(), r.headers()["x-arugula-hub"].to_str().unwrap()), (401, CONTROL));
    // The hub's own cookie still means nothing there.
    let (st, _) = c.on_app(Method::GET, "/api/projects", "ilg_session=session-a", None).await;
    assert_eq!(st, 401);
}

#[tokio::test]
async fn a_replayed_code_fails() {
    let c = control().await;
    let (v, ch) = pkce(2);
    let code = c.code(RETURN, &ch).await;
    assert_eq!(c.swap(APP_ORIGIN, &code, &v, RETURN).await.0, 204);
    let (st, set) = c.swap(APP_ORIGIN, &code, &v, RETURN).await;
    assert_eq!((st, set), (400, None));
}

/// A code is bound to the address it went back to and to its verifier; a
/// wrong one spends it, so the right one can't follow.
#[tokio::test]
async fn a_mismatched_return_or_verifier_fails() {
    let c = control().await;
    let (v, ch) = pkce(3);
    let code = c.code(RETURN, &ch).await;
    assert_eq!(c.swap(APP_ORIGIN, &code, &v, "http://app.test/settings").await.0, 400);
    assert_eq!(c.swap(APP_ORIGIN, &code, &v, RETURN).await.0, 400);
    let code = c.code(RETURN, &ch).await;
    assert_eq!(c.swap(APP_ORIGIN, &code, &pkce(4).0, RETURN).await.0, 400);
    assert_eq!(c.swap(APP_ORIGIN, &code, &v, RETURN).await.0, 400);
    // A made-up code.
    assert_eq!(c.swap(APP_ORIGIN, &"0".repeat(64), &v, RETURN).await.0, 400);
}

/// The swap is the platform page's alone: from its origin, on its host.
#[tokio::test]
async fn the_swap_takes_only_the_platform_page() {
    let c = control().await;
    let (v, ch) = pkce(5);
    let code = c.code(RETURN, &ch).await;
    assert_eq!(c.swap(CONTROL, &code, &v, RETURN).await.0, 403);
    // On control's host the route isn't the platform page's either.
    let r = c
        .http
        .post(format!("{}/api/app/session", c.base))
        .header(header::HOST, "control.test")
        .header(header::ORIGIN, APP_ORIGIN)
        .json(&json!({ "code": code, "verifier": v, "return": RETURN }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403);
}

/// Without a hub session, control's page signs the person in first, then
/// sends them back to /signin/platform with the same question.
#[tokio::test]
async fn without_a_hub_session_control_signs_in_first() {
    let c = control().await;
    let (_, ch) = pkce(6);
    let (st, at) = c.start(None, RETURN, &ch).await;
    assert_eq!(st, 303);
    let kept = at.strip_prefix("/#platform-signin=").unwrap();
    let q = String::from_utf8(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(kept).unwrap()).unwrap();
    let q: std::collections::HashMap<String, String> = url::form_urlencoded::parse(q.as_bytes()).into_owned().collect();
    assert_eq!((q["return"].as_str(), q["challenge"].as_str(), q["state"].as_str()), (RETURN, ch.as_str(), STATE));
    // A session that has ended is none.
    let (st, at) = c.start(Some("ilg_session=session-gone"), RETURN, &ch).await;
    assert!(st == 303 && at.starts_with("/#platform-signin="), "{at}");
}

/// A code goes back only to the platform page's origin, and only for a
/// question shaped as the page asks it.
#[tokio::test]
async fn a_return_off_the_platform_page_is_refused() {
    let c = control().await;
    let (_, ch) = pkce(7);
    for to in [
        "http://evil.test/",
        "http://app.test.evil.test/",
        "https://app.test/",
        "http://app.test:8080/",
        "http://app.test/#x",
        "/p/x",
    ] {
        assert_eq!(c.start(Some("ilg_session=session-a"), to, &ch).await.0, 400, "{to}");
    }
    assert_eq!(c.start(Some("ilg_session=session-a"), RETURN, "short").await.0, 400);
}

/// Signing out on control's page, ending every session, or ending the one
/// the platform page's came from: the platform page's ends at once.
#[tokio::test]
async fn signing_out_on_the_hub_ends_the_platform_session_at_once() {
    let c = control().await;
    let cookie = c.signed_in(8).await;
    assert_eq!(c.on_app(Method::GET, "/api/projects", &cookie, None).await.0, 200);
    let r = c
        .http
        .post(format!("{}/auth/logout", c.base))
        .header(header::HOST, "control.test")
        .header(header::ORIGIN, CONTROL)
        .header(header::COOKIE, "ilg_session=session-a")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    assert_eq!(c.on_app(Method::GET, "/api/projects", &cookie, None).await.0, 401);

    // "End all sessions", from another hub session.
    let now = now_ms();
    c.app.db.add_session(&hash("session-b"), "a", now, now + 3_600_000).unwrap();
    c.app.db.add_session(&hash("session-a"), "a", now, now + 3_600_000).unwrap();
    let cookie = c.signed_in(9).await;
    assert_eq!(c.on_app(Method::GET, "/api/projects", &cookie, None).await.0, 200);
    c.app.db.drop_sessions("a").unwrap();
    assert_eq!(c.on_app(Method::GET, "/api/projects", &cookie, None).await.0, 401);
}

/// The platform page signs itself out; the hub's session stays.
#[tokio::test]
async fn the_platform_page_signs_itself_out() {
    let c = control().await;
    let cookie = c.signed_in(10).await;
    assert_eq!(c.on_app(Method::POST, "/api/app/signout", &cookie, Some(CONTROL)).await.0, 403);
    assert_eq!(c.on_app(Method::POST, "/api/app/signout", &cookie, Some(APP_ORIGIN)).await.0, 204);
    assert_eq!(c.on_app(Method::GET, "/api/projects", &cookie, None).await.0, 401);
    assert_eq!(c.start(Some("ilg_session=session-a"), RETURN, &pkce(11).1).await.0, 303);
}

/// A write on the platform's host with its session but from anywhere else
/// is refused, as control's own site refuses one.
#[tokio::test]
async fn a_cross_origin_write_on_the_platform_host_is_refused() {
    let c = control().await;
    let cookie = c.signed_in(12).await;
    let (st, v) = c.on_app(Method::POST, "/api/chat/live/open", &cookie, Some(CONTROL)).await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = c.on_app(Method::GET, "/api/projects", &cookie, Some("http://evil.test")).await;
    assert_eq!(st, 403, "{v}");
}
