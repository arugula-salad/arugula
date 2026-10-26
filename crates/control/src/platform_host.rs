//! The platform page's host (M151, "The platform page is on app.arugula.io,
//! not on the keys' origin"): `--platform-host app.arugula.io` points that
//! name at this hub, and the hub routes it by `Host` to the platform. The
//! platform still has no public address ("Platform paths stay on
//! control.arugula.io forever"): every platform path keeps working on
//! control's own site, and this host is a second way in to the same routes.
//!
//! On the platform's host:
//! - a platform route (a method and path `routes.toml` gives
//!   `process = "platform"`) goes on to the router, so the gateway answers
//!   it exactly as on control's site, in process or out (but GitHub's
//!   webhook, which comes only to control's site: M161);
//! - nothing of the hub's answers: any other `/api/…`, `/auth/…`,
//!   `/github/…`, `/.well-known/…`, `/d/…`, `/internal/…`, `/signin/…`,
//!   `/control.json` and the platform's `/platform.json`, and every
//!   WebSocket, is a 404;
//! - any other GET or HEAD is the platform page (`arugula_platform::page`):
//!   in process the hub serves the platform crate's embed, out of process it
//!   forwards the GET to the platform, which serves its own (as a 404 for
//!   a path no route of the page's starts with). Never the shell's
//!   `web/dist`. Any other method is a 405.
//!
//! Every answer here carries the page's own headers (the platform page's
//! CSP, no framing, no referrer). The hub's session is host-only on control's
//! site and never reaches this host; the hub drops any cookie a request
//! brings here but this host's own session (M152, `app_signin`), so one set
//! on a parent domain by a sibling host can't act as a session, and no
//! `Set-Cookie` leaves here but that one's.
//!
//! Two of the hub's routes answer here too (M152): `POST /api/app/session`,
//! which swaps a one-time code for this host's session, and
//! `POST /api/app/signout`.

use std::sync::{Arc, LazyLock};

use arugula_platform::page;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{App, Site, gateway::PlatformAt};

/// The platform page's host, and the page's origin there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformHost {
    /// Lower case, without a trailing dot: `app.arugula.io`.
    pub host: String,
    /// Control's public URL's scheme and port on this host, as browsers
    /// send it: `https://app.arugula.io`. `/control.json` says it.
    pub origin: String,
}

/// `--platform-host` as control takes it: a bare host name, never one of
/// control's own sites (the page mustn't share the keys' origin) and never
/// under the block domain (blocks' origins are strangers' code). Empty is
/// none.
pub fn parse(
    raw: &str,
    public_url: &str,
    sites: &[Site],
    block_domain: Option<&str>,
) -> anyhow::Result<Option<PlatformHost>> {
    let h = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if h.is_empty() {
        return Ok(None);
    }
    let name = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '.';
    if !h.chars().all(name) || !h.contains('.') || h.starts_with(['.', '-']) || h.contains("..") {
        anyhow::bail!("--platform-host {raw:?}: a host name, like app.arugula.io");
    }
    for s in sites {
        if s.host().eq_ignore_ascii_case(&h) {
            anyhow::bail!(
                "--platform-host {h} is control's own site ({}): the platform page needs a host of its own",
                s.url
            );
        }
    }
    if let Some(d) = block_domain
        && (h == d || h.ends_with(&format!(".{d}")))
    {
        anyhow::bail!("--platform-host {h} is under the block domain {d}, where blocks' origins are");
    }
    let mut u = url::Url::parse(public_url)?;
    u.set_host(Some(&h)).map_err(|e| anyhow::anyhow!("--platform-host {h}: {e}"))?;
    Ok(Some(PlatformHost { origin: u.origin().ascii_serialization(), host: h }))
}

/// Whether a request is for `host`: its `Host` (or, without one, its
/// URI's authority), without the port or a trailing dot, in any case.
fn is_for(host: &str, req: &Request) -> bool {
    is_host(host, req.headers(), req.uri().authority().map(|a| a.as_str()))
}

/// Whether a request with these headers came in on the platform page's
/// host (M152: its session is this host's own).
pub fn on_platform_host(app: &App, headers: &HeaderMap) -> bool {
    app.cfg.platform_host.as_ref().is_some_and(|ph| is_host(&ph.host, headers, None))
}

fn is_host(host: &str, headers: &HeaderMap, authority: Option<&str>) -> bool {
    let asked = match headers.get(header::HOST).and_then(|h| h.to_str().ok()) {
        Some(h) => h,
        None => match authority {
            Some(a) => a,
            None => return false,
        },
    };
    let name = asked.rsplit_once(':').filter(|(_, p)| p.parse::<u16>().is_ok()).map_or(asked, |(n, _)| n);
    name.trim_end_matches('.').eq_ignore_ascii_case(host)
}

/// The platform's routes, from `routes.toml` (the gateway's list): a
/// method (`ANY` for every one) and the path's segments.
static PLATFORM_ROUTES: LazyLock<Vec<(String, Vec<String>)>> = LazyLock::new(|| {
    crate::gateway::listed_platform_routes()
        .into_iter()
        .map(|(m, p)| (m, p.trim_start_matches('/').split('/').map(str::to_owned).collect()))
        .collect()
});

/// Whether `path`, as axum matches it, is `pattern`'s (`{param}` one
/// segment, `{*rest}` the rest).
fn matches(pattern: &[String], path: &str) -> bool {
    let mut asked = path.strip_prefix('/').unwrap_or(path).split('/');
    for p in pattern {
        if p.starts_with("{*") {
            return asked.next().is_some_and(|s| !s.is_empty());
        }
        match asked.next() {
            Some(s) if p.starts_with('{') && !s.is_empty() => {}
            Some(s) if s == p => {}
            _ => return false,
        }
    }
    asked.next().is_none()
}

/// Whether a method and path are one of the platform's routes. A `GET`
/// takes `HEAD` too, as axum's do.
pub fn platform_route(method: &Method, path: &str) -> bool {
    let method = if *method == Method::HEAD { "GET" } else { method.as_str() };
    PLATFORM_ROUTES.iter().any(|(m, p)| (m == "ANY" || m == method) && matches(p, path))
}

/// Paths that are the hub's (or the platform's own, unpublished) and never
/// the page: answered 404 on the platform's host.
fn not_here(path: &str) -> bool {
    let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
    matches!(first, "api" | "auth" | "github" | ".well-known" | "d" | "internal" | "signin" | "oauth")
        || matches!(path, "/control.json" | "/platform.json")
}

/// GitHub's webhook is the platform's since M161, and comes only to
/// control's own site, where the App says it is: not here.
fn webhook(path: &str) -> bool {
    path.starts_with("/github/")
}

/// The outermost layer, outside even `blocks::split`: a request for the
/// platform's host reaches only the platform's routes and its page.
pub async fn split(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    let Some(ph) = &app.cfg.platform_host else { return next.run(req).await };
    if !is_for(&ph.host, &req) {
        return next.run(req).await;
    }
    // No cookie reaches anything from here but this host's own session.
    let own = crate::app_signin::app_cookie(&app);
    let kept = crate::auth::cookie(req.headers(), own).map(|v| format!("{own}={v}"));
    req.headers_mut().remove(header::COOKIE);
    if let Some(c) = kept.and_then(|c| HeaderValue::from_str(&c).ok()) {
        req.headers_mut().insert(header::COOKIE, c);
    }
    let path = req.uri().path().to_owned();
    let res = if req.headers().contains_key(header::UPGRADE) {
        nothing()
    } else if (platform_route(req.method(), &path) && !webhook(&path)) || crate::app_signin::route(req.method(), &path)
    {
        next.run(req).await
    } else if not_here(&path) {
        nothing()
    } else if !matches!(*req.method(), Method::GET | Method::HEAD) {
        let mut r = StatusCode::METHOD_NOT_ALLOWED.into_response();
        r.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        r
    } else {
        match &app.platform {
            PlatformAt::InProcess(_) => app.cfg.platform_page.serve(req.method(), &path),
            // The platform's own embed, through the gateway's forwarding.
            PlatformAt::Remote(_) => crate::gateway::forward(State(app.clone()), req).await,
        }
    };
    let mut res = secured(res, own);
    // Where the hub is, for the page's sign-in and its links back to
    // control's page (M152): the hub says it, the page never guesses.
    if let Ok(v) = HeaderValue::from_str(&app.cfg.public_url) {
        res.headers_mut().insert(HUB_HEADER, v);
    }
    res
}

/// On every answer on the platform page's host: control's public URL.
pub const HUB_HEADER: header::HeaderName = header::HeaderName::from_static("x-arugula-hub");

fn nothing() -> Response {
    crate::err(StatusCode::NOT_FOUND, "not found").into_response()
}

/// The platform page's headers, on every answer on its host, whoever made
/// it, and no cookie set but this host's own session (`own`, M152).
fn secured(mut res: Response, own: &str) -> Response {
    let h = res.headers_mut();
    let kept: Vec<HeaderValue> = h
        .get_all(header::SET_COOKIE)
        .iter()
        .filter(|v| v.to_str().is_ok_and(|v| v.split_once('=').is_some_and(|(name, _)| name.trim() == own)))
        .cloned()
        .collect();
    h.remove(header::SET_COOKIE);
    for v in kept {
        h.append(header::SET_COOKIE, v);
    }
    set(h, header::CONTENT_SECURITY_POLICY, page::CSP);
    set(h, header::X_FRAME_OPTIONS, "DENY");
    set(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(h, header::REFERRER_POLICY, "no-referrer");
    // Browsers ignore it over plain HTTP (dev's app.localhost), so always.
    set(h, header::STRICT_TRANSPORT_SECURITY, "max-age=31536000");
    set(h, header::HeaderName::from_static("cross-origin-opener-policy"), "same-origin");
    res
}

fn set(h: &mut HeaderMap, name: header::HeaderName, value: &'static str) {
    h.insert(name, HeaderValue::from_static(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sites() -> Vec<Site> {
        vec![Site::new("https://control.arugula.io").unwrap(), Site::new("https://illogical.example").unwrap()]
    }

    #[test]
    fn the_host_is_a_name_of_its_own() {
        let p = |raw: &str, block: Option<&str>| parse(raw, "https://control.arugula.io", &sites(), block);
        let got = p("App.Arugula.IO.", Some("arugula.run")).unwrap().unwrap();
        assert_eq!(got, PlatformHost { host: "app.arugula.io".into(), origin: "https://app.arugula.io".into() });
        assert_eq!(p("", None).unwrap(), None, "unset");
        let dev = parse("app.localhost", "http://localhost:7690", &[Site::new("http://localhost:7690").unwrap()], None);
        assert_eq!(dev.unwrap().unwrap().origin, "http://app.localhost:7690", "the public URL's scheme and port");
        for bad in
            ["control.arugula.io", "illogical.example", "https://app.arugula.io", "app.arugula.io:443", "app", "a b.io"]
        {
            assert!(p(bad, None).is_err(), "{bad}");
        }
        for under in ["arugula.run", "app.arugula.run", "docs.arugula.run"] {
            assert!(p(under, Some("arugula.run")).is_err(), "{under}");
        }
    }

    #[test]
    fn platform_routes_are_routes_toml_s() {
        assert!(platform_route(&Method::GET, "/api/projects"));
        assert!(platform_route(&Method::HEAD, "/api/projects"));
        assert!(platform_route(&Method::POST, "/api/chat/rooms/ch_1/messages"));
        assert!(platform_route(&Method::PATCH, "/api/chat/rooms/ch_1/messages/7"));
        assert!(!platform_route(&Method::DELETE, "/api/chat/rooms/ch_1/read"), "a method it doesn't take");
        assert!(!platform_route(&Method::GET, "/api/chat/rooms//messages"), "an empty parameter");
        assert!(!platform_route(&Method::GET, "/api/projects/"), "a trailing slash");
        assert!(!platform_route(&Method::GET, "/api/me"));
        assert!(!platform_route(&Method::GET, "/"));
        let rest = vec!["d".to_owned(), "{*rest}".to_owned()];
        assert!(matches(&rest, "/d/a/b") && !matches(&rest, "/d/") && !matches(&rest, "/d"));
    }

    /// No hub route is mistaken for a platform one: every hub path in
    /// routes.toml, its parameters filled in, is refused on the platform's
    /// host.
    #[test]
    fn no_hub_route_is_a_platform_route() {
        #[derive(serde::Deserialize)]
        struct List {
            route: Vec<R>,
        }
        #[derive(serde::Deserialize)]
        struct R {
            method: String,
            path: String,
            process: String,
        }
        let list: List = toml::from_str(include_str!("../routes.toml")).unwrap();
        let mut hub = 0;
        for r in list.route.iter().filter(|r| r.process == "hub") {
            let path: String =
                r.path.split('/').map(|s| if s.starts_with('{') { "x" } else { s }).collect::<Vec<_>>().join("/");
            for m in [Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
                assert!(!platform_route(&m, &path), "{} {} ({})", m, path, r.method);
            }
            assert!(not_here(&path), "{path}: a hub path the page would answer");
            hub += 1;
        }
        assert!(hub > 50, "{hub}");
    }
}
