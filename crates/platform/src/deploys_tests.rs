//! Deploys and their approval through the platform's routes (M218): only a
//! project owner's device signs one in, an agent never does (on its owner's
//! own machine too, aru-091), always confirmed with a passkey the hub
//! checks, and a stale deploy is refused. The hub is chat's [`TestHub`],
//! which keeps each account's devices and passkeys as the test says.

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
    deploys::{AGENT, NOT_A_DEVICE, NOT_AN_OWNER, OTHER_OPERATION, PASSKEY, STALE},
    grants::AGENT_UNGRANTED,
    hub::DeviceTrust,
};

/// An account's devices: a phone (its root), a CLI on its machine, and a
/// browser it removed.
pub(crate) struct Devices {
    pub(crate) phone: DeviceKeys,
    cli: DeviceKeys,
    removed: DeviceKeys,
}

pub(crate) fn devices(hub: &TestHub, account: &str) -> Devices {
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
    Devices { phone, cli, removed }
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
pub(crate) async fn serve_with_cli(app: Arc<crate::Platform>) -> String {
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

pub(crate) fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn plan_body(env: &str, sha: &str) -> Option<Value> {
    Some(json!({ "environment": env, "source": { "kind": "commit", "ref": full(sha), "where": "acme/app" } }))
}

/// A short hex id as a full commit id (chant-fly deploys only those,
/// M220): padded with zeros to 40.
pub(crate) fn full(sha: &str) -> String {
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

/// `who`'s `keys` signing `decision` on deploy `d` whose operation hash is
/// `op`, good for an hour, confirmed with `who`'s passkey: the body the
/// approve and deny routes take.
pub(crate) fn signed(keys: &DeviceKeys, who: &str, d: &str, op: &str, decision: Decision) -> Value {
    signed_until(keys, who, d, op, decision, now_ms() + 3_600_000)
}

pub(crate) fn signed_until(keys: &DeviceKeys, who: &str, d: &str, op: &str, decision: Decision, expires: u64) -> Value {
    signed_with(keys, who, d, op, decision, expires, Some(&Pk { of: who, challenge: None, flags: 0x05 }))
}

fn signed_with(
    keys: &DeviceKeys,
    who: &str,
    d: &str,
    op: &str,
    decision: Decision,
    expires: u64,
    pk: Option<&Pk>,
) -> Value {
    let mut a = Approval {
        deploy: d.into(),
        op: op.into(),
        account: who.into(),
        decision,
        expires,
        by: keys.id(),
        sig: String::new(),
    };
    a.sig = hex::encode(keys.signature(a.body().as_bytes()));
    let mut v = json!({ "op": a.op, "expires": a.expires, "by": a.by, "sig": a.sig });
    if let Some(pk) = pk {
        v["passkey"] = assertion(&a.body(), pk);
    }
    v
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
pub(crate) async fn asked(base: &str, who: &str, t: &str, env: &str, sha: &str) -> (String, String) {
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

/// An editor plans and asks; the owner's phone approves, with a signature
/// over the deploy's operation that the log keeps and anyone can check.
/// The owners got a push saying where it's going and what. Nobody else
/// approves: an editor, a viewer, or someone outside the team.
#[tokio::test]
async fn an_owners_phone_approves_and_the_log_keeps_its_signature() {
    let (hub, _app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let b = devices(&hub, "b");
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

    for (who, keys, reason) in [("b", &b.phone, NOT_AN_OWNER), ("x", &a.phone, NOT_AN_OWNER)] {
        let (st, v) = call(
            &base,
            who,
            "POST",
            &format!("/api/deploys/{id}/approve"),
            Some(signed(keys, who, &id, &op, Decision::Once)),
        )
        .await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(reason)), "{who}: {v}");
    }

    let (st, d) = call(&base, "a", "GET", &format!("/api/deploys/{id}"), None).await;
    assert_eq!((st, &d["may_approve"]), (200, &json!(true)), "{d}");
    let (st, d) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{id}/approve"),
        Some(signed(&a.phone, "a", &id, &op, Decision::Once)),
    )
    .await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
    let approval = &d["approvals"][0];
    assert_eq!(
        (&approval["decision"], &approval["device_name"], &approval["device"]),
        (&json!("once"), &json!("Pixel"), &json!(a.phone.id()))
    );
    assert!(approval["id"].as_str().unwrap().starts_with("apv_"));
    assert_eq!(
        d["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect::<Vec<_>>(),
        ["planned", "requested", "approved"]
    );
    assert_eq!(d["log"], json!({ "verified": 3 }));

    // The log alone is enough to check it: the event's detail holds the
    // signed approval, which verifies with the phone's public key.
    let event = _app.db.deploy_history(&id).unwrap().pop().unwrap();
    let detail = event.detail_json().unwrap();
    let from_log = Approval {
        deploy: event.deploy.clone(),
        op: detail["op"].as_str().unwrap().into(),
        account: event.actor.strip_prefix("person ").unwrap().into(),
        decision: Decision::Once,
        expires: detail["expires"].as_u64().unwrap(),
        by: detail["device"].as_str().unwrap().into(),
        sig: detail["signature"].as_str().unwrap().into(),
    };
    assert!(from_log.signed_by(&hex::encode(a.phone.sign_public())));
    assert_eq!(event.actor, "person a");

    // Once is once: it isn't waiting any more.
    let (st, _) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{id}/approve"),
        Some(signed(&a.phone, "a", &id, &op, Decision::Once)),
    )
    .await;
    assert_eq!(st, 409);

    // And a deny is signed the same way.
    let (id2, op2) = asked(&base, "a", &t, &env, "cafef00d").await;
    let (st, d) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{id2}/deny"),
        Some(signed(&a.phone, "a", &id2, &op2, Decision::Deny)),
    )
    .await;
    assert_eq!((st, &d["state"], &d["approvals"][0]["decision"]), (200, &json!("denied"), &json!("deny")), "{d}");
}

/// Agents plan and ask, and never approve or deny: not a's agent on geek,
/// a's own machine, with a's phone's real signature in hand; not a's CLI
/// login; not a's CLI device's key. Neither does a removed device, a
/// signature over another deploy or another operation, or an expired one.
/// The deploy waits through all of it, then a's phone approves it.
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
        && m["body"].as_str().unwrap().contains("an agent on geek-box")));

    let good = signed(&a.phone, "a", &id, &op, Decision::Once);
    let approve = format!("/api/deploys/{id}/approve");
    let deny = format!("/api/deploys/{id}/deny");

    // The agent, with the owner's real signature: refused as an agent.
    for path in [&approve, &deny] {
        let (st, v) = geek(&base, "POST", path, Some(good.clone())).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT)), "{path}: {v}");
    }
    // There's no machine route that approves.
    for path in [format!("/api/daemon/deploys/{id}/approve"), format!("/api/daemon/deploys/{id}/deny")] {
        let (st, _) = geek(&base, "POST", &path, Some(good.clone())).await;
        assert!(st == 404 || st == 405, "{path}: {st}");
    }
    // a's CLI login (what an agent on geek could use), even with the phone's
    // signature: not a device signature.
    let cli = ("x-test-cli", &*format!("a {}", a.cli.id()));
    let (st, v) = as_header(&cli_base, cli, "POST", &approve, Some(good.clone())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "{v}");
    // The CLI device's own key (a file on geek), from a's browser session.
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&a.cli, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "a CLI's key: {v}");
    // A device a removed.
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&a.removed, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "a removed device: {v}");
    // A key nobody certified.
    let (st, v) =
        call(&base, "a", "POST", &approve, Some(signed(&DeviceKeys::generate(), "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "{v}");
    // The phone's signature over another deploy, or as a deny, or for
    // another account, doesn't verify here.
    for wrong in [
        signed(&a.phone, "a", "dep_other", &op, Decision::Once),
        signed(&a.phone, "a", &id, &op, Decision::Deny),
        signed(&a.phone, "b", &id, &op, Decision::Once),
    ] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(wrong)).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("bad_signature")), "{v}");
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
    let (st, v) =
        call(&base, "a", "POST", &approve, Some(signed(&a.phone, "a", &id, &other.hash(), Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(OTHER_OPERATION)), "{v}");
    // Expired, or good for too long.
    for expires in [now_ms() - 1, now_ms() + 8 * 24 * 3_600_000] {
        let (st, _) =
            call(&base, "a", "POST", &approve, Some(signed_until(&a.phone, "a", &id, &op, Decision::Once, expires)))
                .await;
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
/// a good signature from the owner's phone is refused as stale, and the
/// page isn't offered approval for it.
#[tokio::test]
async fn a_stale_deploy_is_refused_even_with_a_good_signature() {
    let (hub, app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let env = production(&base, &t).await;
    let (first, first_op) = asked(&base, "a", &t, &env, "aaaa1111").await;
    let (second, second_op) = asked(&base, "a", &t, &env, "bbbb2222").await;
    let (st, _) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{first}/approve"),
        Some(signed(&a.phone, "a", &first, &first_op, Decision::Once)),
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
        Some(signed(&a.phone, "a", &second, &second_op, Decision::Once)),
    )
    .await;
    assert_eq!((st, v["reason"].as_str()), (409, Some(STALE)), "{v}");
    assert!(app.db.deploy_approvals(&second).unwrap().is_empty());
    // And the waiting list no longer shows it.
    let (st, w) = call(&base, "a", "GET", "/api/deploys/waiting", None).await;
    assert_eq!((st, w["deploys"].as_array().map(Vec::len)), (200, Some(0)), "{w}");
}

/// Every approval and deny carries a passkey's assertion over its bytes,
/// which the hub checks: none, one over another challenge, another
/// account's passkey, or one without user verification are refused, even
/// with the owner's phone's real signature. What a browser's stored key
/// signs alone (an automated browser, its profile on disk) never counts.
/// The assertion is kept with the approval and in its event.
#[tokio::test]
async fn every_approval_needs_the_owners_passkey() {
    let (hub, app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    devices(&hub, "b");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let expires = now_ms() + 3_600_000;
    let approve = format!("/api/deploys/{id}/approve");
    let deny = format!("/api/deploys/{id}/deny");
    let pk = |of, challenge, flags| Pk { of, challenge, flags };
    for (why, body) in [
        ("no passkey", signed_with(&a.phone, "a", &id, &op, Decision::Once, expires, None)),
        (
            "another challenge",
            signed_with(&a.phone, "a", &id, &op, Decision::Once, expires, Some(&pk("a", Some("bm90IHRoaXM"), 0x05))),
        ),
        ("b's passkey", signed_with(&a.phone, "a", &id, &op, Decision::Once, expires, Some(&pk("b", None, 0x05)))),
        (
            "no user verification",
            signed_with(&a.phone, "a", &id, &op, Decision::Once, expires, Some(&pk("a", None, 0x01))),
        ),
    ] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(body)).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "{why}: {v}");
    }
    let (st, v) =
        call(&base, "a", "POST", &deny, Some(signed_with(&a.phone, "a", &id, &op, Decision::Deny, expires, None)))
            .await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(PASSKEY)), "a deny too: {v}");
    assert_eq!(state(&base, &id).await, json!("pending"));
    assert!(app.db.deploy_approvals(&id).unwrap().is_empty());
    assert!(hub.calls_of("verify_passkey") >= 3, "the hub checked them");

    let (st, d) = call(&base, "a", "POST", &approve, Some(signed(&a.phone, "a", &id, &op, Decision::Once))).await;
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

/// A device counts only if browsers certified it all the way to the
/// account's root: a browser certificate a CLI device signed (what an agent
/// holding the owner's CLI key could mint), a chain rooted in a CLI, the
/// desktop app's window or a browser it let in never approves, signature and
/// passkey good. A recovery code may be a link on the way, never the signer.
#[tokio::test]
async fn a_device_a_cli_certified_never_approves() {
    let (hub, _app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let approve = format!("/api/deploys/{id}/approve");
    let cert = |keys: &DeviceKeys, kind, by: &DeviceKeys| {
        let mut c = Cert::new(keys, "a", kind, "made up");
        c.sign_with(by);
        c
    };

    // A browser certificate a's CLI signed, under a's real (browser) root.
    let minted = DeviceKeys::generate();
    let mut root = Cert::new(&a.phone, "a", Kind::Browser, "Pixel");
    root.sign_with(&a.phone);
    let certs = vec![root.clone(), cert(&a.cli, Kind::Cli, &a.phone), cert(&minted, Kind::Browser, &a.cli)];
    hub.devices("a", DeviceTrust { root: a.phone.id(), certs, revocations: vec![], legacy_app_children: vec![] });
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&minted, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "a CLI's browser: {v}");

    // A chain whose root is a CLI.
    let cli_root = DeviceKeys::generate();
    let mut croot = Cert::new(&cli_root, "a", Kind::Cli, "the first CLI");
    croot.sign_with(&cli_root);
    let browser = DeviceKeys::generate();
    let certs = vec![croot, cert(&browser, Kind::Browser, &cli_root)];
    hub.devices("a", DeviceTrust { root: cli_root.id(), certs, revocations: vec![], legacy_app_children: vec![] });
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&browser, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "a CLI-rooted chain: {v}");
    assert!(v["error"].as_str().unwrap().contains("signing in with a passkey"), "says what to do: {v}");

    // The desktop app's window from before it had a kind ("Arugula app on
    // …", a browser certificate), and a browser it let in.
    let app_window = DeviceKeys::generate();
    let mut old_app = Cert::new(&app_window, "a", Kind::Browser, "Arugula app on jake-air");
    old_app.sign_with(&a.phone);
    let let_in = DeviceKeys::generate();
    let certs = vec![root.clone(), old_app, cert(&let_in, Kind::Browser, &app_window)];
    hub.devices("a", DeviceTrust { root: a.phone.id(), certs, revocations: vec![], legacy_app_children: vec![] });
    for (who, keys) in [("the app's window", &app_window), ("a browser the app let in", &let_in)] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(signed(keys, "a", &id, &op, Decision::Once))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "{who}: {v}");
    }
    assert_eq!(state(&base, &id).await, json!("pending"));

    // A browser a recovery code let in after every other was lost counts:
    // the code is the person's own, on paper, and only a browser made it.
    let code = DeviceKeys::generate();
    let recovered = DeviceKeys::generate();
    let certs = vec![root, cert(&code, Kind::Recovery, &a.phone), cert(&recovered, Kind::Browser, &code)];
    hub.devices("a", DeviceTrust { root: a.phone.id(), certs, revocations: vec![], legacy_app_children: vec![] });
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&code, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "a recovery code itself never signs: {v}");
    let (st, d) = call(&base, "a", "POST", &approve, Some(signed(&recovered, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "a recovered browser: {d}");
}

/// A browser the desktop app's window (from before it had a kind)
/// approved back then, as the hub says (`legacy_app_children`), approves:
/// the window counts just above it. So does a team owner whose role such
/// a browser gave. Not when the hub doesn't say so, and never the window
/// itself, nor what it certifies now.
#[tokio::test]
async fn a_browser_a_grandfathered_app_window_let_in_approves() {
    let (hub, _app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let b = devices(&hub, "b");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let approve = format!("/api/deploys/{id}/approve");
    let mut root = Cert::new(&a.phone, "a", Kind::Browser, "Pixel");
    root.sign_with(&a.phone);
    let window = DeviceKeys::generate();
    let mut old_app = Cert::new(&window, "a", Kind::Browser, "Arugula app on jake-air");
    old_app.sign_with(&a.phone);
    let firefox = DeviceKeys::generate();
    let mut ff = Cert::new(&firefox, "a", Kind::Browser, "Firefox on Mac");
    ff.sign_with(&window);
    let edge = DeviceKeys::generate();
    let mut ed = Cert::new(&edge, "a", Kind::Browser, "Edge");
    ed.sign_with(&window);
    let certs = vec![root, old_app, ff, ed];
    let trust = |legacy_app_children: Vec<String>| DeviceTrust {
        root: a.phone.id(),
        certs: certs.clone(),
        revocations: vec![],
        legacy_app_children,
    };

    // The hub doesn't say so (stored or approved after the cutoff).
    hub.devices("a", trust(vec![]));
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&firefox, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "not grandfathered: {v}");

    // It does: still never the window itself, nor what it certifies now.
    hub.devices("a", trust(vec![firefox.id()]));
    for (who, keys) in [("the window itself", &window), ("what it certifies now", &edge)] {
        let (st, v) = call(&base, "a", "POST", &approve, Some(signed(keys, "a", &id, &op, Decision::Once))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "{who}: {v}");
    }
    assert_eq!(state(&base, &id).await, json!("pending"));

    // A team's owner made by a browser b's window let in: b's roster
    // version 2 makes a an owner, signed by b's Firefox.
    let (ar, br) = (a.phone.id(), b.phone.id());
    let mut b_root = Cert::new(&b.phone, "b", Kind::Browser, "Pixel");
    b_root.sign_with(&b.phone);
    let b_window = DeviceKeys::generate();
    let mut b_app = Cert::new(&b_window, "b", Kind::Browser, "illogical app");
    b_app.sign_with(&b.phone);
    let b_firefox = DeviceKeys::generate();
    let mut b_ff = Cert::new(&b_firefox, "b", Kind::Browser, "Firefox");
    b_ff.sign_with(&b_window);
    let b_trust = |legacy_app_children: Vec<String>| DeviceTrust {
        root: br.clone(),
        certs: vec![b_root.clone(), b_app.clone(), b_ff.clone()],
        revocations: vec![],
        legacy_app_children,
    };
    let v1 = roster(1, &[("b", &br, TeamRole::Owner), ("a", &ar, TeamRole::Editor)], &b.phone);
    let both = [("b", br.as_str(), TeamRole::Owner), ("a", ar.as_str(), TeamRole::Owner)];
    hub.rosters("t", vec![v1, roster(2, &both, &b_firefox)]);
    hub.devices("b", b_trust(vec![]));
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&firefox, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_AN_OWNER)), "b's not grandfathered: {v}");
    hub.devices("b", b_trust(vec![b_firefox.id()]));
    let (st, d) = call(&base, "a", "POST", &approve, Some(signed(&firefox, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "both grandfathered: {d}");
}

/// root → window W → browser X → browser Y, X a child the hub lists: X
/// removed since doesn't take Y with it, so Y approves; Y removed doesn't.
#[tokio::test]
async fn a_browser_removed_on_the_way_keeps_what_it_approved() {
    let (hub, _app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let approve = format!("/api/deploys/{id}/approve");
    let mk = |keys: &DeviceKeys, name: &str, by: &DeviceKeys| {
        let mut c = Cert::new(keys, "a", Kind::Browser, name);
        c.created = now_ms() - 60_000;
        c.sign_with(by);
        c
    };
    let (w, x, y) = (DeviceKeys::generate(), DeviceKeys::generate(), DeviceKeys::generate());
    let certs = vec![
        mk(&a.phone, "Pixel", &a.phone),
        mk(&w, "illogical app on geek", &a.phone),
        mk(&x, "Firefox on Linux", &w),
        mk(&y, "Firefox on Mac", &x),
    ];
    let (x_gone, y_gone) = (Revocation::new("a", &x.id(), &a.phone), Revocation::new("a", &y.id(), &a.phone));
    let trust = |revocations: Vec<Revocation>| DeviceTrust {
        root: a.phone.id(),
        certs: certs.clone(),
        revocations,
        legacy_app_children: vec![x.id()],
    };
    hub.devices("a", trust(vec![x_gone.clone(), y_gone]));
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&y, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_A_DEVICE)), "Y removed: {v}");
    hub.devices("a", trust(vec![x_gone]));
    let (st, d) = call(&base, "a", "POST", &approve, Some(signed(&y, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "X removed after it approved Y: {d}");
}

/// A team's owner approves only if a browser made them an owner: the
/// roster version that made a an owner signed by b's CLI (or a's own CLI
/// founding the team) doesn't count, signature and passkey good; the same
/// version signed by b's phone does. A personal project's owner is its
/// account, with no roster.
#[tokio::test]
async fn an_owner_a_cli_made_never_approves() {
    let (hub, app, base, t, _) = seeded().await;
    let a = devices(&hub, "a");
    let b = devices(&hub, "b");
    let env = production(&base, &t).await;
    let (id, op) = asked(&base, "a", &t, &env, "deadbeef").await;
    let approve = format!("/api/deploys/{id}/approve");
    let (ar, br) = (a.phone.id(), b.phone.id());
    let v1 = roster(1, &[("b", &br, TeamRole::Owner), ("a", &ar, TeamRole::Editor)], &b.phone);
    let both = [("b", br.as_str(), TeamRole::Owner), ("a", ar.as_str(), TeamRole::Owner)];

    hub.rosters("t", vec![v1.clone(), roster(2, &both, &b.cli)]);
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&a.phone, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_AN_OWNER)), "made an owner by b's CLI: {v}");
    assert!(v["error"].as_str().unwrap().contains("given by a CLI's or the app's key"), "{v}");

    // a founded the team from its CLI.
    hub.rosters("t", vec![roster(1, &[("a", ar.as_str(), TeamRole::Owner)], &a.cli)]);
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&a.phone, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_AN_OWNER)), "founded from a CLI: {v}");

    // Signed by someone who wasn't an owner before it.
    hub.rosters("t", vec![v1.clone(), roster(2, &both, &a.phone)]);
    let (st, v) = call(&base, "a", "POST", &approve, Some(signed(&a.phone, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(NOT_AN_OWNER)), "a made itself an owner: {v}");
    assert_eq!(state(&base, &id).await, json!("pending"));

    hub.rosters("t", vec![v1, roster(2, &both, &b.phone)]);
    let (st, d) = call(&base, "a", "POST", &approve, Some(signed(&a.phone, "a", &id, &op, Decision::Once))).await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "made an owner by b's phone: {d}");

    // A personal project: its account is its owner, no roster asked.
    let mine = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    hub.rosters("t", vec![]);
    let env = production(&base, &mine).await;
    let (id, op) = asked(&base, "a", &mine, &env, "cafef00d").await;
    let (st, d) = call(
        &base,
        "a",
        "POST",
        &format!("/api/deploys/{id}/approve"),
        Some(signed(&a.phone, "a", &id, &op, Decision::Once)),
    )
    .await;
    assert_eq!((st, &d["state"]), (200, &json!("approved")), "{d}");
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
                &NewDeploy { environment: &env.id, source: &source, plan_digest: &digest("p"), roll_back: None },
                &Actor::person("a"),
                at,
            )
            .unwrap()
            .unwrap();
        db.record_deploy(&d.id, EventKind::Requested, &Actor::agent("a", "geek"), at, json!({})).unwrap().unwrap();
        d.id
    };
    let keys = DeviceKeys::generate();
    let approval = |id: &str, op: &Operation| {
        let a = Approval::new(&keys, id, op, "a", Decision::Once, now_ms() + 1000);
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

    let one = plan(1);
    let two = plan(2);
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
    assert_eq!(db.verify_deploy_log(&env.id).unwrap(), Ok(7));
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
    let a = devices(&hub, "a");
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
    let deny = Some(signed(&a.phone, "a", &denied, &op, Decision::Deny));
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
