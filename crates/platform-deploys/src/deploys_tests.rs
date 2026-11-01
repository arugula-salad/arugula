//! Deploys and their approval through the platform's routes (M218, DK4):
//! only a project owner's passkey approves one (v2: an assertion over its
//! bytes, checked against the account's published passkey log), whatever
//! devices they have; an agent never does (on its owner's own machine too,
//! aru-091); and a stale deploy is refused. The hub is chat's [`TestHub`],
//! which keeps each account's devices and passkeys as the test says.

// Also the feature crates' test harness (#314, the `testing` feature): its
// helpers are `pub`, and only its own tests use the rest.
#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use std::sync::Arc;

use arugula_control_common::Principal;
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Revocation,
    deploy::{Approval, Decision, Operation},
    now_ms,
    team::{Member, Roster, TeamRole},
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
    agents_tests::{give, seeded},
    auth::Asked,
    chat_tests::{TestHub, call},
    db::{
        deploys::{self as d, Actor, EventKind, NewDeploy, Refusal, Source, SourceKind, State, VerifiedApproval},
        projects::{DEPLOYS_PLAN, ISSUES_WRITE},
        sql::Sql as _,
    },
    deploys::{
        AGENT, EXPIRED, NO_PASSKEY, NOT_A_DEVICE, NOT_AN_OWNER, NOT_PENDING, OLD_APPROVAL, OTHER_OPERATION, PASSKEY,
        STALE, SUPERSEDED,
    },
    grants::AGENT_UNGRANTED,
    hub::DeviceTrust,
};

/// An account's devices: a phone (its root), a CLI on its machine, and a
/// browser it removed; and its passkey, `pk-<account>`. Since DK4 only the
/// passkey approves: the devices are there to show they play no part.
pub struct Devices {
    pub phone: DeviceKeys,
    cli: DeviceKeys,
}

pub fn devices(hub: &TestHub, account: &str) -> Devices {
    let phone = DeviceKeys::generate();
    let mut root = Cert::new(&phone, account, Kind::Browser, "Pixel");
    root.sign_with(&phone);
    let cli = DeviceKeys::generate();
    let mut cli_cert = Cert::new(&cli, account, Kind::Cli, "geek's CLI");
    cli_cert.sign_with(&phone);
    let removed = DeviceKeys::generate();
    let mut removed_cert = Cert::new(&removed, account, Kind::Browser, "an old laptop");
    removed_cert.sign_with(&phone);
    let revocation = Revocation::new(account, &removed.id(), &phone);
    hub.passkey(&format!("pk-{account}"), account, passkey_key(account).verifying_key().to_bytes());
    // Team t's roster as `seeded` has it (a owns it, b edits, x views),
    // signed by a's phone, which made a its owner.
    if account == "a" {
        let roots = [("a", phone.id()), ("b", "b-root".to_owned()), ("x", "x-root".to_owned())];
        let members = [(&roots[0], TeamRole::Owner), (&roots[1], TeamRole::Editor), (&roots[2], TeamRole::Viewer)];
        hub.rosters("t", vec![roster(1, &members.map(|((a, r), role)| (*a, r.as_str(), role)), &phone)]);
    }
    hub.devices(
        account,
        DeviceTrust {
            root: phone.id(),
            certs: vec![root, cli_cert, removed_cert],
            revocations: vec![revocation],
            legacy_app_children: vec![],
        },
    );
    Devices { phone, cli }
}

/// A version of team t's roster, signed by `by`.
fn roster(version: u64, members: &[(&str, &str, TeamRole)], by: &DeviceKeys) -> Roster {
    let mut r = Roster {
        v: 1,
        team: "t".into(),
        name: "t".into(),
        version,
        at: version,
        members: members
            .iter()
            .map(|(account, root, role)| Member {
                account: (*account).into(),
                root: (*root).into(),
                role: *role,
                name: (*account).into(),
            })
            .collect(),
        spent: vec![],
        redeem: None,
        by: String::new(),
        sig: String::new(),
    };
    r.sign_with(by);
    r
}

/// The platform over the same app, behind a gate that also vouches for a
/// CLI device (`x-test-cli: <account> <device>`), as the hub's does.
pub async fn serve_with_cli(app: Arc<crate::Platform>) -> String {
    fn asked(headers: &axum::http::HeaderMap) -> Option<Principal> {
        let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
        if let Some(c) = h("x-test-cli") {
            let (account, device) = c.split_once(' ').unwrap();
            Some(Principal::Cli { account: account.into(), device: device.into() })
        } else if let Some(d) = h("x-test-daemon") {
            let (account, daemon) = d.split_once(' ').unwrap();
            Some(Principal::Daemon { account: account.into(), daemon: daemon.into() })
        } else {
            h("cookie")
                .and_then(|c| c.strip_prefix("ilg_session=session-").map(|a| Principal::Person { account: a.into() }))
        }
    }
    async fn gate(mut req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
        if let Some(p) = asked(req.headers()) {
            req.extensions_mut().insert(Asked::Principal(p));
        }
        next.run(req).await
    }
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    crate::register();
    let router = crate::router(app).layer(axum::middleware::from_fn(gate));
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    base
}

async fn as_header(base: &str, header: (&str, &str), method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header(header.0, header.1);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// As a's agent on geek, a's own machine.
async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    as_header(base, ("x-test-daemon", "a geek"), method, path, body).await
}

/// The chant-fly driver's plan for commit `sha` of acme/app to production
/// (M219: the platform makes it, and checks a plan it's given), with the
/// settings [`production`] gives it (M220: defaults filled in).
fn driver_plan(sha: &str) -> String {
    let settings = [
        ("fly_app", "acme-app"),
        ("chant_env", "github"),
        ("chant_dir", "."),
        ("chant_op", "release"),
        ("chant_digest", "plan"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let text =
        arugula_e2e::drivers::plan_text("chant-fly", "production", "commit", &full(sha), "acme/app", &settings, None);
    hex::encode(Sha256::digest(text.as_bytes()))
}

pub fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn plan_body(env: &str, sha: &str) -> Option<Value> {
    Some(json!({ "environment": env, "source": { "kind": "commit", "ref": full(sha), "where": "acme/app" } }))
}

/// A short hex id as a full commit id (chant-fly deploys only those,
/// M220): padded with zeros to 40.
pub fn full(sha: &str) -> String {
    format!("{sha:0<40}")
}

/// An account's test passkey (Ed25519), the same every time.
fn passkey_key(account: &str) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&Sha256::digest(format!("passkey {account}").as_bytes()).into())
}

/// How a test's passkey assertion is made: whose passkey, over what
/// challenge (the approval's, unless said), with which flags (UP and UV
/// unless said).
struct Pk<'a> {
    of: &'a str,
    challenge: Option<&'a str>,
    flags: u8,
}

/// A WebAuthn assertion as `navigator.credentials.get` gives it, by `pk`'s
/// passkey, over SHA-256 of the approval's bytes (`body`), from
/// control's page.
fn assertion(body: &str, pk: &Pk) -> Value {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    use ed25519_dalek::Signer as _;
    let challenge = pk.challenge.map_or_else(|| B64.encode(Sha256::digest(body.as_bytes())), str::to_owned);
    let client =
        json!({ "type": "webauthn.get", "challenge": challenge, "origin": "https://control.example" }).to_string();
    let mut data = Sha256::digest(b"control.example").to_vec();
    data.push(pk.flags);
    data.extend_from_slice(&0u32.to_be_bytes());
    let mut msg = data.clone();
    msg.extend_from_slice(&Sha256::digest(client.as_bytes()));
    json!({
        "id": format!("pk-{}", pk.of),
        "clientDataJSON": B64.encode(client.as_bytes()),
        "authenticatorData": B64.encode(&data),
        "signature": B64.encode(passkey_key(pk.of).sign(&msg).to_bytes()),
    })
}

/// `who`'s passkey's assertion over `body`, as the shared crate takes it.
pub fn passkey_assertion(who: &str, body: &str) -> arugula_e2e::passkey::Assertion {
    serde_json::from_value(assertion(body, &Pk { of: who, challenge: None, flags: 0x05 })).unwrap()
}

/// `who`'s v2 approval (DK4) of `decision` on deploy `d` whose operation
/// hash is `op`, good for an hour, confirmed with `who`'s passkey: the body
/// the approve and deny routes take.
pub fn signed(who: &str, d: &str, op: &str, decision: Decision) -> Value {
    signed_until(who, d, op, decision, now_ms() + 3_600_000)
}

pub fn signed_until(who: &str, d: &str, op: &str, decision: Decision, expires: u64) -> Value {
    signed_with(who, d, op, decision, expires, Some(&Pk { of: who, challenge: None, flags: 0x05 }))
}

/// The v2 approval's bytes for the route's body, and its assertion by `pk`.
fn signed_with(who: &str, d: &str, op: &str, decision: Decision, expires: u64, pk: Option<&Pk>) -> Value {
    let a = v2(who, d, op, decision, expires);
    let mut v = json!({ "op": a.op, "expires": a.expires });
    if let Some(pk) = pk {
        v["passkey"] = assertion(&a.body(), pk);
    }
    v
}

fn v2(who: &str, d: &str, op: &str, decision: Decision, expires: u64) -> Approval {
    Approval {
        deploy: d.into(),
        op: op.into(),
        account: who.into(),
        decision,
        expires,
        by: String::new(),
        sig: String::new(),
        v: arugula_e2e::deploy::V2,
    }
}

/// What a page from before DK4 sent: a v1 approval, signed by `keys`, with
/// `who`'s passkey over its bytes.
fn signed_v1(keys: &DeviceKeys, who: &str, d: &str, op: &str, decision: Decision) -> Value {
    let mut a = Approval {
        deploy: d.into(),
        op: op.into(),
        account: who.into(),
        decision,
        expires: now_ms() + 3_600_000,
        by: keys.id(),
        sig: String::new(),
        v: arugula_e2e::deploy::V1,
    };
    a.sig = hex::encode(keys.signature(a.body().as_bytes()));
    json!({
        "op": a.op, "expires": a.expires, "by": a.by, "sig": a.sig,
        "passkey": assertion(&a.body(), &Pk { of: who, challenge: None, flags: 0x05 }),
    })
}

/// t's environment `production`, made by a, its owner.
async fn production(base: &str, t: &str) -> String {
    let (st, env) = call(
        base,
        "a",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({
            "name": "production", "driver": "chant-fly", "secret_names": ["FLY_API_TOKEN", "GITHUB_TOKEN"],
            "settings": { "fly_app": "acme-app", "chant_env": "github" },
        })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
    env["id"].as_str().unwrap().to_owned()
}

/// Planned by `who` (a person) and asked for: its id and operation hash.
pub async fn asked(base: &str, who: &str, t: &str, env: &str, sha: &str) -> (String, String) {
    let (st, d) = call(base, who, "POST", &format!("/api/projects/{t}/deploys"), plan_body(env, sha)).await;
    assert_eq!((st, &d["state"]), (200, &json!("planned")), "{d}");
    let id = d["id"].as_str().unwrap().to_owned();
    let (st, d) = call(base, who, "POST", &format!("/api/deploys/{id}/request"), None).await;
    assert_eq!((st, &d["state"]), (200, &json!("pending")), "{d}");
    (id, d["operation"]["hash"].as_str().unwrap().to_owned())
}

async fn state(base: &str, id: &str) -> Value {
    call(base, "a", "GET", &format!("/api/deploys/{id}"), None).await.1["state"].clone()
}

/// An editor plans and asks; the owner approves with their passkey (v2,
/// DK4), an assertion over the approval's bytes that the log keeps and
/// anyone can check against the account's published passkey log. The
/// owners got a push saying where it's going and what. Nobody else
/// approves: an editor, a viewer, or someone outside the team.
#[tokio::test]
async fn an_owners_passkey_approves_and_the_log_keeps_its_assertion() {
    let (hub, _app, base, t, _) = seeded().await;
    devices(&hub, "a");
    devices(&hub, "b");
    let env = production(&base, &t).await;
    let (st, v) = call(
        &base,
        "b",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({
            "name": "staging", "driver": "chant-fly",
        })),
    )
    .await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_AN_OWNER)), "only an owner makes one: {v}");
    // chant-fly needs its Fly app (M220), and takes only its own settings.
    for settings in [
        json!({}),
        json!({ "fly_app": "acme-app" }),
        json!({ "fly_app": "acme-app", "chant_env": "github", "token": "x" }),
        json!({ "fly_app": "acme-app", "chant_env": "-x" }),
    ] {
        let (st, v) = call(
            &base,
            "a",
            "POST",
            &format!("/api/projects/{t}/environments"),
            Some(json!({ "name": "staging", "driver": "chant-fly", "settings": settings })),
        )
        .await;
        assert_eq!(st, 400, "{v}");
    }

    // A full commit id only (M220): never a branch or a short id.
    for r in ["main", "deadbeef"] {
        let body = json!({ "environment": env, "source": { "kind": "commit", "ref": r, "where": "acme/app" } });
        let (st, v) = call(&base, "b", "POST", &format!("/api/projects/{t}/deploys"), Some(body)).await;
        assert_eq!(st, 400, "{r}: {v}");
    }

    let (id, op) = asked(&base, "b", &t, &env, "deadbeef").await;
    let (st, d) = call(&base, "x", "GET", &format!("/api/deploys/{id}"), None).await;
    assert_eq!((st, &d["may_approve"]), (200, &json!(false)), "a viewer reads it: {d}");
    // The plan an owner reads (M220) is the one the operation names.
    let text = d["plan_text"].as_str().unwrap();
    assert_eq!(digest(text), driver_plan("deadbeef"), "{text}");
    assert!(text.contains("chant run release") && text.contains("Fly app acme-app"), "{text}");
    assert_eq!(d["environment"]["settings"]["fly_app"], "acme-app");
    let body = d["operation"]["body"].as_str().unwrap();
    assert!(
        body.starts_with(&format!(
            "arugula deploy op v1\nproject {t}\nenvironment {env}\nname production\nplan {}\n",
            driver_plan("deadbeef")
        )),
        "{body}"
    );
    assert_eq!(digest(body), op, "the hash is over the bytes shown");
    assert_eq!(call(&base, "c", "GET", &format!("/api/deploys/{id}"), None).await.0, 404, "someone outside the team");

    // The push: to a, the owner; not to b, who asked.
    let push = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(p) = hub.pushes().into_iter().find(|(_, m, _)| m["deploy"] == json!(id)) {
                return p;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a push for the deploy");
    assert_eq!(push.0, "a");
    assert_eq!(push.1["title"], json!("Approve a deploy to production?"));
    assert!(hub.pushes().iter().all(|(to, _, _)| to != "b"));

    for (who, reason) in [("b", NOT_AN_OWNER), ("x", NOT_AN_OWNER)] {
        let (st, v) = call(
            &base,
            who,
            "POST",
            &format!("/api/deploys/{id}/approve"),
            Some(signed(who, &id, &op, Decision::Once)),
        )
        .await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(reason)), "{who}: {v}");
    }

    let (st, d) = call(&base, "a", "GET", &format!("/api/deploys/{id}"), None).await;
    assert_eq!((st, &d["may_approve"]), (200, &json!(true)), "{d}");
    let (st, d) =
        call(&base, "a", "POST", &format!("/api/deploys/{id}/approve"), Some(signed("a", &id, &op, Decision::Once)))
            .await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    let approval = &d["approvals"][0];
    assert_eq!(
        (&approval["v"], &approval["decision"], &approval["device"], &approval["sig"], &approval["passkey"]["id"]),
        (&json!(2), &json!("once"), &json!(""), &json!(""), &json!("pk-a")),
        "no device signs it: {approval}"
    );
    assert!(approval["body"].as_str().unwrap().starts_with("arugula deploy approval v2\n"), "{approval}");
    assert!(approval["id"].as_str().unwrap().starts_with("apv_"));
    assert_eq!(
        d["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect::<Vec<_>>(),
        ["planned", "requested", "approved"]
    );
    assert_eq!(d["log"], json!({ "verified": 3 }));

    // The log alone is enough to check it: the event's detail holds the
    // approval and its passkey's assertion, which checks out against a's
    // passkey log as the hub publishes it, at the event's time.
    let event = _app.db.deploy_history(&id).unwrap().pop().unwrap();
    let detail = event.detail_json().unwrap();
    assert_eq!((detail["v"].as_u64(), detail.get("device"), detail.get("signature")), (Some(2), None, None));
    let from_log = v2(
        event.actor.strip_prefix("person ").unwrap(),
        &event.deploy,
        detail["op"].as_str().unwrap(),
        Decision::Once,
        detail["expires"].as_u64().unwrap(),
    );
    let assertion: arugula_e2e::passkey::Assertion = serde_json::from_value(json!({
        "id": detail["passkey"]["id"], "clientDataJSON": detail["passkey"]["client_data_json"],
        "authenticatorData": detail["passkey"]["authenticator_data"], "signature": detail["passkey"]["signature"],
    }))
    .unwrap();
    use crate::HubPort as _;
    let log = hub.passkey_log("a").await.unwrap();
    let origins = arugula_e2e::passkey::origins_of_sites(["https://control.example"]);
    assert!(from_log.passkey_checks(&assertion, &log, event.at, &origins).is_ok());
    assert_eq!(event.actor, "person a");

    // Once is once: it isn't waiting any more.
    let (st, _) =
        call(&base, "a", "POST", &format!("/api/deploys/{id}/approve"), Some(signed("a", &id, &op, Decision::Once)))
            .await;
    assert_eq!(st, 409);

    // And a deny is signed the same way.
    let (id2, op2) = asked(&base, "a", &t, &env, "cafef00d").await;
    let (st, d) =
        call(&base, "a", "POST", &format!("/api/deploys/{id2}/deny"), Some(signed("a", &id2, &op2, Decision::Deny)))
            .await;
    assert_eq!((st, &d["state"], &d["approvals"][0]["decision"]), (200, &json!("denied"), &json!("deny")), "{d}");
}

/// Agents plan and ask, and never approve or deny: not a's agent on geek,
/// a's own machine, with a's real passkey assertion in hand; not a's CLI
/// login. There's no machine route that approves. Neither does an old
/// (v1) approval with a device's signature, an assertion over another
/// deploy, decision, account or operation, or an expired one. The deploy
/// waits through all of it, then a's passkey approves it.
#[tokio::test]
async fn an_agent_on_the_owners_own_machine_never_approves() {
    let (hub, app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let env = production(&base, &t).await;
    let cli_base = serve_with_cli(app.clone()).await;

    // No grant, no plan; issues.write isn't deploys.plan.
    let path = format!("/api/daemon/projects/{t}/deploys");
    let (st, v) = geek(&base, "POST", &path, plan_body(&env, "deadbeef")).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", ISSUES_WRITE, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) = geek(&base, "POST", &path, plan_body(&env, "deadbeef")).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "issues.write doesn't plan: {v}");
    assert!(v["error"].as_str().unwrap().contains("deploys.plan"), "{v}");
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", DEPLOYS_PLAN, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, v) =
        call(&base, "a", "POST", "/api/grants", give("geek", DEPLOYS_PLAN, json!({ "kind": "account" }))).await;
    assert_eq!(st, 400, "deploys.plan is over a project: {v}");

    let (st, d) = geek(&base, "POST", &path, plan_body(&env, "deadbeef")).await;
    assert_eq!((st, &d["agent"]["machine"], &d["planned_by"]), (200, &json!("geek"), &json!("a")), "{d}");
    let id = d["id"].as_str().unwrap().to_owned();
    let (st, d) = geek(&base, "POST", &format!("/api/daemon/deploys/{id}/request"), None).await;
    assert_eq!((st, &d["state"], &d["may_approve"]), (200, &json!("pending"), &json!(false)), "{d}");
    let op = d["operation"]["hash"].as_str().unwrap().to_owned();
    // The agent asked, so its owner gets the card.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(hub.pushes().iter().any(|(to, m, _)| to == "a"
        && m["deploy"] == json!(id)
        && m["body"].as_str().unwrap().contains("'s agent on geek-box")));

    let good = signed("a", &id, &op, Decision::Once);
    let approve = format!("/api/deploys/{id}/approve");
    let deny = format!("/api/deploys/{id}/deny");

    // The agent, with the owner's real passkey assertion: refused as an
    // agent.
    for path in [&approve, &deny] {
        let (st, v) = geek(&base, "POST", path, Some(good.clone())).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT)), "{path}: {v}");
    }
    // There's no machine route that approves.
    for path in [format!("/api/daemon/deploys/{id}/approve"), format!("/api/daemon/deploys/{id}/deny")] {
        let (st, _) = geek(&base, "POST", &path, Some(good.clone())).await;
        assert!(st == 404 || st == 405, "{path}: {st}");
    }
    // a's CLI login (what an agent on geek could use), even with a's real
    // passkey assertion: not a browser's session.
    let cli = ("x-test-cli", &*format!("a {}", a.cli.id()));
    let (st, v) = as_header(&cli_base, cli, "POST", &approve, Some(good.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "{v}");
    // A page from before DK4: a v1 with a device's signature, even the
    // phone's, with the passkey over its bytes.
    for keys in [&a.phone, &a.cli] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(signed_v1(keys, "a", &id, &op, Decision::Once))).await;
        assert_eq!((st, v["reason"].as_str()), (400, Some(OLD_APPROVAL)), "a v1: {v}");
    }
    // a's passkey over another deploy, or as a deny, or b's passkey over
    // b's account's bytes: not this approval's.
    for wrong in [
        signed("a", "dep_other", &op, Decision::Once),
        signed("a", &id, &op, Decision::Deny),
        signed("b", &id, &op, Decision::Once),
    ] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(wrong)).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "{v}");
    }
    // Over another operation (another plan): a good signature, the wrong bytes.
    let other = Operation {
        project: t.clone(),
        environment: env.clone(),
        environment_name: "production".into(),
        plan: digest("evil"),
        source_kind: "commit".into(),
        source_ref: "deadbeef".into(),
        source_where: "acme/app".into(),
        planned: "00".repeat(32),
    };
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed("a", &id, &other.hash(), Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(OTHER_OPERATION)), "{v}");
    // Expired, or good for too long.
    for expires in [now_ms() - 1, now_ms() + 8 * 24 * 3_600_000] {
        let (st, _) =
            call(&base, "a", "POST", &approve, Some(signed_until("a", &id, &op, Decision::Once, expires))).await;
        assert_eq!(st, 400);
    }
    assert_eq!(state(&base, &id).await, json!("pending"), "it waited through all of that");
    assert!(app.db.deploy_approvals(&id).unwrap().is_empty());

    let (st, d) = call(&base, "a", "POST", &approve, Some(good)).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    // The agent reads it, and still isn't offered approval.
    let (st, d) = geek(&base, "GET", &format!("/api/daemon/deploys/{id}"), None).await;
    assert_eq!((st, &d["state"], &d["may_approve"]), (200, &json!("approved"), &json!(false)), "{d}");
}

/// Another deploy applied to the environment after this one was planned:
/// a good approval with the owner's passkey is refused as stale, and the
/// page isn't offered approval for it.
#[tokio::test]
async fn a_stale_deploy_is_refused_even_with_a_good_approval() {
    let (hub, app, base, t, _) = seeded().await;
    devices(&hub, "a");
    let env = production(&base, &t).await;
    let (first, first_op) = asked(&base, "a", &t, &env, "aaaa1111").await;
    // Planned, not asked for: asking would stale the first (#334). The
    // first's apply stales it all the same.
    let (st, d) = call(&base, "a", "POST", &format!("/api/projects/{t}/deploys"), plan_body(&env, "bbbb2222")).await;
    assert_eq!(st, 200, "{d}");
    let second = d["id"].as_str().unwrap().to_owned();
    let second_op = d["operation"]["hash"].as_str().unwrap().to_owned();
    let (st, _) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{first}/approve"),
        Some(signed("a", &first, &first_op, Decision::Once)),
    )
    .await;
    assert_eq!(st, 200);
    // The first one's driver run starts applying it.
    app.db.link_deploy_run_for_tests(&first, "run_1");
    app.db.record_deploy(&first, EventKind::Started, &Actor::run("run_1"), now_ms(), json!({})).unwrap().unwrap();

    let (st, d) = call(&base, "a", "GET", &format!("/api/deploys/{second}"), None).await;
    assert_eq!(
        (st, &d["state"], &d["may_approve"], &d["stale"]["deploy"]),
        (200, &json!("stale"), &json!(false), &json!(first)),
        "{d}"
    );
    let (st, v) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{second}/approve"),
        Some(signed("a", &second, &second_op, Decision::Once)),
    )
    .await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(STALE)), "{v}");
    assert!(app.db.deploy_approvals(&second).unwrap().is_empty());
    // And the waiting list no longer shows it.
    let (st, w) = call(&base, "a", "GET", "/api/deploys/waiting", None).await;
    assert_eq!((st, w["deploys"].as_array().map(Vec::len)), (200, Some(0)), "{w}");
}

/// A newer request to the environment supersedes an older pending deploy
/// (#334): it reads stale with `why: superseded` and the newer deploy, the
/// owner's signed approval of it is refused, and the inbox lists only the
/// newer one.
#[tokio::test]
async fn a_superseded_deploy_says_why_and_is_not_approved() {
    let (hub, app, base, t, _) = seeded().await;
    devices(&hub, "a");
    let env = production(&base, &t).await;
    let (old, old_op) = asked(&base, "a", &t, &env, "aaaa1111").await;
    let (new, _) = asked(&base, "a", &t, &env, "bbbb2222").await;

    let (st, d) = call(&base, "a", "GET", &format!("/api/deploys/{old}"), None).await;
    assert_eq!(
        (st, &d["state"], &d["may_approve"], &d["stale"]["deploy"], &d["stale"]["why"]),
        (200, &json!("stale"), &json!(false), &json!(new), &json!("superseded")),
        "{d}"
    );
    let (st, v) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{old}/approve"),
        Some(signed("a", &old, &old_op, Decision::Once)),
    )
    .await;
    assert_eq!(st, 409, "{v}");
    assert!(app.db.deploy_approvals(&old).unwrap().is_empty());
    let (_, w) = call(&base, "a", "GET", "/api/deploys/waiting", None).await;
    let ids: Vec<&str> = w["deploys"].as_array().unwrap().iter().filter_map(|d| d["id"].as_str()).collect();
    assert_eq!(ids, [new.as_str()], "{w}");
}

/// DK3: the passkey is checked against the account's passkey log as the hub
/// publishes it, with no verdict from the hub: one the log says was removed
/// before the approval came no longer approves.
#[tokio::test]
async fn a_passkey_removed_from_the_log_no_longer_approves() {
    let (hub, _app, base, t, _) = seeded().await;
    devices(&hub, "a");
    devices(&hub, "b");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    hub.passkey_removed("pk-a", 1);
    let approve = format!("/api/deploys/{id}/approve");
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed("a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "{v}");
    assert!(v["error"].as_str().is_some_and(|e| e.contains("wasn't one of the account's then")), "{v}");
    assert_eq!(hub.calls_of("verify_passkey"), 0, "not asked for a verdict");
    assert_eq!(state(&base, &id).await, json!("pending"));
}

/// Every approval and deny is a passkey's assertion over its bytes, checked
/// against the account's published passkey log: none, one over another
/// challenge, another account's passkey, or one without user verification
/// are refused. The assertion, with its credential id, is kept with the
/// approval and in its event.
#[tokio::test]
async fn every_approval_needs_the_owners_passkey() {
    let (hub, app, base, t, _) = seeded().await;
    devices(&hub, "a");
    devices(&hub, "b");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let expires = now_ms() + 3_600_000;
    let approve = format!("/api/deploys/{id}/approve");
    let deny = format!("/api/deploys/{id}/deny");
    let pk = |of, challenge, flags| Pk { of, challenge, flags };
    for (why, body) in [
        ("no passkey", signed_with("a", &id, &op, Decision::Once, expires, None)),
        (
            "another challenge",
            signed_with("a", &id, &op, Decision::Once, expires, Some(&pk("a", Some("bm90IHRoaXM"), 0x05))),
        ),
        ("b's passkey", signed_with("a", &id, &op, Decision::Once, expires, Some(&pk("b", None, 0x05)))),
        ("no user verification", signed_with("a", &id, &op, Decision::Once, expires, Some(&pk("a", None, 0x01)))),
    ] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(body)).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "{why}: {v}");
    }
    let (st, v) =
        call(&base, "a", "POST", &deny, Some(signed_with("a", &id, &op, Decision::Deny, expires, None))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "a deny too: {v}");
    assert_eq!(state(&base, &id).await, json!("pending"));
    assert!(app.db.deploy_approvals(&id).unwrap().is_empty());
    assert!(hub.calls_of("passkey_log") >= 3, "checked against the hub's published log (DK3)");
    assert_eq!(hub.calls_of("verify_passkey"), 0, "not asked for a verdict");

    let (st, d) = call(&base, "a", "POST", &approve, Some(signed("a", &id, &op, Decision::Once))).await;
    assert_eq!(
        (st, &d["state"], &d["approvals"][0]["passkey"]["id"]),
        (200, &json!("approved"), &json!("pk-a")),
        "{d}"
    );
    let event = app.db.deploy_history(&id).unwrap().pop().unwrap();
    let detail = event.detail_json().unwrap();
    assert_eq!(detail["passkey"]["id"], json!("pk-a"), "the log keeps the assertion: {detail}");
    assert!(detail["passkey"]["client_data_json"].as_str().is_some_and(|c| !c.is_empty()));
}

/// DK4 (aru-126): no device plays a part. An owner whose every device came
/// in through a CLI's or the app's certificate (a CLI-rooted chain, a
/// browser a CLI certified, the app's window and what it let in), and whose
/// owner role a CLI's key gave, approves with their passkey; the platform
/// doesn't ask the hub for devices or rosters. The owner role is read now:
/// one made an editor since doesn't approve. A personal project's owner is
/// its account.
#[tokio::test]
async fn an_owner_whose_browser_a_cli_certified_approves_with_a_passkey() {
    let (hub, app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let b = devices(&hub, "b");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let approve = format!("/api/deploys/{id}/approve");
    let cert = |keys: &DeviceKeys, kind, name: &str, by: &DeviceKeys| {
        let mut c = Cert::new(keys, "a", kind, name);
        c.sign_with(by);
        c
    };
    // a's root is a CLI; its browser, a CLI certified it; the app's window
    // let another in.
    let cli_root = DeviceKeys::generate();
    let (browser, window, let_in) = (DeviceKeys::generate(), DeviceKeys::generate(), DeviceKeys::generate());
    let certs = vec![
        cert(&cli_root, Kind::Cli, "the first CLI", &cli_root),
        cert(&browser, Kind::Browser, "Firefox", &cli_root),
        cert(&window, Kind::App, "Arugula app on jake-air", &browser),
        cert(&let_in, Kind::Browser, "Safari", &window),
    ];
    hub.devices("a", DeviceTrust { root: cli_root.id(), certs, revocations: vec![], legacy_app_children: vec![] });
    // b's CLI made a an owner of t.
    let (ar, br) = (cli_root.id(), b.phone.id());
    let v1 = roster(1, &[("b", &br, TeamRole::Owner), ("a", &ar, TeamRole::Editor)], &b.phone);
    let both = [("b", br.as_str(), TeamRole::Owner), ("a", ar.as_str(), TeamRole::Owner)];
    hub.rosters("t", vec![v1, roster(2, &both, &b.cli)]);
    let _ = a;

    // Made an editor since: not an owner now.
    hub.team("t", &[("a", TeamRole::Editor), ("b", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed("a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_AN_OWNER)), "an editor now: {v}");
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();

    let (st, d) = call(&base, "a", "POST", &approve, Some(signed("a", &id, &op, Decision::Once))).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    assert_eq!((hub.calls_of("device_trust"), hub.calls_of("team_rosters")), (0, 0), "no device or roster asked");

    // A personal project: its account is its owner.
    let mine = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    let env = production(&base, &mine).await;
    let (id, op) = asked(&base, "a", &mine, &env, "cafef00d").await;
    let (st, d) =
        call(&base, "a", "POST", &format!("/api/deploys/{id}/approve"), Some(signed("a", &id, &op, Decision::Once)))
            .await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
}

/// DK4: an account with no passkey can't approve or deny, whatever devices
/// it has: neither one that never had one nor one whose every passkey was
/// removed. The deploy waits.
#[tokio::test]
async fn an_account_with_no_passkey_cant_approve() {
    let (hub, app, base, t, _) = seeded().await;
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    // a has no passkey: an assertion from a key the hub never registered.
    for path in [format!("/api/deploys/{id}/approve"), format!("/api/deploys/{id}/deny")] {
        let d = if path.ends_with("deny") { Decision::Deny } else { Decision::Once };
        let (st, v) = call(&base, "a", "POST", &path, Some(signed("a", &id, &op, d))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "{v}");
        assert!(v["error"].as_str().unwrap().contains("has no passkey"), "{v}");
    }
    // a's passkey, removed.
    devices(&hub, "a");
    hub.passkey_removed("pk-a", 1);
    let (st, v) =
        call(&base, "a", "POST", &format!("/api/deploys/{id}/approve"), Some(signed("a", &id, &op, Decision::Once)))
            .await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "{v}");
    assert_eq!(state(&base, &id).await, json!("pending"));
    assert!(app.db.deploy_approvals(&id).unwrap().is_empty());
}

/// History (DK4): an approval recorded before the switch (v1: a device's
/// signature, with a passkey's assertion beside it) still reads on the
/// deploy's page as it was made: its v1 bytes, its device's signature over
/// them, its passkey's assertion, which still checks out against the
/// account's passkey log at the time it was recorded; and the log's chain
/// still checks.
#[tokio::test]
async fn an_old_v1_approval_still_verifies_on_the_deploy_page() {
    let (hub, app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let (_, operation) = app.db.deploy_operation(&id).unwrap().unwrap();
    let expires = now_ms() + 3_600_000;
    let v1 = Approval::v1(&a.phone, &id, &operation, "a", Decision::Once, expires);
    assert_eq!(v1.op, op);
    let pk = assertion(&v1.body(), &Pk { of: "a", challenge: None, flags: 0x05 });
    let proof = d::PasskeyProof {
        id: pk["id"].as_str().unwrap().into(),
        client_data_json: pk["clientDataJSON"].as_str().unwrap().into(),
        authenticator_data: pk["authenticatorData"].as_str().unwrap().into(),
        signature: pk["signature"].as_str().unwrap().into(),
    };
    let recorded = VerifiedApproval::unchecked_for_tests(
        &id,
        &operation.plan,
        &operation.planned,
        &v1.op,
        "a",
        &v1.by,
        d::Decision::Once,
        expires,
        &v1.sig,
    )
    .with_passkey_for_tests(proof.clone());
    let at = now_ms();
    app.db.record_deploy_approval(&recorded, "Pixel", at).unwrap().unwrap();

    let (st, d) = call(&base, "a", "GET", &format!("/api/deploys/{id}"), None).await;
    assert_eq!(st, 200, "{d}");
    let shown = &d["approvals"][0];
    assert_eq!(
        (&shown["v"], &shown["device"], &shown["device_name"], &shown["passkey"]["id"]),
        (&json!(1), &json!(a.phone.id()), &json!("Pixel"), &json!("pk-a")),
        "{shown}"
    );
    assert_eq!(shown["body"].as_str().unwrap(), v1.body());
    assert_eq!(d["log"], json!({ "verified": 3 }), "the chain checks");
    // Read back, it's the v1 it was: its device's signature holds, and its
    // passkey's assertion checks out against the log at its time.
    let stored = app.db.deploy_approvals(&id).unwrap().pop().unwrap();
    let back = stored.signed();
    assert_eq!(back, v1);
    assert!(back.signed_by(&hex::encode(a.phone.sign_public())));
    use crate::HubPort as _;
    let log = hub.passkey_log("a").await.unwrap();
    let origins = arugula_e2e::passkey::origins_of_sites(["https://control.example"]);
    let assertion = arugula_e2e::passkey::Assertion {
        id: proof.id,
        client_data_json: proof.client_data_json,
        authenticator_data: proof.authenticator_data,
        signature: proof.signature,
    };
    assert!(back.passkey_checks(&assertion, &log, stored.at, &origins).is_ok());
    // Its event keeps the v1's fields.
    let detail = app.db.deploy_history(&id).unwrap().pop().unwrap().detail_json().unwrap();
    assert_eq!(
        (detail["device"].as_str(), detail["signature"].as_str()),
        (Some(v1.by.as_str()), Some(v1.sig.as_str()))
    );
}

/// Asks are limited per project and per asker (a person, or an agent's
/// machine), and only asks that were recorded count; a plan's digest is
/// kept in lowercase.
#[tokio::test]
async fn asks_are_limited_per_project_and_asker() {
    let (hub, _app, base, t, _) = seeded().await;
    devices(&hub, "a");
    let env = production(&base, &t).await;
    let mut body = plan_body(&env, "deadbeef").unwrap();
    body["plan"] = driver_plan("deadbeef").to_uppercase().into();
    let (st, d) = call(&base, "a", "POST", &format!("/api/projects/{t}/deploys"), Some(body)).await;
    assert_eq!((st, &d["plan"]), (200, &json!(driver_plan("deadbeef"))), "{d}");
    // Another plan than the driver's is refused (M219); none is made.
    let mut other = plan_body(&env, "deadbeef").unwrap();
    other["plan"] = digest("not the driver's").into();
    let (st, v) = call(&base, "a", "POST", &format!("/api/projects/{t}/deploys"), Some(other)).await;
    assert_eq!((st, v["reason"].as_str()), (400, Some(crate::deploys::OTHER_PLAN)), "{v}");
    let mut none = plan_body(&env, "deadbeef").unwrap();
    none.as_object_mut().unwrap().remove("plan");
    let (st, d) = call(&base, "a", "POST", &format!("/api/projects/{t}/deploys"), Some(none)).await;
    assert_eq!((st, &d["plan"]), (200, &json!(driver_plan("deadbeef"))), "made when left out: {d}");
    let first = d["id"].as_str().unwrap().to_owned();
    let plan = |who: &'static str, n: u32| {
        let base = base.clone();
        let (t, env) = (t.clone(), env.clone());
        async move {
            let (st, d) = call(
                &base,
                who,
                "POST",
                &format!("/api/projects/{t}/deploys"),
                plan_body(&env, &format!("{who}{n:07x}")),
            )
            .await;
            assert_eq!(st, 200, "{d}");
            d["id"].as_str().unwrap().to_owned()
        }
    };
    let ask = |who: &'static str, id: String| {
        let base = base.clone();
        async move { call(&base, who, "POST", &format!("/api/deploys/{id}/request"), None).await.0 }
    };
    // Asks that fail (b asks again for one already asked for) don't count.
    assert_eq!(ask("b", first.clone()).await, 200);
    for _ in 0..40 {
        assert_eq!(ask("b", first.clone()).await, 409);
    }
    let mut refused = None;
    for n in 0..40 {
        if ask("b", plan("b", n).await).await == 429 {
            refused = Some(n);
            break;
        }
    }
    assert_eq!(refused, Some(29), "b's 31st recorded ask in an hour is refused");
    // a's asks are a's own.
    assert_eq!(ask("a", plan("a", 0).await).await, 200, "another asker isn't held back");
}

/// The owner's waiting list: deploys pending in projects they own, not an
/// editor's.
#[tokio::test]
async fn the_waiting_list_is_the_owners() {
    let (hub, _app, base, t, _) = seeded().await;
    devices(&hub, "a");
    let env = production(&base, &t).await;
    let (id, _) = asked(&base, "b", &t, &env, "deadbeef").await;
    let (st, w) = call(&base, "a", "GET", "/api/deploys/waiting", None).await;
    assert_eq!((st, &w["deploys"][0]["id"], &w["deploys"][0]["may_approve"]), (200, &json!(id), &json!(true)), "{w}");
    let (_, w) = call(&base, "b", "GET", "/api/deploys/waiting", None).await;
    assert_eq!(w["deploys"], json!([]), "b edits t and doesn't own it");
}

/// DK5: `can-decide` answers before the page raises a passkey prompt, with
/// a reason for each way the approval would be refused: no passkey, not an
/// owner, superseded, decided, only planned, stale and expired. When it can,
/// it names the account's live passkeys and their RP.
#[tokio::test]
async fn can_decide_says_why_not_before_the_passkey_prompt() {
    let (hub, app, base, t, _) = seeded().await;
    let env = production(&base, &t).await;
    let can = |who: &'static str, id: String| {
        let base = base.clone();
        async move { call(&base, who, "GET", &format!("/api/deploys/{id}/can-decide"), None).await }
    };
    let reason = |v: &Value| (v["can"].clone(), v["reason"].as_str().map(str::to_owned));
    let (first, _) = asked(&base, "b", &t, &env, "aaaa1111").await;

    // a owns t and has no passkey yet.
    let (st, v) = can("a", first.clone()).await;
    assert_eq!((st, reason(&v)), (200, (json!(false), Some(NO_PASSKEY.into()))), "{v}");
    assert!(v["do"].as_str().unwrap().contains("Add one on Control"), "{v}");
    devices(&hub, "a");
    let (st, v) = can("a", first.clone()).await;
    assert_eq!(
        (st, &v["can"], &v["passkeys"], &v["rp"]),
        (200, &json!(true), &json!(["pk-a"]), &json!("control.example")),
        "{v}"
    );

    // Not an owner: b edits, x views; c isn't in the team at all.
    for who in ["b", "x"] {
        let (st, v) = can(who, first.clone()).await;
        assert_eq!((st, reason(&v)), (200, (json!(false), Some(NOT_AN_OWNER.into()))), "{who}: {v}");
    }
    assert_eq!(can("c", first.clone()).await.0, 404);
    // An owner made an editor since: by the roster now.
    hub.team("t", &[("a", TeamRole::Editor), ("b", TeamRole::Owner), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(reason(&can("a", first.clone()).await.1), (json!(false), Some(NOT_AN_OWNER.into())));
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();

    // A newer ask supersedes it, and says which.
    let (second, second_op) = asked(&base, "a", &t, &env, "bbbb2222").await;
    let (_, v) = can("a", first.clone()).await;
    assert_eq!((reason(&v), &v["newer"]), ((json!(false), Some(SUPERSEDED.into())), &json!(second)), "{v}");

    // Planned, not asked: not pending.
    let (_, d) = call(&base, "a", "POST", &format!("/api/projects/{t}/deploys"), plan_body(&env, "cccc3333")).await;
    let third = d["id"].as_str().unwrap().to_owned();
    let (_, v) = can("a", third.clone()).await;
    assert_eq!(reason(&v), (json!(false), Some(NOT_PENDING.into())), "{v}");
    assert!(v["why"].as_str().unwrap().contains("Nobody has asked"), "{v}");

    // Decided: not pending, approved already.
    let approve = format!("/api/deploys/{second}/approve");
    let (st, d) = call(&base, "a", "POST", &approve, Some(signed("a", &second, &second_op, Decision::Once))).await;
    assert_eq!(st, 200, "{d}");
    let (_, v) = can("a", second.clone()).await;
    assert_eq!(reason(&v), (json!(false), Some(NOT_PENDING.into())), "{v}");
    assert!(v["why"].as_str().unwrap().contains("approved already"), "{v}");

    // Stale: the second one's run applies it, after the third was planned.
    app.db.link_deploy_run_for_tests(&second, "run_1");
    app.db.record_deploy(&second, EventKind::Started, &Actor::run("run_1"), now_ms(), json!({})).unwrap().unwrap();
    assert_eq!(reason(&can("a", third.clone()).await.1), (json!(false), Some(STALE.into())));

    // Expired: its approval ran out before it was applied (a's own project).
    let mine = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    let env = production(&base, &mine).await;
    let (fourth, _) = asked(&base, "a", &mine, &env, "dddd4444").await;
    app.db.record_deploy(&fourth, EventKind::Expired, &Actor::Platform, now_ms(), json!({})).unwrap().unwrap();
    assert_eq!(reason(&can("a", fourth).await.1), (json!(false), Some(EXPIRED.into())));

    // Its passkey removed from the log: none again.
    let (fifth, _) = asked(&base, "a", &mine, &env, "eeee5555").await;
    assert_eq!(can("a", fifth.clone()).await.1["can"], json!(true));
    hub.passkey_removed("pk-a", 1);
    assert_eq!(reason(&can("a", fifth).await.1), (json!(false), Some(NO_PASSKEY.into())));
}

/// DK5: an owner who asks for approval themselves gets the push too, saying
/// they asked, so the approval can be made from whichever device has it.
#[tokio::test]
async fn an_owner_who_asks_gets_the_push_too() {
    let (hub, _app, base, t, _) = seeded().await;
    let env = production(&base, &t).await;
    let (id, _) = asked(&base, "a", &t, &env, "deadbeef").await;
    let push = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(p) = hub.pushes().into_iter().find(|(_, m, _)| m["deploy"] == json!(id)) {
                return p;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a push for the deploy");
    assert_eq!(push.0, "a");
    assert!(push.1["body"].as_str().unwrap().ends_with("; you asked"), "{}", push.1);
}

/// In the database: an approval's row is written only with its event, when
/// it names the deploy's plan and log as they are, and never on a stale
/// deploy, all in one transaction. Run on each dialect.
#[test]
fn recorded_only_over_the_operation_and_never_stale() {
    let db = crate::db::Db::test();
    db.c()
        .execute(
            "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES ('p', 'p', 'p', FALSE, 1)",
            crate::db::sql::params![],
        )
        .unwrap();
    let env = db.create_environment("p", "production", "chant-fly", &[], 1).unwrap();
    let source = Source { kind: SourceKind::Commit, reference: "abc".into(), place: "acme/app".into() };
    let plan = |at| {
        let d = db
            .plan_deploy(
                &NewDeploy {
                    environment: &env.id,
                    source: &source,
                    plan_digest: &digest("p"),
                    roll_back: None,
                    subject: None,
                    expected_version: None,
                },
                &Actor::person("a"),
                at,
            )
            .unwrap()
            .unwrap();
        d.id
    };
    let ask = |id: &str, at| {
        db.record_deploy(id, EventKind::Requested, &Actor::agent("a", "geek"), at, json!({})).unwrap().unwrap();
    };
    let keys = DeviceKeys::generate();
    let approval = |id: &str, op: &Operation| {
        let a = Approval::v1(&keys, id, op, "a", Decision::Once, now_ms() + 1000);
        VerifiedApproval::unchecked_for_tests(
            id,
            &op.plan,
            &op.planned,
            &a.op,
            "a",
            &a.by,
            d::Decision::Once,
            a.expires,
            &a.sig,
        )
    };

    // `two` is planned and not asked for: a request for it would stale `one`
    // (#334), and an apply stales it all the same.
    let one = plan(1);
    let two = plan(2);
    ask(&one, 2);
    let (_, op) = db.deploy_operation(&one).unwrap().unwrap();
    let mut other = op.clone();
    other.planned = "00".repeat(32);
    assert!(matches!(
        db.record_deploy_approval(&approval(&one, &other), "phone", 3).unwrap(),
        Err(Refusal::Mismatch(_))
    ));
    let (r, stored) = db.record_deploy_approval(&approval(&one, &op), "phone", 3).unwrap().unwrap();
    assert_eq!((r.deploy.state, stored.seq, stored.decision), (State::Approved, r.events[0].seq, Decision::Once));
    assert_eq!((r.events[0].actor.as_str(), stored.device_name.as_str()), ("person a", "phone"));

    db.link_deploy_run_for_tests(&one, "run_1");
    db.record_deploy(&one, EventKind::Started, &Actor::run("run_1"), 4, json!({})).unwrap().unwrap();
    let (_, op2) = db.deploy_operation(&two).unwrap().unwrap();
    assert!(matches!(
        db.record_deploy_approval(&approval(&two, &op2), "phone", 5).unwrap(),
        Err(Refusal::Stale { .. })
    ));
    assert!(db.deploy_approvals(&two).unwrap().is_empty());
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(6));
}

/// M222: an apps-platform environment plans only a full commit of one of
/// the list's organization's repositories, and a deploy shows the plan's
/// text (the list, the pointer's path, the commit), whose digest is its
/// plan.
#[tokio::test]
async fn an_apps_platform_plan_names_the_list_and_the_pointer() {
    let (_hub, _app, base, t, _) = seeded().await;
    let (st, env) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({ "name": "home", "driver": "apps-platform" })),
    )
    .await;
    assert_eq!(st, 200, "{env}");
    let env = env["id"].as_str().unwrap();
    // The read token's name is the driver's; the write token's may be the
    // environment's own (a fine-grained token instead of the mint).
    let envs = format!("/api/projects/{t}/environments");
    let with = |name: &str, secrets: &[&str]| {
        Some(json!({ "name": name, "driver": "apps-platform", "secret_names": secrets }))
    };
    let (st, v) = call(&base, "a", "POST", &envs, with("home2", &["SOURCE_TOKEN"])).await;
    assert_eq!(st, 400, "{v}");
    assert!(v.to_string().contains("SOURCE_TOKEN is the token"), "{v}");
    let (st, own) = call(&base, "a", "POST", &envs, with("home3", &["APPS_PLATFORM_TOKEN"])).await;
    assert_eq!(st, 200, "{own}");
    let sha = "d".repeat(40);
    let plan = |kind: &str, sha: &str, place: &str| {
        Some(json!({ "environment": env, "source": { "kind": kind, "ref": sha, "where": place } }))
    };
    let path = format!("/api/projects/{t}/deploys");
    for (body, why) in [
        (plan("commit", &sha, "acme/app"), "arugula-salad/<app>"),
        (plan("commit", &sha, "arugula-salad/planter-apps"), "arugula-salad/<app>"),
        (plan("artifact", &sha, "arugula-salad/app"), "not an artifact"),
        (plan("commit", "deadbeef", "arugula-salad/app"), "full commit sha"),
    ] {
        let (st, v) = call(&base, "a", "POST", &path, body).await;
        assert_eq!(st, 400, "{v}");
        assert!(v.to_string().contains(why), "{v}");
    }
    // Its run would mint a token that writes the list, and this platform
    // has no GitHub App to mint it: refused when it's planned, not when it
    // runs. An environment with a token of its own plans.
    // Its run reads the app's repository, so that must be the project's.
    let (st, v) = call(&base, "a", "POST", &path, plan("commit", &sha, "arugula-salad/app")).await;
    assert_eq!(st, 409, "{v}");
    assert!(v.to_string().contains("arugula-salad/app isn't a repository linked to this project"), "{v}");
    let link = crate::db::forge::LinkedRepo {
        repo_id: 4242,
        full_name: "arugula-salad/app".into(),
        private: true,
        project_id: t.clone(),
        installation_id: 9,
        linked_by: None,
        linked_at: 1,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    let install = crate::db::forge::Installation {
        id: 9,
        account_id: 500,
        login: "arugula-salad".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    _app.db.link_repo(&link, &install, "a", 1).unwrap();
    let (st, v) = call(&base, "a", "POST", &path, plan("commit", &sha, "arugula-salad/app")).await;
    assert_eq!(st, 409, "{v}");
    assert!(v.to_string().contains("no GitHub App to mint APPS_PLATFORM_TOKEN"), "{v}");
    let body =
        json!({ "environment": own["id"], "source": { "kind": "commit", "ref": sha, "where": "arugula-salad/app" } });
    let (st, d) = call(&base, "a", "POST", &path, Some(body)).await;
    assert_eq!(st, 200, "{d}");
    let text = d["plan_text"].as_str().unwrap();
    assert_eq!(digest(text), d["plan"].as_str().unwrap(), "the text is what the digest is over");
    assert!(
        text.contains("driver apps-platform\n")
            && text.contains("repo arugula-salad/planter-apps")
            && text.contains("path apps/app.yaml\n")
            && text.contains(&format!("at commit {sha};")),
        "{text}"
    );
}

/// M221: Arugula's own release, for the project the platform names only.
/// Another project can't make an environment for it; the one named can,
/// holding no secret (its tokens are minted); it plans only a full commit
/// of arugula-salad/arugula, and only where the App can write it.
#[tokio::test]
async fn an_arugula_release_is_planned_only_in_arugulas_project() {
    let (_hub, _app, base, t, _) = seeded().await;
    let envs = format!("/api/projects/{t}/environments");
    let release = |name: &str, secrets: &[&str], settings: Value| {
        Some(json!({ "name": name, "driver": "arugula-release", "secret_names": secrets, "settings": settings }))
    };
    let (st, v) = call(&base, "a", "POST", &envs, release("release", &[], json!({}))).await;
    assert_eq!(st, 403, "not the project the platform names: {v}");
    assert!(v.to_string().contains("only Arugula's own project"), "{v}");
    crate::drivers::arugula_release::TEST_PROJECTS.lock().unwrap().push(t.clone());
    let (st, v) = call(&base, "a", "POST", &envs, release("release2", &["RELEASE_TOKEN"], json!({}))).await;
    assert_eq!(st, 400, "the minted token's name: {v}");
    let (st, v) = call(&base, "a", "POST", &envs, release("release3", &[], json!({ "bump": "huge" }))).await;
    assert_eq!(st, 400, "{v}");
    let (st, env) = call(&base, "a", "POST", &envs, release("release", &[], json!({ "bump": "minor" }))).await;
    assert_eq!(st, 200, "{env}");
    assert_eq!(env["settings"], json!({ "bump": "minor", "lane": "daemon" }), "{env}");
    let env = env["id"].as_str().unwrap();
    let sha = "e".repeat(40);
    let plan = |sha: &str, place: &str| {
        Some(json!({ "environment": env, "source": { "kind": "commit", "ref": sha, "where": place } }))
    };
    let path = format!("/api/projects/{t}/deploys");
    for (body, why) in [
        (plan(&sha, "acme/arugula"), "arugula-salad/arugula only"),
        (plan("deadbeef", "arugula-salad/arugula"), "full 40-hex id"),
    ] {
        let (st, v) = call(&base, "a", "POST", &path, body).await;
        assert_eq!(st, 400, "{v}");
        assert!(v.to_string().contains(why), "{v}");
    }
    // Its run mints a token that writes arugula-salad/arugula, and this
    // platform has no GitHub App: refused when it's planned (the plan's
    // text is checked where it has one, in the apply's test).
    let (st, v) = call(&base, "a", "POST", &path, plan(&sha, "arugula-salad/arugula")).await;
    assert_eq!(st, 409, "{v}");
    assert!(v.to_string().contains("no GitHub App to mint RELEASE_TOKEN"), "{v}");
}

/// What the page makes an environment with (M252): the drivers the project
/// may name, each one's settings, and which of its secrets the platform
/// mints for each run (an environment never holds an `always` one). Its
/// members read it; arugula-release only where it's allowed.
#[tokio::test]
async fn the_drivers_a_project_may_name_say_their_settings_and_secrets() {
    let (_hub, _app, base, t, _) = seeded().await;
    let path = format!("/api/projects/{t}/drivers");
    let (st, _) = call(&base, "c", "GET", &path, None).await;
    assert_eq!(st, 404, "not in the team");
    let (st, v) = call(&base, "x", "GET", &path, None).await;
    assert_eq!(st, 200, "a viewer reads it: {v}");
    let names: Vec<&str> = v["drivers"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    // planter's (M233) only where PLANTER_PROJECTS names the project.
    assert_eq!(names, ["chant-fly", "apps-platform", "fake"], "{v}");
    let driver =
        |v: &Value, n: &str| v["drivers"].as_array().unwrap().iter().find(|d| d["name"] == n).cloned().unwrap();
    let fly = driver(&v, "chant-fly");
    assert_eq!(
        fly["secrets"],
        json!([{ "name": "FLY_API_TOKEN", "minted": null }, { "name": "GITHUB_TOKEN", "minted": "unless_held" }])
    );
    assert_eq!(
        fly["settings"][0],
        json!({ "name": "fly_app", "required": true, "default": null, "choices": [],
        "about": "The Fly app it deploys." })
    );
    assert_eq!(fly["rolls_back"], json!(true));
    assert_eq!(
        driver(&v, "apps-platform")["secrets"],
        json!([{ "name": "APPS_PLATFORM_TOKEN", "minted": "unless_held" }, { "name": "SOURCE_TOKEN", "minted": "always" }])
    );
    assert_eq!(driver(&v, "fake")["settings"], json!([]));

    crate::drivers::arugula_release::TEST_PROJECTS.lock().unwrap().push(t.clone());
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    let release = driver(&v, "arugula-release");
    assert_eq!(
        release["secrets"],
        json!([{ "name": "RELEASE_TOKEN", "minted": "always" }, { "name": "RELEASE_READ_TOKEN", "minted": "always" }])
    );
    assert_eq!(release["settings"][0]["choices"], json!(["patch", "minor", "major"]));
}

/// A deploy names its environment by id or (M252) by its name in the
/// project, from a person or an agent; another project's environment of
/// that name isn't found.
#[tokio::test]
async fn a_deploy_names_its_environment_by_name_too() {
    let (_hub, _app, base, t, u) = seeded().await;
    let env = production(&base, &t).await;
    let (st, v) = call(
        &base,
        "c",
        "POST",
        &format!("/api/projects/{u}/environments"),
        Some(json!({ "name": "staging", "driver": "fake", "secret_names": ["APP_TOKEN"] })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let path = format!("/api/projects/{t}/deploys");
    let (st, d) = call(&base, "b", "POST", &path, plan_body("production", "deadbeef")).await;
    assert_eq!((st, &d["environment"]["id"]), (200, &json!(env)), "{d}");
    assert_eq!(d["plan"], json!(driver_plan("deadbeef")), "the same plan as by its id");
    for name in ["staging", "nope", ""] {
        let (st, v) = call(&base, "b", "POST", &path, plan_body(name, "deadbeef")).await;
        assert_eq!(st, 404, "{name}: {v}");
    }
    let (st, g) =
        call(&base, "a", "POST", "/api/grants", give("geek", DEPLOYS_PLAN, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 200, "{g}");
    let (st, d) =
        geek(&base, "POST", &format!("/api/daemon/projects/{t}/deploys"), plan_body("production", "beef")).await;
    assert_eq!((st, &d["environment"]["id"], &d["agent"]["machine"]), (200, &json!(env), &json!("geek")), "{d}");
    assert!(d["plan_text"].as_str().unwrap().contains("fly_app acme-app"), "{d}");
}

/// A project's history (`GET /api/projects/{project}/deploys`): every
/// member reads it, a viewer too, and nobody else; newest first, by
/// environment when asked (by its name or id), a page at a time; each with
/// its state, source, roll-back mark, planner, and when it was planned,
/// decided and finished.
#[tokio::test]
async fn a_projects_history_lists_its_deploys_newest_first() {
    let (hub, _app, base, t, _) = seeded().await;
    devices(&hub, "a");
    let env = production(&base, &t).await;
    let (st, staging) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{t}/environments"),
        Some(json!({ "name": "staging", "driver": "fake", "secret_names": ["APP_TOKEN"] })),
    )
    .await;
    assert_eq!(st, 200, "{staging}");
    let staging = staging["id"].as_str().unwrap().to_owned();
    let tick = || tokio::time::sleep(std::time::Duration::from_millis(3));
    // Denied by a's phone, then one to staging and one still planned.
    let (denied, op) = asked(&base, "b", &t, &env, "deadbeef").await;
    let deny = Some(signed("a", &denied, &op, Decision::Deny));
    let (st, v) = call(&base, "a", "POST", &format!("/api/deploys/{denied}/deny"), deny).await;
    assert_eq!((st, &v["state"]), (200, &json!("denied")), "{v}");
    let plan = async |who: &str, env: &str, sha: &str| {
        tick().await;
        let (st, d) = call(&base, who, "POST", &format!("/api/projects/{t}/deploys"), plan_body(env, sha)).await;
        assert_eq!(st, 200, "{d}");
        d["id"].as_str().unwrap().to_owned()
    };
    let to_staging = plan("b", &staging, "beef").await;
    let planned = plan("a", &env, "cafe").await;

    let history = format!("/api/projects/{t}/deploys");
    let ids = |v: &Value| -> Vec<String> {
        v["deploys"].as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap().to_owned()).collect()
    };
    for who in ["a", "b", "x"] {
        let (st, v) = call(&base, who, "GET", &history, None).await;
        assert_eq!((st, ids(&v)), (200, vec![planned.clone(), to_staging.clone(), denied.clone()]), "{who}: {v}");
        assert_eq!(v["next"], Value::Null);
    }
    let (st, v) = call(&base, "c", "GET", &history, None).await;
    assert_eq!(st, 404, "c isn't in t: {v}");

    let (_, v) = call(&base, "x", "GET", &history, None).await;
    let d = &v["deploys"][2];
    assert_eq!(
        (&d["state"], &d["environment"]["name"], &d["source"]["ref"], &d["source"]["where"], &d["planned_by"]),
        (&json!("denied"), &json!("production"), &json!(full("deadbeef")), &json!("acme/app"), &json!("b")),
        "{d}"
    );
    assert_eq!((&d["roll_back"], &d["agent"]), (&Value::Null, &Value::Null));
    assert_eq!(d["planned_at"], d["created_at"]);
    assert!(d["decided_at"].as_u64().is_some() && d["decided_at"] == d["finished_at"], "{d}");
    let d = &v["deploys"][0];
    assert_eq!((&d["state"], &d["decided_at"], &d["finished_at"]), (&json!("planned"), &Value::Null, &Value::Null));

    // By environment, by its name or its id; one the project hasn't is 404.
    let (_, v) = call(&base, "x", "GET", &format!("{history}?environment=production"), None).await;
    assert_eq!(ids(&v), [planned.clone(), denied.clone()]);
    let (_, v) = call(&base, "x", "GET", &format!("{history}?environment={staging}"), None).await;
    assert_eq!(ids(&v), [to_staging.as_str()]);
    let (st, _) = call(&base, "x", "GET", &format!("{history}?environment=nope"), None).await;
    assert_eq!(st, 404);

    // A page at a time: `next` is the next page's `before`.
    let (_, v) = call(&base, "x", "GET", &format!("{history}?limit=2"), None).await;
    assert_eq!((ids(&v), &v["next"]), (vec![planned.clone(), to_staging.clone()], &json!(to_staging)));
    let (_, v) = call(&base, "x", "GET", &format!("{history}?limit=2&before={to_staging}"), None).await;
    assert_eq!((ids(&v), &v["next"]), (vec![denied.clone()], &Value::Null));
    let (st, _) = call(&base, "x", "GET", &format!("{history}?before=dep_nope"), None).await;
    assert_eq!(st, 400, "before is one of the project's deploys");
}
