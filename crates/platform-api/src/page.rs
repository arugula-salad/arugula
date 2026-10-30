//! The platform page (M150's build of `web/app.html` into
//! `web/dist-platform`), embedded here and served on a host of its own,
//! `app.arugula.io` ("The platform page is on app.arugula.io, not on the
//! keys' origin", M151). That host points at the hub, which routes it by
//! `Host` (`crates/control/src/platform_host.rs`): in process the hub calls
//! [`Page::serve`]; out of process it forwards the page's GETs here, and the
//! `arugula-platform` binary answers them with [`behind_hub`], only for a
//! call the hub vouched for (the platform has no public address).
//!
//! It's a single-page app with real paths (`/p/:project/chat/:room`…), so
//! any path without a file extension is `index.html`; a missing file with
//! one (`/assets/app-x.js`) is a 404. A path whose first segment no route
//! starts with is `index.html` too (the page says Not found), as a 404
//! (UI audit #193). The hub sets the page's headers
//! ([`CSP`] and the rest) on everything on its host.

use std::{borrow::Cow, path::PathBuf};

use arugula_control_common::principal::PrincipalKey;
use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

#[derive(rust_embed::Embed)]
#[folder = "../../web/dist-platform"]
#[allow_missing = true]
struct Build;

/// The platform page's Content-Security-Policy, which the hub puts on
/// every answer on the platform's host: its own scripts only (no inline
/// script, no `eval`), its own origin's API only, framed by nothing and
/// framing nothing. Preact sets `style` attributes, hence
/// `'unsafe-inline'` for styles alone.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; connect-src 'self'; frame-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

/// [`CSP`], or with live presence (control has an arugula.stream app, whose
/// relay is at `presence`, an https origin): scripts also come from the
/// relay (its SDK and chunks), may be `blob:` URLs the page made (its Krisp
/// noise filter's audio worklet and worker) and may compile WebAssembly
/// (`'wasm-unsafe-eval'`, not JS `eval`: the camera's trackers); and the
/// page may connect to the relay's socket, LiveKit Cloud's (any `wss:`
/// host, per project) and https (Krisp's models, the relay's assets).
pub fn csp(presence: Option<&str>) -> Cow<'static, str> {
    match presence {
        None => Cow::Borrowed(CSP),
        Some(o) => Cow::Owned(
            CSP.replacen("script-src 'self';", &format!("script-src 'self' {o} blob: 'wasm-unsafe-eval';"), 1)
                .replacen("connect-src 'self';", "connect-src 'self' wss: https:;", 1),
        ),
    }
}

/// The first path segments the page's router (`web/src/platform/paths.ts`)
/// has routes under: a page path that starts with any other is answered
/// as a 404 (UI audit #193). This is a copy of `TOP` there; keep the two
/// in step (a test below reads it).
pub const PAGES: &[&str] =
    &["p", "chat", "dm", "issues", "runs", "installed", "inbox", "settings", "deploys", "boards"];

/// Where the page's files come from: the build embedded in this binary, or
/// a directory (development: `vite build --watch` writes there).
#[derive(Debug, Clone, Default)]
pub struct Page {
    dir: Option<PathBuf>,
}

impl Page {
    /// The embedded build, or `dir` when it's given.
    pub fn new(dir: Option<PathBuf>) -> Self {
        Page { dir }
    }

    fn file(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        match &self.dir {
            Some(d) => std::fs::read(d.join(path)).ok().map(Cow::Owned),
            None => Build::get(path).map(|f| f.data),
        }
    }

    /// The answer to `method` on `path`: the file, else (for a path
    /// without an extension) `index.html`, as a 404 when no route starts
    /// with its first segment.
    pub fn serve(&self, method: &Method, path: &str) -> Response {
        if !matches!(*method, Method::GET | Method::HEAD) {
            let mut r = StatusCode::METHOD_NOT_ALLOWED.into_response();
            r.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            return r;
        }
        let path = path.trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        // Nothing outside the build, and no dotfiles.
        if path.split('/').any(|s| s.starts_with('.')) || path.contains("//") || path.contains('\\') {
            return StatusCode::NOT_FOUND.into_response();
        }
        if let Some(data) = self.file(path) {
            return file(path, data);
        }
        let last = path.rsplit('/').next().unwrap_or(path);
        if last.contains('.') {
            return StatusCode::NOT_FOUND.into_response();
        }
        let first = path.split('/').next().unwrap_or(path);
        match self.file("index.html") {
            Some(data) if path == "index.html" || PAGES.contains(&first) => file("index.html", data),
            // Still the page, which says Not found, but not a 200.
            Some(data) => (StatusCode::NOT_FOUND, file("index.html", data)).into_response(),
            None => (StatusCode::NOT_FOUND, "the platform page isn't built: run `just web`").into_response(),
        }
    }
}

fn file(path: &str, data: Cow<'static, [u8]>) -> Response {
    // Vite's hashed files never change; everything else is asked again.
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime(path))
        .header(header::CACHE_CONTROL, cache)
        .body(Body::from(data))
        .expect("a response")
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json" | "webmanifest") => "application/json",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// The `arugula-platform` binary's answer to the page GETs the hub
/// forwards: any path no route took, only with the hub's signed principal
/// header ([`crate::remote::vouched`]), as every forwarded call. A request
/// that didn't come through the hub is refused (401).
pub fn behind_hub(page: Page, key: PrincipalKey) -> Router {
    Router::new()
        .fallback(move |method: Method, uri: Uri| {
            let page = page.clone();
            async move { page.serve(&method, uri.path()) }
        })
        .layer(axum::middleware::from_fn_with_state(key, crate::remote::vouched))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> (PathBuf, Page) {
        let d = std::env::temp_dir().join(format!("arugula-page-{}", hex::encode(arugula_e2e::random::<6>())));
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("index.html"), "<!doctype html><div id=root></div>").unwrap();
        std::fs::write(d.join("assets/app-1.js"), "export {}").unwrap();
        std::fs::write(d.join("icon.svg"), "<svg/>").unwrap();
        (d.clone(), Page::new(Some(d)))
    }

    async fn body(r: Response) -> String {
        String::from_utf8(axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap()
    }

    fn header(r: &Response, h: header::HeaderName) -> &str {
        r.headers().get(h).and_then(|v| v.to_str().ok()).unwrap_or("")
    }

    /// A page path is index.html; a file is itself, cached as Vite's hashes
    /// allow; a missing file is a 404, never the page.
    #[tokio::test]
    async fn paths_are_the_page_and_files_are_files() {
        let (d, page) = dir();
        for p in ["/", "/p/prj_8f2k-arugula", "/p/prj_8f2k/chat/ch_1", "/dm/r", "/settings", "/inbox/"] {
            let r = page.serve(&Method::GET, p);
            assert_eq!(r.status(), 200, "{p}");
            assert_eq!(header(&r, header::CONTENT_TYPE), "text/html; charset=utf-8");
            assert_eq!(header(&r, header::CACHE_CONTROL), "no-cache");
            assert!(body(r).await.contains("id=root"), "{p}");
        }
        let r = page.serve(&Method::GET, "/assets/app-1.js");
        assert_eq!((r.status().as_u16(), header(&r, header::CONTENT_TYPE)), (200, "text/javascript"));
        assert_eq!(header(&r, header::CACHE_CONTROL), "public, max-age=31536000, immutable");
        assert_eq!(page.serve(&Method::GET, "/icon.svg").status(), 200);
        assert_eq!(page.serve(&Method::HEAD, "/").status(), 200);
        for p in ["/assets/gone.js", "/favicon.ico", "/../Cargo.toml", "/assets/../index.html", "/.env", "/p//x"] {
            assert_eq!(page.serve(&Method::GET, p).status(), 404, "{p}");
        }
        // A first segment no route starts with: the page (it says Not
        // found), as a 404 (UI audit #193).
        for p in ["/agents", "/agents/", "/nope/p/prj_1", "/P/prj_1", "/index"] {
            let r = page.serve(&Method::GET, p);
            assert_eq!(
                (r.status().as_u16(), header(&r, header::CONTENT_TYPE)),
                (404, "text/html; charset=utf-8"),
                "{p}"
            );
            assert!(body(r).await.contains("id=root"), "{p}");
        }
        let r = page.serve(&Method::POST, "/");
        assert_eq!((r.status().as_u16(), header(&r, header::ALLOW)), (405, "GET, HEAD"));
        std::fs::remove_file(d.join("index.html")).unwrap();
        assert_eq!(page.serve(&Method::GET, "/p/x").status(), 404, "not built");
        let _ = std::fs::remove_dir_all(d);
    }

    /// `PAGES` is the web router's `TOP`, and every first segment its
    /// `parse` compares is in it: a route added there without a copy here
    /// would be a 404 (UI audit #193).
    #[test]
    fn pages_are_the_web_routers_first_segments() {
        let ts =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/src/platform/paths.ts")).unwrap();
        let quoted = |s: &str| -> Vec<String> { s.split('"').skip(1).step_by(2).map(str::to_owned).collect() };
        let top = ts.split("export const TOP = [").nth(1).and_then(|s| s.split(']').next()).expect("TOP in paths.ts");
        let mut top = quoted(top);
        let mut pages: Vec<String> = PAGES.iter().map(|s| s.to_string()).collect();
        top.sort();
        pages.sort();
        assert_eq!(top, pages, "PAGES here and TOP in web/src/platform/paths.ts");
        // `parts[0] === "chat"`, `parts[0] !== "p"`: the literal after each.
        let read: Vec<String> = ts
            .split("parts[0] ")
            .skip(1)
            .filter(|s| s.starts_with("=== \"") || s.starts_with("!== \""))
            .filter_map(|s| quoted(&s.chars().skip(4).take(36).collect::<String>()).into_iter().next())
            .collect();
        assert!(read.len() >= 5, "parse's first segments: {read:?}");
        for seg in read {
            assert!(PAGES.contains(&seg.as_str()), "{seg:?}: parse reads it, PAGES lacks it");
        }
    }

    /// Out of process, the page is answered only for the hub.
    #[tokio::test]
    async fn the_binary_serves_the_page_only_for_the_hub() {
        use arugula_control_common::principal::{self, NotVouched, Vouch};
        let key = PrincipalKey::new(&[7; 32]).unwrap();
        let (d, page) = dir();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let app = Router::new()
            .route("/platform.json", axum::routing::get(|| async { "up" }))
            .merge(behind_hub(page, key.clone()));
        tokio::spawn(async move { axum::serve(l, app).await });
        let http = reqwest::Client::new();
        let r = http.get(format!("{base}/p/prj_1")).send().await.unwrap();
        assert_eq!(r.status(), 401, "not through the hub");
        let other = PrincipalKey::new(&[8; 32]).unwrap();
        let sign = |k: &PrincipalKey, pq: &str| {
            k.sign_vouch(&Vouch::Not(NotVouched::Nobody), "GET", pq, "req", arugula_e2e::now_ms())
        };
        let r = http.get(format!("{base}/p/prj_1")).header(principal::HEADER, sign(&other, "/p/prj_1")).send();
        assert_eq!(r.await.unwrap().status(), 401, "another key");
        let r = http.get(format!("{base}/p/prj_1")).header(principal::HEADER, sign(&key, "/p/prj_1")).send();
        let r = r.await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.text().await.unwrap().contains("id=root"));
        let r = http.get(format!("{base}/assets/app-1.js")).header(principal::HEADER, sign(&key, "/assets/app-1.js"));
        assert_eq!(r.send().await.unwrap().status(), 200);
        assert_eq!(http.get(format!("{base}/platform.json")).send().await.unwrap().status(), 200, "health");
        let _ = std::fs::remove_dir_all(d);
    }
}
