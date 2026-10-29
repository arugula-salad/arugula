//! The platform page's host (M151) over HTTP, with the platform in
//! process: which host gets what, that nothing of the hub's answers on the
//! platform's host, that no cookie crosses it either way, and that
//! control's own site is as it was. `gateway_wire` has the same out of
//! process.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use arugula_e2e::now_ms;
use reqwest::{Method, header};
use serde_json::Value;

use crate::{App, auth::hash, platform_host::PlatformHost};

const CONTROL: &str = "http://control.test";
const APP: &str = "app.test";

/// A stand-in build of a page: `index.html` saying `which`, and one hashed
/// asset. Removed when the test ends.
pub(crate) struct Files(pub(crate) PathBuf);

impl Files {
    pub(crate) fn page(which: &str) -> Self {
        let d = std::env::temp_dir().join(format!("arugula-{which}-{}", hex::encode(arugula_e2e::random::<8>())));
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("index.html"), format!("<!doctype html><title>{which}</title><div id=root></div>"))
            .unwrap();
        std::fs::write(d.join("assets/app-1.js"), format!("// {which}")).unwrap();
        Files(d)
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Control {
    app: Arc<App>,
    base: String,
    http: reqwest::Client,
    _files: (Files, Files),
}

/// Control at `http://control.test`, its platform page at `app.test`
/// (unless `tweak` says otherwise), each with a stand-in build.
async fn control(tweak: impl FnOnce(&mut App)) -> Control {
    let shell = Files::page("shell");
    let platform = Files::page("platform");
    let mut app = App::for_tests(CONTROL);
    app.cfg.static_dir = Some(shell.0.clone());
    app.cfg.platform_host = crate::platform_host::parse(APP, CONTROL, &app.cfg.sites, None).unwrap();
    app.cfg.platform_page = arugula_platform_api::page::Page::new(Some(platform.0.clone()));
    tweak(&mut app);
    let app = app.start();
    let now = now_ms();
    app.db.account_for("test", "a", "a", "a", now).unwrap();
    app.db.add_session(&hash("session-a"), "a", now, now + 60_000).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Control { app, base, http, _files: (shell, platform) }
}

const COOKIE: &str = "ilg_session=session-a";

impl Control {
    async fn get(&self, host: &str, method: Method, path: &str) -> Answer {
        let r = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header(header::HOST, host)
            .header(header::COOKIE, COOKIE)
            .send()
            .await
            .unwrap();
        Answer::of(r).await
    }
}

#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) headers: header::HeaderMap,
    pub(crate) body: String,
}

impl Answer {
    pub(crate) async fn of(r: reqwest::Response) -> Self {
        Answer { status: r.status().as_u16(), headers: r.headers().clone(), body: r.text().await.unwrap() }
    }

    pub(crate) fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
    }

    /// The platform page's headers, and no cookie set.
    pub(crate) fn is_the_platforms(&self) -> bool {
        self.header("content-security-policy") == arugula_platform_api::page::CSP
            && self.header("x-frame-options") == "DENY"
            && self.header("x-content-type-options") == "nosniff"
            && self.header("referrer-policy") == "no-referrer"
            && self.header("strict-transport-security") == "max-age=31536000"
            && self.header("cross-origin-opener-policy") == "same-origin"
            && !self.headers.contains_key("set-cookie")
    }
}

/// On the platform's host, every page path is the platform page's
/// index.html and its files are its own, under its CSP; a missing file is a
/// 404. The host is matched without its port or a trailing dot, in any case.
#[tokio::test]
async fn the_platform_host_serves_the_platform_page() {
    let c = control(|_| {}).await;
    for path in ["/", "/p/prj_8f2k-arugula", "/p/prj_8f2k-arugula/chat/ch_3j9-general", "/dm/r_1", "/settings"] {
        let a = c.get(APP, Method::GET, path).await;
        assert_eq!(a.status, 200, "{path}");
        assert!(a.body.contains("<title>platform</title>"), "{path}: {}", a.body);
        assert!(a.is_the_platforms(), "{path}: {:?}", a.headers);
        assert_eq!(a.header("cache-control"), "no-cache");
    }
    let a = c.get(APP, Method::GET, "/assets/app-1.js").await;
    assert_eq!((a.status, a.body.as_str()), (200, "// platform"));
    assert_eq!(a.header("cache-control"), "public, max-age=31536000, immutable");
    assert!(a.is_the_platforms());
    let a = c.get(APP, Method::GET, "/assets/gone.js").await;
    assert_eq!(a.status, 404);
    assert!(a.is_the_platforms());
    // A path no route of the page's starts with: the page (it says Not
    // found), as a 404 (UI audit #193).
    let a = c.get(APP, Method::GET, "/agents").await;
    assert_eq!(a.status, 404);
    assert!(a.body.contains("<title>platform</title>") && a.is_the_platforms(), "{}", a.body);
    for host in ["APP.test", "app.test.", "app.test:8443"] {
        let a = c.get(host, Method::GET, "/p/x").await;
        assert!(a.status == 200 && a.body.contains("platform"), "{host}");
    }
    let a = c.get(APP, Method::POST, "/").await;
    assert_eq!((a.status, a.header("allow")), (405, "GET, HEAD"));
    assert!(a.is_the_platforms());
    assert_eq!(c.get(APP, Method::HEAD, "/p/x").await.status, 200);
}

/// Nothing of the hub's answers on the platform's host, signed in or not,
/// and no answer there sets a cookie.
#[tokio::test]
async fn nothing_of_the_hubs_answers_on_the_platform_host() {
    let c = control(|_| {}).await;
    // On control's own site they answer (`/api/me` signed in).
    assert_eq!(c.get("control.test", Method::GET, "/api/me").await.status, 200);
    for (m, path) in [
        (Method::GET, "/api/me"),
        (Method::GET, "/api/devices"),
        (Method::GET, "/control.json"),
        (Method::GET, "/auth/github"),
        (Method::GET, "/auth/github/callback?code=x&state=y"),
        (Method::POST, "/auth/logout"),
        (Method::POST, "/auth/passkey/login"),
        (Method::POST, "/github/webhook"),
        (Method::GET, "/.well-known/webauthn"),
        (Method::GET, "/d/abc"),
        (Method::GET, "/api/relay/m"),
        (Method::GET, "/api"),
        (Method::GET, "/platform.json"),
        (Method::POST, "/internal/platform/v1/chat_on"),
    ] {
        let a = c.get(APP, m.clone(), path).await;
        assert_eq!(a.status, 404, "{m} {path}: {}", a.body);
        assert!(a.is_the_platforms(), "{m} {path}: {:?}", a.headers);
    }
    // A WebSocket, wherever it's for.
    for path in ["/api/relay/m", "/api/chat/live", "/"] {
        let r = c
            .http
            .get(format!("{}{path}", c.base))
            .header(header::HOST, APP)
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==") // gitleaks:allow (RFC 6455's sample nonce)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404, "{path}");
    }
}

/// A platform route on the platform's host reaches the platform through
/// the gateway, as on control's site. Until M152 nobody's signed in there:
/// the hub's session cookie is dropped at the door, so it's a 401, never a
/// 404, and never the session control's site would have taken.
#[tokio::test]
async fn platform_routes_reach_the_gateway_on_the_platform_host() {
    let c = control(|_| {}).await;
    let on_control = c.get("control.test", Method::GET, "/api/projects").await;
    assert_eq!(on_control.status, 200, "{}", on_control.body);
    let a = c.get(APP, Method::GET, "/api/projects").await;
    assert_eq!(a.status, 401, "{}", a.body);
    assert_eq!(serde_json::from_str::<Value>(&a.body).unwrap()["error"], "sign in first");
    assert!(a.is_the_platforms());
    for path in ["/api/chat", "/api/workspaces", "/api/billing"] {
        assert_eq!(c.get(APP, Method::GET, path).await.status, 401, "{path}");
    }
    // A method the route doesn't take is no platform route: the hub's 404.
    assert_eq!(c.get(APP, Method::DELETE, "/api/projects").await.status, 404);
}

/// Control's own site is as it was: the shell's page under control's CSP,
/// and `/control.json` says where the platform page is.
#[tokio::test]
async fn controls_own_host_is_unchanged() {
    let c = control(|_| {}).await;
    let a = c.get("control.test", Method::GET, "/").await;
    assert_eq!(a.status, 200);
    assert!(a.body.contains("<title>shell</title>"), "{}", a.body);
    assert!(a.header("content-security-policy").contains("connect-src 'self' wss: ws: https:"), "{:?}", a.headers);
    assert!(a.headers.get("cross-origin-opener-policy").is_none());
    let a = c.get("control.test", Method::GET, "/p/prj_1").await;
    assert_eq!(a.status, 404, "control's page has no such path");
    let info: Value = serde_json::from_str(&c.get("control.test", Method::GET, "/control.json").await.body).unwrap();
    assert_eq!(info["platform"], "http://app.test");
    assert_eq!(c.app.cfg.platform_host, Some(PlatformHost { host: APP.into(), origin: "http://app.test".into() }));

    // Without a platform host nothing changes, on any host.
    let off = control(|a| a.cfg.platform_host = None).await;
    let info: Value = serde_json::from_str(&off.get("control.test", Method::GET, "/control.json").await.body).unwrap();
    assert!(info.get("platform").is_some_and(Value::is_null), "said, as null: {info}");
    let a = off.get(APP, Method::GET, "/").await;
    assert!(a.status == 200 && a.body.contains("<title>shell</title>"), "{}", a.body);
    assert_eq!(off.get(APP, Method::GET, "/api/me").await.status, 200);
}

/// The real build (`web/dist-platform`, `just web`), embedded, works under
/// its CSP: no inline script in its index.html, its own scripts and styles
/// only, and served from the embed on the platform's host.
#[tokio::test]
async fn the_real_platform_build_works_under_its_csp() {
    let c = control(|a| a.cfg.platform_page = Default::default()).await;
    let a = c.get(APP, Method::GET, "/p/prj_1/chat/ch_1").await;
    assert_eq!(a.status, 200, "the platform page isn't built: `just web` (or `cd web && pnpm build`) first");
    assert!(a.body.contains("id=\"root\""), "{}", a.body);
    let tags: Vec<&str> = a.body.split("<script").skip(1).collect();
    assert!(!tags.is_empty());
    for t in &tags {
        let open = &t[..t.find('>').unwrap()];
        assert!(open.contains(" src=\"/assets/"), "every script is a file of its own: <script{open}>");
        assert!(t[open.len() + 1..].trim_start().starts_with("</script>"), "no inline script: <script{t}");
    }
    assert!(!handler(&a.body), "no inline event handlers");
    let src = tags[0].split("src=\"").nth(1).unwrap().split('"').next().unwrap();
    let js = c.get(APP, Method::GET, src).await;
    assert_eq!((js.status, js.header("content-type")), (200, "text/javascript"), "{src}");
    assert_eq!(js.header("cache-control"), "public, max-age=31536000, immutable");
    assert!(!js.body.contains("new Function(") && !js.body.contains("eval("), "nothing needs 'unsafe-eval'");
    for css in a.body.split("href=\"").skip(1).map(|s| s.split('"').next().unwrap()).filter(|h| h.ends_with(".css")) {
        assert!(css.starts_with("/assets/"), "{css}");
        assert_eq!(c.get(APP, Method::GET, css).await.status, 200, "{css}");
    }
}

/// An `on…=` attribute in the page's HTML, which the CSP would refuse.
fn handler(html: &str) -> bool {
    html.split(char::is_whitespace)
        .filter_map(|w| w.strip_prefix("on")?.split_once('='))
        .any(|(name, _)| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase()))
}
