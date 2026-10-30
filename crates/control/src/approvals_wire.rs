//! Approvals are judged now, over HTTP: a device the account no longer
//! trusts (removed, or a recovery code retired or used) approves nothing
//! new, whatever time its certificate claims; an approval is dated within
//! minutes of control's clock; and a removed device's sign-ins end with it.

use arugula_e2e::{Cert, DeviceKeys, Kind, Revocation, now_ms};
use serde_json::{Value, json};

use crate::{
    auth::{SESSION_COOKIE, hash},
    routing_wire::{Control, control, person, session},
};

/// An approved device of `kind` that `by` signed.
fn device(c: &Control, by: &DeviceKeys, kind: Kind, name: &str) -> DeviceKeys {
    let k = DeviceKeys::generate();
    let mut cert = Cert::new(&k, "a1", kind, name);
    cert.created = now_ms() - 5 * 60_000;
    cert.sign_with(by);
    c.app.db.put_device(&cert, true, now_ms()).unwrap();
    k
}

/// A session cookie that's `device`'s sign-in.
fn session_of(c: &Control, device: &DeviceKeys) -> String {
    let cookie = session(&c.app, "a1");
    let token = cookie.strip_prefix(&format!("{SESSION_COOKIE}=")).unwrap();
    c.app.db.set_session_device(&hash(token), "a1", &device.id()).unwrap();
    cookie
}

/// New keys of `kind`, as they ask, and `by`'s certificate for them at `at`.
fn ask(by: &DeviceKeys, kind: Kind, name: &str, at: u64) -> (Cert, Cert) {
    let k = DeviceKeys::generate();
    let ask = Cert::new(&k, "a1", kind, name);
    let mut cert = ask.clone();
    cert.created = at;
    cert.sign_with(by);
    (ask, cert)
}

impl Control {
    /// Enroll `cert` already signed: whether control took it as approved.
    async fn presigned(&self, cookie: &str, cert: &Cert) -> bool {
        let (st, v) = self.as_person(cookie, "POST", "/api/devices", Some(json!({ "cert": cert }))).await;
        assert_eq!(st, 200, "{v}");
        v["approved"] == json!(true)
    }

    /// `ask` waits, then `cert` approves it: the answer.
    async fn approve(&self, cookie: &str, ask: &Cert, cert: &Cert) -> (u16, Value) {
        let (st, v) = self.as_person(cookie, "POST", "/api/devices", Some(json!({ "cert": ask }))).await;
        assert_eq!((st, &v["approved"]), (200, &json!(false)), "{v}");
        self.as_person(cookie, "POST", &format!("/api/devices/{}/approve", ask.device), Some(json!({ "cert": cert })))
            .await
    }
}

/// A removed CLI's key approves nothing, even dated before it was removed;
/// its sign-in ends when it's removed, and others' don't.
#[tokio::test]
async fn a_removed_devices_key_approves_nothing_new() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let cli = device(&c, &root, Kind::Cli, "terminal");
    let theirs = session_of(&c, &cli);
    let mine = session_of(&c, &root);
    let before = now_ms() - 60_000;
    let gone = json!({ "revocation": Revocation::new("a1", &cli.id(), &root) });
    let (st, v) = c.as_person(&mine, "POST", "/api/revocations", Some(gone)).await;
    assert_eq!(st, 200, "{v}");
    let (st, _) = c.as_person(&theirs, "GET", "/api/me", None).await;
    assert_eq!(st, 401, "the removed device's sign-in ended");
    let (st, _) = c.as_person(&mine, "GET", "/api/me", None).await;
    assert_eq!(st, 200, "the browser's didn't");

    let cookie = session(&c.app, "a1");
    let (_, cert) = ask(&cli, Kind::Cli, "another terminal", before);
    assert!(!c.presigned(&cookie, &cert).await, "a removed key's presigned certificate waits");
    let (ask, cert) = ask(&cli, Kind::Cli, "a third terminal", before);
    let (st, v) = c.approve(&cookie, &ask, &cert).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("approver_untrusted")), "{v}");
}

/// An approval is dated within minutes of control's clock: one from an
/// hour ago, or an hour on, is refused, saying so; one dated now counts.
#[tokio::test]
async fn approvals_are_dated_now() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let cookie = session(&c.app, "a1");
    for at in [now_ms() - 3_600_000, now_ms() + 3_600_000] {
        let (_, cert) = ask(&root, Kind::Cli, "terminal", at);
        assert!(!c.presigned(&cookie, &cert).await);
        let (a, cert) = ask(&root, Kind::Browser, "phone", at);
        let (st, v) = c.approve(&cookie, &a, &cert).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some("clock")), "{v}");
    }
    let (_, cert) = ask(&root, Kind::Cli, "terminal", now_ms());
    assert!(c.presigned(&cookie, &cert).await);
    let (a, cert) = ask(&root, Kind::Browser, "phone", now_ms());
    let (st, v) = c.approve(&cookie, &a, &cert).await;
    assert_eq!(st, 200, "{v}");
}

/// A recovery code lets in one device; a retired one, none.
#[tokio::test]
async fn a_recovery_code_lets_in_one_device() {
    let c = control(|_| {}).await;
    let root = person(&c.app, "jake", "a1");
    let code = device(&c, &root, Kind::Recovery, "recovery 1");
    let cookie = session(&c.app, "a1");
    let (a, cert) = ask(&code, Kind::Browser, "new laptop", now_ms());
    let (st, v) = c.approve(&cookie, &a, &cert).await;
    assert_eq!(st, 200, "{v}");
    let (a, cert) = ask(&code, Kind::Browser, "another laptop", now_ms());
    let (st, v) = c.approve(&cookie, &a, &cert).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("recovery_spent")), "{v}");

    // A code retired before it let anyone in: none, even dated before.
    let old = device(&c, &root, Kind::Recovery, "recovery 2");
    c.app.db.add_revocation(&Revocation::new("a1", &old.id(), &root)).unwrap();
    let (a, cert) = ask(&old, Kind::Browser, "a third laptop", now_ms() - 60_000);
    let (st, v) = c.approve(&cookie, &a, &cert).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some("approver_untrusted")), "{v}");
}
