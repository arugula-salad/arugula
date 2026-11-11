//! The launch switches (#531) on the hub: the operator's command, sign-ups
//! paused and limited, a suspended account refused everywhere, the relay's
//! cap, and what the platform is told.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use arugula_control_common::Principal;
use arugula_e2e::now_ms;
use arugula_platform_api::HubPort as _;
use axum::{
    Json, Router,
    extract::State as AxState,
    http::{HeaderMap, StatusCode, header},
    routing::{delete, get, post},
};
use serde_json::{Value, json};

use super::*;
use crate::{
    Github,
    auth::{SESSION_COOKIE, hash},
    db::Db,
    routing_wire::{control, daemon, person, session, v2},
};

fn temp_db() -> (PathBuf, Db) {
    let dir = std::env::temp_dir().join(format!("arugula-switches-{}", hex::encode(arugula_e2e::random::<6>())));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("control.db");
    let db = Db::open(&path).unwrap();
    (path, db)
}

fn cmd(path: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
    let a = Args::parse_from(
        ["arugula-control switches"]
            .iter()
            .chain(args)
            .map(|s| (*s).to_owned())
            .chain(["--db".into(), path.display().to_string()]),
    );
    let c = Connection::open(&a.db).unwrap();
    command(&c, a.what)
}

fn read(db: &Db) -> State {
    let (g, a) = db.switches().unwrap();
    State::from_rows(&g, &a)
}

/// Each of the operator's commands sets the row the hub reads, in the
/// hub's own file, and says what it did; what isn't so is refused and
/// changes nothing.
#[test]
fn the_operators_command_sets_each_switch() {
    let (path, db) = temp_db();
    db.account_for("github", "4242", "Miner", "acct-x", 1).unwrap();
    db.account_for("github", "77", "sam", "acct-s", 1).unwrap();
    assert_eq!(read(&db), State::defaults(), "nothing set: the defaults");
    assert_eq!((BOXES, BOX_HOURS, SIGNUPS_PER_HOUR, RELAY_MB), (2, 60, 60, 10_000), "Jake's moderate caps");

    let said = cmd(&path, &["signups", "pause"]).unwrap();
    assert!(said.contains(PAUSED_SAY), "{said}");
    assert_eq!(read(&db).signups_paused.as_deref(), Some(""));
    cmd(&path, &["signups", "pause", "--message", "Back on Monday."]).unwrap();
    assert_eq!(read(&db).signups_paused.as_deref(), Some("Back on Monday."));
    cmd(&path, &["signups", "open"]).unwrap();
    assert_eq!(read(&db).signups_paused, None);

    cmd(&path, &["hosted", "off"]).unwrap();
    assert_eq!(read(&db).hosted, Hosted::Off);
    cmd(&path, &["hosted", "open"]).unwrap();
    assert_eq!(read(&db).hosted, Hosted::Open);
    cmd(&path, &["hosted", "waitlist"]).unwrap();
    assert_eq!(read(&db).hosted, Hosted::Waitlist);
    assert!(db.switches().unwrap().0.is_empty(), "the default is no row");

    cmd(&path, &["cap", "boxes", "3"]).unwrap();
    cmd(&path, &["cap", "box-hours", "100", "--account", "github:miner"]).unwrap();
    cmd(&path, &["cap", "relay-mb", "500", "--account", "acct-s"]).unwrap();
    cmd(&path, &["cap", "signups-per-hour", "5"]).unwrap();
    let s = read(&db);
    assert_eq!((s.boxes, s.box_hours, s.signups_per_hour, s.relay_mb), (3, 60, 5, None));
    let x = s.standing("acct-x");
    assert_eq!((x.boxes, x.box_hours), (3, 100), "everyone's boxes, its own hours");
    assert_eq!((s.relay_bytes("acct-s"), s.relay_bytes("acct-x")), (Some(500_000_000), None));
    cmd(&path, &["cap", "box-hours", "default", "--account", "acct-x"]).unwrap();
    assert_eq!(read(&db).standing("acct-x").box_hours, 60);
    for bad in [
        &["cap", "boxes", "lots"][..],
        &["cap", "signups-per-hour", "9", "--account", "acct-x"],
        &["cap", "boxes", "1", "--account", "nobody"],
        &["admit", "github:nobody"],
        &["suspend", "acct-x", "--by", "jake", "--why", " "],
    ] {
        let before = db.switches().unwrap();
        assert!(cmd(&path, bad).is_err(), "{bad:?}");
        assert_eq!(db.switches().unwrap(), before, "{bad:?} changed nothing");
    }

    // The waitlist, first first; an admission takes an account off it.
    db.wait_for_box("acct-x", 10).unwrap();
    db.wait_for_box("acct-s", 20).unwrap();
    db.wait_for_box("acct-x", 30).unwrap();
    let w = cmd(&path, &["waitlist"]).unwrap();
    assert!(w.starts_with("waiting: 2\n  acct-x"), "{w}");
    assert!(w.contains("github:Miner"), "{w}");
    cmd(&path, &["admit", "github:Miner", "--by", "jake"]).unwrap();
    let s = read(&db);
    assert!(s.standing("acct-x").admitted && !s.standing("acct-s").admitted);
    assert!(cmd(&path, &["waitlist"]).unwrap().starts_with("waiting: 1\n  acct-s"));
    cmd(&path, &["unadmit", "acct-x"]).unwrap();
    assert!(!read(&db).standing("acct-x").admitted);

    cmd(&path, &["suspend", "github:miner", "--by", "jake", "--why", "mining on a hosted box"]).unwrap();
    let s = read(&db);
    assert!(s.suspended("acct-x") && !s.suspended("acct-s"));
    assert_eq!(s.accounts["acct-x"].suspended.as_deref(), Some("jake: mining on a hosted box"));
    let st = cmd(&path, &["status", "acct-x"]).unwrap();
    assert!(st.contains("suspended: 1\n  acct-x  jake: mining on a hosted box"), "{st}");
    assert!(st.contains("account acct-x: suspended, "), "{st}");
    cmd(&path, &["unsuspend", "acct-x"]).unwrap();
    assert!(!read(&db).suspended("acct-x"));

    // Deleting an account takes its switches with it.
    db.delete_account(&crate::db::Erase { account: "acct-s", disband: &[], retain_for: &[] }).unwrap();
    assert!(!read(&db).accounts.contains_key("acct-s"));
    drop(db);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// ---------------------------------------------------------------- GitHub sign-in

/// What the fake GitHub saw: tokens revoked.
type Revoked = Arc<Mutex<usize>>;

/// A code `c<id>` is GitHub user `<id>`, whose login is `gh<id>`.
async fn fake_github(revoked: Revoked) -> String {
    let r = Router::new()
        .route(
            "/login/oauth/access_token",
            post(|body: String| async move {
                let id = body.split('&').find_map(|kv| kv.strip_prefix("code=c")).unwrap_or("0").to_owned();
                Json(json!({ "access_token": format!("ghu_{id}"), "token_type": "bearer" }))
            }),
        )
        .route(
            "/user",
            get(|h: HeaderMap| async move {
                let id: u64 =
                    h[header::AUTHORIZATION].to_str().unwrap().strip_prefix("Bearer ghu_").unwrap().parse().unwrap();
                Json(json!({ "id": id, "login": format!("gh{id}") }))
            }),
        )
        .route(
            "/applications/cid/token",
            delete(|AxState(r): AxState<Revoked>| async move {
                *r.lock().unwrap() += 1;
                StatusCode::NO_CONTENT
            }),
        )
        .with_state(revoked);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    base
}

struct Rig {
    app: Arc<App>,
    base: String,
    revoked: Revoked,
    http: reqwest::Client,
}

/// Control signing in with a fake GitHub, where GitHub 8 is `acct-old`.
async fn rig() -> Rig {
    let revoked = Revoked::default();
    let api = fake_github(revoked.clone()).await;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let mut app = App::for_tests(&base);
    app.cfg.github = Some(Github { client_id: "cid".into(), client_secret: "s".into(), url: api.clone(), api });
    let app = app.start();
    app.db.account_for("github", "8", "gh8", "acct-old", now_ms()).unwrap();
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Rig { app, base, revoked, http }
}

/// How a GitHub sign-in as user `id` ended.
struct Ended {
    status: StatusCode,
    session: Option<String>,
    body: String,
}

impl Rig {
    async fn sign_in(&self, id: u64) -> Ended {
        let s = self.http.get(format!("{}/auth/github", self.base)).send().await.unwrap();
        let kept = s.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
        let st = kept.split_once('=').unwrap().1.split_once('.').unwrap().0.to_owned();
        let r = self
            .http
            .get(format!("{}/auth/github/callback?code=c{id}&state={st}", self.base))
            .header(header::COOKIE, &kept)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let session = r.headers().get_all(header::SET_COOKIE).iter().find_map(|v| {
            v.to_str()
                .unwrap()
                .strip_prefix(&format!("{SESSION_COOKIE}="))
                .map(|v| v.split(';').next().unwrap().to_owned())
                .filter(|v| !v.is_empty())
        });
        Ended { status, session, body: r.text().await.unwrap() }
    }

    fn set(&self, name: &str, value: Option<&str>) {
        self.app.db.set_switch(name, value, now_ms()).unwrap();
        refresh(&self.app).unwrap();
    }

    fn account_of(&self, id: u64) -> Option<String> {
        self.app.db.identity_account("github", &id.to_string()).unwrap()
    }
}

/// While sign-ups are paused, GitHub's sign-in makes no account: the
/// newcomer gets a page that says so (the operator's words when there are
/// some), and the token GitHub gave is revoked. Someone with an account
/// signs in as ever. Open again, the newcomer signs up.
#[tokio::test]
async fn paused_sign_ups_turn_newcomers_away_with_a_page() {
    let r = rig().await;
    r.set("signups", Some(""));
    let e = r.sign_in(7).await;
    assert_eq!(e.status, StatusCode::FORBIDDEN);
    assert!(e.session.is_none());
    assert!(e.body.contains("<h1>New sign-ups are paused</h1>"), "{}", e.body);
    assert!(e.body.contains("Arugula isn&#39;t taking new sign-ups right now") || e.body.contains("isn't taking"));
    assert_eq!(r.account_of(7), None, "no account made");
    assert_eq!(*r.revoked.lock().unwrap(), 1, "GitHub's token went");
    r.set("signups", Some("We're full for today: back tomorrow."));
    assert!(r.sign_in(7).await.body.contains("back tomorrow"));

    let e = r.sign_in(8).await;
    assert_eq!(e.status, StatusCode::SEE_OTHER, "{}", e.body);
    assert_eq!(r.app.db.session(&hash(&e.session.unwrap()), now_ms()).unwrap().as_deref(), Some("acct-old"));

    r.set("signups", None);
    let e = r.sign_in(7).await;
    assert_eq!(e.status, StatusCode::SEE_OTHER);
    assert!(e.session.is_some() && r.account_of(7).is_some());
}

/// New accounts per client address an hour, by GitHub or a passkey, are
/// the operator's cap: past it, a plain refusal; signing in to an account
/// that's there isn't counted.
#[tokio::test]
async fn sign_ups_per_address_are_capped() {
    let r = rig().await;
    r.set("cap.signups-per-hour", Some("2"));
    assert_eq!(r.sign_in(1).await.status, StatusCode::SEE_OTHER);
    for _ in 0..3 {
        assert_eq!(r.sign_in(8).await.status, StatusCode::SEE_OTHER, "signing in isn't signing up");
    }
    let start = || {
        r.http
            .post(format!("{}/auth/passkey/register", r.base))
            .header(header::ORIGIN, &r.base)
            .json(&json!({ "name": "Sam" }))
            .send()
    };
    assert_eq!(start().await.unwrap().status(), StatusCode::OK, "the second, by a passkey");
    let e = r.sign_in(2).await;
    assert_eq!(e.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(e.body.contains("Too many new accounts from here"), "{}", e.body);
    assert_eq!(r.account_of(2), None);
    let refused = start().await.unwrap();
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    let b: Value = refused.json().await.unwrap();
    assert_eq!(b["reason"], SIGNUPS_LIMITED, "{b}");

    // Paused, the passkey sign-up says so, in the same words as the page.
    r.set("signups", Some(""));
    let paused = start().await.unwrap();
    assert_eq!(paused.status(), StatusCode::FORBIDDEN);
    let b: Value = paused.json().await.unwrap();
    assert_eq!(b, json!({ "error": PAUSED_SAY, "reason": SIGNUPS_PAUSED }));
}

/// A suspended account doesn't sign in with GitHub: a page says so.
#[tokio::test]
async fn a_suspended_account_cannot_sign_in() {
    let r = rig().await;
    r.app.db.set_account_switch("acct-old", "suspended", Some("jake: proxying"), now_ms()).unwrap();
    refresh(&r.app).unwrap();
    let e = r.sign_in(8).await;
    assert_eq!(e.status, StatusCode::FORBIDDEN);
    assert!(e.session.is_none());
    assert!(e.body.contains("<h1>This account is suspended</h1>"), "{}", e.body);
    assert!(!e.body.contains("proxying"), "the operator's reason is for the record");
    r.app.db.set_account_switch("acct-old", "suspended", None, now_ms()).unwrap();
    refresh(&r.app).unwrap();
    assert_eq!(r.sign_in(8).await.status, StatusCode::SEE_OTHER);
}

// ---------------------------------------------------------------- suspended

/// A suspended account's session, its machines' signatures and its
/// principal at the platform are refused, its relay sockets hang up and new
/// ones are refused; lifting the suspension lets them all in again.
#[tokio::test]
async fn a_suspended_account_is_refused_everywhere() {
    let c = control(|_| {}).await;
    person(&c.app, "miner", "a1");
    let d = daemon(&c.app, "a1", "rig");
    let cookie = session(&c.app, "a1");
    let me = || c.as_person(&cookie, "GET", "/api/me", None);
    let trust = || async {
        let pq = "/api/daemon/trust";
        c.http.get(format!("{}{pq}", c.base)).header("x-arugula-auth", v2(&d, "GET", pq, b"")).send().await.unwrap()
    };
    assert_eq!(me().await.0, 200);
    assert_eq!(trust().await.status(), 200);
    let mut ws = crate::routing_wire::dial_relay(&c, &d).await;
    let person = Principal::Person { account: "a1".into() };
    assert!(c.app.hub_port.resolve(&person).await.unwrap().is_some());

    c.app.db.set_account_switch("a1", "suspended", Some("jake: mining"), now_ms()).unwrap();
    refresh(&c.app).unwrap();
    let (s, b) = me().await;
    assert_eq!((s, &b), (403, &json!({ "error": SUSPENDED_SAY, "reason": SUSPENDED })));
    let t = trust().await;
    assert_eq!(t.status(), 403);
    assert_eq!(t.json::<Value>().await.unwrap()["error"], SUSPENDED_SAY);
    assert!(c.app.hub_port.resolve(&person).await.unwrap().is_none(), "no one to the platform");
    use futures_util::StreamExt as _;
    let hung_up = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(hung_up.is_ok(), "its relay socket hung up");
    assert!(matches!(c.app.relay.admit(&c.app.db, "a1", true, false), Err(crate::relay::Refused::Account(_))));

    c.app.db.set_account_switch("a1", "suspended", None, now_ms()).unwrap();
    refresh(&c.app).unwrap();
    assert_eq!(me().await.0, 200);
    assert_eq!(trust().await.status(), 200);
    assert!(c.app.hub_port.resolve(&person).await.unwrap().is_some());
    assert!(c.app.relay.admit(&c.app.db, "a1", true, false).is_ok());
}

// ---------------------------------------------------------------- the relay's cap

/// The operator's relay cap replaces `--relay-daily-mb` for everyone, an
/// account's own for it, on sockets already open too; with billing on,
/// billing's allowance applies and the cap doesn't.
#[test]
fn the_relay_cap_follows_the_switch() {
    let switches = Arc::new(Switches::default());
    let r =
        crate::relay::Relay::new(crate::relay::Caps { sockets: 0, daemons: 0, daily_bytes: 100, total: 0, budget: 0 })
            .with_switches(switches.clone());
    let db = Db::memory();
    let t = r.admit(&db, "a1", false, false).unwrap();
    assert!(!t.count(100));
    assert!(t.count(1), "past --relay-daily-mb");
    let row = |name: &str, value: &str| Row { name: name.into(), value: value.into(), at: 1 };
    switches.set(State::from_rows(&[row("cap.relay-mb", "1")], &[]));
    assert!(!t.count(1_000), "everyone's cap, 1 MB, on an open socket");
    assert!(t.count(1_000_000));
    switches.set(State::from_rows(&[row("cap.relay-mb", "1")], &[("a1".into(), row("cap.relay-mb", "5"))]));
    assert!(!t.count(1), "its own, 5 MB");
    switches.set(State::from_rows(&[row("cap.relay-mb", "0")], &[]));
    assert!(!t.count(10_000_000), "0 is no cap");
    let billed = r.admit(&db, "a2", false, true).unwrap();
    switches.set(State::from_rows(&[row("cap.relay-mb", "1")], &[]));
    assert!(!billed.count(10_000_000), "billing's allowance applies instead");
}

// ---------------------------------------------------------------- the platform's view

/// What the platform is told of each account, and who goes on the
/// waitlist: an account turned away from a new box while it's waitlisted,
/// and not one that's admitted, nor anyone while new boxes are open.
#[tokio::test]
async fn standings_and_the_waitlist() {
    let app = App::for_tests("http://control.test").start();
    for a in ["a1", "a2"] {
        app.db.account_for("github", a, a, a, now_ms()).unwrap();
    }
    let all = ["a1".to_owned(), "a2".to_owned(), "gone".to_owned()];
    let s = app.hub_port.standings(&all, false).await.unwrap();
    assert_eq!(s.len(), 2, "no account, left out");
    assert_eq!(
        s[0],
        Standing {
            account: "a1".into(),
            suspended: false,
            hosted: "waitlist".into(),
            admitted: false,
            boxes: BOXES,
            box_hours: BOX_HOURS
        }
    );
    let waiting = || read(&app.db).accounts.iter().filter(|(_, a)| a.waiting.is_some()).count();
    assert_eq!(waiting(), 0, "asking isn't waiting");
    app.hub_port.standings(&all[..1], true).await.unwrap();
    assert_eq!(waiting(), 1);
    app.db.set_account_switch("a2", "admitted", Some("jake"), now_ms()).unwrap();
    app.db.set_account_switch("a2", "cap.boxes", Some("5"), now_ms()).unwrap();
    app.db.set_switch("hosted", Some("off"), now_ms()).unwrap();
    refresh(&app).unwrap();
    app.hub_port.standings(&all[1..2], true).await.unwrap();
    let s = app.hub_port.standings(&all[1..2], false).await.unwrap();
    assert_eq!((s[0].admitted, s[0].boxes, s[0].hosted.as_str()), (true, 5, "off"));
    app.db.set_switch("hosted", Some("open"), now_ms()).unwrap();
    refresh(&app).unwrap();
    app.db.account_for("github", "a3", "a3", "a3", now_ms()).unwrap();
    app.hub_port.standings(&["a3".to_owned()], true).await.unwrap();
    assert_eq!(waiting(), 1, "only a1 waits");
}
