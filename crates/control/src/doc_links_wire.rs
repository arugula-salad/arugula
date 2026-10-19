//! Shared Docs on control (#1064), over HTTP: what a signed test daemon
//! may put, commit and delete, what is refused before a part is stored,
//! and what the viewer's host answers (and doesn't).

use std::{net::SocketAddr, sync::Arc};

use arugula_e2e::{Cert, DeviceKeys, Kind, now_ms};
use axum::http::header;
use serde_json::{Value, json};

use crate::{App, auth::hash, db::doc_links::MAX_LINK};

/// A machine, for the database calls that take one.
const A1: crate::db::doc_links::Maker = crate::db::doc_links::Maker { account: "a1", daemon: "d1" };
const CONTROL: &str = "http://control.test";
const DOMAIN: &str = "blocks.test";
const DOCS: &str = "docs.blocks.test";
/// What decision 5 says the viewer is served under.
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src data:; \
                   connect-src 'self'; worker-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

struct Control {
    app: Arc<App>,
    base: String,
    http: reqwest::Client,
    _files: Files,
}

/// A stand-in web build, removed when the test ends.
struct Files(std::path::PathBuf);

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Control at `http://control.test` with blocks (and the viewer) under
/// `blocks.test`, and a stand-in web build with the viewer's page.
async fn control(tweak: impl FnOnce(&mut App)) -> Control {
    let files =
        Files(std::env::temp_dir().join(format!("arugula-doc-links-{}", hex::encode(arugula_e2e::random::<8>()))));
    std::fs::create_dir_all(files.0.join("assets")).unwrap();
    std::fs::write(files.0.join("index.html"), "<!doctype html><title>Arugula control</title>").unwrap();
    std::fs::write(files.0.join("doc.html"), "<!doctype html><title>the viewer</title>").unwrap();
    std::fs::write(files.0.join("assets/doc-abc.js"), "// the viewer").unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let mut app = App::for_tests(CONTROL);
    app.cfg.block_domain = Some(DOMAIN.into());
    app.cfg.static_dir = Some(files.0.to_owned());
    tweak(&mut app);
    let app = Arc::new(app);
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Control { app, base, http, _files: files }
}

fn person(app: &App, login: &str, id: &str) {
    app.db.account_for("github", &format!("gh-{id}"), login, id, now_ms()).unwrap();
    let keys = DeviceKeys::generate();
    let mut c = Cert::new(&keys, id, Kind::Browser, "laptop");
    c.sign_with(&keys);
    app.db.put_device(&c, true, now_ms()).unwrap();
}

/// A daemon joined to `account`.
fn daemon(app: &App, account: &str, name: &str) -> DeviceKeys {
    let keys = DeviceKeys::generate();
    let cert = Cert::new(&keys, account, Kind::Daemon, name);
    app.db.put_device(&cert, true, now_ms()).unwrap();
    app.db.put_daemon(account, &cert.device, name, &[]).unwrap();
    keys
}

/// A link id: 128 bits as 32 lower-case hex digits.
fn id(n: u32) -> String {
    format!("{n:032x}")
}

fn v2(keys: &DeviceKeys, method: &str, pq: &str, body: &[u8]) -> String {
    let ms = now_ms();
    let nonce = hex::encode(arugula_e2e::random::<16>());
    let msg = crate::auth::daemon_auth_message_v2(method, pq, ms, &nonce, body);
    format!("v2 {} {ms} {nonce} {}", keys.id(), hex::encode(keys.signature(msg.as_bytes())))
}

async fn answer(r: reqwest::Response) -> (u16, Value) {
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

impl Control {
    fn session(&self, account: &str) -> String {
        let t = crate::auth::token();
        self.app.db.add_session(&hash(&t), account, now_ms(), now_ms() + 3_600_000).unwrap();
        format!("{}={t}", crate::auth::SESSION_COOKIE)
    }

    /// Anyone's request, to `host`.
    fn at(&self, host: &str, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}{path}", self.base)).header(header::HOST, host)
    }

    /// A signed request of `keys`, as a machine makes it.
    fn signed(&self, keys: &DeviceKeys, method: &str, path: &str, body: Vec<u8>) -> reqwest::RequestBuilder {
        self.http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("x-arugula-auth", v2(keys, method, path, &body))
            .header(
                header::CONTENT_TYPE,
                if method == "POST" { "application/json" } else { "application/octet-stream" },
            )
            .body(body)
    }

    async fn put(&self, keys: &DeviceKeys, link: &str, ver: u64, n: u32, body: &[u8]) -> (u16, Value) {
        answer(
            self.signed(keys, "PUT", &format!("/api/doc-links/{link}/v/{ver}/parts/{n}"), body.to_vec())
                .send()
                .await
                .unwrap(),
        )
        .await
    }

    async fn commit(&self, keys: &DeviceKeys, link: &str, ver: u64, body: Value) -> (u16, Value) {
        let b = serde_json::to_vec(&body).unwrap();
        answer(self.signed(keys, "POST", &format!("/api/doc-links/{link}/v/{ver}/commit"), b).send().await.unwrap())
            .await
    }

    async fn unshare(&self, keys: &DeviceKeys, link: &str) -> (u16, Value) {
        answer(self.signed(keys, "DELETE", &format!("/api/doc-links/{link}"), vec![]).send().await.unwrap()).await
    }

    async fn quota(&self, keys: &DeviceKeys) -> (u16, Value) {
        answer(self.signed(keys, "GET", "/api/doc-links/quota", vec![]).send().await.unwrap()).await
    }

    async fn as_person(&self, cookie: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("cookie", cookie)
            .header("origin", CONTROL)
            .header(header::HOST, "control.test");
        if let Some(b) = body {
            req = req.json(&b);
        }
        answer(req.send().await.unwrap()).await
    }

    /// The viewer, on its own host.
    async fn view(&self, path: &str) -> reqwest::Response {
        self.at(DOCS, reqwest::Method::GET, path).send().await.unwrap()
    }

    /// A link with a served version made of these parts.
    async fn share(&self, keys: &DeviceKeys, link: &str, ver: u64, parts: &[&[u8]]) {
        for (n, p) in parts.iter().enumerate() {
            assert_eq!(self.put(keys, link, ver, n as u32, p).await.0, 200);
        }
        let size: usize = parts.iter().map(|p| p.len()).sum();
        let (st, v) = self.commit(keys, link, ver, json!({ "parts": parts.len(), "size": size })).await;
        assert_eq!(st, 200, "{v}");
    }
}

#[tokio::test]
async fn a_machine_puts_commits_replaces_and_unshares_and_anyone_gets_it() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    let link = id(1);

    // Nothing is served until the commit.
    c.put(&d, &link, 1, 0, b"first part").await;
    assert_eq!(c.view(&format!("/{link}/data/meta")).await.status(), 404);
    c.share(&d, &link, 1, &[b"first part", b"second", b"third"]).await;

    let meta: Value = c.view(&format!("/{link}/data/meta")).await.json().await.unwrap();
    assert_eq!(meta, json!({ "ver": 1, "parts": 3, "size": 10 + 6 + 5 }));
    for (n, part) in [b"first part".as_slice(), b"second", b"third"].into_iter().enumerate() {
        let r = c.view(&format!("/{link}/data/{n}")).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "application/octet-stream");
        assert_eq!(r.headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(r.headers()[header::ETAG], format!("\"{link}-1-{n}\"").as_str());
        assert_eq!(r.bytes().await.unwrap().as_ref(), part);
    }
    assert_eq!(c.view(&format!("/{link}/data/3")).await.status(), 404, "past the last part");
    assert_eq!(c.view(&format!("/{link}/data/01")).await.status(), 404, "a number is written one way");

    // The same link, a new version: the old parts go in the commit.
    c.put(&d, &link, 2, 0, b"new").await;
    assert_eq!(c.app.db.doc_part_keys(&link), vec![(1, 0), (1, 1), (1, 2), (2, 0)]);
    let (st, _) = c.commit(&d, &link, 2, json!({ "parts": 1, "size": 3 })).await;
    assert_eq!(st, 200);
    assert_eq!(c.app.db.doc_part_keys(&link), vec![(2, 0)]);
    let meta: Value = c.view(&format!("/{link}/data/meta")).await.json().await.unwrap();
    assert_eq!(meta, json!({ "ver": 2, "parts": 1, "size": 3 }));
    assert_eq!(c.view(&format!("/{link}/data/0")).await.bytes().await.unwrap().as_ref(), b"new");
    assert_eq!(c.view(&format!("/{link}/data/1")).await.status(), 404);

    // Unshare: gone at once, and the page still loads to say so.
    assert_eq!(c.unshare(&d, &link).await.0, 200);
    assert_eq!(c.view(&format!("/{link}/data/meta")).await.status(), 404);
    assert_eq!(c.view(&format!("/{link}/data/0")).await.status(), 404);
    assert_eq!(c.view(&format!("/{link}")).await.status(), 200);
    assert_eq!(c.app.db.doc_rows(), (0, 0));
    assert_eq!(c.unshare(&d, &link).await.0, 404, "twice");
}

#[tokio::test]
async fn a_commit_needs_exactly_its_parts_and_a_newer_version() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    let link = id(2);
    for n in 0..3 {
        c.put(&d, &link, 1, n, &[b'x'; 10]).await;
    }
    let commit = |parts: u32, size: u64| c.commit(&d, &link, 1, json!({ "parts": parts, "size": size }));

    // A part that never came.
    let (st, v) = commit(4, 30).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("incomplete")), "{v}");
    assert!(v["error"].as_str().unwrap().contains("part 3"), "{v}");
    // A part beyond the count (the size is right for all three, so only the count refuses).
    let (st, v) = commit(2, 30).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("incomplete")), "{v}");
    // A size that isn't the parts' sum.
    let (st, v) = commit(3, 31).await;
    assert_eq!((st, v["reason"].as_str()), (409, Some("incomplete")), "{v}");
    assert_eq!(c.view(&format!("/{link}/data/meta")).await.status(), 404, "none of them served it");

    assert_eq!(commit(3, 30).await.0, 200);
    // Not newer than what's served: the commit, and a part.
    assert_eq!(commit(3, 30).await.1["reason"], "stale");
    assert_eq!(c.put(&d, &link, 1, 0, b"again").await.1["reason"], "stale");
    assert_eq!(c.commit(&d, &link, 1, json!({ "parts": 3, "size": 30 })).await.0, 409);
    assert_eq!(c.put(&d, &link, 2, 0, b"fine").await.0, 200);
}

#[tokio::test]
async fn ids_versions_and_part_numbers_are_checked() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    let ok = id(3);
    for bad in [&ok[1..], &ok.to_uppercase().replace('0', "A"), &format!("{ok}0"), &ok.replace('0', "g")] {
        assert_eq!(c.put(&d, bad, 1, 0, b"x").await.0, 400, "{bad}");
        assert_eq!(c.unshare(&d, bad).await.0, 400, "{bad}");
    }
    assert_eq!(c.put(&d, &ok, 0, 0, b"x").await.0, 400, "version 0");
    assert_eq!(c.put(&d, &ok, 1, 48, b"x").await.0, 400, "part 48 of 0..47");
    assert_eq!(c.put(&d, &ok, 1, 47, b"x").await.0, 200);
    assert_eq!(c.put(&d, &ok, 1, 0, b"").await.0, 400, "an empty part");
    for (parts, ver) in [(0, 1), (49, 1), (1, 0)] {
        assert_eq!(c.commit(&d, &ok, ver, json!({ "parts": parts, "size": 1 })).await.0, 400, "{parts} {ver}");
    }
    assert_eq!(c.app.db.doc_rows(), (1, 1), "only the one good part");
}

#[tokio::test]
async fn a_put_is_refused_before_anything_is_stored() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");

    // The disk: over 80% full.
    *c.app.docs.fake_disk.lock().unwrap() = Some((81, 100));
    let (st, v) = c.put(&d, &id(4), 1, 0, b"x").await;
    assert_eq!((st, v["reason"].as_str()), (507, Some("disk")), "{v}");
    assert!(v["error"].as_str().unwrap().contains("disk"));
    assert_eq!(c.app.db.doc_rows(), (0, 0));
    *c.app.docs.fake_disk.lock().unwrap() = Some((80, 100));
    assert_eq!(c.put(&d, &id(4), 1, 0, b"x").await.0, 200, "80% is fine");
    *c.app.docs.fake_disk.lock().unwrap() = None;

    // A part over the signed body's 1 MB.
    let big = vec![7u8; (1 << 20) + 1];
    assert_eq!(c.put(&d, &id(5), 1, 0, &big).await.0, 413);
    assert_eq!(c.put(&d, &id(5), 1, 0, &big[1..]).await.0, 200, "1 MiB exactly");
    assert_eq!(c.app.db.doc_rows(), (2, 2));

    // A link's uncommitted bytes: 47 MiB waiting under version 1, then
    // another version of the same link.
    let (link, mib) = (id(6), vec![0u8; 1 << 20]);
    for n in 0..47 {
        c.app.db.seed_doc(&link, "a1", &d.id(), 0, 0, Some((1, n, &mib)));
    }
    assert_eq!(c.put(&d, &link, 2, 0, &mib).await.0, 200, "48 MiB is the cap");
    let before = c.app.db.doc_rows();
    let (st, v) = c.put(&d, &link, 2, 1, b"x").await;
    assert_eq!((st, v["reason"].as_str()), (413, Some("too_big")), "{v}");
    assert_eq!(c.app.db.doc_rows(), before, "nothing was stored");
    // Replacing a part counts the new one, not both.
    assert_eq!(c.put(&d, &link, 2, 0, &mib).await.0, 200);
}

#[tokio::test]
async fn an_accounts_bytes_and_links_are_held_before_the_put() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    let mib = vec![0u8; 1 << 20];

    // 199 MiB served, another account's bytes don't count.
    for (i, size) in [(10, 100u64 << 20), (11, 99 << 20)] {
        c.app.db.seed_doc(&id(i), "a1", &d.id(), 1, size, None);
    }
    c.app.db.seed_doc(&id(12), "b1", "other", 1, 150 << 20, None);
    assert_eq!(c.put(&d, &id(13), 1, 0, &mib).await.0, 200, "199 + 1 = 200 MiB");
    let before = c.app.db.doc_rows();
    let (st, v) = c.put(&d, &id(13), 1, 1, b"x").await;
    assert_eq!((st, v["reason"].as_str()), (413, Some("quota")), "{v}");
    assert_eq!(c.app.db.doc_rows(), before, "nothing was stored");
    assert!(v["error"].as_str().unwrap().contains("200 MB"));
    // Unsharing frees room.
    assert_eq!(c.unshare(&d, &id(11)).await.0, 200);
    assert_eq!(c.put(&d, &id(13), 1, 1, b"x").await.0, 200);

    // Half-uploaded links add up too: they're bytes control holds.
    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    c.app.db.seed_doc(&id(20), "a1", &d.id(), 0, 0, Some((1, 0, &vec![0u8; 150 << 20])));
    c.app.db.seed_doc(&id(21), "a1", &d.id(), 0, 0, Some((1, 0, &vec![0u8; 49 << 20])));
    assert_eq!(c.put(&d, &id(22), 1, 0, &mib).await.0, 200);
    assert_eq!(c.put(&d, &id(22), 1, 1, b"x").await.1["reason"], "quota");

    // 100 links, counting one still uploading.
    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    for i in 0..99 {
        c.app.db.seed_doc(&id(100 + i), "a1", &d.id(), 1, 1, None);
    }
    assert_eq!(c.put(&d, &id(300), 1, 0, b"x").await.0, 200, "the 100th");
    let (st, v) = c.put(&d, &id(301), 1, 0, b"x").await;
    assert_eq!((st, v["reason"].as_str()), (413, Some("links")), "{v}");
    assert_eq!(c.app.db.doc_rows().0, 100, "no row for the 101st");
    // More parts of a link it has are fine.
    assert_eq!(c.put(&d, &id(300), 1, 1, b"x").await.0, 200);
    let (_, q) = c.quota(&d).await;
    assert_eq!((q["links"].as_u64(), q["max_links"].as_u64()), (Some(100), Some(100)));
}

/// The commit checks again what the put checked: a part that got in
/// another way (an older build, a lower limit) can't be served.
#[tokio::test]
async fn a_commit_checks_the_limits_again() {
    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    let commit = |link: String, size: u64| {
        let (c, d) = (&c, &d);
        async move { c.commit(d, &link, 1, json!({ "parts": 1, "size": size })).await }
    };

    let big = vec![0u8; (MAX_LINK + 1) as usize];
    c.app.db.seed_doc(&id(30), "a1", &d.id(), 0, 0, Some((1, 0, &big)));
    let (st, v) = commit(id(30), big.len() as u64).await;
    assert_eq!((st, v["reason"].as_str()), (413, Some("too_big")), "{v}");

    c.app.db.seed_doc(&id(31), "a1", &d.id(), 1, 199 << 20, None);
    c.app.db.seed_doc(&id(32), "a1", &d.id(), 0, 0, Some((1, 0, &vec![0u8; 2 << 20])));
    let (st, v) = commit(id(32), 2 << 20).await;
    assert_eq!((st, v["reason"].as_str()), (413, Some("quota")), "{v}");

    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    for i in 0..100 {
        c.app.db.seed_doc(&id(200 + i), "a1", &d.id(), 1, 1, None);
    }
    c.app.db.seed_doc(&id(40), "a1", &d.id(), 0, 0, Some((1, 0, b"x")));
    let (st, v) = c.commit(&d, &id(40), 1, json!({ "parts": 1, "size": 1 })).await;
    assert_eq!((st, v["reason"].as_str()), (413, Some("links")), "{v}");
    // A link it has already may be updated, though the account is full.
    c.app.db.seed_doc(&id(200), "a1", &d.id(), 1, 1, Some((2, 0, b"y")));
    assert_eq!(c.commit(&d, &id(200), 2, json!({ "parts": 1, "size": 1 })).await.0, 200);
}

#[tokio::test]
async fn a_link_belongs_to_the_machine_that_made_it() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    person(&c.app, "bob", "b1");
    let (mine, other_box, bobs) =
        (daemon(&c.app, "a1", "mine"), daemon(&c.app, "a1", "other"), daemon(&c.app, "b1", "bob"));
    let link = id(50);
    c.put(&mine, &link, 1, 0, b"mine").await;

    let nowhere = c.put(&mine, &id(51), 1, 0, b"x").await; // a link that doesn't exist: any answer to compare
    assert_eq!(nowhere.0, 200);
    let gone = c.unshare(&mine, &id(99)).await;
    for who in [&other_box, &bobs] {
        // The same answer as for an id nobody has.
        assert_eq!(c.put(who, &link, 1, 0, b"theirs").await, gone);
        assert_eq!(c.put(who, &link, 2, 0, b"theirs").await, gone);
        assert_eq!(c.commit(who, &link, 1, json!({ "parts": 1, "size": 4 })).await, gone);
        assert_eq!(c.unshare(who, &link).await, gone);
    }
    assert_eq!(gone.0, 404);
    assert_eq!(c.app.db.doc_part_keys(&link), vec![(1, 0)]);
    assert_eq!(c.app.db.doc_part(&link, 1, 0).unwrap().unwrap(), b"mine", "not overwritten");
    let (st, _) = c.commit(&mine, &link, 1, json!({ "parts": 1, "size": 4 })).await;
    assert_eq!(st, 200);
    assert_eq!(c.view(&format!("/{link}/data/0")).await.bytes().await.unwrap().as_ref(), b"mine");

    // People, by their sessions: any link of their account, never another's.
    let (ann, bob) = (c.session("a1"), c.session("b1"));
    let (st, _) = c.as_person(&bob, "DELETE", &format!("/api/doc-links/{link}"), None).await;
    assert_eq!(st, 404);
    assert_eq!(c.view(&format!("/{link}/data/0")).await.status(), 200, "still shared");
    let (st, _) = c.as_person(&ann, "DELETE", &format!("/api/doc-links/{link}"), None).await;
    assert_eq!(st, 200, "ann's session, though her machine made it");
    assert_eq!(c.view(&format!("/{link}/data/0")).await.status(), 404);
    assert_eq!(c.as_person(&ann, "DELETE", "/api/doc-links/nope", None).await.0, 400);
    // A link isn't a machine's without a signature, and a session isn't one.
    let r = c.http.put(format!("{}/api/doc-links/{link}/v/3/parts/0", c.base)).body("x").send().await.unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(c.quota(&mine).await.0, 200);
    assert_eq!(c.as_person(&ann, "GET", "/api/doc-links/quota", None).await.0, 401);
}

#[tokio::test]
async fn people_list_their_accounts_links_and_the_quota_is_the_machines() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    person(&c.app, "bob", "b1");
    let (d, bobs) = (daemon(&c.app, "a1", "geek"), daemon(&c.app, "b1", "bob"));
    c.share(&d, &id(60), 1, &[b"abcd", b"efg"]).await;
    c.share(&bobs, &id(61), 1, &[b"bobs"]).await;
    c.put(&d, &id(62), 1, 0, b"not committed").await;

    let ann = c.session("a1");
    let (st, list) = c.as_person(&ann, "GET", "/api/doc-links", None).await;
    assert_eq!(st, 200);
    let l = list.as_array().unwrap();
    assert_eq!(l.len(), 1, "{list}: bob's and the unfinished one aren't listed");
    assert_eq!(
        (l[0]["id"].as_str(), l[0]["size"].as_u64(), l[0]["daemon"].as_str()),
        (Some(id(60).as_str()), Some(7), Some(d.id().as_str()))
    );
    assert_eq!(l[0]["opens"], 0);
    assert!(l[0]["made"].as_u64().unwrap() > 0 && l[0]["updated"].as_u64().unwrap() >= l[0]["made"].as_u64().unwrap());
    assert_eq!(c.as_person("", "GET", "/api/doc-links", None).await.0, 401);

    let (st, q) = c.quota(&d).await;
    assert_eq!(st, 200);
    assert_eq!(
        q,
        json!({ "used": 7, "limit": 200u64 << 20, "links": 2, "max_links": 100, "max_link": 48u64 << 20 }),
        "served bytes, and links counting the one uploading"
    );
}

#[tokio::test]
async fn the_docs_host_answers_nothing_but_the_viewer_and_sets_no_cookie() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    c.share(&d, &id(70), 1, &[b"x"]).await;
    let cookie = c.session("a1");

    // Control's own host does answer these, and clears the cookie.
    let (st, me) = c.as_person(&cookie, "GET", "/api/me", None).await;
    assert_eq!(st, 200, "{me}");
    let r = c
        .at("control.test", reqwest::Method::POST, "/auth/logout")
        .header("cookie", &cookie)
        .header("origin", CONTROL)
        .send()
        .await
        .unwrap();
    assert!(r.headers().contains_key(header::SET_COOKIE), "control's logout sets a cookie");
    let cookie = c.session("a1");

    let mut all = Vec::new();
    for (method, path) in [
        ("GET", "/api/me"),
        ("GET", "/api/doc-links"),
        ("GET", "/api/doc-links/quota"),
        ("DELETE", "/api/doc-links/70000000000000000000000000000000"),
        ("POST", "/auth/logout"),
        ("GET", "/control.json"),
        ("GET", "/"),
        ("GET", "/index.html"),
        ("GET", "/api/devices"),
        ("GET", &format!("/d/{}", id(70))),
        ("POST", &format!("/{}", id(70))),
        ("PUT", &format!("/{}/data/0", id(70))),
        ("DELETE", &format!("/{}", id(70))),
        ("GET", &format!("/{}/", id(70))),
        ("GET", &format!("/{}/data", id(70))),
        ("GET", &format!("/{}/data/0/1", id(70))),
        ("GET", "/assets"),
        ("GET", "/assets/"),
        ("GET", "/assets/.hidden"),
        ("GET", "/assets/nothing.js"),
        ("GET", "/.arugula/boot.js"),
    ] {
        let r = c
            .at(DOCS, method.parse().unwrap(), path)
            .header("cookie", &cookie)
            .header("origin", format!("http://{DOCS}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 404, "{method} {path}");
        all.push(r);
    }
    // A machine's signed call to a control route, on the docs host.
    let r = c.signed(&d, "GET", "/api/doc-links/quota", vec![]).header(header::HOST, DOCS).send().await.unwrap();
    assert_eq!(r.status(), 404);
    all.push(r);
    // With its port, and viewer answers too.
    for path in [
        format!("/{}", id(70)),
        format!("/{}/data/meta", id(70)),
        format!("/{}/data/0", id(70)),
        "/assets/doc-abc.js".into(),
    ] {
        let r =
            c.at(&format!("{DOCS}:8443"), reqwest::Method::GET, &path).header("cookie", &cookie).send().await.unwrap();
        assert_eq!(r.status(), 200, "{path}");
        all.push(r);
    }
    for r in &all {
        assert!(!r.headers().contains_key(header::SET_COOKIE), "{}", r.url());
        assert_eq!(r.headers()[header::CONTENT_SECURITY_POLICY], CSP, "{}", r.url());
    }
    // The session still works afterwards: nothing on the docs host ended it.
    assert_eq!(c.as_person(&cookie, "GET", "/api/me", None).await.0, 200);
    // Another label under the domain isn't the viewer.
    for host in [format!("x.{DOCS}"), "docsx.blocks.test".to_owned(), DOMAIN.to_owned()] {
        let r = c.at(&host, reqwest::Method::GET, &format!("/{}", id(70))).send().await.unwrap();
        assert_eq!(r.status(), 404, "{host}");
    }
}

#[tokio::test]
async fn the_viewer_has_its_headers_its_page_its_files_and_etags() {
    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    let link = id(80);
    c.share(&d, &link, 1, &[b"one", b"two"]).await;

    // Every kind of answer, shared or not.
    for path in [
        format!("/{link}"),
        format!("/{}", id(81)),
        format!("/{link}/data/meta"),
        format!("/{link}/data/1"),
        format!("/{}/data/0", id(81)),
        "/assets/doc-abc.js".into(),
        "/nothing".into(),
    ] {
        let r = c.view(&path).await;
        let h = r.headers();
        assert_eq!(h[header::CONTENT_SECURITY_POLICY], CSP, "{path}");
        assert_eq!(h[header::REFERRER_POLICY], "no-referrer", "{path}");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{path}");
        assert_eq!(h[header::X_FRAME_OPTIONS], "DENY", "{path}");
    }
    // The page loads for any id (it says "isn't shared" itself); the build's.
    for l in [&link, &id(81)] {
        let r = c.view(&format!("/{l}")).await;
        assert_eq!(
            (r.status().as_u16(), r.headers()[header::CONTENT_TYPE].to_str().unwrap()),
            (200, "text/html; charset=utf-8")
        );
        assert!(r.text().await.unwrap().contains("the viewer"));
    }
    let r = c.view("/assets/doc-abc.js").await;
    assert_eq!(r.text().await.unwrap(), "// the viewer");

    // Without a build there is still a page.
    let bare = control(|a| a.cfg.static_dir = Some(std::env::temp_dir().join("arugula-no-such-dir"))).await;
    let r = bare.view(&format!("/{link}")).await;
    assert_eq!(r.status(), 200);
    assert!(r.text().await.unwrap().contains("A shared Doc"));

    // A second GET of the same version is a 304; a new version isn't.
    for what in ["0", "1", "meta"] {
        let path = format!("/{link}/data/{what}");
        let first = c.view(&path).await;
        let tag = first.headers()[header::ETAG].to_str().unwrap().to_owned();
        assert_eq!(tag, format!("\"{link}-1-{}\"", if what == "meta" { "meta" } else { what }));
        for sent in [tag.clone(), format!("W/{tag}"), format!("\"other\", {tag}"), "*".into()] {
            let r = c.at(DOCS, reqwest::Method::GET, &path).header(header::IF_NONE_MATCH, &sent).send().await.unwrap();
            assert_eq!(r.status(), 304, "{path} {sent}");
            assert_eq!(r.headers()[header::ETAG], tag.as_str());
            assert_eq!(r.headers()[header::CACHE_CONTROL], "no-cache");
            assert_eq!(r.headers()[header::CONTENT_SECURITY_POLICY], CSP);
            assert!(r.bytes().await.unwrap().is_empty());
        }
        let r = c.at(DOCS, reqwest::Method::GET, &path).header(header::IF_NONE_MATCH, "\"nope\"").send().await.unwrap();
        assert_eq!(r.status(), 200, "{path}: another tag");
    }
    c.share(&d, &link, 2, &[b"new one", b"new two"]).await;
    let r = c
        .at(DOCS, reqwest::Method::GET, &format!("/{link}/data/0"))
        .header(header::IF_NONE_MATCH, format!("\"{link}-1-0\""))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "a new version isn't fresh");
    assert_eq!(r.headers()[header::ETAG], format!("\"{link}-2-0\"").as_str());
}

#[tokio::test]
async fn viewing_is_limited_per_address_and_counted_per_open() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    let link = id(90);
    c.share(&d, &link, 1, &[b"x"]).await;
    let ann = c.session("a1");
    let opens = || async { c.as_person(&ann, "GET", "/api/doc-links", None).await.1[0]["opens"].as_u64().unwrap() };

    // A page open is the meta, not each part; a 304 of it is an open too.
    c.view(&format!("/{link}/data/meta")).await;
    c.view(&format!("/{link}/data/0")).await;
    c.view(&format!("/{link}/data/0")).await;
    c.at(DOCS, reqwest::Method::GET, &format!("/{link}/data/meta"))
        .header(header::IF_NONE_MATCH, "*")
        .send()
        .await
        .unwrap();
    c.view(&format!("/{}/data/meta", id(91))).await;
    assert_eq!(opens().await, 2);
    // Flushed to the database, and not counted twice after.
    c.app.docs.flush(&c.app.db);
    assert_eq!(c.app.db.doc_links("a1").unwrap()[0].opens, 2);
    assert_eq!(opens().await, 2);
    c.view(&format!("/{link}/data/meta")).await;
    assert_eq!(opens().await, 3);
    c.app.docs.flush(&c.app.db);
    c.app.docs.flush(&c.app.db);
    assert_eq!(c.app.db.doc_links("a1").unwrap()[0].opens, 3);

    // The address that asks too much is told to wait; the page isn't data.
    let ip = "127.0.0.1".parse().unwrap();
    for _ in 0..crate::doc_links::VIEWS.1 {
        let _ = c.app.limits.check(crate::doc_links::VIEWS, ip);
    }
    assert_eq!(c.view(&format!("/{link}/data/0")).await.status(), 429);
    assert_eq!(c.view(&format!("/{link}/data/meta")).await.status(), 429);
    assert_eq!(c.view(&format!("/{link}")).await.status(), 200);
}

#[tokio::test]
async fn control_says_where_docs_are_viewed() {
    let c = control(|_| {}).await;
    let j: Value =
        c.at("control.test", reqwest::Method::GET, "/control.json").send().await.unwrap().json().await.unwrap();
    assert_eq!((j["doc_links"].as_u64(), j["doc_viewer"].as_str()), (Some(1), Some("http://docs.blocks.test")));
    let off = control(|a| a.cfg.block_domain = None).await;
    let j: Value =
        off.at("control.test", reqwest::Method::GET, "/control.json").send().await.unwrap().json().await.unwrap();
    assert_eq!((j["doc_links"].as_u64(), j["doc_viewer"].as_str()), (Some(1), Some("http://control.test/d")));

    use crate::doc_links::viewer_url;
    assert_eq!(viewer_url("https://control.arugula.run", Some("arugula.run")), "https://docs.arugula.run");
    assert_eq!(viewer_url("https://c.example:8443", Some("arugula.run")), "https://docs.arugula.run:8443");
    assert_eq!(viewer_url("http://127.0.0.1:7690", None), "http://127.0.0.1:7690/d");
}

#[tokio::test]
async fn with_no_block_domain_the_viewer_is_under_d_on_controls_origin() {
    let off = control(|a| a.cfg.block_domain = None).await;
    let d = daemon(&off.app, "a1", "geek");
    let link = id(110);
    off.share(&d, &link, 1, &[b"abc"]).await;
    for host in ["control.test", "elsewhere.example"] {
        let get = |p: String| off.at(host, reqwest::Method::GET, &p).send();
        let page = get(format!("/d/{link}")).await.unwrap();
        assert_eq!(page.status(), 200);
        assert_eq!(page.headers()[header::CONTENT_SECURITY_POLICY], CSP, "not control's own");
        assert_eq!(page.headers()[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(page.headers()[header::X_FRAME_OPTIONS], "DENY");
        assert!(!page.headers().contains_key(header::SET_COOKIE));
        assert!(page.text().await.unwrap().contains("the viewer"));
        let meta: Value = get(format!("/d/{link}/data/meta")).await.unwrap().json().await.unwrap();
        assert_eq!(meta, json!({ "ver": 1, "parts": 1, "size": 3 }));
        let part = get(format!("/d/{link}/data/0")).await.unwrap();
        assert_eq!(part.headers()[header::CONTENT_SECURITY_POLICY], CSP);
        assert_eq!(part.bytes().await.unwrap().as_ref(), b"abc");
        assert_eq!(get(format!("/d/{link}/other")).await.unwrap().status(), 404);
        assert_eq!(get("/d/".into()).await.unwrap().status(), 404);
        assert_eq!(get("/d/api/me".into()).await.unwrap().status(), 404);
        // Not for anything but reads.
        let r = off.at(host, reqwest::Method::DELETE, &format!("/d/{link}")).send().await.unwrap();
        assert_eq!(r.status(), 404);
    }
    // Not at the root, as it is on the docs host.
    assert_eq!(
        off.at("control.test", reqwest::Method::GET, &format!("/{link}/data/0")).send().await.unwrap().status(),
        404
    );

    // With a block domain, /d/ on control's origin is nothing.
    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    c.share(&d, &link, 1, &[b"abc"]).await;
    for p in [format!("/d/{link}"), format!("/d/{link}/data/0"), format!("/d/{link}/data/meta")] {
        for host in ["control.test", "127.0.0.1"] {
            let r = c.at(host, reqwest::Method::GET, &p).send().await.unwrap();
            assert_eq!(r.status(), 404, "{host}{p}");
        }
    }
    assert_eq!(c.view(&format!("/{link}/data/0")).await.status(), 200);
}

#[tokio::test]
async fn removing_a_machine_or_an_account_deletes_its_links() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    person(&c.app, "bob", "b1");
    let (leaves, stays, bobs) =
        (daemon(&c.app, "a1", "leaves"), daemon(&c.app, "a1", "stays"), daemon(&c.app, "b1", "bob"));
    c.share(&leaves, &id(120), 1, &[b"one", b"two"]).await;
    c.put(&leaves, &id(121), 1, 0, b"half").await;
    c.share(&stays, &id(122), 1, &[b"three"]).await;
    c.share(&bobs, &id(123), 1, &[b"four"]).await;
    assert_eq!(c.app.db.doc_rows(), (4, 5));

    // A machine leaves (the same call as being removed from the account).
    let r = c.signed(&leaves, "POST", "/api/daemon/leave", b"{}".to_vec()).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(c.app.db.doc_rows(), (2, 2), "its links and parts, finished or not");
    assert_eq!(c.view(&format!("/{}/data/meta", id(120))).await.status(), 404);
    assert_eq!(c.view(&format!("/{}/data/meta", id(122))).await.status(), 200);

    // An account is deleted.
    let ann = c.session("a1");
    let (st, v) = c.as_person(&ann, "POST", "/api/me/delete", Some(json!({ "confirm": "ann" }))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(c.app.db.doc_rows(), (1, 1));
    assert_eq!(c.view(&format!("/{}/data/meta", id(122))).await.status(), 404);
    assert_eq!(c.view(&format!("/{}/data/meta", id(123))).await.status(), 200, "bob's is not ann's");
    assert!(c.app.db.mentions(&id(122)).is_empty() && c.app.db.mentions(&id(120)).is_empty());
}

#[tokio::test]
async fn parts_nobody_committed_are_swept_after_a_day() {
    let c = control(|_| {}).await;
    let day = 24 * 3600 * 1000;
    let (old, now) = (1_000_000, 1_000_000 + 2 * day);
    let db = &c.app.db;
    // A first upload never committed; a served link with a newer half upload;
    // a recent half upload.
    db.put_doc_part(&id(130), A1, 1, 0, b"abandoned", old).unwrap();
    db.put_doc_part(&id(131), A1, 1, 0, b"served", old).unwrap();
    db.commit_doc_link(&id(131), A1, 1, 1, 6, old).unwrap();
    db.put_doc_part(&id(131), A1, 2, 0, b"half", old).unwrap();
    db.put_doc_part(&id(132), A1, 1, 0, b"recent", now).unwrap();

    assert_eq!(db.sweep_doc_links(now - day).unwrap(), 2);
    assert_eq!(db.doc_part_keys(&id(130)), vec![]);
    assert_eq!(db.doc_part_keys(&id(131)), vec![(1, 0)], "the served version stays");
    assert_eq!(db.doc_part_keys(&id(132)), vec![(1, 0)]);
    assert_eq!(db.doc_rows(), (2, 2), "the abandoned link's row went too");
    assert_eq!(db.sweep_doc_links(now - day).unwrap(), 0);
    assert!(db.doc_served(&id(131)).unwrap().is_some());
}

/// Served bytes are counted once, and an update's own served version not at
/// all: the commit replaces it.
#[tokio::test]
async fn served_bytes_count_once_and_an_update_replaces_its_own() {
    let c = control(|_| {}).await;
    person(&c.app, "ann", "a1");
    let d = daemon(&c.app, "a1", "geek");
    let mib = vec![1u8; 1 << 20];

    // 150 MiB served, parts and all: a new 40 MiB link still goes up.
    for i in 0..3 {
        let part = vec![i as u8; 50 << 20];
        c.app.db.seed_doc(&id(140 + i), "a1", &d.id(), 1, 50 << 20, Some((1, 0, &part)));
    }
    let (new, forty) = (id(150), 40);
    for n in 0..forty {
        assert_eq!(c.put(&d, &new, 1, n, &mib).await.0, 200, "part {n}");
    }
    let (st, v) = c.commit(&d, &new, 1, json!({ "parts": forty, "size": forty as u64 * (1 << 20) })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(c.quota(&d).await.1["used"], 190u64 << 20);

    // 190 MiB served in all: the 40 MiB link can be updated, though the
    // other 150 and a whole new 40 would be over; its own old version isn't
    // counted against it.
    for n in 0..forty {
        assert_eq!(c.put(&d, &new, 2, n, &mib).await.0, 200, "update part {n}");
    }
    let (st, v) = c.commit(&d, &new, 2, json!({ "parts": forty, "size": forty as u64 * (1 << 20) })).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(c.app.db.doc_part_keys(&new).iter().filter(|(ver, _)| *ver == 1).count(), 0);
    // But the others' 150 still count: 10 MiB more of a new link is the cap.
    for n in 0..10 {
        assert_eq!(c.put(&d, &id(151), 1, n, &mib).await.0, 200);
    }
    assert_eq!(c.put(&d, &id(151), 1, 10, b"x").await.1["reason"], "quota");
}

/// A page that began on one version asks for its parts by it (`?v=`), so an
/// Update landing mid-load is a 404 it can restart from, never a mix.
#[tokio::test]
async fn a_part_asked_for_by_version_is_a_404_once_another_is_served() {
    let c = control(|_| {}).await;
    let d = daemon(&c.app, "a1", "geek");
    let link = id(90);
    c.share(&d, &link, 1, &[b"one", b"two"]).await;
    assert_eq!(c.view(&format!("/{link}/data/0?v=1")).await.bytes().await.unwrap().as_ref(), b"one");
    assert_eq!(c.view(&format!("/{link}/data/0")).await.status(), 200, "no ?v= is the served version");
    for bad in ["v=2", "v=0", "v=x", "v="] {
        assert_eq!(c.view(&format!("/{link}/data/0?{bad}")).await.status(), 404, "{bad}");
    }
    assert_eq!(c.view(&format!("/{link}/data/meta?v=2")).await.status(), 200, "meta says which is served");
    c.share(&d, &link, 2, &[b"new"]).await;
    assert_eq!(c.view(&format!("/{link}/data/0?v=1")).await.status(), 404);
    assert_eq!(c.view(&format!("/{link}/data/0?v=2")).await.bytes().await.unwrap().as_ref(), b"new");
}
