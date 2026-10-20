//! The route list (`crates/control/routes.toml`, #1152): every route the
//! router registers is listed, once, with the process that answers it, and
//! nothing else is. The paths come from `routes()` itself, and the
//! platform's from `arugula_platform::paths()`; their methods
//! from the router, which says in `Allow` what a path takes when it's asked
//! with a method nothing takes.
use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    route: Vec<Route>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    method: String,
    path: String,
    process: Process,
    auth: Auth,
    clients: Vec<Client>,
    #[serde(default)]
    #[expect(dead_code, reason = "for people")]
    note: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Process {
    Hub,
    Platform,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Auth {
    Public,
    Session,
    Daemon,
    DaemonOptional,
    SessionOrDaemon,
    CloudDriver,
    SignIn,
    JoinProof,
    PollToken,
    AppTicket,
    Webhook,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Client {
    Page,
    Daemon,
    Cli,
    App,
    Browser,
    Github,
    Stripe,
    Operator,
}

const METHODS: [&str; 6] = ["GET", "POST", "PUT", "PATCH", "DELETE", "ANY"];

fn listed() -> BTreeSet<(String, String)> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("routes.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let list: List = toml::from_str(&text).unwrap_or_else(|e| panic!("crates/control/routes.toml: {e}"));
    let mut seen = BTreeSet::new();
    for r in list.route {
        let at = format!("{} {}", r.method, r.path);
        assert!(METHODS.contains(&r.method.as_str()), "routes.toml: {at}: the method is one of {METHODS:?}");
        assert!(!r.clients.is_empty(), "routes.toml: {at}: say who calls it");
        let providers = r.clients.iter().all(|c| matches!(c, Client::Github | Client::Stripe));
        assert_eq!(
            r.auth == Auth::Webhook,
            providers,
            "routes.toml: {at}: webhooks, and only webhooks, are GitHub's or Stripe's"
        );
        assert!(seen.insert((r.method, r.path)), "routes.toml lists {at} twice");
    }
    seen
}

/// A path the router matches to the route it was registered as: no static
/// segment of control's is `probe`.
fn probe_path(path: &str) -> String {
    path.split('/').map(|s| if s.starts_with('{') { "probe" } else { s }).collect::<Vec<_>>().join("/")
}

/// Every method and path the router answers.
async fn registered() -> BTreeSet<(String, String)> {
    // The hub's, and the platform's it mounts (#1153).
    let mut paths = crate::routes().paths;
    paths.extend(arugula_platform::paths());
    let mut once = BTreeSet::new();
    for p in &paths {
        assert!(once.insert(*p), "{p} is registered by two `.route` calls: make it one");
    }
    let c = crate::routing_wire::control(|_| {}).await;
    let probe = reqwest::Method::from_bytes(b"PROBE").unwrap();
    let mut got = BTreeSet::new();
    for path in paths {
        let res = c.http.request(probe.clone(), format!("{}{}", c.base, probe_path(path))).send().await.unwrap();
        // No `Allow`: one handler for every method (`any`).
        let Some(allow) = res.headers().get(reqwest::header::ALLOW) else {
            got.insert(("ANY".to_owned(), path.to_owned()));
            continue;
        };
        assert_eq!(res.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED, "{path}");
        let allow: Vec<&str> = allow.to_str().unwrap().split(',').map(str::trim).collect();
        for m in &allow {
            // A `GET` answers `HEAD` too.
            if *m != "HEAD" || !allow.contains(&"GET") {
                got.insert(((*m).to_owned(), path.to_owned()));
            }
        }
    }
    got
}

#[tokio::test]
async fn every_route_is_listed_once() {
    let listed = listed();
    let registered = registered().await;
    let show = |s: BTreeSet<&(String, String)>| s.iter().map(|(m, p)| format!("\n  {m} {p}")).collect::<String>();
    let missing = show(registered.difference(&listed).collect());
    let extra = show(listed.difference(&registered).collect());
    assert!(
        missing.is_empty() && extra.is_empty(),
        "crates/control/routes.toml doesn't match the router.\nRegistered, not listed:{missing}\nListed, not registered:{extra}"
    );
}

/// What the platform's router answers is listed as the platform's (#1152),
/// Stripe's webhook among them (M139).
#[test]
fn the_platform_answers_platform_routes() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("routes.toml");
    let list: List = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let platform = arugula_platform::paths();
    assert!(platform.contains(&"/api/stripe/webhook"), "Stripe's webhook is the platform's");
    for r in list.route.iter().filter(|r| platform.contains(&r.path.as_str())) {
        assert!(
            r.process == Process::Platform,
            "routes.toml: {} {}: the platform answers it, so `process = \"platform\"`",
            r.method,
            r.path
        );
    }
}
