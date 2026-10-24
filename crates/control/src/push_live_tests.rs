//! Live push (`push_live.rs`) over HTTP: a live subscription's stream hears
//! what control would push its account, and only that account; under what
//! it chose; from control, the platform and its daemons alike; and ends
//! when the subscription goes.

use std::time::Duration;

use arugula_e2e::{DeviceKeys, now_ms, push::PushSub};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde_json::{Value, json};

use crate::routing_wire::{Control, control, daemon, person, session};

/// A live subscription's device end: its keys and its stream.
struct Listener {
    ua: p256::SecretKey,
    auth: [u8; 16],
    endpoint: String,
    listen: String,
}

/// Subscribe `account`'s device `keys` at a new live endpoint, as the page
/// does for the desktop app.
async fn subscribe(c: &Control, cookie: &str, account: &str, keys: &DeviceKeys, seed: u8) -> Listener {
    let ua = p256::SecretKey::from_slice(&[seed; 32]).unwrap();
    let auth = [seed; 16];
    let endpoint = format!("{}/api/push/live/{}", c.base, hex::encode([seed; 16]));
    let mut sub = PushSub {
        v: 1,
        account: account.into(),
        device: String::new(),
        endpoint: endpoint.clone(),
        p256dh: B64.encode(ua.public_key().to_sec1_bytes()),
        auth: B64.encode(auth),
        at: now_ms(),
        sig: String::new(),
    };
    sub.sign_with(keys);
    let (st, v) = c.as_person(cookie, "POST", "/api/push/subscribe", Some(json!({ "sub": sub }))).await;
    assert_eq!(st, 200, "{v}");
    let listen = v["listen"].as_str().expect("a live endpoint's listen token").to_owned();
    Listener { ua, auth, endpoint, listen }
}

/// An open stream.
struct Stream {
    res: reqwest::Response,
    buf: String,
}

impl Stream {
    async fn open(c: &Control, endpoint: &str, token: &str) -> Result<Self, u16> {
        let res = c.http.get(endpoint).bearer_auth(token).send().await.unwrap();
        if res.status() != 200 {
            return Err(res.status().as_u16());
        }
        assert_eq!(res.headers()["content-type"], "text/event-stream");
        Ok(Self { res, buf: String::new() })
    }

    /// The next event's data, or None when nothing comes soon (or it ended).
    async fn next(&mut self, wait: Duration) -> Option<String> {
        let until = tokio::time::Instant::now() + wait;
        loop {
            while let Some(i) = self.buf.find("\n\n") {
                let ev: String = self.buf.drain(..i + 2).collect();
                let data: Vec<&str> = ev.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect();
                if !data.is_empty() {
                    return Some(data.join("\n"));
                }
            }
            let chunk = tokio::time::timeout_at(until, self.res.chunk()).await.ok()?.ok()??;
            self.buf.push_str(&String::from_utf8_lossy(&chunk));
        }
    }

    async fn quiet(&mut self) -> bool {
        self.next(Duration::from_millis(400)).await.is_none()
    }

    /// The stream ended: the body is done.
    async fn ended(&mut self) -> bool {
        matches!(tokio::time::timeout(Duration::from_secs(5), self.res.chunk()).await, Ok(Ok(None)) | Ok(Err(_)))
    }
}

impl Listener {
    /// An event's notice, decrypted as a browser would (RFC 8291).
    fn open(&self, data: &str) -> Value {
        use aes_gcm::{Aes128Gcm, KeyInit, aead::Aead};
        use hkdf::Hkdf;
        use sha2::Sha256;

        let body = B64.decode(data).unwrap();
        let (salt, rest) = body.split_at(16);
        let id_len = rest[4] as usize;
        let (as_public, sealed) = rest[5..].split_at(id_len);
        let ua_public = self.ua.public_key().to_sec1_bytes();
        let shared = p256::ecdh::diffie_hellman(
            self.ua.to_nonzero_scalar(),
            p256::PublicKey::from_sec1_bytes(as_public).unwrap().as_affine(),
        );
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(&ua_public);
        info.extend_from_slice(as_public);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(&self.auth), shared.raw_secret_bytes().as_ref()).expand(&info, &mut ikm).unwrap();
        let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
        prk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let mut plain = Aes128Gcm::new_from_slice(&cek).unwrap().decrypt(&nonce.into(), sealed).unwrap();
        assert_eq!(plain.pop(), Some(2));
        serde_json::from_slice(&plain).unwrap()
    }
}

const SOON: Duration = Duration::from_secs(5);

#[tokio::test]
async fn a_live_stream_hears_its_own_account_alone() {
    use crate::push::push_account;
    use arugula_platform::hub::PushOptions;

    let c = control(|_| {}).await;
    let jake = person(&c.app, "jake", "a1jake");
    let ada = person(&c.app, "ada", "a2ada");
    let (jc, ac) = (session(&c.app, "a1jake"), session(&c.app, "a2ada"));
    let j = subscribe(&c, &jc, "a1jake", &jake, 1).await;
    let a = subscribe(&c, &ac, "a2ada", &ada, 2).await;

    // Only with its own token: not none, not another's.
    assert_eq!(Stream::open(&c, &j.endpoint, "").await.err(), Some(401));
    assert_eq!(Stream::open(&c, &j.endpoint, &a.listen).await.err(), Some(401));
    // Nor a subscription for someone else's account.
    let (st, _) = c
        .as_person(
            &ac,
            "POST",
            "/api/push/subscribe",
            Some(json!({ "sub": { "v": 1, "account": "a1jake", "device": ada.id(), "endpoint": format!("{}/api/push/live/{}", c.base, "ab".repeat(16)), "p256dh": "x", "auth": "y", "at": 1, "sig": "" } })),
        )
        .await;
    assert_eq!(st, 400);
    let mut js = Stream::open(&c, &j.endpoint, &j.listen).await.unwrap();
    let mut ads = Stream::open(&c, &a.endpoint, &a.listen).await.unwrap();

    // The platform's push to Jake: his stream, encrypted for his keys.
    let msg =
        json!({ "title": "Ada", "body": "hi", "tag": "chat-r1", "chat": { "room": "ch_0123456789abcdef", "seq": 3 } });
    let got = push_account(&c.app, "a1jake", msg.to_string().as_bytes(), PushOptions::default()).await.unwrap();
    assert_eq!(got.sent, 1);
    let n = j.open(&js.next(SOON).await.expect("Jake's stream hears it"));
    assert_eq!(n, msg);
    assert!(ads.quiet().await, "Ada's doesn't");

    // Control's own notice to Ada: hers alone.
    crate::push::notify(&c.app, vec!["a2ada".into()], "control-x", "Something waits".into(), "b".into());
    let n = a.open(&ads.next(SOON).await.expect("Ada's stream hears it"));
    assert_eq!((n["title"].as_str(), n["control"].as_bool()), (Some("Something waits"), Some(true)));
    assert!(js.quiet().await, "Jake's doesn't");

    // Ada taking Jake's endpoint off does nothing to his.
    let (st, _) = c.as_person(&ac, "POST", "/api/push/unsubscribe", Some(json!({ "endpoint": j.endpoint }))).await;
    assert_eq!(st, 200);
    push_account(&c.app, "a1jake", msg.to_string().as_bytes(), PushOptions::default()).await.unwrap();
    assert!(js.next(SOON).await.is_some(), "still Jake's, still open");

    // Jake taking it off: his stream ends, and can't open again.
    let (st, _) = c.as_person(&jc, "POST", "/api/push/unsubscribe", Some(json!({ "endpoint": j.endpoint }))).await;
    assert_eq!(st, 200);
    assert!(js.ended().await);
    assert_eq!(Stream::open(&c, &j.endpoint, &j.listen).await.err(), Some(404));
}

/// What Jake chose (`/api/push/prefs`) holds for his live stream as for a
/// push service: the platform's kinds, and what his machines relay, which
/// come encrypted by the daemon (control only passes them on).
#[tokio::test]
async fn a_live_stream_hears_what_was_chosen_from_machines_too() {
    use crate::push::push_account;
    use arugula_platform::hub::PushOptions;

    let c = control(|_| {}).await;
    let jake = person(&c.app, "jake", "a1jake");
    let jc = session(&c.app, "a1jake");
    let geek = daemon(&c.app, "a1jake", "geek");
    let j = subscribe(&c, &jc, "a1jake", &jake, 3).await;
    let mut js = Stream::open(&c, &j.endpoint, &j.listen).await.unwrap();

    // The daemon gets the live subscription like any other, and relays to
    // it what it encrypted itself.
    let subs = || async {
        let r = c
            .http
            .get(format!("{}{}", c.base, arugula_control_wire::PUSH_SUBS))
            .header("x-arugula-auth", crate::routing_wire::v2(&geek, "GET", arugula_control_wire::PUSH_SUBS, b""))
            .send()
            .await
            .unwrap();
        r.json::<Value>().await.unwrap()["subs"].as_array().unwrap().clone()
    };
    let got = subs().await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["endpoint"], j.endpoint);
    let relay = |payload: Value| {
        let s: PushSub = serde_json::from_value(got[0].clone()).unwrap();
        let body = arugula_e2e::push::encrypt(
            payload.to_string().as_bytes(),
            &B64.decode(&s.p256dh).unwrap(),
            &B64.decode(&s.auth).unwrap(),
            &arugula_e2e::push::ephemeral(),
            &arugula_e2e::random(),
        )
        .unwrap();
        let req = json!({ "endpoint": s.endpoint, "body": base64::engine::general_purpose::STANDARD.encode(body) });
        let c = &c;
        let geek = &geek;
        async move { c.daemon_post(geek, arugula_control_wire::PUSH, &req).await }
    };
    let pane = json!({ "title": "%3 needs you", "body": "Allow?", "pane": 3, "tag": "pane-3", "daemon": "d1" });
    let (st, v) = relay(pane.clone()).await;
    assert_eq!((st, v["status"].as_u64()), (200, Some(201)));
    assert_eq!(j.open(&js.next(SOON).await.expect("the daemon's notice")), pane);

    // Machines and projects off: held back, the account's notices still go.
    let (st, _) =
        c.as_person(&jc, "POST", "/api/push/prefs", Some(json!({ "machines": false, "projects": false }))).await;
    assert_eq!(st, 200);
    let (_, v) = relay(pane.clone()).await;
    assert_eq!(v["status"], 204, "a daemon that hasn't heard: held back");
    assert!(subs().await.is_empty(), "nor does it get the subscription now");
    let platform = |tag: &str| {
        let msg = json!({ "title": "t", "body": "b", "tag": tag }).to_string();
        let app = c.app.clone();
        async move { push_account(&app, "a1jake", msg.as_bytes(), PushOptions::default()).await.unwrap().sent }
    };
    assert_eq!(platform("inbox-i1").await, 0);
    assert!(js.quiet().await);
    assert_eq!(platform("agent-token-t1").await, 1, "about the account: always");
    assert_eq!(j.open(&js.next(SOON).await.unwrap())["tag"], "agent-token-t1");

    // A stream that drops and comes back gets what it missed, by its id.
    drop(js);
    assert_eq!(platform("chat-r2").await, 1);
    let res = c.http.get(&j.endpoint).bearer_auth(&j.listen).header("last-event-id", "0").send().await.unwrap();
    let mut back = Stream { res, buf: String::new() };
    let mut tags = Vec::new();
    while let Some(d) = back.next(Duration::from_millis(500)).await {
        tags.push(j.open(&d)["tag"].clone());
    }
    assert_eq!(tags, [json!("pane-3"), json!("agent-token-t1"), json!("chat-r2")], "everything since event 0");

    // Its device revoked: the open stream ends, and it can't listen again.
    c.app.db.add_revocation(&arugula_e2e::Revocation::new("a1jake", &jake.id(), &jake)).unwrap();
    crate::push::notify(&c.app, vec!["a1jake".into()], "control-y", "t".into(), "b".into());
    assert!(back.ended().await, "no more for a revoked device");
    assert_eq!(Stream::open(&c, &j.endpoint, &j.listen).await.err(), Some(404));
}
