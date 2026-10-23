//! The hub as the platform's gateway (M140), over real sockets: a hub with
//! `--platform http://…`, its internal API on a listener of its own, and
//! the platform as its own server reaching the hub only through
//! `HttpHub`. Then a fake platform, to see exactly what crosses: what's
//! dropped, what's signed, what streams.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_common::principal::{self, NotVouched, PrincipalKey, Vouch};
use arugula_e2e::now_ms;
use arugula_platform::{
    Platform, PlatformPort, page,
    remote::{self, HttpHub},
};
use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use futures_util::StreamExt as _;
use serde_json::Value;

use crate::{
    App,
    gateway::{PlatformAt, Remote},
    routing_wire::{daemon, session, v2},
};

/// The platform page's host in these tests.
const APP: &str = "app.control.test";

/// The platform's stand-in page build, one for the run.
static PAGE: std::sync::LazyLock<crate::platform_host_wire::Files> =
    std::sync::LazyLock::new(|| crate::platform_host_wire::Files::page("platform"));

fn key() -> PrincipalKey {
    PrincipalKey::new(&[5; 32]).unwrap()
}

async fn listener() -> (tokio::net::TcpListener, String) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    (l, base)
}

fn serve_hub(app: &Arc<App>, l: tokio::net::TcpListener) {
    let router = crate::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(l, router).await });
}

/// A hub whose platform is at `platform`: its public address.
async fn hub(platform: &str) -> (Arc<App>, String) {
    hub_with(platform, None).await
}

/// [`hub`], with the GitHub App's slug (M161: the App is the platform's).
async fn hub_with(platform: &str, github_app: Option<String>) -> (Arc<App>, String) {
    let mut app = App::for_tests("http://control.test");
    app.github_app = github_app;
    app.platform = PlatformAt::Remote(Remote::new(platform, key()).unwrap());
    // The platform page's host (M151).
    app.cfg.platform_host = crate::platform_host::parse(APP, "http://control.test", &app.cfg.sites, None).unwrap();
    let app = app.start();
    let (l, base) = listener().await;
    serve_hub(&app, l);
    (app, base)
}

pub(crate) struct Split {
    pub(crate) hub: Arc<App>,
    pub(crate) base: String,
    pub(crate) platform: Arc<Platform>,
}

/// The hub and a real platform, each its own server.
pub(crate) async fn split() -> Split {
    split_with(|hub| hub).await
}

/// [`split`], with the platform's `HttpHub` wrapped (`hub_latency_bench`
/// adds a delay to every hub call, as between two regions).
pub(crate) async fn split_with(
    wrap: impl FnOnce(Arc<dyn arugula_platform::HubPort>) -> Arc<dyn arugula_platform::HubPort>,
) -> Split {
    split_on(arugula_platform::db::Db::memory(), wrap).await
}

/// [`split_with`], with the platform on `db` (`hub_latency_bench` on
/// Postgres).
pub(crate) async fn split_on(
    db: arugula_platform::db::Db,
    wrap: impl FnOnce(Arc<dyn arugula_platform::HubPort>) -> Arc<dyn arugula_platform::HubPort>,
) -> Split {
    split_github(db, wrap, None).await
}

/// [`split_on`], with the GitHub App on the platform and its slug on the
/// hub (M161, `forge_wire`).
pub(crate) async fn split_github(
    db: arugula_platform::db::Db,
    wrap: impl FnOnce(Arc<dyn arugula_platform::HubPort>) -> Arc<dyn arugula_platform::HubPort>,
    github: Option<arugula_platform::forge::GithubApp>,
) -> Split {
    let (internal_l, internal) = listener().await;
    let (platform_l, platform_base) = listener().await;
    let mut p = Platform::new(
        arugula_platform::Config {
            public_url: "http://control.test".into(),
            sites: vec!["http://control.test".into()],
            relay_free_bytes: 0,
        },
        db,
        wrap(Arc::new(HttpHub::new(&internal, key()).unwrap())),
        reqwest::Client::new(),
        Default::default(),
    );
    let slug = github.as_ref().map(|g| g.slug.clone());
    p.github = github;
    let p = Arc::new(p);
    let port: Arc<dyn PlatformPort> = p.clone();
    // As the binary's: the routes, the internal API and the page (M151),
    // a stand-in build of it.
    let router = Router::new()
        .merge(arugula_platform::router(p.clone()).layer(axum::middleware::from_fn_with_state(key(), remote::vouched)))
        .merge(remote::serve_platform(port, key()))
        .merge(page::behind_hub(page::Page::new(Some(PAGE.0.clone())), key()));
    tokio::spawn(async move { axum::serve(platform_l, router).await });
    let (app, base) = hub_with(&platform_base, slug).await;
    let a2 = app.clone();
    tokio::spawn(async move { axum::serve(internal_l, crate::internal::router(&a2, key())).await });
    let now = now_ms();
    app.db.account_for("test", "a", "a", "a", now).unwrap();
    app.db.add_session(&crate::auth::hash("session-a"), "a", now, now + 60_000).unwrap();
    Split { hub: app, base, platform: p }
}

async fn answer(r: reqwest::Response) -> (u16, Value) {
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// Through the gateway, the platform answers as it does in process: a
/// person by their session, a machine by its signature (its `cloud.drive`
/// grant checked by the platform, which asks the hub over HTTP), each
/// refusal word for word, a signature good once, and a principal header a
/// client made up worth nothing. The hub's own calls into the platform go
/// over HTTP too.
#[tokio::test]
async fn the_platform_answers_through_the_gateway() {
    let s = split().await;
    let http = reqwest::Client::new();
    let cookie = "ilg_session=session-a";

    let r = http.get(format!("{}/api/workspaces", s.base)).header("cookie", cookie).send().await.unwrap();
    assert_eq!(answer(r).await, (200, serde_json::json!({ "workspaces": [] })));
    let r = http.get(format!("{}/api/workspaces", s.base)).send().await.unwrap();
    assert_eq!(answer(r).await, (401, serde_json::json!({ "error": "sign in first" })));
    let r = http
        .post(format!("{}/api/projects/prj_x/workspaces/hw_x/secrets/X", s.base))
        .header("cookie", cookie)
        .header("origin", "https://elsewhere.test")
        .send()
        .await
        .unwrap();
    assert_eq!(answer(r).await.0, 405, "PUT, not POST: the method list is routes.toml's");
    let r = http
        .put(format!("{}/api/projects/prj_x/workspaces/hw_x/secrets/X", s.base))
        .header("cookie", cookie)
        .header("origin", "https://elsewhere.test")
        .json(&serde_json::json!({ "value": "v" }))
        .send()
        .await
        .unwrap();
    assert_eq!(answer(r).await, (403, serde_json::json!({ "error": "cross-origin request refused" })));

    // A made-up principal header is dropped: the hub vouches for nobody.
    let forged = key().sign(
        &arugula_control_common::Principal::Person { account: "a".into() },
        "GET",
        "/api/workspaces",
        "req-x",
        now_ms(),
    );
    let r = http.get(format!("{}/api/workspaces", s.base)).header(principal::HEADER, forged).send().await.unwrap();
    assert_eq!(answer(r).await.0, 401, "even one signed with the right key");

    // A machine drives its account's Cloud workspaces: the platform finds
    // its grant from the hub (`daemons`, over HTTP).
    let keys = daemon(&s.hub, "a", "geek");
    let path = "/api/workspaces";
    let sig = v2(&keys, "GET", path, b"");
    let r = http.get(format!("{}{path}", s.base)).header("x-arugula-auth", &sig).send().await.unwrap();
    assert_eq!(answer(r).await.0, 200, "a joined machine drives at once");
    let r = http.get(format!("{}{path}", s.base)).header("x-arugula-auth", &sig).send().await.unwrap();
    assert_eq!(
        answer(r).await,
        (401, serde_json::json!({ "error": "that signature was used already" })),
        "a signature is good once, and the hub's refusal is the answer"
    );
    let r = http.get(format!("{}/api/billing", s.base)).header("cookie", cookie).send().await.unwrap();
    assert_eq!(answer(r).await.1["billing"], false);

    // The hub asks the platform over HTTP: who pays, whether to delete.
    assert!(s.hub.platform.active_plans(&["account:a".into()]).await.unwrap().is_empty());
    assert!(s.hub.platform.delete_blockers("a", false).await.unwrap().is_empty());
    assert!(s.platform.db.mentions("geek").is_empty(), "nothing of the hub's is in the platform's tables");
}

/// Where a forwarded call's time went: the platform's `Server-Timing`
/// entries, as it said them, then the gateway's `up` and `gw`, in one
/// header, with nothing in it but names, durations and counts.
#[tokio::test]
async fn where_the_time_went_comes_back() {
    let s = split().await;
    let r = reqwest::Client::new()
        .get(format!("{}/api/workspaces", s.base))
        .header("cookie", "ilg_session=session-a")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let all: Vec<&str> = r.headers().get_all("server-timing").iter().map(|v| v.to_str().unwrap()).collect();
    assert_eq!(all.len(), 1, "one header: {all:?}");
    let names: Vec<&str> = all[0].split(", ").map(|e| e.split(';').next().unwrap()).collect();
    assert_eq!(names, ["db", "dbq", "hub", "hubn", "app", "up", "gw"], "{}", all[0]);
    let n = |name: &str| -> f64 {
        let e = all[0].split(", ").find(|e| e.starts_with(&format!("{name};"))).unwrap();
        e.split_once('=').unwrap().1.parse().unwrap()
    };
    assert!(n("app") <= n("up") && n("up") <= n("gw"), "{}", all[0]);
    assert!(
        all[0].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ";=., ".contains(c)),
        "names and numbers only: {}",
        all[0]
    );
}

/// Chat's live stream goes through as it comes, unbuffered, with
/// `Last-Event-ID` passed on: an id from another epoch is answered with a
/// resync, after the hello, while the stream stays open.
#[tokio::test]
async fn chat_streams_through_the_gateway() {
    let s = split().await;
    s.hub.db.set_labs_flag("account:a", "chat", Some(true)).unwrap();
    let r = reqwest::Client::new()
        .get(format!("{}/api/chat/live", s.base))
        .header("cookie", "ilg_session=session-a")
        .header("last-event-id", "anotherepoch-3")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/event-stream");
    let mut stream = r.bytes_stream();
    let mut seen = String::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !(seen.contains("hello") && seen.contains("resync")) {
            seen.push_str(&String::from_utf8_lossy(&stream.next().await.unwrap().unwrap()));
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the stream's first events didn't come through: {seen:?}"));
    let more = tokio::time::timeout(Duration::from_millis(300), stream.next()).await;
    assert!(more.is_err(), "and it's still open: {more:?}");
}

/// What the fake platform saw of each call.
#[derive(Default, Clone)]
struct Seen(Arc<Mutex<Vec<(String, HeaderMap, usize)>>>);

/// A platform that answers whatever it's sent: `/slow` streams three
/// chunks a second apart; anything else says 429 with `Retry-After: 7`
/// and a body, after reading the request's whole body.
async fn fake_platform() -> (String, Seen) {
    let seen = Seen::default();
    let s2 = seen.clone();
    let router = Router::new().fallback(move |req: Request| {
        let seen = s2.clone();
        async move {
            let (parts, body) = req.into_parts();
            let n = axum::body::to_bytes(body, 64 << 20).await.unwrap().len();
            let pq = parts.uri.path_and_query().unwrap().as_str().to_owned();
            seen.0.lock().unwrap().push((pq.clone(), parts.headers.clone(), n));
            if pq == "/api/chat/live" {
                let chunks = futures_util::stream::unfold(0, |i| async move {
                    if i == 3 {
                        return None;
                    }
                    if i > 0 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Some((Ok::<_, std::io::Error>(format!("data: {i}\n\n")), i + 1))
                });
                return Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(chunks))
                    .unwrap();
            }
            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("retry-after", "7")
                .header("x-from", "platform")
                .body(Body::from(format!("{{\"got\":{n}}}")))
                .unwrap()
        }
    });
    let (l, base) = listener().await;
    tokio::spawn(async move { axum::serve(l, router).await });
    (base, seen)
}

/// The hub drops the cookie, the signature and any principal header a
/// client sent, keeps the rest (the Host header among them), signs what it
/// found for exactly that call, and passes the answer back as it is:
/// status, `Retry-After`, body. A big body streams through whole; a
/// stream's chunks come as they're sent.
#[tokio::test]
async fn what_crosses_the_gateway() {
    let (platform, seen) = fake_platform().await;
    let (app, base) = hub(&platform).await;
    let cookie = session(&app, "acct-a");
    app.db.account_for("test", "acct-a", "alice", "acct-a", now_ms()).unwrap();
    let http = reqwest::Client::new();
    let r = http
        .post(format!("{base}/api/chat/rooms/r1/messages?x=1"))
        .header("cookie", &cookie)
        .header("host", "control.test")
        .header("x-arugula-principal", "v1 made up")
        .header("x-arugula-internal", "v1 made up")
        .header("origin", "http://control.test")
        .body("{\"text\":\"hi\"}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    assert_eq!(r.headers()["retry-after"], "7");
    assert_eq!(r.headers()["x-from"], "platform");
    assert_eq!(r.text().await.unwrap(), "{\"got\":13}");
    let (pq, h, n) = seen.0.lock().unwrap().pop().unwrap();
    assert_eq!((pq.as_str(), n), ("/api/chat/rooms/r1/messages?x=1", 13), "path, query and body untouched");
    assert!(h.get("cookie").is_none() && h.get("x-arugula-internal").is_none());
    assert_eq!(h["host"], "control.test");
    assert_eq!(h["origin"], "http://control.test");
    let (vouch, _) = key()
        .verify_vouch(h[principal::HEADER].to_str().unwrap(), "POST", &pq, now_ms())
        .expect("signed for this call");
    assert_eq!(vouch, Vouch::Principal(arugula_control_common::Principal::Person { account: "acct-a".into() }));

    // Nobody signed in: the platform still gets a header, saying how the
    // hub would have refused a session (a webhook takes none).
    http.post(format!("{base}/api/stripe/webhook")).body("{}").send().await.unwrap();
    let (pq, h, _) = seen.0.lock().unwrap().pop().unwrap();
    let (vouch, _) = key().verify_vouch(h[principal::HEADER].to_str().unwrap(), "POST", &pq, now_ms()).unwrap();
    assert_eq!(
        vouch,
        Vouch::Not(NotVouched::Refused {
            status: 401,
            error: "sign in first".into(),
            by: arugula_control_common::principal::Credential::Session
        })
    );

    // A daemon's signed call: the signature stays with the hub, and the
    // body it covers goes on as it was.
    let keys = daemon(&app, "acct-a", "geek");
    let body = vec![b'x'; 1000];
    let path = arugula_control_wire::chat::CHAT_IMPORT;
    let sig = v2(&keys, "POST", path, &body);
    http.post(format!("{base}{path}")).header("x-arugula-auth", sig).body(body).send().await.unwrap();
    let (_, h, n) = seen.0.lock().unwrap().pop().unwrap();
    assert!(h.get("x-arugula-auth").is_none());
    let (vouch, _) = key().verify_vouch(h[principal::HEADER].to_str().unwrap(), "POST", path, now_ms()).unwrap();
    assert_eq!(
        vouch,
        Vouch::Principal(arugula_control_common::Principal::Daemon { account: "acct-a".into(), daemon: keys.id() })
    );
    assert_eq!(n, 1000);

    // A big unsigned body streams through whole.
    let body = vec![b'y'; 3 << 20];
    let r = http.post(format!("{base}{path}")).header("cookie", &cookie).body(body).send().await.unwrap();
    assert_eq!(r.text().await.unwrap(), format!("{{\"got\":{}}}", 3 << 20));

    // A stream's chunks arrive a second apart, not all at the end.
    let r = http.get(format!("{base}/api/chat/live")).header("cookie", &cookie).send().await.unwrap();
    let started = std::time::Instant::now();
    let mut stream = r.bytes_stream();
    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(&first[..], b"data: 0\n\n");
    assert!(started.elapsed() < Duration::from_millis(800), "the first chunk came at once");
    let mut rest = Vec::new();
    while let Some(c) = stream.next().await {
        rest.extend_from_slice(&c.unwrap());
    }
    assert_eq!(rest, b"data: 1\n\ndata: 2\n\n");
    assert!(started.elapsed() >= Duration::from_secs(2));
}

/// A platform that can't be reached (a deploy restarting it): tried again
/// for a few seconds, then the maintenance answer, which clients wait out.
/// Hub routes don't notice.
#[tokio::test]
async fn an_unreachable_platform_is_a_short_maintenance() {
    let (l, gone) = listener().await;
    drop(l);
    let (app, base) = hub(&gone).await;
    let cookie = session(&app, "acct-a");
    let http = reqwest::Client::new();
    let started = std::time::Instant::now();
    let r = http
        .post(format!("{base}/api/chat/rooms/r1/messages"))
        .header("cookie", &cookie)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 503);
    assert_eq!(r.headers()["retry-after"], "5");
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        serde_json::json!({ "error": "Chat and Cloud are down for maintenance; try again in 5 seconds.", "reason": "maintenance" })
    );
    assert!(started.elapsed() >= Duration::from_secs(4), "tried again first");
    assert_eq!(http.get(format!("{base}/control.json")).send().await.unwrap().status(), 200);
}

/// Through Flycast (M144) the platform's address is Fly's proxy, which
/// takes the connection even with no machine behind it and closes it
/// unanswered. A `GET` is tried again and then gets the maintenance answer;
/// a `POST`, which may have arrived, is a 502 at once.
#[tokio::test]
async fn a_proxy_with_no_platform_behind_it_is_a_short_maintenance_for_reads() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((c, _)) = l.accept().await {
            drop(c);
        }
    });
    let (app, base) = hub(&proxy).await;
    let cookie = session(&app, "acct-a");
    let http = reqwest::Client::new();
    let started = std::time::Instant::now();
    let r = http.get(format!("{base}/api/chat")).header("cookie", &cookie).send().await.unwrap();
    assert_eq!(r.status(), 503);
    assert_eq!(r.headers()["retry-after"], "5");
    assert!(started.elapsed() >= Duration::from_secs(4), "tried again first");
    let started = std::time::Instant::now();
    let r = http
        .post(format!("{base}/api/chat/rooms/r1/messages"))
        .header("cookie", &cookie)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 502, "a post may have arrived: not sent again");
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// Out of process, deleting an account is two steps: the hub commits, then
/// tells the platform. A deletion the platform didn't hear of is kept
/// (`untold_deletions`) and told again until it's taken.
#[tokio::test]
async fn a_deletion_the_platform_missed_is_told_again() {
    let erase = |app: &App, account: &str| {
        app.db.account_for("test", account, account, account, now_ms()).unwrap();
        app.db.delete_account(&crate::db::Erase { account, disband: &[], retain_for: &[] }).unwrap();
    };
    let (l, gone) = listener().await;
    drop(l);
    let (away, _) = hub(&gone).await;
    erase(&away, "acct-gone");
    assert_eq!(crate::account::tell_untold(&away).await, 0, "the platform is away");
    assert_eq!(away.db.untold_deletions().unwrap().len(), 1, "so it's kept");

    let s = split().await;
    erase(&s.hub, "acct-gone");
    assert_eq!(s.hub.db.untold_deletions().unwrap().len(), 1);
    assert_eq!(crate::account::tell_untold(&s.hub).await, 1);
    assert!(s.hub.db.untold_deletions().unwrap().is_empty(), "taken, and forgotten");
}

/// Out of process, the platform page's host is answered by the platform:
/// a page GET is forwarded with the hub's signed principal header and
/// served from the platform's own build, under the page's headers; a
/// platform route reaches the platform (nobody signed in there yet), and
/// nothing of the hub's or the platform's own answers.
#[tokio::test]
async fn the_platform_page_comes_from_the_platform() {
    use crate::platform_host_wire::Answer;
    let s = split().await;
    let http = reqwest::Client::new();
    let get = |m: reqwest::Method, path: &str| {
        let r = http
            .request(m, format!("{}{path}", s.base))
            .header("host", APP)
            .header("cookie", "ilg_session=session-a")
            .send();
        async move { Answer::of(r.await.unwrap()).await }
    };
    for path in ["/", "/p/prj_8f2k-arugula/chat/ch_1", "/p/prj_1/workspaces/cw_1/files"] {
        let a = get(reqwest::Method::GET, path).await;
        assert_eq!(a.status, 200, "{path}: {}", a.body);
        assert!(a.body.contains("<title>platform</title>"), "{path}");
        assert!(a.is_the_platforms(), "{path}: {:?}", a.headers);
    }
    let a = get(reqwest::Method::GET, "/assets/app-1.js").await;
    assert_eq!((a.status, a.body.as_str()), (200, "// platform"));
    assert_eq!(a.header("cache-control"), "public, max-age=31536000, immutable");
    assert_eq!(get(reqwest::Method::GET, "/assets/gone.js").await.status, 404);
    let a = get(reqwest::Method::GET, "/api/projects").await;
    assert_eq!((a.status, a.is_the_platforms()), (401, true), "{}", a.body);
    for path in ["/api/me", "/control.json", "/platform.json", "/auth/github"] {
        let a = get(reqwest::Method::GET, path).await;
        assert_eq!((a.status, a.is_the_platforms()), (404, true), "{path}");
    }
    assert_eq!(get(reqwest::Method::POST, "/p/x").await.status, 405);
    // Control's own site is still the hub's.
    let r = http.get(format!("{}/api/projects", s.base)).header("cookie", "ilg_session=session-a").send().await;
    assert_eq!(r.unwrap().status(), 200);
}

/// Out of process (M152): signed in on control's page, the platform page's
/// sign-in gives a session on its host, and the gateway answers the
/// platform's routes there as that account; signing out on the hub ends it.
#[tokio::test]
async fn the_platform_page_signs_in_through_the_hub() {
    use base64::Engine as _;
    use sha2::Digest as _;
    let s = split().await;
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let verifier = "v".repeat(43);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    let back = "http://app.control.test/";
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("return", back)
        .append_pair("challenge", &challenge)
        .append_pair("state", "state-0123456789ab")
        .finish();
    let r = http
        .get(format!("{}/signin/platform?{q}", s.base))
        .header("cookie", "ilg_session=session-a")
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    let at = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
    assert_eq!(st, 303, "{at} {}", r.text().await.unwrap_or_default());
    let code = at.split("#code=").nth(1).unwrap().split('&').next().unwrap().to_owned();
    let r = http
        .post(format!("{}/api/app/session", s.base))
        .header("host", APP)
        .header("origin", "http://app.control.test")
        .json(&serde_json::json!({ "code": code, "verifier": verifier, "return": back }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 204);
    let cookie = r.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let projects = || http.get(format!("{}/api/projects", s.base)).header("host", APP).header("cookie", &cookie).send();
    let (st, v) = answer(projects().await.unwrap()).await;
    assert_eq!((st, &v["me"]["account"]), (200, &serde_json::json!("a")), "{v}");
    s.hub.db.drop_sessions("a").unwrap();
    assert_eq!(projects().await.unwrap().status().as_u16(), 401);
}
