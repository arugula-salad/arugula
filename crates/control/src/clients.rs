//! Who may call a route, as routes.toml's `clients` says, enforced: a CLI
//! device's signature counts only on a route whose row lists `cli`.
//!
//! A CLI's key is a file on its machine, where an agent can read it, so
//! what only the page does (allowing the desktop app's sign-in, adding a
//! passkey, enrolling and approving devices, recovery codes, and every
//! other `page` route) refuses it, whatever the handler's extractor takes.
//! The list is read from routes.toml itself, so a new route gets what its
//! row says, and a route with no row (which routes.rs's test refuses) takes
//! no CLI signature at all. Daemons' signatures are left to the handlers'
//! extractors, which take them only where a daemon belongs.

#[cfg(test)]
use std::collections::HashSet;
use std::{collections::HashMap, sync::LazyLock};

use arugula_e2e::Kind;
use axum::{
    extract::{MatchedPath, Request},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{auth::Signed, refusal};

/// The reason code a CLI's signature is refused with on a page's route.
pub const PAGE_ONLY: &str = "page_only";
/// And what it says.
pub const PAGE_ONLY_SAY: &str = "this is for Arugula's page, in a browser: a CLI's login can't do it";

#[derive(Deserialize)]
struct List {
    route: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    method: String,
    path: String,
    clients: Vec<String>,
}

/// Each listed route, by method and path, to whether a CLI may call it.
static CLI_MAY: LazyLock<HashMap<(String, String), bool>> = LazyLock::new(|| {
    let list: List = toml::from_str(include_str!("../routes.toml")).expect("routes.toml parses");
    list.route.into_iter().map(|r| ((r.method, r.path), r.clients.iter().any(|c| c == "cli"))).collect()
});

/// Whether a CLI's signature may call `method` on the route registered as
/// `path`. A `HEAD` is its `GET`; a route registered for every method is
/// listed as `ANY`. Not listed: no.
pub fn cli_may(method: &Method, path: &str) -> bool {
    let m = if method == Method::HEAD { "GET" } else { method.as_str() };
    let get = |m: &str| CLI_MAY.get(&(m.to_owned(), path.to_owned())).copied();
    get(m).or_else(|| get("ANY")).unwrap_or(false)
}

/// The routes no CLI calls, as (method, path): for tests that try each.
#[cfg(test)]
pub fn page_only() -> HashSet<(String, String)> {
    CLI_MAY.iter().filter(|(_, may)| !**may).map(|(k, _)| k.clone()).collect()
}

/// Refuses a CLI device's signature on a route whose row doesn't list
/// `cli`. Runs after `auth::verify_daemon`, which checked the signature.
pub async fn gate(req: Request, next: Next) -> Response {
    if let Some(Signed(Ok(cert))) = req.extensions().get::<Signed>()
        && cert.kind == Kind::Cli
    {
        let path = req.extensions().get::<MatchedPath>().map(|p| p.as_str().to_owned());
        if !path.is_some_and(|p| cli_may(req.method(), &p)) {
            return refusal(StatusCode::UNAUTHORIZED, PAGE_ONLY, PAGE_ONLY_SAY).into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{Cert, DeviceKeys, now_ms};
    use serde_json::Value;

    use super::*;
    use crate::routing_wire::{control, person, v2};

    #[test]
    fn rows_say_who_calls_them() {
        assert!(cli_may(&Method::GET, "/api/devices"), "the CLI reads the account's devices");
        assert!(!cli_may(&Method::POST, "/api/devices"), "but only the page enrolls one");
        assert!(!cli_may(&Method::POST, "/api/app-login/{id}/allow"));
        assert!(!cli_may(&Method::POST, "/auth/passkey/register"));
        assert!(cli_may(&Method::HEAD, "/api/directory"), "a HEAD is its GET");
        assert!(!cli_may(&Method::GET, "/api/not-a-route"), "not listed: no");
        assert!(page_only().contains(&("POST".to_owned(), "/api/devices/{id}/approve".to_owned())));
    }

    /// A path the router matches to the route it was registered as.
    fn probe_path(path: &str) -> String {
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
    }

    /// Every route routes.toml says no CLI calls refuses an approved CLI's
    /// signature, before its handler looks: 401, `page_only`. Those that
    /// list `cli` don't refuse it so.
    #[tokio::test]
    async fn a_cli_signature_is_refused_on_every_route_that_isnt_the_clis() {
        let c = control(|_| {}).await;
        let root = person(&c.app, "jake", "a1");
        let cli = DeviceKeys::generate();
        let mut cert = Cert::new(&cli, "a1", Kind::Cli, "Arugula CLI");
        cert.sign_with(&root);
        c.app.db.put_device(&cert, true, now_ms()).unwrap();
        let ask = |method: &str, path: &str| {
            let method = if method == "ANY" { "POST" } else { method };
            let path = probe_path(path);
            let body: &[u8] = if method == "GET" || method == "DELETE" { b"" } else { b"{}" };
            let mut req = c
                .http
                .request(method.parse().unwrap(), format!("{}{path}", c.base))
                .header("x-arugula-auth", v2(&cli, method, &path, body));
            if !body.is_empty() {
                req = req.header("content-type", "application/json").body(body.to_vec());
            }
            async move {
                // A route that streams or waits may not answer at once:
                // it wasn't refused, which is all this asks.
                match tokio::time::timeout(std::time::Duration::from_secs(5), req.send()).await {
                    Ok(Ok(r)) => (r.status().as_u16(), r.json::<Value>().await.unwrap_or_default()),
                    _ => (0, Value::Null),
                }
            }
        };
        let page_only = page_only();
        assert!(page_only.len() > 100, "routes.toml has a hundred page routes and more");
        for (method, path) in &page_only {
            let (st, v) = ask(method, path).await;
            assert_eq!((st, v["reason"].as_str()), (401, Some(PAGE_ONLY)), "{method} {path}: {v}");
        }
        for ((method, path), _) in CLI_MAY.iter().filter(|(_, may)| **may) {
            let (_, v) = ask(method, path).await;
            assert_ne!(v["reason"].as_str(), Some(PAGE_ONLY), "{method} {path} is the CLI's too");
        }
        // The page, with its cookie, isn't refused so.
        let cookie = crate::routing_wire::session(&c.app, "a1");
        let (st, _) = c.as_person(&cookie, "GET", "/api/me/passkeys", None).await;
        assert_eq!(st, 200);
    }
}
