//! The hub as an issuer (M228) over HTTP: a registered client signs a
//! person in with the authorization code flow and PKCE, checks the ID token
//! against the JWKS and reads userinfo; codes work once, with their own
//! client, address and verifier; an unregistered client or address gets
//! nothing sent anywhere; tokens say who and never a project or a role;
//! an access token ends with its hub session; a rotation keeps every token
//! a client holds verifiable; and a hud box gets hud's own assertion. Each
//! client takes only its own kind's flow, and a token issued to one client
//! is never another's.

use std::{net::SocketAddr, sync::Arc};

use arugula_control_common::issuer::{IssuerKey, Jwks, Refused, verify_hud, verify_id_token};
use arugula_e2e::now_ms;
use base64::Engine as _;
use reqwest::header;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    App,
    auth::hash,
    db::issuer::{Client, ClientKind},
    issuer::{CODES_PER_SESSION, Issuer, Keys, access_token_for},
};

pub(crate) const CONTROL: &str = "http://control.test";
pub(crate) const CLIENT: &str = "studio-test";
pub(crate) const BACK: &str = "https://studio.example/auth/arugula/callback";
pub(crate) const COOKIE: &str = "ilg_session=session-a";

pub(crate) struct Control {
    pub(crate) base: String,
    pub(crate) http: reqwest::Client,
    pub(crate) app: Arc<App>,
}

/// Keys with a previous one published for ten more minutes.
pub(crate) fn keys(current: &IssuerKey, next: Option<&IssuerKey>, previous: Option<&IssuerKey>) -> Keys {
    Keys { current: current.clone(), next: next.cloned(), previous: previous.map(|k| (k.clone(), now() + 600)) }
}

pub(crate) fn client(id: &str, kind: ClientKind, back: &str) -> Client {
    Client { id: id.into(), kind, name: format!("{id} (test)"), redirect_uris: vec![back.into()], created: 0 }
}

/// A control with these keys, a registered client, and account `a` signed
/// in with GitHub (id 4242, login jake-gh) as `session-a`.
pub(crate) async fn control(keys: Keys) -> Control {
    let mut app = App::for_tests(CONTROL);
    app.issuer = Some(Issuer::new(keys));
    let app = app.start();
    let now = now_ms();
    app.db.account_for("github", "4242", "jake-gh", "a", now).unwrap();
    app.db.add_session(&hash("session-a"), "a", now, now + 3_600_000).unwrap();
    app.db.put_issuer_client(&client(CLIENT, ClientKind::Oidc, BACK)).unwrap();
    // `a` has said yes to CLIENT's consent screen (M229, links_wire).
    app.db.put_issuer_grant("a", CLIENT, now).unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Control { base, http, app }
}

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A PKCE verifier and its S256 challenge.
pub(crate) fn pkce(seed: u8) -> (String, String) {
    let verifier = b64url(&[seed; 32]);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

pub(crate) fn query(u: &str) -> std::collections::HashMap<String, String> {
    url::Url::parse(u).unwrap().query_pairs().into_owned().collect()
}

impl Control {
    pub(crate) async fn get(&self, path: &str, cookie: Option<&str>) -> (u16, String, String) {
        let mut r = self.http.get(format!("{}{path}", self.base)).header(header::HOST, "control.test");
        if let Some(c) = cookie {
            r = r.header(header::COOKIE, c);
        }
        let r = r.send().await.unwrap();
        let at = r.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        (r.status().as_u16(), at, r.text().await.unwrap())
    }

    pub(crate) fn authorize_path(&self, client: &str, back: &str, challenge: &str, method: &str) -> String {
        let q = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("response_type", "code")
            .append_pair("client_id", client)
            .append_pair("redirect_uri", back)
            .append_pair("scope", "openid")
            .append_pair("state", "st-1")
            .append_pair("nonce", "n-1")
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", method)
            .finish();
        format!("/oauth/authorize?{q}")
    }

    /// A code for the signed-in browser.
    /// GET `path` as `cookie`; a consent screen (M229) is answered
    /// `decision` from control's own page, and what that answers is the
    /// answer. Whether there was a screen, too.
    pub(crate) async fn through_consent(&self, path: &str, cookie: &str, decision: &str) -> (u16, String, bool) {
        let (st, at, body) = self.get(path, Some(cookie)).await;
        if st != 200 {
            return (st, at, false);
        }
        let ticket = body.split("name=\"ticket\" value=\"").nth(1).and_then(|r| r.split('"').next()).expect(&body);
        let (st, at) = self.answer(ticket, cookie, decision, Some(CONTROL)).await;
        (st, at, true)
    }

    /// POST the consent screen's answer, from `origin`.
    pub(crate) async fn answer(
        &self,
        ticket: &str,
        cookie: &str,
        decision: &str,
        origin: Option<&str>,
    ) -> (u16, String) {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([("ticket", ticket), ("decision", decision)])
            .finish();
        let mut r = self
            .http
            .post(format!("{}/oauth/consent", self.base))
            .header(header::HOST, "control.test")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        let r = r.body(body).send().await.unwrap();
        let at = r.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        (r.status().as_u16(), at)
    }

    async fn code(&self, challenge: &str) -> String {
        let (st, at, _) =
            self.through_consent(&self.authorize_path(CLIENT, BACK, challenge, "S256"), COOKIE, "allow").await;
        assert_eq!(st, 303, "{at}");
        assert!(at.starts_with(&format!("{BACK}?")), "{at}");
        let q = query(&at);
        assert_eq!(q["state"], "st-1");
        assert_eq!(q["iss"], CONTROL);
        q["code"].clone()
    }

    pub(crate) async fn token(&self, form: &[(&str, &str)]) -> (u16, Value) {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let r = self
            .http
            .post(format!("{}/oauth/token", self.base))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap())
    }

    async fn swap(&self, code: &str, verifier: &str) -> (u16, Value) {
        self.token(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", BACK),
            ("client_id", CLIENT),
            ("code_verifier", verifier),
        ])
        .await
    }

    /// An ID token and an access token, the whole way.
    pub(crate) async fn sign_in(&self, seed: u8) -> (String, String) {
        let (v, c) = pkce(seed);
        let code = self.code(&c).await;
        let (st, t) = self.swap(&code, &v).await;
        assert_eq!(st, 200, "{t}");
        (t["id_token"].as_str().unwrap().to_owned(), t["access_token"].as_str().unwrap().to_owned())
    }

    async fn jwks(&self) -> Jwks {
        let r = self.http.get(format!("{}/.well-known/jwks.json", self.base)).send().await.unwrap();
        assert_eq!(r.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        r.json().await.unwrap()
    }

    async fn userinfo(&self, access: &str) -> (u16, Value) {
        let r = self.http.get(format!("{}/oauth/userinfo", self.base)).bearer_auth(access).send().await.unwrap();
        let st = r.status().as_u16();
        (st, r.json().await.unwrap_or(Value::Null))
    }
}

fn now() -> u64 {
    now_ms() / 1000
}

fn payload(jwt: &str) -> Value {
    let p = jwt.split('.').nth(1).unwrap();
    serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p).unwrap()).unwrap()
}

#[tokio::test]
async fn a_client_signs_in_through_the_hub_and_verifies_against_the_jwks() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;

    // Discovery says where everything is, and S256 only.
    let r = c.http.get(format!("{}/.well-known/openid-configuration", c.base)).send().await.unwrap();
    let d: Value = r.json().await.unwrap();
    assert_eq!(d["issuer"], CONTROL);
    assert_eq!(d["jwks_uri"], format!("{CONTROL}/.well-known/jwks.json"));
    assert_eq!(d["code_challenge_methods_supported"], serde_json::json!(["S256"]));

    // Signed out: control's page signs them in, then comes back to this
    // very request; asked not to prompt, the client hears so.
    let (_, ch) = pkce(1);
    let path = c.authorize_path(CLIENT, BACK, &ch, "S256");
    let (st, at, _) = c.get(&path, None).await;
    assert_eq!(st, 303);
    let resume = at.strip_prefix("/#issuer-signin=").unwrap();
    let resume = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(resume).unwrap();
    assert_eq!(String::from_utf8(resume).unwrap(), path);
    let (st, at, _) = c.get(&format!("{path}&prompt=none"), None).await;
    assert_eq!(st, 303);
    assert_eq!(query(&at)["error"], "login_required");

    // Signed in: a code, then tokens.
    let (id_token, access) = c.sign_in(1).await;
    let jwks = c.jwks().await;
    let claims = verify_id_token(&id_token, &jwks, CONTROL, CLIENT, now()).unwrap();
    assert_eq!(claims.sub, "a");
    assert_eq!(claims.github_id, Some(4242));
    assert_eq!(claims.nonce.as_deref(), Some("n-1"));
    assert!(claims.exp - claims.iat <= 600, "minutes, not hours");
    // Who, never what they may do: no project or role claim, and no login.
    let mut names: Vec<String> = payload(&id_token).as_object().unwrap().keys().cloned().collect();
    names.sort();
    assert_eq!(names, ["aud", "exp", "github_id", "iat", "iss", "nonce", "sub"]);
    assert!(!id_token.contains("jake-gh"));
    // For another client, it's refused.
    assert_eq!(verify_id_token(&id_token, &jwks, CONTROL, "hud-box", now()), Err(Refused::Audience));

    let (st, me) = c.userinfo(&access).await;
    assert_eq!(st, 200);
    assert_eq!(me["sub"], "a");
    assert_eq!(me["github_id"], 4242);
    assert_eq!(me["client_id"], CLIENT);
    // No name the person didn't set, and never the login.
    assert!(me.get("name").is_none(), "{me}");
    assert!(!me.to_string().to_lowercase().contains("jake-gh"), "{me}");
    assert!(me.get("projects").is_none() && me.get("roles").is_none());
    assert_eq!(c.userinfo("aat_nope").await.0, 401);
}

#[tokio::test]
async fn codes_take_pkce_and_exact_addresses() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;
    let (v, ch) = pkce(2);

    // An address that isn't registered, or a client that isn't: shown an
    // error, sent nowhere.
    for path in [
        c.authorize_path(CLIENT, "https://studio.example/auth/arugula/callback/evil", &ch, "S256"),
        c.authorize_path(CLIENT, "https://evil.example/cb", &ch, "S256"),
        c.authorize_path("nobody", BACK, &ch, "S256"),
    ] {
        let (st, at, _) = c.get(&path, Some(COOKIE)).await;
        assert_eq!((st, at.as_str()), (400, ""), "{path}");
    }
    // No PKCE, or plain: back to the client with the error.
    for method in ["plain", ""] {
        let (st, at, _) = c.get(&c.authorize_path(CLIENT, BACK, &ch, method), Some(COOKIE)).await;
        assert_eq!(st, 303);
        assert_eq!(query(&at)["error"], "invalid_request", "{method}");
    }
    let (st, at, _) = c.get(&c.authorize_path(CLIENT, BACK, "short", "S256"), Some(COOKIE)).await;
    assert_eq!((st, query(&at)["error"].as_str()), (303, "invalid_request"));

    // The wrong verifier, address or client: refused, and the code is
    // gone either way.
    let (wrong, _) = pkce(3);
    for form in [
        [("code_verifier", wrong.as_str()), ("redirect_uri", BACK), ("client_id", CLIENT)],
        [("code_verifier", v.as_str()), ("redirect_uri", "https://studio.example/other"), ("client_id", CLIENT)],
        [("code_verifier", v.as_str()), ("redirect_uri", BACK), ("client_id", "someone-else")],
    ] {
        let code = c.code(&ch).await;
        let mut f = vec![("grant_type", "authorization_code"), ("code", code.as_str())];
        f.extend(form);
        let (st, e) = c.token(&f).await;
        assert_eq!((st, e["error"].as_str()), (400, Some("invalid_grant")), "{form:?}");
        assert_eq!(c.swap(&code, &v).await.0, 400, "a refused code is used up");
    }
    // The right one works once.
    let code = c.code(&ch).await;
    assert_eq!(c.swap(&code, &v).await.0, 200);
    let (st, e) = c.swap(&code, &v).await;
    assert_eq!((st, e["error"].as_str()), (400, Some("invalid_grant")));
    // Not the code flow.
    let (st, e) = c.token(&[("grant_type", "refresh_token"), ("refresh_token", "x")]).await;
    assert_eq!((st, e["error"].as_str()), (400, Some("unsupported_grant_type")));
}

#[tokio::test]
async fn codes_and_tokens_run_out_and_end_with_the_hub_session() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;
    let (v, ch) = pkce(4);

    // A code a minute old is stale.
    let code = c.code(&ch).await;
    c.app.issuer.as_ref().unwrap().age_codes(crate::issuer::CODE_MS);
    assert_eq!(c.swap(&code, &v).await.1["error"], "invalid_grant");

    // An access token past its expiry is refused.
    let (_, access) = c.sign_in(4).await;
    assert_eq!(c.userinfo(&access).await.0, 200);
    c.app.db.conn().execute("UPDATE issuer_tokens SET expires = 1", []).unwrap();
    assert_eq!(c.userinfo(&access).await.0, 401);

    // Signing out on the hub ends the access tokens it made, at once; a
    // code made before the sign-out is refused too.
    let (_, access) = c.sign_in(4).await;
    let code = c.code(&ch).await;
    c.app.db.drop_session(&hash("session-a")).unwrap();
    assert_eq!(c.userinfo(&access).await.0, 401);
    assert_eq!(c.swap(&code, &v).await.1["error"], "invalid_grant");
    assert_eq!(c.app.db.conn().query_row("SELECT COUNT(*) FROM issuer_tokens", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

/// A rotation as docs/control-ops.md runs it, one restart per step: the
/// new key is published before it signs, and the old one stays published
/// until its tokens have run out. A client that cached the JWKS while the
/// new key was next verifies the new key's tokens without fetching again.
#[tokio::test]
async fn a_rotation_overlaps() {
    let (old, new) = (IssuerKey::generate(), IssuerKey::generate());

    // 1. The old key signs.
    let c = control(keys(&old, None, None)).await;
    let (t1, _) = c.sign_in(5).await;
    let cached1 = c.jwks().await;
    assert_eq!(cached1.keys.len(), 1);
    verify_id_token(&t1, &cached1, CONTROL, CLIENT, now()).unwrap();

    // 2. The new key published as next: the old one still signs.
    let c = control(keys(&old, Some(&new), None)).await;
    let (t2, _) = c.sign_in(5).await;
    let cached2 = c.jwks().await;
    assert_eq!(cached2.keys.iter().map(|k| k.kid.as_str()).collect::<Vec<_>>(), [old.kid(), new.kid()]);
    assert_eq!(payload_kid(&t2), old.kid());

    // 3. The new key signs; the old one is previous.
    let c = control(keys(&new, None, Some(&old))).await;
    let (t3, _) = c.sign_in(5).await;
    assert_eq!(payload_kid(&t3), new.kid());
    // A client still on step 2's JWKS takes it; one on step 1's meets an
    // unknown kid, fetches again, and takes it.
    verify_id_token(&t3, &cached2, CONTROL, CLIENT, now()).unwrap();
    assert_eq!(verify_id_token(&t3, &cached1, CONTROL, CLIENT, now()), Err(Refused::Signature));
    let fresh = c.jwks().await;
    verify_id_token(&t3, &fresh, CONTROL, CLIENT, now()).unwrap();
    // Tokens the old key signed still verify against what's published.
    verify_id_token(&t1, &fresh, CONTROL, CLIENT, now()).unwrap();
    verify_id_token(&t2, &fresh, CONTROL, CLIENT, now()).unwrap();

    // 4. The old key dropped once its tokens ran out: they no longer
    // verify, and the new key's still do.
    let c = control(keys(&new, None, None)).await;
    let last = c.jwks().await;
    assert_eq!(verify_id_token(&t1, &last, CONTROL, CLIENT, now()), Err(Refused::Signature));
    verify_id_token(&t3, &last, CONTROL, CLIENT, now()).unwrap();
}

fn payload_kid(jwt: &str) -> String {
    let h = jwt.split('.').next().unwrap();
    let h: Value =
        serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(h).unwrap()).unwrap();
    h["kid"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_hud_box_gets_huds_own_assertion() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;
    let page = "https://box1.example/hud/";
    c.app.db.put_issuer_client(&client("box1", ClientKind::Hud, page)).unwrap();
    let path = format!(
        "/oauth/hud?client_id=box1&redirect_uri={}",
        url::form_urlencoded::byte_serialize(page.as_bytes()).collect::<String>()
    );

    let (st, at, _) = c.get(&path, Some(COOKIE)).await;
    assert_eq!(st, 303);
    let assertion = at.strip_prefix(&format!("{page}#hud-forge=")).unwrap();
    let jwks = Jwks { keys: vec![k.jwk()] };
    let claims = verify_hud(assertion, &jwks, "box1", now()).unwrap();
    assert_eq!(claims.sub, "github:jake-gh");
    assert_eq!((claims.account.as_str(), claims.github_id), ("a", 4242));
    assert_eq!(verify_hud(assertion, &jwks, "box2", now()), Err(Refused::Audience));
    // A fresh nonce each time: hud takes each once.
    let (_, again, _) = c.get(&path, Some(COOKIE)).await;
    assert_ne!(again, at);

    // Signed out: sign in first. Another address: nothing sent.
    let (st, at, _) = c.get(&path, None).await;
    assert_eq!(st, 303);
    assert!(at.starts_with("/#issuer-signin="));
    let (st, at, _) = c.get("/oauth/hud?client_id=box1&redirect_uri=https%3A%2F%2Fevil.example%2F", Some(COOKIE)).await;
    assert_eq!((st, at.as_str()), (400, ""));

    // The box's nonce comes back in the assertion; a malformed one is
    // refused.
    let (st, at, _) = c.get(&format!("{path}&nonce=box-nonce-0123"), Some(COOKIE)).await;
    assert_eq!(st, 303);
    let a = at.split_once("#hud-forge=").unwrap().1;
    assert_eq!(verify_hud(a, &jwks, "box1", now()).unwrap().nonce.as_deref(), Some("box-nonce-0123"));
    assert_eq!(c.get(&format!("{path}&nonce=x"), Some(COOKIE)).await.0, 400);

    // Another GitHub account here signed in with the same login since (a
    // rename, and the name taken): the login vouches for no one.
    c.app.db.account_for("github", "9999", "Jake-GH", "c", now_ms()).unwrap();
    let (st, at, body) = c.get(&path, Some(COOKIE)).await;
    assert_eq!((st, at.as_str()), (409, ""), "{body}");
    c.app.db.conn().execute("UPDATE identities SET login = 'someone-new' WHERE subject = '9999'", []).unwrap();
    assert_eq!(c.get(&path, Some(COOKIE)).await.0, 303);

    // An account that never signed in with GitHub has no login for hud.
    let now = now_ms();
    c.app.db.account_for("passkey", "pk1", "", "b", now).unwrap();
    c.app.db.add_session(&hash("session-b"), "b", now, now + 3_600_000).unwrap();
    let (st, _, body) = c.get(&path, Some("ilg_session=session-b")).await;
    assert_eq!(st, 403, "{body}");
}

#[tokio::test]
async fn without_a_key_there_is_no_issuer() {
    let mut app = App::for_tests(CONTROL);
    app.issuer = None;
    let app = app.start();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let svc = crate::router(app).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    for p in ["/.well-known/openid-configuration", "/.well-known/jwks.json", "/oauth/authorize", "/oauth/userinfo"] {
        assert_eq!(reqwest::get(format!("{base}{p}")).await.unwrap().status(), 404, "{p}");
    }
}

/// Each kind of client takes its own flow only; a token issued to one
/// client is never another's.
#[tokio::test]
async fn clients_take_their_own_flow_and_tokens_stay_theirs() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;
    let page = "https://box1.example/hud/";
    c.app.db.put_issuer_client(&client("box1", ClientKind::Hud, page)).unwrap();
    c.app.db.put_issuer_client(&client("other", ClientKind::Oidc, "https://other.example/cb")).unwrap();
    let (_, ch) = pkce(7);

    // A hud box can't take a code; a product can't take an assertion.
    let (st, at, _) = c.get(&c.authorize_path("box1", page, &ch, "S256"), Some(COOKIE)).await;
    assert_eq!((st, at.as_str()), (400, ""));
    let enc = |u: &str| url::form_urlencoded::byte_serialize(u.as_bytes()).collect::<String>();
    let (st, at, _) = c.get(&format!("/oauth/hud?client_id={CLIENT}&redirect_uri={}", enc(BACK)), Some(COOKIE)).await;
    assert_eq!((st, at.as_str()), (400, ""));

    // A token issued to CLIENT is CLIENT's: another client asking with it
    // is refused, and userinfo says whose it is.
    let (_, access) = c.sign_in(7).await;
    assert_eq!(access_token_for(&c.app, &access, CLIENT).as_deref(), Some("a"));
    assert_eq!(access_token_for(&c.app, &access, "other"), None);
    let r = c.http.get(format!("{}/oauth/userinfo?client_id=other", c.base)).bearer_auth(&access).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r =
        c.http.get(format!("{}/oauth/userinfo?client_id={CLIENT}", c.base)).bearer_auth(&access).send().await.unwrap();
    assert_eq!(r.status(), 200);

    // Made a hud box, the client's tokens end.
    c.app.db.put_issuer_client(&client(CLIENT, ClientKind::Hud, BACK)).unwrap();
    assert_eq!(c.userinfo(&access).await.0, 401);
}

/// A name the person set is userinfo's `name`; nothing else ever is.
#[tokio::test]
async fn only_a_name_they_set() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;
    let (id_token, access) = c.sign_in(8).await;
    assert!(!payload(&id_token).to_string().contains("jake-gh"));
    assert!(c.userinfo(&access).await.1.get("name").is_none());
    c.app.db.conn().execute("UPDATE accounts SET name = 'Jake' WHERE id = 'a'", []).unwrap();
    assert_eq!(c.userinfo(&access).await.1["name"], "Jake");
}

/// Addresses are kept normalized, and a client sends them as kept.
#[test]
fn redirect_addresses_are_normalized() {
    let db = crate::db::Db::memory();
    let kept = db.put_issuer_client(&client("n", ClientKind::Oidc, "HTTPS://Studio.Example:443/cb")).unwrap();
    assert_eq!(kept.redirect_uris, ["https://studio.example/cb"]);
    for bad in ["http://studio.example/cb", "https://studio.example/cb#x", "https://u@studio.example/cb", "/cb"] {
        assert!(db.put_issuer_client(&client("n", ClientKind::Oidc, bad)).is_err(), "{bad}");
    }
    assert!(db.put_issuer_client(&client("n", ClientKind::Oidc, "http://127.0.0.1:9/cb")).is_ok());
}

/// Codes waiting per hub session are capped, and `/oauth/*` is
/// rate-limited per address.
#[tokio::test]
async fn codes_are_capped_and_calls_limited() {
    let k = IssuerKey::generate();
    let c = control(keys(&k, None, None)).await;
    let (_, ch) = pkce(9);
    for _ in 0..CODES_PER_SESSION {
        c.code(&ch).await;
    }
    let (st, at, _) = c.get(&c.authorize_path(CLIENT, BACK, &ch, "S256"), Some(COOKIE)).await;
    assert_eq!(st, 303);
    assert_eq!(query(&at)["error"], "temporarily_unavailable");

    let ip = "127.0.0.1".parse().unwrap();
    while c.app.limits.check(crate::limit::ISSUER, ip).is_ok() {}
    assert_eq!(c.userinfo("aat_x").await.0, 429);
    assert_eq!(c.get(&c.authorize_path(CLIENT, BACK, &ch, "S256"), Some(COOKIE)).await.0, 429);
}

/// The hub trusts the key that signs and a live previous one, never next:
/// next signs nothing until it's made the key.
#[tokio::test]
async fn the_hub_never_trusts_next() {
    use arugula_control_common::issuer::{ID_TOKEN_SECS, IdClaims, sign_id_token};
    let (cur, next, old) = (IssuerKey::generate(), IssuerKey::generate(), IssuerKey::generate());
    let c = control(keys(&cur, Some(&next), Some(&old))).await;
    let issuer = c.app.issuer.as_ref().unwrap();
    let claims = IdClaims {
        iss: CONTROL.into(),
        sub: "a".into(),
        aud: CLIENT.into(),
        iat: now(),
        exp: now() + ID_TOKEN_SECS,
        github_id: Some(4242),
        nonce: None,
    };
    for (k, ok) in [(&cur, true), (&old, true), (&next, false)] {
        let t = sign_id_token(k, &claims);
        assert_eq!(issuer.verify(CONTROL, &t, CLIENT).is_ok(), ok, "{}", k.kid());
    }
    // Published all the same, for clients to have it before its turn.
    assert_eq!(c.jwks().await.keys.len(), 3);
    // A previous key past its time is neither published nor trusted.
    let c = control(Keys { current: cur.clone(), next: None, previous: Some((old.clone(), now() - 1)) }).await;
    assert_eq!(c.jwks().await.keys.len(), 1);
    let t = sign_id_token(&old, &claims);
    assert_eq!(c.app.issuer.as_ref().unwrap().verify(CONTROL, &t, CLIENT), Err(Refused::Signature));
}
