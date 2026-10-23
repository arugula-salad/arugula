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
/// doesn't make browsers or recovery codes with it any more, nor with a
/// browser it made: every link back to the root is a browser's own. It
/// still makes apps.
#[tokio::test]
async fn an_old_app_root_and_what_it_made_dont_approve_browsers() {
    let c = control(|_| {}).await;
    c.app.db.account_for("github", "gh-a1", "jake", "a1", now_ms()).unwrap();
    let rk = DeviceKeys::generate();
    let mut root = Cert::new(&rk, "a1", Kind::Browser, "Arugula app on jake-air");
    root.sign_with(&rk);
    c.app.db.put_device(&root, true, now_ms()).unwrap();
    // A browser it approved before: stands, but approves no browser now.
    let made = device(&c, &rk, "a1", Kind::Browser, "Firefox on Linux");
    let cookie = session(&c.app, "a1");
    for by in [&rk, &made] {
        let (_, b) = signed(by, "a1", Kind::Browser, "Chrome on Mac");
        let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": b }))).await;
        assert_eq!((st, &v["approved"]), (200, &json!(false)), "it waits: {v}");
        let (st, v) = c
            .as_person(&cookie, "POST", &format!("/api/devices/{}/approve", b.device), Some(json!({ "cert": b })))
            .await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{v}");
        let (_, code) = signed(by, "a1", Kind::Recovery, "recovery 1");
        let (st, _) = c.as_person(&cookie, "POST", "/api/recovery", Some(json!({ "certs": [code] }))).await;
        assert_eq!(st, 403);
    }
    let (_, app) = signed(&rk, "a1", Kind::App, "Arugula app on mini");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": app }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(true)), "{v}");
}

/// A browser or a phone is removed only from a browser or a phone all the
/// way back; the app removes machines.
#[tokio::test]
async fn only_a_browser_removes_a_browser() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let app = device(&c, &root, "a1", Kind::App, "Arugula app on box");
    let old_app = device(&c, &root, "a1", Kind::Browser, "Arugula app on geek");
    let phone = device(&c, &root, "a1", Kind::Browser, "Safari on iPhone");
    let machine = crate::routing_wire::daemon(&c.app, "a1", "box");
    let cookie = session(&c.app, "a1");
    let revoke = |device: &DeviceKeys, by: &DeviceKeys| json!({ "revocation": arugula_e2e::Revocation::new("a1", &device.id(), by) });
    for by in [&app, &old_app] {
        let (st, v) = c.as_person(&cookie, "POST", "/api/revocations", Some(revoke(&phone, by))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{v}");
    }
    let (st, v) = c.as_person(&cookie, "POST", "/api/revocations", Some(revoke(&machine, &app))).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = c.as_person(&cookie, "POST", "/api/revocations", Some(revoke(&phone, &root))).await;
    assert_eq!(st, 200, "{v}");
}

/// A team's owner or editor is made only from a browser or a phone all the
/// way back: a roster or an invite that raises anyone, from the desktop
/// app (or one of its devices from before it had a kind), is refused. Its
/// viewer invites go through.
#[tokio::test]
async fn only_a_browser_makes_a_teams_owner_or_editor() {
    use arugula_e2e::team::{AskInvite, TeamRole};

    use crate::routing_wire::{member, roster};
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let bob = person(&c.app, "bob", "b1");
    let app = device(&c, &root, "a1", Kind::App, "Arugula app on box");
    let old_app = device(&c, &root, "a1", Kind::Browser, "Arugula app on geek");
    let team = "0123456789abcdef";
    let v1 = roster(team, 1, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Viewer)], &root);
    let t = crate::db::Team {
        id: team.into(),
        name: "Acme".into(),
        founder: "a1".into(),
        founder_root: root.id(),
        locked: false,
    };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&v1).unwrap(), now_ms()).unwrap();
    let cookie = session(&c.app, "a1");
    let path = format!("/api/teams/{team}/roster");
    let promote = |by: &DeviceKeys| json!({ "roster": roster(team, 2, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Owner)], by) });
    let (st, _) = c.as_person(&cookie, "POST", &path, Some(promote(&app))).await;
    assert_eq!(st, 403, "the app");
    let (st, v) = c.as_person(&cookie, "POST", &path, Some(promote(&old_app))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "an old app device: {v}");
    let (st, v) = c.as_person(&cookie, "POST", &path, Some(promote(&root))).await;
    assert_eq!(st, 200, "the browser: {v}");

    let ask = |role: TeamRole, by: &DeviceKeys| {
        let k = DeviceKeys::generate();
        let mut a = AskInvite {
            team: team.into(),
            role,
            made: now_ms(),
            expires: now_ms() + 60_000,
            key: hex::encode(k.sign_public()),
            by: String::new(),
            sig: String::new(),
        };
        a.sign_with(by);
        json!({ "role": role, "ask": a })
    };
    let invites = format!("/api/teams/{team}/invites");
    for by in [&app, &old_app] {
        let (st, v) = c.as_person(&cookie, "POST", &invites, Some(ask(TeamRole::Owner, by))).await;
        assert_eq!(st, 400, "an owner's invite from the app: {v}");
        let (st, v) = c.as_person(&cookie, "POST", &invites, Some(ask(TeamRole::Viewer, by))).await;
        assert_eq!(st, 200, "a viewer's: {v}");
    }
    let (st, v) = c.as_person(&cookie, "POST", &invites, Some(ask(TeamRole::Owner, &root))).await;
    assert_eq!(st, 200, "an owner's from the browser: {v}");

    // The app makes editors (Jake, 2026-10-22); a CLI doesn't.
    let cli = device(&c, &root, "a1", Kind::Cli, "Arugula CLI");
    for (by, want) in [(&app, 200), (&old_app, 200), (&cli, 400)] {
        let (st, v) = c.as_person(&cookie, "POST", &invites, Some(ask(TeamRole::Editor, by))).await;
        assert_eq!(st, want, "an editor's invite: {v}");
    }
    let to = |version, role, by: &DeviceKeys| json!({ "roster": roster(team, version, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, role)], by) });
    assert_eq!(c.as_person(&cookie, "POST", &path, Some(to(3, TeamRole::Viewer, &root))).await.0, 200);
    let (st, v) = c.as_person(&cookie, "POST", &path, Some(to(4, TeamRole::Editor, &cli))).await;
    assert_eq!(st, 403, "a CLI making an editor: {v}");
    let (st, v) = c.as_person(&cookie, "POST", &path, Some(to(4, TeamRole::Editor, &app))).await;
    assert_eq!(st, 200, "the app making an editor: {v}");
    let (st, _) = c.as_person(&cookie, "POST", &path, Some(to(5, TeamRole::Owner, &app))).await;
    assert_eq!(st, 403, "but not an owner");
}

/// The desktop app founds a team (just its owner in it).
#[tokio::test]
async fn the_app_founds_a_team() {
    use arugula_e2e::team::TeamRole;

    use crate::routing_wire::{member, roster};
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let app = device(&c, &root, "a1", Kind::App, "Arugula app on box");
    let cookie = session_via(&c, "a1", Via::App);
    let r = roster("00112233aabbccdd", 1, vec![member("a1", &root, TeamRole::Owner)], &app);
    let (st, v) = c.as_person(&cookie, "POST", "/api/teams", Some(json!({ "roster": r }))).await;
    assert!(st < 300, "{st} {v}");
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

/// A recovery code goes only from a browser or a phone all the way back,
/// and now: not from the app, not backdated (which would take out the
/// browser it let in), and new codes are new keys.
#[tokio::test]
async fn a_recovery_code_goes_only_from_a_browser_and_now() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let app = device(&c, &root, "a1", Kind::App, "Arugula app on box");
    let code = device(&c, &root, "a1", Kind::Recovery, "recovery 1");
    let cookie = session(&c.app, "a1");
    let revoke = |by: &DeviceKeys, at: Option<u64>| {
        let mut r = arugula_e2e::Revocation::new("a1", &code.id(), by);
        if let Some(at) = at {
            r.at = at;
            r.sig = hex::encode(by.signature(r.body().as_bytes()));
        }
        r
    };
    let (st, v) =
        c.as_person(&cookie, "POST", "/api/revocations", Some(json!({ "revocation": revoke(&app, None) }))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "the app: {v}");
    let (st, v) =
        c.as_person(&cookie, "POST", "/api/revocations", Some(json!({ "revocation": revoke(&root, Some(1)) }))).await;
    assert_eq!(st, 400, "backdated: {v}");

    // New codes: the old one revoked by the app, or backdated, refused;
    // the old one posted again as new, refused; by the browser, now: kept.
    let (_, new) = signed(&root, "a1", Kind::Recovery, "recovery 2");
    let (_, old_again) = {
        let mut cert = c.app.db.device("a1", &code.id()).unwrap().unwrap().0;
        cert.sign_with(&root);
        (code.id(), cert)
    };
    for (certs, r, want) in [
        (vec![new.clone()], revoke(&app, None), 403),
        (vec![new.clone()], revoke(&root, Some(1)), 403),
        (vec![old_again], revoke(&root, None), 409),
        (vec![new.clone()], revoke(&root, None), 200),
    ] {
        let (st, v) =
            c.as_person(&cookie, "POST", "/api/recovery", Some(json!({ "certs": certs, "revocations": [r] }))).await;
        assert_eq!(st, want, "{v}");
    }
}

/// The desktop app's window can't delete the account, remove a passkey or
/// sign out everywhere; a browser's page can.
#[tokio::test]
async fn the_apps_window_doesnt_delete_remove_passkeys_or_sign_out_everywhere() {
    let c = control(|_| {}).await;
    person(&c.app, "jake", "a1");
    c.app.db.add_passkey("pk1", "a1", -8, &[7; 32], 1).unwrap();
    c.app.db.add_passkey("pk2", "a1", -8, &[8; 32], 1).unwrap();
    let app = session_via(&c, "a1", Via::App);
    for (path, body) in [
        ("/api/me/delete", json!({ "confirm": "jake" })),
        ("/api/me/passkeys/pk1/remove", json!({})),
        ("/api/me/sessions/end-all", json!({})),
    ] {
        let (st, v) = c.as_person(&app, "POST", path, Some(body)).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("app_window")), "{path}: {v}");
    }
    assert_eq!(c.app.db.passkey_count("a1").unwrap(), 2);
    let browser = session(&c.app, "a1");
    let (st, v) = c.as_person(&browser, "POST", "/api/me/passkeys/pk1/remove", Some(json!({}))).await;
    assert_eq!(st, 200, "{v}");
    let (st, v) = c.as_person(&browser, "POST", "/api/me/sessions/end-all", Some(json!({}))).await;
    assert!(st < 300, "{st} {v}");
}

/// What the desktop app's window (from before it had a kind) approved,
/// both stored and approved by control before #1343 shipped (`legacy_app`),
/// counts again: that browser approves a browser and recovery codes,
/// removes a browser and makes a team's owner. The window itself doesn't;
/// nor a browser it certified after the cutoff, or one a window stored
/// after let in, or one a window asked before and approved after let in;
/// nor a recovery code it made before the cutoff, approving a browser now.
#[tokio::test]
async fn a_grandfathered_app_window_counts_on_the_way() {
    use arugula_e2e::team::TeamRole;

    use crate::{
        legacy_app::APP_KIND_SHIPPED_MS as CUTOFF,
        routing_wire::{member, roster},
    };
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let bob = person(&c.app, "bob", "b1");
    let put = |by: &DeviceKeys, name: &str, at: u64| {
        let (k, cert) = signed(by, "a1", Kind::Browser, name);
        c.app.db.put_device(&cert, true, at).unwrap();
        k
    };
    let window = put(&root, "Arugula app on jake-air", CUTOFF - 60_000);
    let firefox = put(&window, "Firefox on Mac", CUTOFF - 30_000);
    let edge = put(&window, "Edge", now_ms());
    let late = put(&root, "Arugula app on jake-mini", CUTOFF);
    let chrome = put(&late, "Chrome on Mac", CUTOFF - 1);
    // Asked before the cutoff, approved after: what it let in, though
    // stored before, doesn't count.
    let (waited, wc) = signed(&root, "a1", Kind::Browser, "Arugula app on geek");
    c.app.db.put_device(&wc, false, CUTOFF - 60_000).unwrap();
    c.app.db.put_device(&wc, true, now_ms()).unwrap();
    let opera = put(&waited, "Opera", CUTOFF - 30_000);
    let cookie = session(&c.app, "a1");
    let refused = [
        (&window, "the window itself"),
        (&edge, "what it certified after"),
        (&late, "a window stored after"),
        (&chrome, "what that let in"),
        (&opera, "what a window approved after let in"),
    ];

    // A browser asks; Firefox approves it.
    let k = DeviceKeys::generate();
    let ask = Cert::new(&k, "a1", Kind::Browser, "Safari on iPhone");
    let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": ask }))).await;
    assert_eq!((st, &v["approved"]), (200, &json!(false)), "{v}");
    let approve = format!("/api/devices/{}/approve", ask.device);
    for (by, who) in refused {
        let mut cert = ask.clone();
        cert.sign_with(by);
        let (st, v) = c.as_person(&cookie, "POST", &approve, Some(json!({ "cert": cert }))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{who}: {v}");
    }
    // A recovery code the window made before the cutoff.
    let (code, cc) = signed(&window, "a1", Kind::Recovery, "recovery 0");
    c.app.db.put_device(&cc, true, CUTOFF - 30_000).unwrap();
    let mut cert = ask.clone();
    cert.sign_with(&code);
    let (st, v) = c.as_person(&cookie, "POST", &approve, Some(json!({ "cert": cert }))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "a code the window made: {v}");
    c.app.db.add_revocation(&arugula_e2e::Revocation::new("a1", &code.id(), &root)).unwrap();
    let mut cert = ask.clone();
    cert.sign_with(&firefox);
    let (st, v) = c.as_person(&cookie, "POST", &approve, Some(json!({ "cert": cert }))).await;
    assert_eq!(st, 200, "{v}");

    // Recovery codes.
    for (by, who) in refused {
        let (_, code) = signed(by, "a1", Kind::Recovery, "recovery 1");
        let (st, v) = c.as_person(&cookie, "POST", "/api/recovery", Some(json!({ "certs": [code] }))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{who}: {v}");
    }
    let (_, code) = signed(&firefox, "a1", Kind::Recovery, "recovery 1");
    let (st, v) = c.as_person(&cookie, "POST", "/api/recovery", Some(json!({ "certs": [code] }))).await;
    assert_eq!(st, 200, "{v}");

    // Removing a browser.
    let phone = device(&c, &root, "a1", Kind::Browser, "Pixel");
    let revoke = |by: &DeviceKeys| json!({ "revocation": arugula_e2e::Revocation::new("a1", &phone.id(), by) });
    for (by, who) in refused {
        let (st, v) = c.as_person(&cookie, "POST", "/api/revocations", Some(revoke(by))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{who}: {v}");
    }
    let (st, v) = c.as_person(&cookie, "POST", "/api/revocations", Some(revoke(&firefox))).await;
    assert_eq!(st, 200, "{v}");

    // Making a team's owner.
    let team = "0123456789abcdef";
    let v1 = roster(team, 1, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Viewer)], &root);
    let t = crate::db::Team {
        id: team.into(),
        name: "Acme".into(),
        founder: "a1".into(),
        founder_root: root.id(),
        locked: false,
    };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&v1).unwrap(), now_ms()).unwrap();
    let path = format!("/api/teams/{team}/roster");
    let promote = |by: &DeviceKeys| json!({ "roster": roster(team, 2, vec![member("a1", &root, TeamRole::Owner), member("b1", &bob, TeamRole::Owner)], by) });
    for (by, who) in refused {
        let (st, v) = c.as_person(&cookie, "POST", &path, Some(promote(by))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("needs_browser")), "{who}: {v}");
    }
    let (st, v) = c.as_person(&cookie, "POST", &path, Some(promote(&firefox))).await;
    assert_eq!(st, 200, "{v}");
}

/// root → window W → browser X → browser Y, W and X stored and approved
/// before the cutoff: X removed since doesn't take Y with it (a removed
/// device's past approvals stand), so Y approves a browser. Y removed
/// approves nothing, and neither does W removed.
#[tokio::test]
async fn a_browser_removed_since_keeps_what_it_approved() {
    use crate::legacy_app::APP_KIND_SHIPPED_MS as CUTOFF;
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let put = |by: &DeviceKeys, name: &str, at: u64| {
        let k = DeviceKeys::generate();
        let mut cert = Cert::new(&k, "a1", Kind::Browser, name);
        cert.created = now_ms() - 60_000;
        cert.sign_with(by);
        c.app.db.put_device(&cert, true, at).unwrap();
        k
    };
    let w = put(&root, "illogical app on geek", CUTOFF - 60_000);
    let x = put(&w, "Firefox on Linux", CUTOFF - 30_000);
    let y = put(&x, "Firefox on Mac", now_ms());
    c.app.db.add_revocation(&arugula_e2e::Revocation::new("a1", &x.id(), &root)).unwrap();
    assert_eq!(crate::legacy_app::children(&c.app.db, "a1").unwrap(), vec![x.id()], "X stays a child");
    let cookie = session(&c.app, "a1");
    let approve_by = |by: &DeviceKeys, name: &str| {
        let k = DeviceKeys::generate();
        let ask = Cert::new(&k, "a1", Kind::Browser, name);
        let mut cert = ask.clone();
        cert.sign_with(by);
        (ask, cert)
    };
    let try_approve = |ask: Cert, cert: Cert| {
        let c = &c;
        let cookie = cookie.clone();
        async move {
            let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(json!({ "cert": ask }))).await;
            assert_eq!((st, &v["approved"]), (200, &json!(false)), "{v}");
            let path = format!("/api/devices/{}/approve", ask.device);
            c.as_person(&cookie, "POST", &path, Some(json!({ "cert": cert }))).await
        }
    };
    let (ask, cert) = approve_by(&y, "Safari on iPhone");
    let (st, v) = try_approve(ask, cert).await;
    assert_eq!(st, 200, "Y, though X was removed: {v}");

    // W removed: it approves nothing new.
    c.app.db.add_revocation(&arugula_e2e::Revocation::new("a1", &w.id(), &root)).unwrap();
    let (ask, cert) = approve_by(&w, "Edge on Windows");
    let (st, v) = try_approve(ask, cert).await;
    assert_eq!(st, 403, "W removed: {v}");
    // Y, which W's child approved before either was removed, still does.
    let (ask, cert) = approve_by(&y, "Chrome on Linux");
    let (st, v) = try_approve(ask, cert).await;
    assert_eq!(st, 200, "Y, though W and X were removed: {v}");

    // Y removed: refused.
    c.app.db.add_revocation(&arugula_e2e::Revocation::new("a1", &y.id(), &root)).unwrap();
    let (ask, cert) = approve_by(&y, "Brave");
    let (st, v) = try_approve(ask, cert).await;
    assert_eq!(st, 403, "Y removed: {v}");
}
