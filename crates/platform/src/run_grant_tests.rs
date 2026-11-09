//! The standing project grant (M211) through the platform's routes,
//! against the test hub: offered once there's a hosted workspace, signed
//! and revoked by owners only, with a passkey (DK6), kept exactly as
//! signed, relayed with a start and a revoke, and what a machine's answers
//! become. Runs on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, Postgres.

use std::sync::Arc;

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use arugula_e2e::{
    DeviceKeys,
    hosted::{Delegation, HostedKey},
    now_ms,
    passkey::Assertion,
    project_grant::{GrantRevoke, ProjectGrant, Running},
    team::TeamRole,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::sql::{Sql as _, params},
};

/// alice owns team t1, bob edits it, vic views it; olga is elsewhere. alice
/// and bob have passkeys.
async fn serve() -> (Arc<TestHub>, Arc<Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("vic", TeamRole::Viewer)]);
    hub.team("t2", &[("olga", TeamRole::Owner)]);
    for who in ["alice", "bob"] {
        hub.passkey(&pk_id(who), who, pk_key(who).verifying_key().to_bytes());
    }
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let p = Platform::new(cfg, crate::db::Db::test(), hub.clone(), reqwest::Client::new(), Default::default());
    let app = Arc::new(p);
    let project = app.db.team_project("t1", "t1", 0).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let router = crate::testing::router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    (hub, app, base, project)
}

/// A hosted workspace of `project`, its box enrolled as `machine`.
fn workspace(app: &Platform, project: &str, machine: &str) -> String {
    let id = crate::hosted::new_id();
    app.db.add_hosted_workspace(&id, project, "dev", &Default::default(), "alice", 10, now_ms()).unwrap();
    app.db
        .c()
        .execute("UPDATE hosted_workspaces SET machine = ?1, state = 'ready' WHERE id = ?2", params![machine, id])
        .unwrap();
    id
}

const DAY: u64 = 24 * 60 * 60 * 1000;

fn delegation(project: &str, owner: &str, keys: &DeviceKeys, at: u64) -> Delegation {
    let mut d = Delegation {
        project: project.into(),
        owner: owner.into(),
        key: HostedKey::from_seed(&[1; 32]).public(),
        at,
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(keys);
    d
}

/// `who`'s passkey (Ed25519), and its id.
fn pk_key(who: &str) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&Sha256::digest(format!("passkey {who}").as_bytes()).into())
}

fn pk_id(who: &str) -> String {
    format!("pk-{who}")
}

/// `who`'s passkey's assertion over `challenge`, user verified, from
/// control's page, as `navigator.credentials.get` gives it.
fn assertion(who: &str, challenge: &str) -> Assertion {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    use ed25519_dalek::Signer as _;
    let client =
        json!({ "type": "webauthn.get", "challenge": challenge, "origin": "https://control.example" }).to_string();
    let mut data = Sha256::digest(b"control.example").to_vec();
    data.push(0x05);
    data.extend_from_slice(&0u32.to_be_bytes());
    let mut msg = data.clone();
    msg.extend_from_slice(&Sha256::digest(client.as_bytes()));
    Assertion {
        id: pk_id(who),
        client_data_json: B64.encode(client.as_bytes()),
        authenticator_data: B64.encode(&data),
        signature: B64.encode(pk_key(who).sign(&msg).to_bytes()),
    }
}

/// `who`'s v2 grant for 90 days, under the delegation made at 1, confirmed
/// with `who`'s passkey.
fn grant(project: &str, owner: &str, who: &str) -> ProjectGrant {
    grant_under(&delegation(project, owner, &owner_keys(), 1), who, now_ms(), now_ms() + 90 * DAY)
}

/// The device of alice's that signed the project's delegation: the same
/// one each time, so the delegation is.
fn owner_keys() -> DeviceKeys {
    DeviceKeys { noise_private: [0; 32], noise_public: [3; 32], sign: ed25519_dalek::SigningKey::from_bytes(&[2; 32]) }
}

fn grant_under(d: &Delegation, who: &str, made: u64, expires: u64) -> ProjectGrant {
    confirmed(ProjectGrant::v2(d, who, made, expires), who)
}

/// `g` (its bytes as they are now) confirmed with `who`'s passkey.
fn confirmed(mut g: ProjectGrant, who: &str) -> ProjectGrant {
    g.passkey = Some(assertion(who, &g.passkey_challenge()));
    g
}

/// `who`'s v3 revoke of `g`'s project's grants under its delegation, now,
/// confirmed with `who`'s passkey.
fn revoke(g: &ProjectGrant, running: Running, who: &str) -> GrantRevoke {
    let mut r = GrantRevoke::v3(&g.project, &g.delegation, running, now_ms(), who);
    r.passkey = Some(assertion(who, &r.passkey_challenge()));
    r
}

async fn sign(base: &str, who: &str, project: &str, g: &ProjectGrant) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/runs/grant"), Some(json!({ "grant": g }))).await
}

async fn unsign(base: &str, who: &str, project: &str, r: &GrantRevoke) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/runs/grant/revoke"), Some(json!({ "revoke": r }))).await
}

async fn start(base: &str, who: &str, project: &str, w: &str) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/runs/start"), Some(json!({ "workspace": w }))).await
}

async fn read(base: &str, who: &str, project: &str) -> Value {
    let (status, v) = call(base, who, "GET", &format!("/api/projects/{project}/runs/grant"), None).await;
    assert_eq!(status, 200, "{v}");
    v
}

/// DK6: a grant and a revoke are an owner's passkey assertion over their
/// bytes, checked against the poster's published passkey log. A device's
/// signature (a v1 grant, a device's revoke, from a page loaded before) is
/// refused as old; so is a grant with no passkey, one over other bytes,
/// another account's, from an account with no passkey, or with a passkey
/// removed since.
#[tokio::test]
async fn only_an_owners_passkey_grants_and_revokes() {
    let (hub, app, base, project) = serve().await;
    workspace(&app, &project, "m1");
    let g = grant(&project, "team:t1", "alice");
    hub.set_delegation(&project, Some(&g.delegation));
    let reason = |v: &Value| v["reason"].as_str().map(str::to_owned);
    // M211's v1, signed by a browser's device key: no longer taken.
    let keys = DeviceKeys::generate();
    let mut v1 = ProjectGrant::new(&delegation(&project, "team:t1", &keys, 1), now_ms(), Some(now_ms() + DAY));
    v1.sign_with(&keys);
    let (status, v) = sign(&base, "alice", &project, &v1).await;
    assert_eq!((status, reason(&v).as_deref()), (400, Some(crate::run_grant::OLD_GRANT)), "{v}");
    // No passkey; one over other bytes (its expiry changed after);
    // another account's passkey.
    let refused = [
        ProjectGrant { passkey: None, ..g.clone() },
        ProjectGrant { expires: Some(now_ms() + 30 * DAY), ..g.clone() },
        ProjectGrant { passkey: Some(assertion("bob", &g.passkey_challenge())), ..g.clone() },
    ];
    for bad in &refused {
        let (status, v) = sign(&base, "alice", &project, bad).await;
        assert_eq!((status, reason(&v).as_deref()), (403, Some(crate::approvals::PASSKEY)), "{v}");
    }
    // One that names another account than the poster's.
    assert_eq!(sign(&base, "alice", &project, &grant(&project, "team:t1", "bob")).await.0, 400);
    // An owner with no passkey: told to add one.
    hub.team("t1", &[("alice", TeamRole::Owner), ("carol", TeamRole::Owner)]);
    let (status, v) = sign(&base, "carol", &project, &grant(&project, "team:t1", "carol")).await;
    assert_eq!(status, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("no passkey"), "{v}");
    // Alice's own: kept, its assertion with it.
    let (status, kept) = sign(&base, "alice", &project, &g).await;
    assert_eq!(status, 201, "{kept}");
    assert_eq!((kept["body"].as_str(), &kept["grant"]["passkey"]), (Some(g.body().as_str()), &json!(g.passkey)));
    let live = app.db.live_run_grant(&project, now_ms()).unwrap().unwrap();
    assert_eq!(live.grant, g, "read back whole");
    // A device's revoke (M211's): refused as old; a passkey's: kept.
    let old = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, now_ms(), &keys);
    let (status, v) = unsign(&base, "alice", &project, &old).await;
    assert_eq!((status, reason(&v).as_deref()), (400, Some(crate::run_grant::OLD_GRANT)), "{v}");
    let unsigned = GrantRevoke { passkey: None, ..revoke(&g, Running::Finish, "alice") };
    assert_eq!(unsign(&base, "alice", &project, &unsigned).await.0, 403);
    hub.answers_ops("m1", OpAnswer::Done { url: None, routed: None });
    assert_eq!(unsign(&base, "alice", &project, &revoke(&g, Running::Finish, "alice")).await.0, 200);
    // A passkey removed since: refused.
    hub.passkey_removed(&pk_id("alice"), now_ms() - 1);
    let (status, v) = sign(&base, "alice", &project, &grant(&project, "team:t1", "alice")).await;
    assert_eq!(status, 403, "{v}");
}

/// Offered with the first hosted workspace; an owner's signed grant is
/// kept as signed; renewing is signing another, which takes its place; a
/// start sends it to the workspace's machine with the revokes and the
/// granter's passkey log; a revoke goes to every machine and stops the
/// next start here.
#[tokio::test]
async fn a_grant_signed_used_and_revoked() {
    let (hub, app, base, project) = serve().await;
    let g = grant(&project, "team:t1", "alice");
    hub.set_delegation(&project, Some(&g.delegation));
    let v = read(&base, "vic", &project).await;
    assert_eq!((v["offer"].as_bool(), v["grant"].is_null(), v["owner"].as_str()), (Some(false), true, Some("team:t1")));
    let w = workspace(&app, &project, "m1");
    assert_eq!(read(&base, "alice", &project).await["offer"], true, "offered with a hosted workspace");

    assert_eq!(sign(&base, "bob", &project, &grant(&project, "team:t1", "bob")).await.0, 403, "an editor");
    assert_eq!(sign(&base, "olga", &project, &g).await.0, 404, "not a member");
    let (status, kept) = sign(&base, "alice", &project, &g).await;
    assert_eq!(status, 201, "{kept}");
    assert_eq!(kept["body"], g.body());
    assert_eq!(sign(&base, "alice", &project, &g).await.0, 409, "here already");
    let v = read(&base, "vic", &project).await;
    assert_eq!((v["offer"].as_bool(), v["grant"]["grant"]["id"].as_str()), (Some(false), Some(g.id.as_str())));
    assert_eq!((v["stale"].as_bool(), v["renew"].as_bool()), (Some(false), Some(false)));
    // Renewed: one passkey, and the new one takes the old one's place.
    let d = delegation(&project, "team:t1", &owner_keys(), 1);
    let renewed = grant_under(&d, "alice", now_ms(), now_ms() + 90 * DAY);
    assert_eq!(sign(&base, "alice", &project, &renewed).await.0, 201);
    let v = read(&base, "vic", &project).await;
    assert_eq!(v["grant"]["grant"]["id"].as_str(), Some(renewed.id.as_str()));
    assert_eq!(v["past"][0]["grant"]["id"].as_str(), Some(g.id.as_str()));
    assert!(v["past"][0]["superseded_at"].is_u64(), "{v}");

    // A start: the grant, the project's revokes and alice's passkey log go
    // to the machine.
    hub.answers_ops("m1", OpAnswer::Done { url: None, routed: None });
    let (status, v) = start(&base, "bob", &project, "dev").await;
    assert_eq!(status, 200, "{v}");
    assert_eq!((v["started"].as_bool(), v["machine"].as_str()), (Some(true), Some("m1")));
    assert_eq!(start(&base, "vic", &project, "dev").await.0, 403, "a viewer");
    let asked = hub.ops_asked();
    let Some((m, DaemonOp::RunStart(s))) = asked.last() else { panic!("{asked:?}") };
    assert_eq!((m.as_str(), s.workspace.as_str(), s.cap.as_str()), ("m1", w.as_str(), "runs.start"));
    assert_eq!((&s.grant, s.revokes.len(), s.run.as_ref()), (&renewed, 0, None));
    let log = s.passkey_log.as_ref().expect("the granter's passkey log");
    assert_eq!((log.account.as_str(), log.events[0].credential.as_str()), ("alice", "pk-alice"));

    // The machine refuses it: its reason, word for word.
    hub.answers_ops(
        "m1",
        OpAnswer::Cant { why: "grant_passkey: the standing grant's passkey doesn't check out".into() },
    );
    let (status, v) = start(&base, "alice", &project, "dev").await;
    assert_eq!((status, v["reason"].as_str()), (409, Some("grant_passkey")), "{v}");
    hub.answers_ops("m1", OpAnswer::Offline);
    assert_eq!(start(&base, "alice", &project, "dev").await.0, 503);

    // Revoked: by an owner only; every machine hears it; no more starts.
    let r = revoke(&renewed, Running::Stop, "alice");
    assert_eq!(unsign(&base, "bob", &project, &revoke(&renewed, Running::Stop, "bob")).await.0, 403);
    hub.answers_ops("m1", OpAnswer::Done { url: None, routed: None });
    let (status, v) = unsign(&base, "alice", &project, &r).await;
    assert_eq!((status, v["told"].as_u64(), v["revoked"].as_u64()), (200, Some(1), Some(2)), "{v}");
    let Some((m, DaemonOp::RunRevoke(rr))) = hub.ops_asked().last().cloned() else { panic!() };
    assert_eq!((m.as_str(), &rr.revoke), ("m1", &r));
    assert_eq!(unsign(&base, "alice", &project, &r).await.0, 409, "once");
    let (status, v) = start(&base, "alice", &project, "dev").await;
    assert_eq!((status, v["reason"].as_str()), (409, Some("no_grant")), "{v}");
    let v = read(&base, "alice", &project).await;
    assert_eq!((v["offer"].as_bool(), v["past"][0]["revoke"]["running"].as_str()), (Some(true), Some("stop")));

    // A grant made before the revoke can't come in after it.
    let before = grant_under(&d, "alice", r.at - 1, now_ms() + 90 * DAY);
    assert_eq!(sign(&base, "alice", &project, &before).await.0, 409, "revoked before it got here");
    // Signed again (from the phone, say): the next start carries the
    // revoke, for a machine that was asleep through it.
    let g2 = grant_under(&d, "alice", r.at + 1, now_ms() + 90 * DAY);
    assert_eq!(sign(&base, "alice", &project, &g2).await.0, 201);
    assert_eq!(start(&base, "alice", &project, &w).await.0, 200);
    let Some((_, DaemonOp::RunStart(s))) = hub.ops_asked().last().cloned() else { panic!() };
    assert_eq!((s.grant, s.revokes), (g2, vec![r]));
}

/// A v1 grant kept before DK6 (a device's signature) still goes to the
/// machines until it expires, with no passkey log; a passkey revoke takes
/// it back.
#[tokio::test]
async fn a_v1_grant_kept_before_still_starts_until_revoked() {
    let (hub, app, base, project) = serve().await;
    workspace(&app, &project, "m1");
    let keys = DeviceKeys::generate();
    let d = delegation(&project, "team:t1", &keys, 1);
    hub.set_delegation(&project, Some(&d.id()));
    let mut v1 = ProjectGrant::new(&d, now_ms(), Some(now_ms() + 90 * DAY));
    v1.sign_with(&keys);
    app.db.add_run_grant(&v1, "alice", now_ms()).unwrap();
    hub.answers_ops("m1", OpAnswer::Done { url: None, routed: None });
    assert_eq!(start(&base, "alice", &project, "dev").await.0, 200);
    let Some((_, DaemonOp::RunStart(s))) = hub.ops_asked().last().cloned() else { panic!() };
    assert_eq!((&s.grant, s.passkey_log.is_none()), (&v1, true));
    assert_eq!(unsign(&base, "alice", &project, &revoke(&v1, Running::Finish, "alice")).await.0, 200);
    assert_eq!(start(&base, "alice", &project, "dev").await.1["reason"], "no_grant");
}

/// A grant names the project's live delegation, and expires: an expired
/// one isn't live and a new one takes its place; one under a delegation
/// that isn't live any more is offered again and replaced.
#[tokio::test]
async fn expiry_and_the_live_delegation() {
    let (hub, app, base, project) = serve().await;
    workspace(&app, &project, "m1");
    let keys = DeviceKeys::generate();
    let d1 = delegation(&project, "team:t1", &keys, 1);
    let g = grant_under(&d1, "alice", now_ms(), now_ms() + 90 * DAY);
    // No live delegation, or another one: refused.
    let (status, v) = sign(&base, "alice", &project, &g).await;
    assert_eq!((status, v["reason"].as_str()), (409, Some("delegation_not_live")), "{v}");
    hub.set_delegation(&project, Some("someone-elses"));
    assert_eq!(sign(&base, "alice", &project, &g).await.0, 409);
    hub.set_delegation(&project, Some(&d1.id()));
    // Near its expiry: renewing is offered.
    let soon = grant_under(&d1, "alice", now_ms(), now_ms() + 3 * DAY);
    assert_eq!(sign(&base, "alice", &project, &soon).await.0, 201);
    assert_eq!(read(&base, "alice", &project).await["renew"], true);
    // Expired: not live, nothing starts, and the next one is kept.
    app.db.c().execute("UPDATE run_grants SET expires_at = 1 WHERE id = ?1", params![soon.id]).unwrap();
    let v = read(&base, "alice", &project).await;
    assert_eq!((v["grant"].is_null(), v["offer"].as_bool()), (true, Some(true)));
    assert_eq!(start(&base, "alice", &project, "dev").await.1["reason"], "no_grant");
    assert_eq!(sign(&base, "alice", &project, &g).await.0, 201);
    // The project's delegation changes: the grant is stale, offered again,
    // and one under the new delegation replaces it.
    let d2 = delegation(&project, "team:t1", &keys, 2);
    hub.set_delegation(&project, Some(&d2.id()));
    let v = read(&base, "alice", &project).await;
    assert_eq!((v["stale"].as_bool(), v["offer"].as_bool()), (Some(true), Some(true)));
    let g2 = grant_under(&d2, "alice", now_ms(), now_ms() + 90 * DAY);
    assert_eq!(sign(&base, "alice", &project, &g2).await.0, 201);
    let v = read(&base, "alice", &project).await;
    assert_eq!((v["grant"]["grant"]["id"].as_str(), v["stale"].as_bool()), (Some(g2.id.as_str()), Some(false)));
}

/// What the platform refuses to keep: a grant for another project or
/// owner, narrowed, from the wrong time, already expired, with no expiry or
/// one too far off; and a revoke that isn't this project's, or covers
/// nothing.
#[tokio::test]
async fn what_isnt_kept() {
    let (hub, _app, base, project) = serve().await;
    let good = grant(&project, "team:t1", "alice");
    hub.set_delegation(&project, Some(&good.delegation));
    let mut bad = vec![
        grant("prj_other", "team:t1", "alice"),
        grant(&project, "team:t2", "alice"),
        grant(&project, "account:alice", "alice"),
    ];
    for g in [
        ProjectGrant { caps: vec!["runs.start".into()], ..good.clone() },
        ProjectGrant { made: now_ms() - 3_600_000, ..good.clone() },
        ProjectGrant { made: now_ms() - 1000, expires: Some(now_ms() - 500), ..good.clone() },
        ProjectGrant { expires: None, ..good.clone() },
        ProjectGrant { expires: Some(now_ms() + 400 * DAY), ..good.clone() },
    ] {
        bad.push(confirmed(g, "alice"));
    }
    for g in bad {
        let (status, v) = sign(&base, "alice", &project, &g).await;
        assert_eq!(status, 400, "{v} for {g:?}");
    }
    let r = revoke(&good, Running::Finish, "alice");
    assert_eq!(unsign(&base, "alice", &project, &r).await.0, 409, "covers nothing yet");
    assert_eq!(sign(&base, "alice", &project, &good).await.0, 201);
    let mut elsewhere = GrantRevoke::v3(&good.project, "another", Running::Finish, now_ms(), "alice");
    elsewhere.passkey = Some(assertion("alice", &elsewhere.passkey_challenge()));
    assert_eq!(unsign(&base, "alice", &project, &elsewhere).await.0, 409, "another delegation");
    let r = GrantRevoke { project: "prj_other".into(), ..revoke(&good, Running::Finish, "alice") };
    assert_eq!(unsign(&base, "alice", &project, &r).await.0, 400);
    // No workspace by that name.
    assert_eq!(start(&base, "alice", &project, "nope").await.0, 404);
}
