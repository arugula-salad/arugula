//! Hosted machines (M178) over HTTP, as the owner's page, the platform, the
//! box and the people around it use them: delegating a project, enrolling
//! a box once by ticket or by sandbox id, teammates reaching it and nobody
//! else, revoking, and removing.

use std::time::Duration;

use arugula_control_wire::hosted::{EnrollAsk, EnrollGiven, Enrolled, Enrollment};
use arugula_e2e::{
    DeviceKeys, Trust,
    hosted::{Delegation, DelegationRevoke, HostedRefusal, Owners, accept, enroll_proof_body},
    now_ms,
    team::{Roster, TeamRole},
};
use arugula_platform::HubPort;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::{
    auth::hash,
    db::Team,
    routing_wire::{Control, control, member, person, roster, session, v2},
};

const TEAM: &str = "00aa11bb22cc33dd";
const PROJECT: &str = "prj_0123456789abcdef0123456789abcdef";

fn delegation(c: &Control, owner: &str, by: &DeviceKeys) -> Delegation {
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

fn ask(owner: &str, workspace: &str, sandbox: Option<&str>) -> EnrollAsk {
    EnrollAsk {
        project: PROJECT.into(),
        owner: owner.into(),
        workspace: workspace.into(),
        name: "hosted box".into(),
        sandbox: sandbox.map(Into::into),
    }
}

/// What the box sends, with the ticket or the sandbox id.
fn enroll_body(keys: &DeviceKeys, ticket: Option<&str>, sandbox: Option<&str>) -> Value {
    let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
    let at = now_ms();
    let credential = hash(ticket.or(sandbox).unwrap());
    let proof = hex::encode(keys.signature(enroll_proof_body(&credential, &noise, &sign, at).as_bytes()));
    json!({ "ticket": ticket, "sandbox": sandbox, "noise": noise, "sign": sign, "at": at, "proof": proof })
}

impl Control {
    async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
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
    async fn reach(&self, cookie: &str, id: &str) -> u16 {
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
    let crate::gateway::PlatformAt::InProcess(p) = &c.app.platform else { panic!("in process") };
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
    let mut req = format!("{}/api/relay/dial", c.base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("x-arugula-auth", v2(&boxk, "GET", "/api/relay/dial", b"").parse().unwrap());
    let (_socket, _) = tokio_tungstenite::connect_async(req).await.expect("it dials the relay");
    for _ in 0..50 {
        if c.app.relay.online(&boxk.id()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(c.app.relay.online(&boxk.id()));

    // Cy's directory lists it, and Cy's client accepts it for the project.
    let (_, dir) = c.as_person(&cy_s, "GET", "/api/directory", None).await;
    let entry = dir["daemons"].as_array().unwrap().iter().find(|d| d["id"] == json!(boxk.id())).cloned().unwrap();
    assert_eq!(entry["online"], json!(true));
    let (_, teams) = c.as_person(&cy_s, "GET", "/api/teams", None).await;
    let r: Roster = serde_json::from_value(teams["teams"][0]["roster"].clone()).unwrap();
    let certs = serde_json::from_value(teams["teams"][0]["certs"].clone()).unwrap();
    let hosted = &entry["hosted"];
    let cert = serde_json::from_value(hosted["cert"].clone()).unwrap();
    let del: Delegation = serde_json::from_value(hosted["delegation"].clone()).unwrap();
    let owners = Owners::Team { roster: &r, certs: &certs };
    assert_eq!(accept("c1", &cert, &del, &[], &owners).unwrap().role, TeamRole::Editor);
    // Ox's isn't told of it, and refuses it if shown it.
    let (_, dir) = c.as_person(&ox_s, "GET", "/api/directory", None).await;
    assert!(!dir.to_string().contains(&boxk.id()), "{dir}");
    assert_eq!(accept("x1", &cert, &del, &[], &owners), Err(HostedRefusal::NotInProject));
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
    assert!(accept("c1", &cert, &del, &revs, &owners).is_ok());

    // Removed as a device is: hung up on, and its key signs nothing again.
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
    let crate::gateway::PlatformAt::InProcess(p) = &c.app.platform else { panic!("in process") };
    assert_eq!(p.hub.hosted_enrolled(&ws).await.unwrap(), enrolled);

    // Mine to reach (routed: it isn't on the relay, so 503), and nobody
    // else's.
    assert_eq!(c.reach(&me_s, &k.id()).await, 503);
    assert_eq!(c.reach(&you_s, &k.id()).await, 404);
    let t = Trust { account: "m1".into(), root: me.id() };
    let (certs, revs) = (c.app.db.devices("m1").unwrap().0, c.app.db.revocations("m1").unwrap());
    let owners = Owners::Account { trust: &t, certs: &certs, revocations: &revs };
    assert!(accept("m1", &e.cert, &e.delegation, &[], &owners).is_ok());
    assert_eq!(accept("y1", &e.cert, &e.delegation, &[], &owners), Err(HostedRefusal::NotInProject));

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
