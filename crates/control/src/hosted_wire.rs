//! Hosted machines (M178) over HTTP, as the owner's page, the platform, the
//! box and the people around it use them: delegating a project, enrolling
//! a box once by ticket or by sandbox id, teammates reaching it and nobody
//! else, revoking, and removing.

use std::time::Duration;

use arugula_control_wire::hosted::{EnrollAsk, EnrollGiven, Enrolled, Enrollment};
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Revocation, Trust,
    hosted::{Delegation, DelegationRevoke, HostedCert, HostedRefusal, Owners, accept, ends_listed, enroll_proof_body},
    now_ms,
    team::{AccountCerts, Roster, TeamRole},
};
use arugula_platform_api::HubPort;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::{
    auth::hash,
    db::Team,
    routing_wire::{Control, control, daemon, member, person, roster, session, v2},
};

const TEAM: &str = "00aa11bb22cc33dd";
pub(crate) const PROJECT: &str = "prj_0123456789abcdef0123456789abcdef";

pub(crate) fn delegation(c: &Control, owner: &str, by: &DeviceKeys) -> Delegation {
    let mut d = Delegation {
        project: PROJECT.into(),
        owner: owner.into(),
        key: c.app.hosted_key.public(),
        at: now_ms(),
        by: String::new(),
        sig: String::new(),
    };
    d.sign_with(by);
    d
}

pub(crate) fn ask(owner: &str, workspace: &str, sandbox: Option<&str>) -> EnrollAsk {
    EnrollAsk {
        project: PROJECT.into(),
        owner: owner.into(),
        workspace: workspace.into(),
        name: "hosted box".into(),
        sandbox: sandbox.map(Into::into),
        purpose: None,
    }
}

/// What the box sends, with the ticket or the sandbox id.
pub(crate) fn enroll_body(keys: &DeviceKeys, ticket: Option<&str>, sandbox: Option<&str>) -> Value {
    let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
    let at = now_ms();
    let credential = hash(ticket.or(sandbox).unwrap());
    let proof = hex::encode(keys.signature(enroll_proof_body(&credential, &noise, &sign, at).as_bytes()));
    json!({ "ticket": ticket, "sandbox": sandbox, "noise": noise, "sign": sign, "at": at, "proof": proof })
}

impl Control {
    pub(crate) async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let r = self.http.post(format!("{}{path}", self.base)).json(body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or_default())
    }

    async fn signed_get(&self, keys: &DeviceKeys, pq: &str) -> u16 {
        let r = self
            .http
            .get(format!("{}{pq}", self.base))
            .header("x-arugula-auth", v2(keys, "GET", pq, b""))
            .send()
            .await
            .unwrap();
        r.status().as_u16()
    }

    /// A person's relay socket to `id`: its upgrade's status.
    pub(crate) async fn reach(&self, cookie: &str, id: &str) -> u16 {
        let mut req =
            format!("{}/api/relay/c/{id}", self.base.replace("http://", "ws://")).into_client_request().unwrap();
        req.headers_mut().insert("cookie", cookie.parse().unwrap());
        req.headers_mut().insert("origin", self.base.parse().unwrap());
        match tokio_tungstenite::connect_async(req).await {
            Ok((_, r)) => r.status().as_u16(),
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) => r.status().as_u16(),
            Err(e) => panic!("{e}"),
        }
    }
}

impl Control {
    /// A machine's relay socket to `id` (M184): its upgrade's status.
    async fn reach_as_machine(&self, keys: &DeviceKeys, id: &str) -> u16 {
        let path = format!("/api/relay/c/{id}");
        let mut req = format!("{}{path}", self.base.replace("http://", "ws://")).into_client_request().unwrap();
        req.headers_mut().insert("x-arugula-auth", v2(keys, "GET", &path, b"").parse().unwrap());
        match tokio_tungstenite::connect_async(req).await {
            Ok((_, r)) => r.status().as_u16(),
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) => r.status().as_u16(),
            Err(e) => panic!("{e}"),
        }
    }
}

/// A dial of the relay as `arugulad` makes it, saying these direct URLs,
/// signed over the path and query as the daemon signs.
pub(crate) fn dial_saying(
    c: &Control,
    keys: &DeviceKeys,
    urls: &[&str],
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut url = url::Url::parse(&format!("{}/api/relay/dial", c.base.replace("http://", "ws://"))).unwrap();
    url.query_pairs_mut().append_pair("urls", &serde_json::to_string(urls).unwrap());
    let signed = format!("{}?{}", url.path(), url.query().unwrap());
    let mut req = url.as_str().into_client_request().unwrap();
    req.headers_mut().insert("x-arugula-auth", v2(keys, "GET", &signed, b"").parse().unwrap());
    req
}

/// Fran owns Acme, Cy is an editor in it, and Ox isn't.
async fn acme() -> (Control, DeviceKeys, DeviceKeys, DeviceKeys) {
    let c = control(|_| {}).await;
    let fran = person(&c.app, "fran", "f1");
    let cy = person(&c.app, "cy", "c1");
    let ox = person(&c.app, "ox", "x1");
    let r = roster(TEAM, 1, vec![member("f1", &fran, TeamRole::Owner), member("c1", &cy, TeamRole::Editor)], &fran);
    let t = Team { id: TEAM.into(), name: "Acme".into(), founder: "f1".into(), founder_root: fran.id(), locked: false };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    (c, fran, cy, ox)
}

#[tokio::test]
async fn an_owner_delegates_control_enrolls_a_box_and_only_the_project_reaches_it() {
    let (c, fran, cy, ox) = acme().await;
    let (fran_s, cy_s, ox_s) = (session(&c.app, "f1"), session(&c.app, "c1"), session(&c.app, "x1"));
    let owner = format!("team:{TEAM}");

    // No delegation: the platform can't make an enrollment.
    let given = c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_1", None)).await.unwrap();
    assert_eq!(given, EnrollGiven::NoDelegation);

    // Only an owner's own device delegates.
    for (cookie, keys) in [(&cy_s, &cy), (&ox_s, &ox), (&cy_s, &fran)] {
        let (st, _) = c
            .as_person(
                cookie,
                "POST",
                "/api/hosted/delegations",
                Some(json!({ "delegation": delegation(&c, &owner, keys) })),
            )
            .await;
        assert_eq!(st, 403);
    }
    let mut stale = delegation(&c, &owner, &fran);
    stale.key = hex::encode([7u8; 32]);
    stale.sign_with(&fran);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": stale }))).await;
    assert_eq!(st, 409, "a delegation to another key");
    let d = delegation(&c, &owner, &fran);
    let (st, v) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!((st, v["id"].as_str()), (200, Some(d.id().as_str())), "{v}");
    let (_, list) = c.as_person(&cy_s, "GET", "/api/hosted/delegations", None).await;
    assert_eq!(list["key"], json!(c.app.hosted_key.public()));
    assert_eq!(list["delegations"][0]["id"], json!(d.id()), "teammates see it");
    // The platform asks which delegation is live (M211), in process and
    // through its own view of the hub.
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), Some(d.id()));
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, "team:0000000000000000").await.unwrap(), None);
    let p = c.app.platform.inner();
    assert_eq!(p.hub.live_delegation(PROJECT, &owner).await.unwrap(), Some(d.id()));
    let (_, list) = c.as_person(&ox_s, "GET", "/api/hosted/delegations", None).await;
    assert_eq!(list["delegations"], json!([]));

    // The platform makes a box with a ticket; the box enrolls once.
    let EnrollGiven::Ready { ticket: Some(ticket), expires } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_1", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    assert!(expires > now_ms());
    let boxk = DeviceKeys::generate();
    let mut bad = enroll_body(&boxk, Some(&ticket), None);
    bad["proof"] = json!(hex::encode(DeviceKeys::generate().signature(b"x")));
    assert_eq!(c.post("/api/hosted/enroll", &bad).await.0, 401, "a proof by another key");
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&boxk, Some(&ticket), None)).await;
    assert_eq!(st, 200, "{e}");
    let e: Enrollment = serde_json::from_value(e).unwrap();
    assert_eq!((e.cert.device.as_str(), e.cert.workspace.as_str()), (boxk.id().as_str(), "hw_1"));
    assert_eq!(e.team.as_ref().unwrap().team, TEAM);

    // It signs its own routes, and nothing a daemon's signature opens.
    assert_eq!(c.signed_get(&boxk, "/api/hosted/trust").await, 200);
    assert_eq!(c.signed_get(&boxk, "/api/daemon/trust").await, 403);
    assert_eq!(c.signed_get(&boxk, "/api/directory").await, 403);
    assert_eq!(c.signed_get(&boxk, "/api/devices").await, 403);
    // It dials the relay. A stock box says no direct URLs; one that does is
    // reached only through the relay all the same, so control keeps none.
    let (_socket, _) = tokio_tungstenite::connect_async(dial_saying(&c, &boxk, &["http://10.0.0.5:7681"]))
        .await
        .expect("it dials the relay");
    for _ in 0..50 {
        if c.app.relay.online(&boxk.id()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(c.app.relay.online(&boxk.id()));

    // Cy's directory lists it, with no direct URLs, and Cy's client
    // accepts it for the project.
    let (_, dir) = c.as_person(&cy_s, "GET", "/api/directory", None).await;
    let entry = dir["daemons"].as_array().unwrap().iter().find(|d| d["id"] == json!(boxk.id())).cloned().unwrap();
    assert_eq!(entry["online"], json!(true));
    assert_eq!(entry["urls"], json!([]), "reached only through the relay: {entry}");
    // A row an older control left direct URLs in comes back without them.
    c.app.db.put_hosted_urls(&boxk.id(), &["http://10.0.0.5:7681".into()]).unwrap();
    let (_, dir) = c.as_person(&cy_s, "GET", "/api/directory", None).await;
    let again = dir["daemons"].as_array().unwrap().iter().find(|d| d["id"] == json!(boxk.id())).cloned().unwrap();
    assert_eq!(again["urls"], json!([]), "{again}");
    let (_, teams) = c.as_person(&cy_s, "GET", "/api/teams", None).await;
    let r: Roster = serde_json::from_value(teams["teams"][0]["roster"].clone()).unwrap();
    let certs = serde_json::from_value(teams["teams"][0]["certs"].clone()).unwrap();
    let hosted = &entry["hosted"];
    let cert = serde_json::from_value(hosted["cert"].clone()).unwrap();
    let del: Delegation = serde_json::from_value(hosted["delegation"].clone()).unwrap();
    let owners = Owners::Team { roster: &r, certs: &certs };
    assert_eq!(accept("c1", &cert, &del, &[], &[], &owners).unwrap().role, TeamRole::Editor);
    // Ox's isn't told of it, and refuses it if shown it.
    let (_, dir) = c.as_person(&ox_s, "GET", "/api/directory", None).await;
    assert!(!dir.to_string().contains(&boxk.id()), "{dir}");
    assert_eq!(accept("x1", &cert, &del, &[], &[], &owners), Err(HostedRefusal::NotInProject));
    // Nor is it anyone's own: not Fran's devices, nor her machines.
    let (_, devs) = c.as_person(&fran_s, "GET", "/api/devices", None).await;
    assert!(!devs.to_string().contains(&boxk.id()), "{devs}");

    // The relay routes teammates to it, and nobody else.
    assert_eq!(c.reach(&cy_s, &boxk.id()).await, 101);
    assert_eq!(c.reach(&fran_s, &boxk.id()).await, 101);
    assert_eq!(c.reach(&ox_s, &boxk.id()).await, 404);

    // Revoking stops the next enrollment, even one recorded before.
    let EnrollGiven::Ready { ticket: Some(early), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_2", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let spite = DelegationRevoke::new(&d, now_ms(), &cy);
    let (st, _) = c.as_person(&cy_s, "POST", "/api/hosted/delegations/revoke", Some(json!({ "revoke": spite }))).await;
    assert_eq!(st, 403, "an editor doesn't revoke");
    let r = DelegationRevoke::new(&d, now_ms(), &fran);
    let (st, v) = c.as_person(&fran_s, "POST", "/api/hosted/delegations/revoke", Some(json!({ "revoke": r }))).await;
    assert_eq!(st, 200, "{v}");
    let given = c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_3", None)).await.unwrap();
    assert_eq!(given, EnrollGiven::NoDelegation);
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None, "none live now");
    let (st, _) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), Some(&early), None)).await;
    assert_eq!(st, 403);
    // What enrolled before stays, still accepted with the revocation.
    let (_, dir) = c.as_person(&cy_s, "GET", "/api/directory", None).await;
    let entry = dir["daemons"].as_array().unwrap().iter().find(|d| d["id"] == json!(boxk.id())).cloned().unwrap();
    let revs: Vec<DelegationRevoke> = serde_json::from_value(entry["hosted"]["revocations"].clone()).unwrap();
    assert_eq!(revs.len(), 1);
    assert!(accept("c1", &cert, &del, &revs, &[], &owners).is_ok());

    // Removed, on control's word: hung up on, unlisted, and its key signs
    // nothing here again.
    let (st, _) = c.as_person(&cy_s, "DELETE", &format!("/api/hosted/machines/{}", boxk.id()), None).await;
    assert_eq!(st, 403, "an editor doesn't remove it");
    let (st, _) = c.as_person(&fran_s, "DELETE", &format!("/api/hosted/machines/{}", boxk.id()), None).await;
    assert_eq!(st, 200);
    assert!(!c.app.relay.online(&boxk.id()));
    assert_eq!(c.signed_get(&boxk, "/api/hosted/trust").await, 410);
    assert_eq!(c.reach(&cy_s, &boxk.id()).await, 404);
    let (st, _) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), Some(&ticket), None)).await;
    assert_eq!(st, 409, "a ticket works once");
}

#[tokio::test]
async fn a_snapshot_box_enrolls_once_by_its_sandbox_id() {
    let c = control(|_| {}).await;
    let me = person(&c.app, "me", "m1");
    person(&c.app, "you", "y1");
    let (me_s, you_s) = (session(&c.app, "m1"), session(&c.app, "y1"));
    let owner = "account:m1";
    let d = delegation(&c, owner, &me);
    let (st, _) = c.as_person(&you_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 403, "someone else's account");
    let (st, _) = c.as_person(&me_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);

    let sbx = "isb7fz2k1q0x9example";
    let given = c.app.hub_port.hosted_enrollment(&ask(owner, "hw_9", Some(sbx))).await.unwrap();
    assert!(matches!(given, EnrollGiven::Ready { ticket: None, .. }), "{given:?}");
    // Asked again for the same box (a lost answer): the same enrollment.
    // For another workspace: one enrollment per sandbox, ever.
    let again = c.app.hub_port.hosted_enrollment(&ask(owner, "hw_9", Some(sbx))).await.unwrap();
    assert_eq!(again, given);
    let other = c.app.hub_port.hosted_enrollment(&ask(owner, "hw_8", Some(sbx))).await.unwrap();
    assert_eq!(other, EnrollGiven::Taken);
    // Neither credential, both, or a wrong one: refused.
    let k = DeviceKeys::generate();
    let mut both = enroll_body(&k, Some("t"), None);
    both["sandbox"] = json!(sbx);
    assert_eq!(c.post("/api/hosted/enroll", &both).await.0, 400);
    assert_eq!(c.post("/api/hosted/enroll", &enroll_body(&k, None, Some("isbnotours"))).await.0, 404);
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&k, None, Some(sbx))).await;
    assert_eq!(st, 200, "{e}");
    let e: Enrollment = serde_json::from_value(e).unwrap();
    assert_eq!(e.trust.as_ref().map(|t| t.account.as_str()), Some("m1"));
    let ws = ["hw_9".to_owned()];
    let enrolled = c.app.hub_port.hosted_enrolled(&ws).await.unwrap();
    assert_eq!(enrolled, vec![Enrolled { workspace: "hw_9".into(), machine: k.id(), taken: false }]);
    // The same through the platform's own view of the hub (its timing
    // wrapper forwards each call).
    let p = c.app.platform.inner();
    assert_eq!(p.hub.hosted_enrolled(&ws).await.unwrap(), enrolled);

    // Mine to reach (routed: it isn't on the relay, so 503), and nobody
    // else's.
    assert_eq!(c.reach(&me_s, &k.id()).await, 503);
    assert_eq!(c.reach(&you_s, &k.id()).await, 404);
    let t = Trust { account: "m1".into(), root: me.id() };
    let (certs, revs) = (c.app.db.devices("m1").unwrap().0, c.app.db.revocations("m1").unwrap());
    let owners = Owners::Account { trust: &t, certs: &certs, revocations: &revs };
    assert!(accept("m1", &e.cert, &e.delegation, &[], &[], &owners).is_ok());
    assert_eq!(accept("y1", &e.cert, &e.delegation, &[], &[], &owners), Err(HostedRefusal::NotInProject));

    // Fail closed: a second box with the same sandbox id is refused, and
    // the machine that used it goes too; the platform hears it was taken.
    let (st, _) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), None, Some(sbx))).await;
    assert_eq!(st, 409);
    assert_eq!(c.signed_get(&k, "/api/hosted/trust").await, 410);
    let enrolled = c.app.hub_port.hosted_enrolled(&ws).await.unwrap();
    assert_eq!(enrolled, vec![Enrolled { workspace: "hw_9".into(), machine: String::new(), taken: true }]);
}

#[tokio::test]
async fn a_gone_box_takes_its_machine_and_unused_enrollment() {
    let c = control(|_| {}).await;
    let me = person(&c.app, "me", "m1");
    let me_s = session(&c.app, "m1");
    let d = delegation(&c, "account:m1", &me);
    c.as_person(&me_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let EnrollGiven::Ready { ticket: Some(t), .. } =
        c.app.hub_port.hosted_enrollment(&ask("account:m1", "hw_1", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let k = DeviceKeys::generate();
    assert_eq!(c.post("/api/hosted/enroll", &enroll_body(&k, Some(&t), None)).await.0, 200);
    // A rebuild: the old box is gone, and the new one's enrollment, made
    // before it, isn't used yet.
    let unused = c.app.hub_port.hosted_enrollment(&ask("account:m1", "hw_1", Some("isbnew"))).await.unwrap();
    assert!(matches!(unused, EnrollGiven::Ready { .. }));
    c.app.hub_port.hosted_gone(&["hw_1".to_owned()]).await.unwrap();
    assert_eq!(c.signed_get(&k, "/api/hosted/trust").await, 410);
    assert!(c.app.hub_port.hosted_enrolled(&["hw_1".to_owned()]).await.unwrap().is_empty());
    let (st, _) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), None, Some("isbnew"))).await;
    assert_eq!(st, 404, "its unused enrollment went with it");
    // The used ticket stays used.
    let (st, _) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), Some(&t), None)).await;
    assert_eq!(st, 409);
}

#[tokio::test]
async fn deleting_the_owner_takes_its_hosted_machines_and_delegations() {
    let c = control(|_| {}).await;
    let me = person(&c.app, "me", "m1");
    let me_s = session(&c.app, "m1");
    let d = delegation(&c, "account:m1", &me);
    c.as_person(&me_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let EnrollGiven::Ready { ticket: Some(t), .. } =
        c.app.hub_port.hosted_enrollment(&ask("account:m1", "hw_5", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let k = DeviceKeys::generate();
    assert_eq!(c.post("/api/hosted/enroll", &enroll_body(&k, Some(&t), None)).await.0, 200);
    let gone = c.app.db.delete_account(&crate::db::Erase { account: "m1", disband: &[], retain_for: &[] }).unwrap();
    assert!(gone.contains(&k.id()));
    assert!(c.app.db.hosted(&k.id()).unwrap().is_none());
    assert!(c.app.db.delegations_of(&["account:m1".into()]).unwrap().is_empty());
    assert!(c.app.db.mentions("m1").iter().all(|m| !m.starts_with("hosted") && !m.starts_with("project_delegations")));
}

/// M185: a box asks before an agent turn, signed as its hosted machine,
/// and the hub hands it to the platform for the workspace and machine its
/// certificate names (the platform's answer is its own tests'); nobody
/// else's signature gets there.
#[tokio::test]
async fn a_box_asks_the_platform_about_its_turns() {
    let (c, fran, cy, _ox) = acme().await;
    let fran_s = session(&c.app, "f1");
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    let EnrollGiven::Ready { ticket: Some(ticket), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_1", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let boxk = DeviceKeys::generate();
    assert_eq!(c.post("/api/hosted/enroll", &enroll_body(&boxk, Some(&ticket), None)).await.0, 200);

    let signed_post = |keys: &DeviceKeys, path: &str, body: &[u8]| {
        c.http
            .post(format!("{}{path}", c.base))
            .header("x-arugula-auth", v2(keys, "POST", path, body))
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
    };
    let body = br#"{"agent":"claude"}"#;
    let r = signed_post(&boxk, "/api/hosted/turns", body).await.unwrap();
    // This control's platform has no such workspace: it says so, through
    // the hub.
    assert_eq!(r.status(), 404);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["error"], "this machine isn't a live cloud workspace");
    let r = signed_post(&boxk, "/api/hosted/turns/ht_1/end", b"").await.unwrap();
    assert_eq!((r.status().as_u16(), r.json::<Value>().await.unwrap()["ended"].clone()), (200, json!(false)));

    // A person's device, and no signature at all, aren't a hosted machine.
    assert_eq!(signed_post(&cy, "/api/hosted/turns", body).await.unwrap().status(), 401);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/turns", Some(json!({ "agent": "claude" }))).await;
    assert_eq!(st, 401);
}

/// M184: a project person's own machine is routed to the project's hosted
/// machine while it holds a live cloud.drive grant, and not once its owner
/// revokes it; a machine outside the project never is.
#[tokio::test]
async fn a_machine_reaches_a_hosted_one_only_with_cloud_drive() {
    let (c, fran, _cy, _ox) = acme().await;
    let (fran_s, cy_s) = (session(&c.app, "f1"), session(&c.app, "c1"));
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    let EnrollGiven::Ready { ticket: Some(ticket), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_1", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let boxk = DeviceKeys::generate();
    assert_eq!(c.post("/api/hosted/enroll", &enroll_body(&boxk, Some(&ticket), None)).await.0, 200);
    let mut req = format!("{}/api/relay/dial", c.base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("x-arugula-auth", v2(&boxk, "GET", "/api/relay/dial", b"").parse().unwrap());
    let (_socket, _) = tokio_tungstenite::connect_async(req).await.expect("it dials the relay");
    for _ in 0..50 {
        if c.app.relay.online(&boxk.id()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let laptop = daemon(&c.app, "c1", "laptop");
    let elsewhere = daemon(&c.app, "x1", "elsewhere");
    assert_eq!(c.reach_as_machine(&laptop, &boxk.id()).await, 101, "Cy's machine, with its grant");
    assert_eq!(c.reach_as_machine(&elsewhere, &boxk.id()).await, 404, "not the project's");
    let (_, listed) = c.as_person(&cy_s, "GET", "/api/grants", None).await;
    let grant = listed["grants"].as_array().unwrap().iter().find(|g| g["subject"]["id"] == json!(laptop.id())).unwrap();
    let id = grant["id"].as_str().unwrap();
    let (st, _) = c.as_person(&cy_s, "POST", &format!("/api/grants/{id}/revoke"), Some(json!({}))).await;
    assert_eq!(st, 200);
    assert_eq!(c.reach_as_machine(&laptop, &boxk.id()).await, 403, "its grant revoked");
    // People are routed as before.
    assert_eq!(c.reach(&cy_s, &boxk.id()).await, 101);
}

/// M219: a deploy's box. The platform asks for its enrollment with
/// `purpose: deploy`; the box hears it in its enrollment, and its machine
/// is listed to nobody and routed to nobody, the project's owner included.
#[tokio::test]
async fn a_deploy_box_is_nobodys_to_reach() {
    let (c, fran, _cy, _ox) = acme().await;
    let fran_s = session(&c.app, "f1");
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    let mut a = ask(&owner, "hw_9", None);
    a.purpose = Some("deploy".into());
    let EnrollGiven::Ready { ticket: Some(ticket), .. } = c.app.hub_port.hosted_enrollment(&a).await.unwrap() else {
        panic!("no ticket")
    };
    let boxk = DeviceKeys::generate();
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&boxk, Some(&ticket), None)).await;
    assert_eq!(st, 200, "{e}");
    let e: Enrollment = serde_json::from_value(e).unwrap();
    assert_eq!(e.purpose.as_deref(), Some("deploy"), "the box hears what it's for");
    let mut req = format!("{}/api/relay/dial", c.base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("x-arugula-auth", v2(&boxk, "GET", "/api/relay/dial", b"").parse().unwrap());
    let (_socket, _) = tokio_tungstenite::connect_async(req).await.expect("it dials the relay");
    for _ in 0..50 {
        if c.app.relay.online(&boxk.id()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (_, dir) = c.as_person(&fran_s, "GET", "/api/directory", None).await;
    assert!(!dir.to_string().contains(&boxk.id()), "not listed, to its owner either: {dir}");
    assert_eq!(c.reach(&fran_s, &boxk.id()).await, 404, "nobody is routed to it");
    // An ordinary box beside it is reached as before.
    let EnrollGiven::Ready { ticket: Some(t2), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_10", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let other = DeviceKeys::generate();
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&other, Some(&t2), None)).await;
    assert_eq!(st, 200, "{e}");
    assert_eq!(serde_json::from_value::<Enrollment>(e).unwrap().purpose, None);
}

/// M263: a snapshot's builder is enrolled as a deploy's box is, with
/// `purpose: snapshot`: it hears it, and nobody is routed to it. Any other
/// purpose is refused.
#[tokio::test]
async fn a_snapshot_builder_is_nobodys_to_reach() {
    let (c, fran, _cy, _ox) = acme().await;
    let fran_s = session(&c.app, "f1");
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    let mut a = ask(&owner, "hw_9", None);
    a.purpose = Some("snapshot".into());
    let EnrollGiven::Ready { ticket: Some(ticket), .. } = c.app.hub_port.hosted_enrollment(&a).await.unwrap() else {
        panic!("no ticket")
    };
    let boxk = DeviceKeys::generate();
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&boxk, Some(&ticket), None)).await;
    assert_eq!(st, 200, "{e}");
    let e: Enrollment = serde_json::from_value(e).unwrap();
    assert_eq!(e.purpose.as_deref(), Some("snapshot"), "the box hears what it's for");
    assert_eq!(c.reach(&fran_s, &boxk.id()).await, 404, "nobody is routed to it");
    let mut odd = ask(&owner, "hw_10", None);
    odd.purpose = Some("anything".into());
    assert!(c.app.hub_port.hosted_enrollment(&odd).await.is_err());
}

/// Three owners and an editor, in one team.
async fn three_owners() -> (Control, DeviceKeys, DeviceKeys, DeviceKeys, DeviceKeys, Roster) {
    let c = control(|_| {}).await;
    let (fran, gus, hal, cy) = (
        person(&c.app, "fran", "f1"),
        person(&c.app, "gus", "g1"),
        person(&c.app, "hal", "h1"),
        person(&c.app, "cy", "c1"),
    );
    let r = roster(
        TEAM,
        1,
        vec![
            member("f1", &fran, TeamRole::Owner),
            member("g1", &gus, TeamRole::Owner),
            member("h1", &hal, TeamRole::Owner),
            member("c1", &cy, TeamRole::Editor),
        ],
        &fran,
    );
    let t = Team { id: TEAM.into(), name: "Acme".into(), founder: "f1".into(), founder_root: fran.id(), locked: false };
    c.app.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    (c, fran, gus, hal, cy, r)
}

/// Fran sends the next roster, with her passkey over it.
async fn fran_sends(c: &Control, r: &Roster) {
    let pk = crate::team_owners_wire::site_passkey(c, "f1", "pk-f1", 1);
    let body = crate::team_owners_wire::with_passkey(c, r, Some(("pk-f1", &pk)));
    let (st, v) = c.as_person(&session(&c.app, "f1"), "POST", &format!("/api/teams/{TEAM}/roster"), Some(body)).await;
    assert_eq!(st, 200, "{v}");
}

async fn send_revoke(c: &Control, account: &str, r: &DelegationRevoke) -> (u16, Value) {
    c.as_person(&session(&c.app, account), "POST", "/api/hosted/delegations/revoke", Some(json!({ "revoke": r }))).await
}

/// A revoke stays in force when its signer leaves the team, or is made an
/// editor: nothing more enrolls under the delegation, an enrollment recorded
/// before it included, and only a new delegation brings enrolling back.
#[tokio::test]
async fn a_revoke_stays_when_its_owner_leaves() {
    let (c, fran, gus, hal, cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let fran_s = session(&c.app, "f1");
    let d = delegation(&c, &owner, &fran);
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    let EnrollGiven::Ready { ticket: Some(early), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_early", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    // A box enrolled before the revoke: it stays.
    let EnrollGiven::Ready { ticket: Some(t), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_before", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let before = DeviceKeys::generate();
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&before, Some(&t), None)).await;
    assert_eq!(st, 200, "{e}");
    let before_e: Enrollment = serde_json::from_value(e).unwrap();
    assert!(before_e.closed.is_empty(), "nothing's ended yet");
    // Control's clock moves on past the certificate's time.
    tokio::time::sleep(Duration::from_millis(5)).await;

    let (st, v) = send_revoke(&c, "g1", &DelegationRevoke::new(&d, now_ms(), &gus)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None);

    // Gus leaves, an ordinary change by another owner.
    let (f, h, y) = (
        member("f1", &fran, TeamRole::Owner),
        member("h1", &hal, TeamRole::Owner),
        member("c1", &cy, TeamRole::Editor),
    );
    fran_sends(&c, &roster(TEAM, 2, vec![f.clone(), h.clone(), y.clone()], &fran)).await;
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None, "it stays revoked");
    let platform = c.app.platform.inner();
    assert_eq!(platform.hub.live_delegation(PROJECT, &owner).await.unwrap(), None, "the platform sees it so too");
    assert_eq!(
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_new", None)).await.unwrap(),
        EnrollGiven::NoDelegation
    );
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), Some(&early), None)).await;
    assert_eq!(st, 403, "an enrollment recorded before it stays refused: {e}");

    // People's clients hear control's word, and go by it: a certificate made
    // after the revoke (as a control that kept enrolling would make) is
    // refused, and the box from before stays.
    let (_, dir) = c.as_person(&session(&c.app, "c1"), "GET", "/api/directory", None).await;
    let entry = dir["daemons"].as_array().unwrap().iter().find(|m| m["id"] == json!(before.id())).cloned().unwrap();
    // As the CLI and a daemon read it.
    let (revs, closed) = ends_listed(&entry["hosted"]);
    assert_eq!(revs.len(), 1);
    assert_eq!(closed.len(), 1);
    assert!(closed[0].closes(&d));
    let (_, teams) = c.as_person(&session(&c.app, "c1"), "GET", "/api/teams", None).await;
    let now_roster: Roster = serde_json::from_value(teams["teams"][0]["roster"].clone()).unwrap();
    let certs: AccountCerts = serde_json::from_value(teams["teams"][0]["certs"].clone()).unwrap();
    let owners = Owners::Team { roster: &now_roster, certs: &certs };
    assert_eq!(accept("c1", &before_e.cert, &d, &revs, &closed, &owners).unwrap().role, TeamRole::Editor);
    let late = DeviceKeys::generate();
    let mut late_cert = HostedCert::new(
        &d,
        "hw_late",
        "late",
        &hex::encode(late.noise_public),
        &hex::encode(late.sign_public()),
        now_ms(),
    );
    c.app.hosted_key.sign(&mut late_cert);
    assert!(accept("c1", &late_cert, &d, &revs, &[], &owners).is_ok(), "gus isn't an owner now");
    assert_eq!(accept("c1", &late_cert, &d, &revs, &closed, &owners), Err(HostedRefusal::Revoked));
    // So does its box, which takes it with its refresh, and keeps it.
    let pq = "/api/hosted/trust";
    let r = c.http.get(format!("{}{pq}", c.base)).header("x-arugula-auth", v2(&before, "GET", pq, b"")).send().await;
    let e: Enrollment = r.unwrap().json().await.unwrap();
    assert_eq!(e.closed, closed);

    // Sending the same delegation again changes nothing.
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None, "the same one stays revoked");

    // A new delegation, which an owner signs on purpose, is the way back.
    let mut again = delegation(&c, &owner, &fran);
    again.at += 1;
    again.sign_with(&fran);
    assert_ne!(again.id(), d.id());
    let (st, _) = c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": again }))).await;
    assert_eq!(st, 200);
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), Some(again.id()));
}

/// The same when its signer is made an editor, not removed.
#[tokio::test]
async fn a_revoke_stays_when_its_owner_is_made_an_editor() {
    let (c, fran, gus, hal, cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    let fran_s = session(&c.app, "f1");
    c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let (st, v) = send_revoke(&c, "g1", &DelegationRevoke::new(&d, now_ms(), &gus)).await;
    assert_eq!(st, 200, "{v}");
    let members = vec![
        member("f1", &fran, TeamRole::Owner),
        member("g1", &gus, TeamRole::Editor),
        member("h1", &hal, TeamRole::Owner),
        member("c1", &cy, TeamRole::Editor),
    ];
    fran_sends(&c, &roster(TEAM, 2, members, &fran)).await;
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None);
}

/// The account's own project: a revoke from a device stays when that device
/// is taken off the account (a lost phone, say).
#[tokio::test]
async fn a_revoke_stays_when_its_device_is_removed() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "me", "m1");
    let phone = DeviceKeys::generate();
    let mut pc = Cert::new(&phone, "m1", Kind::Browser, "phone");
    pc.sign_with(&root);
    c.app.db.put_device(&pc, true, now_ms()).unwrap();
    let s = session(&c.app, "m1");
    let owner = "account:m1".to_owned();
    let d = delegation(&c, &owner, &root);
    let (st, _) = c.as_person(&s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    assert_eq!(st, 200);
    let EnrollGiven::Ready { ticket: Some(early), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_early", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let (st, v) = send_revoke(&c, "m1", &DelegationRevoke::new(&d, now_ms(), &phone)).await;
    assert_eq!(st, 200, "{v}");
    let gone = json!({ "revocation": Revocation::new("m1", &phone.id(), &root) });
    let (st, v) = c.as_person(&s, "POST", "/api/revocations", Some(gone)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None, "it stays revoked");
    assert_eq!(
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_new", None)).await.unwrap(),
        EnrollGiven::NoDelegation
    );
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), Some(&early), None)).await;
    assert_eq!(st, 403, "{e}");
    // A client that holds the account's chain, the phone off it, still
    // refuses a certificate made since.
    let (_, devs) = c.as_person(&s, "GET", "/api/devices", None).await;
    let certs: Vec<Cert> = serde_json::from_value(devs["certs"].clone()).unwrap();
    let removals: Vec<Revocation> = serde_json::from_value(devs["revocations"].clone()).unwrap();
    let t = Trust { account: "m1".into(), root: root.id() };
    let owners = Owners::Account { trust: &t, certs: &certs, revocations: &removals };
    let row = c.app.db.delegation(&d.id()).unwrap().unwrap();
    let closed = vec![c.app.hosted_key.close(&d, row.ended().unwrap())];
    let late = DeviceKeys::generate();
    let mut cert = HostedCert::new(
        &d,
        "hw_late",
        "late",
        &hex::encode(late.noise_public),
        &hex::encode(late.sign_public()),
        now_ms() + 1,
    );
    c.app.hosted_key.sign(&mut cert);
    assert!(accept("m1", &cert, &d, &row.revokes, &[], &owners).is_ok(), "the phone isn't the account's now");
    assert_eq!(accept("m1", &cert, &d, &row.revokes, &closed, &owners), Err(HostedRefusal::Revoked));
}

/// Every owner's revoke is kept, and the answer says what became of each.
#[tokio::test]
async fn every_owners_revoke_is_kept_and_answered() {
    let (c, fran, gus, hal, _cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let fran_s = session(&c.app, "f1");
    let d = delegation(&c, &owner, &fran);
    c.as_person(&fran_s, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let (g, h) = (DelegationRevoke::new(&d, now_ms(), &gus), DelegationRevoke::new(&d, now_ms(), &hal));
    let (st, v) = send_revoke(&c, "g1", &g).await;
    assert_eq!((st, &v), (200, &json!({ "first": true, "kept": true })));
    let (st, v) = send_revoke(&c, "h1", &h).await;
    assert_eq!((st, &v), (200, &json!({ "first": false, "kept": true })));
    // The same device again: it has its word in already.
    let (st, v) = send_revoke(&c, "h1", &DelegationRevoke::new(&d, now_ms() + 1, &hal)).await;
    assert_eq!((st, &v), (200, &json!({ "first": false, "kept": false })));

    let (_, list) = c.as_person(&fran_s, "GET", "/api/hosted/delegations", None).await;
    let row = &list["delegations"][0];
    assert_eq!(row["revoke"], json!(g), "the first, where a page from before reads it");
    assert_eq!(row["revokes"], json!([g, h]));
}

/// Someone who's out can't revoke: not as themselves, not through someone
/// else's session, and not by dating it before they left. The delegation
/// stays live after each.
#[tokio::test]
async fn someone_removed_cant_revoke() {
    let (c, fran, gus, hal, cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    c.as_person(&session(&c.app, "f1"), "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let phone = device_of(&c, &hal, "h1");
    // A revoke gus signed while he was an owner, and never sent.
    let written_before = DelegationRevoke::new(&d, now_ms(), &gus);
    let members = vec![
        member("f1", &fran, TeamRole::Owner),
        member("h1", &hal, TeamRole::Owner),
        member("c1", &cy, TeamRole::Editor),
    ];
    fran_sends(&c, &roster(TEAM, 2, members, &fran)).await;
    let gone = json!({ "revocation": Revocation::new("h1", &phone.id(), &hal) });
    let (st, v) = c.as_person(&session(&c.app, "h1"), "POST", "/api/revocations", Some(gone)).await;
    assert_eq!(st, 200, "{v}");

    let refused = [
        ("gus, as himself", "g1", written_before.clone(), 403),
        ("gus, now", "g1", DelegationRevoke::new(&d, now_ms(), &gus), 403),
        ("hal's session, gus's signature", "h1", written_before.clone(), 403),
        ("hal's removed phone", "h1", DelegationRevoke::new(&d, now_ms(), &phone), 403),
        ("an editor", "c1", DelegationRevoke::new(&d, now_ms(), &cy), 403),
        ("an owner, an hour ago", "h1", DelegationRevoke::new(&d, now_ms() - 3_600_000, &hal), 400),
    ];
    for (who, session_of, r, want) in refused {
        let (st, v) = send_revoke(&c, session_of, &r).await;
        assert_eq!(st, want, "{who}: {v}");
        assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), Some(d.id()), "{who}");
    }
    let (_, list) = c.as_person(&session(&c.app, "f1"), "GET", "/api/hosted/delegations", None).await;
    assert_eq!(list["delegations"][0]["revoke"], Value::Null);
    assert_eq!(list["delegations"][0]["revokes"], json!([]));
    assert!(c.app.db.delegation(&d.id()).unwrap().unwrap().ended().is_none());
}

/// A second browser of `of`'s account.
fn device_of(c: &Control, of: &DeviceKeys, account: &str) -> DeviceKeys {
    let k = DeviceKeys::generate();
    let mut cert = Cert::new(&k, account, Kind::Browser, "phone");
    cert.sign_with(of);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    k
}

/// A project has one live delegation: a second waits for the first to end.
/// The same one sent again is no second. One whose signer has left, or that
/// names another key, has ended by itself, so changing owners or control's key
/// needs no overlap. Someone else's for the same project id is no obstacle.
#[tokio::test]
async fn a_project_has_one_live_delegation() {
    let (c, fran, gus, hal, cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let post = |account: &'static str, d: Delegation| {
        let c = &c;
        async move {
            c.as_person(&session(&c.app, account), "POST", "/api/hosted/delegations", Some(json!({ "delegation": d })))
                .await
                .0
        }
    };
    let dated = |by: &DeviceKeys, ms: u64| {
        let mut d = delegation(&c, &owner, by);
        d.at += ms;
        d.sign_with(by);
        d
    };
    let d1 = dated(&fran, 0);
    assert_eq!(post("f1", d1.clone()).await, 200);
    assert_eq!(post("f1", d1.clone()).await, 200, "the same one again");
    assert_eq!(post("h1", dated(&hal, 1)).await, 409, "another owner's");
    assert_eq!(post("f1", dated(&fran, 2)).await, 409, "the same owner's");
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), Some(d1.id()));

    // Someone else's account, for the same project id: theirs, not this one's.
    let elsewhere = delegation(&c, "account:c1", &cy);
    assert_eq!(post("c1", elsewhere).await, 200);

    // Fran leaves: hers has ended by itself, so hal's is taken.
    let members = vec![
        member("f1", &fran, TeamRole::Editor),
        member("g1", &gus, TeamRole::Owner),
        member("h1", &hal, TeamRole::Owner),
        member("c1", &cy, TeamRole::Editor),
    ];
    fran_sends(&c, &roster(TEAM, 2, members, &fran)).await;
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None);
    let d2 = dated(&hal, 3);
    assert_eq!(post("h1", d2.clone()).await, 200);
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), Some(d2.id()));
}

/// A project that holds two live ones from before the rule of one: a revoke
/// of either ends both, and control says so of each.
#[tokio::test]
async fn a_revoke_ends_every_delegation_of_its_project() {
    let (c, fran, gus, _hal, _cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let d1 = delegation(&c, &owner, &fran);
    let mut d2 = delegation(&c, &owner, &gus);
    d2.at += 1;
    d2.sign_with(&gus);
    for (d, by) in [(&d1, "f1"), (&d2, "g1")] {
        assert!(c.app.db.add_delegation(d, by, now_ms(), |_| false).unwrap());
    }
    let EnrollGiven::Ready { ticket: Some(early), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_early", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let (st, v) = send_revoke(&c, "g1", &DelegationRevoke::new(&d2, now_ms(), &gus)).await;
    assert_eq!((st, &v), (200, &json!({ "first": true, "kept": true })));
    assert_eq!(c.app.hub_port.live_delegation(PROJECT, &owner).await.unwrap(), None);
    assert_eq!(c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_n", None)).await.unwrap(), EnrollGiven::NoDelegation);
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&DeviceKeys::generate(), Some(&early), None)).await;
    assert_eq!(st, 403, "{e}");
    let row = c.app.db.delegation(&d1.id()).unwrap().unwrap();
    assert!(row.ended().is_some() && row.revokes.is_empty());
    let (_, list) = c.as_person(&session(&c.app, "f1"), "GET", "/api/hosted/delegations", None).await;
    assert!(list["delegations"].as_array().unwrap().iter().all(|d| d["ended"] == json!(true)), "{list}");
    // Over: a new one is taken.
    let mut d3 = delegation(&c, &owner, &fran);
    d3.at += 2;
    d3.sign_with(&fran);
    let (st, _) =
        c.as_person(&session(&c.app, "f1"), "POST", "/api/hosted/delegations", Some(json!({ "delegation": d3 }))).await;
    assert_eq!(st, 200);
}

/// One delegation keeps so many revokes, and the answer says when it didn't.
#[tokio::test]
async fn a_delegations_revokes_are_bounded() {
    let (c, fran, gus, ..) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    assert!(c.app.db.add_delegation(&d, "f1", now_ms(), |_| false).unwrap());
    let first = DelegationRevoke::new(&d, now_ms(), &gus);
    assert!(c.app.db.revoke_delegation(&first, now_ms()).unwrap().first);
    for _ in 1..crate::db::hosted::MAX_REVOKES {
        let r = DelegationRevoke::new(&d, now_ms(), &DeviceKeys::generate());
        assert!(c.app.db.revoke_delegation(&r, now_ms()).unwrap().kept);
    }
    let over = DelegationRevoke::new(&d, now_ms(), &DeviceKeys::generate());
    let taken = c.app.db.revoke_delegation(&over, now_ms()).unwrap();
    assert!(!taken.first && !taken.kept);
    let row = c.app.db.delegation(&d.id()).unwrap().unwrap();
    assert_eq!(row.revokes.len(), crate::db::hosted::MAX_REVOKES);
    assert_eq!(row.revoke(), Some(&first));
}

/// Clients go by when a delegation ended, whenever the machine came to be
/// listed: one enrolled after that time is refused, and one from before stays.
#[tokio::test]
async fn a_machine_made_after_the_end_is_refused_and_one_from_before_stays() {
    let (c, fran, gus, hal, cy, _) = three_owners().await;
    let owner = format!("team:{TEAM}");
    let d = delegation(&c, &owner, &fran);
    c.as_person(&session(&c.app, "f1"), "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let EnrollGiven::Ready { ticket: Some(t), .. } =
        c.app.hub_port.hosted_enrollment(&ask(&owner, "hw_before", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let before = DeviceKeys::generate();
    let (st, e) = c.post("/api/hosted/enroll", &enroll_body(&before, Some(&t), None)).await;
    assert_eq!(st, 200, "{e}");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let (st, v) = send_revoke(&c, "g1", &DelegationRevoke::new(&d, now_ms(), &gus)).await;
    assert_eq!(st, 200, "{v}");
    let members = vec![
        member("f1", &fran, TeamRole::Owner),
        member("h1", &hal, TeamRole::Owner),
        member("c1", &cy, TeamRole::Editor),
    ];
    fran_sends(&c, &roster(TEAM, 2, members, &fran)).await;
    // In the table already: a machine enrolled under the delegation since.
    let late = DeviceKeys::generate();
    let mut cert = HostedCert::new(
        &d,
        "hw_late",
        "late",
        &hex::encode(late.noise_public),
        &hex::encode(late.sign_public()),
        now_ms(),
    );
    c.app.hosted_key.sign(&mut cert);
    c.app.db.add_hosted(&cert, now_ms()).unwrap();

    let (_, dir) = c.as_person(&session(&c.app, "c1"), "GET", "/api/directory", None).await;
    let (_, teams) = c.as_person(&session(&c.app, "c1"), "GET", "/api/teams", None).await;
    let held: Roster = serde_json::from_value(teams["teams"][0]["roster"].clone()).unwrap();
    let certs: AccountCerts = serde_json::from_value(teams["teams"][0]["certs"].clone()).unwrap();
    let owners = Owners::Team { roster: &held, certs: &certs };
    let answer = |id: String| {
        let entry = dir["daemons"].as_array().unwrap().iter().find(|m| m["id"] == json!(id)).cloned().unwrap();
        let (revs, closed) = ends_listed(&entry["hosted"]);
        let cert: HostedCert = serde_json::from_value(entry["hosted"]["cert"].clone()).unwrap();
        accept("c1", &cert, &d, &revs, &closed, &owners).map(|a| a.role)
    };
    assert_eq!(answer(before.id()), Ok(TeamRole::Editor));
    assert_eq!(answer(late.id()), Err(HostedRefusal::Revoked));
}
