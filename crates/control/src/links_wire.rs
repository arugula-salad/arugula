//! Account links (M229) over HTTP: a product's server links its own user
//! to the account an ID token it was issued is for, by numeric GitHub id
//! only; the same link again is no change; a user or an account already
//! linked otherwise is refused until that link ends; the same GitHub login
//! with another numeric id links nothing; and an unlink on the platform
//! page is audited as the person's, seen by the product on its next call,
//! and sticks: a token from before it never links again (the tombstone),
//! and the next sign-in through that product asks the person again (the
//! consent screen).

use arugula_control_common::issuer::IssuerKey;
use arugula_e2e::now_ms;
use reqwest::header;
use serde_json::{Value, json};

use crate::{
    auth::hash,
    db::issuer::{ClientKind, set_client_secret},
    issuer_wire::{BACK, CLIENT, COOKIE, Control, client, control, keys, pkce, query},
};

pub(crate) const SECRET: &str = "acs_test-secret";

impl Control {
    /// `CLIENT` gets the link secret [`SECRET`].
    pub(crate) fn give_secret(&self, client: &str, secret: &str) {
        set_client_secret(&self.app.db.conn(), client, &hash(secret), now_ms()).unwrap();
    }

    /// Another account signed in with GitHub as (`github_id`, `login`),
    /// and its session cookie.
    pub(crate) fn person(&self, account: &str, github_id: &str, login: &str) -> String {
        let now = now_ms();
        self.app.db.account_for("github", github_id, login, account, now).unwrap();
        self.app.db.add_session(&hash(&format!("session-{account}")), account, now, now + 3_600_000).unwrap();
        format!("ilg_session=session-{account}")
    }

    /// An ID token for `CLIENT`, for whoever `cookie` is, saying yes to
    /// the consent screen if it's shown.
    async fn id_token_as(&self, cookie: &str, seed: u8) -> String {
        self.id_token_asked(cookie, seed).await.0
    }

    /// The same, and whether the consent screen was shown.
    async fn id_token_asked(&self, cookie: &str, seed: u8) -> (String, bool) {
        let (v, ch) = pkce(seed);
        let (st, at, asked) =
            self.through_consent(&self.authorize_path(CLIENT, BACK, &ch, "S256"), cookie, "allow").await;
        assert_eq!(st, 303, "{at}");
        let code = query(&at)["code"].clone();
        let (st, t) = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", BACK),
                ("client_id", CLIENT),
                ("code_verifier", &v),
            ])
            .await;
        assert_eq!(st, 200, "{t}");
        (t["id_token"].as_str().unwrap().to_owned(), asked)
    }

    pub(crate) async fn links(
        &self,
        method: &str,
        path: &str,
        auth: Option<(&str, &str)>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut r = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some((id, secret)) = auth {
            r = r.basic_auth(id, Some(secret));
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let r = r.send().await.unwrap();
        let st = r.status().as_u16();
        (st, r.json().await.unwrap_or(Value::Null))
    }

    async fn link(&self, user: &str, github_id: u64, assertion: &str) -> (u16, Value) {
        self.link_named(user, github_id, assertion, &format!("{user} here")).await
    }

    async fn link_named(&self, user: &str, github_id: u64, assertion: &str, name: &str) -> (u16, Value) {
        let body = json!({ "user": user, "github_id": github_id, "assertion": assertion, "name": name });
        self.links("POST", "/oauth/links", Some((CLIENT, SECRET)), Some(body)).await
    }

    async fn show(&self, user: &str) -> (u16, Value) {
        self.links("GET", &format!("/oauth/links/{user}"), Some((CLIENT, SECRET)), None).await
    }

    /// A platform route, as the platform page calls it with `cookie`.
    pub(crate) async fn page(&self, method: &str, path: &str, cookie: &str) -> (u16, Value) {
        let r = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header(header::COOKIE, cookie)
            .send()
            .await
            .unwrap();
        let st = r.status().as_u16();
        (st, r.json().await.unwrap_or(Value::Null))
    }

    fn rows(&self) -> i64 {
        self.app.db.conn().query_row("SELECT COUNT(*) FROM account_links", [], |r| r.get(0)).unwrap()
    }

    /// The audit, newest first: (event, by).
    fn audit(&self, account: &str) -> Vec<(String, String)> {
        self.app.db.account_link_events(account).unwrap().into_iter().map(|e| (e.event, e.by)).collect()
    }
}

fn said(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

#[tokio::test]
async fn a_product_links_its_user_and_the_person_unlinks_on_the_platform_page() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    c.give_secret(CLIENT, SECRET);
    // Account `a` is GitHub id 4242 (issuer_wire's control).
    let token = c.sign_in(1).await.0;

    // Only the product's server, with its id and link secret.
    let body = json!({ "user": "studio-u1", "github_id": 4242, "assertion": token });
    let (st, v) = c.links("POST", "/oauth/links", None, Some(body.clone())).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_client")));
    let (st, _) = c.links("POST", "/oauth/links", Some((CLIENT, "acs_wrong")), Some(body.clone())).await;
    assert_eq!(st, 401);
    assert_eq!(c.show("studio-u1").await.0, 404);
    assert_eq!(c.links("GET", "/oauth/links/studio-u1", None, None).await.0, 401);
    // A body that isn't one.
    let (st, v) = c.links("POST", "/oauth/links", Some((CLIENT, SECRET)), Some(json!({ "user": "x" }))).await;
    assert_eq!((st, v["error"].as_str()), (400, Some("invalid_request")));
    let (st, _) = c.link("", 4242, &token).await;
    assert_eq!(st, 400);

    // Linked, by the number.
    let (st, v) = c.link("studio-u1", 4242, &token).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["platform_account"], "a");
    assert_eq!(
        (v["client"].as_str(), v["user"].as_str(), v["created"].as_bool()),
        (Some(CLIENT), Some("studio-u1"), Some(true))
    );
    let at = v["linked_at"].as_u64().unwrap();
    // Idempotent: the same link again is no change.
    let (st, v) = c.link("studio-u1", 4242, &token).await;
    assert_eq!((st, v["created"].as_bool(), v["linked_at"].as_u64()), (200, Some(false), Some(at)));
    assert_eq!(c.rows(), 1);
    let (st, v) = c.show("studio-u1").await;
    assert_eq!((st, v["platform_account"].as_str()), (200, Some("a")));
    assert_eq!(c.audit("a"), said(&[("linked", "product")]));
    // A new name is kept, and audited.
    let (st, v) = c.link_named("studio-u1", 4242, &token, "Jake G").await;
    assert_eq!((st, v["created"].as_bool()), (200, Some(false)));
    assert_eq!(c.audit("a"), said(&[("renamed", "product"), ("linked", "product")]));

    // A hud box's assertion isn't an ID token: never a link, even for the
    // same account. A hud box's secret (M237) never links either.
    let page = "https://box1.example/hud/";
    c.app.db.put_issuer_client(&client("box1", ClientKind::Hud, page)).unwrap();
    assert!(set_client_secret(&c.app.db.conn(), "nobody", &hash("acs_box"), now_ms()).is_err());
    c.give_secret("box1", "acs_box");
    let (st, v) = c.links("POST", "/oauth/links", Some(("box1", "acs_box")), Some(body.clone())).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_client")));
    assert_eq!(c.links("GET", "/oauth/links/studio-u1", Some(("box1", "acs_box")), None).await.0, 401);
    let enc = |u: &str| url::form_urlencoded::byte_serialize(u.as_bytes()).collect::<String>();
    let (st, at_hud, _) =
        c.get(&format!("/oauth/hud?client_id=box1&redirect_uri={}&nonce=nonce-0001", enc(page)), Some(COOKIE)).await;
    assert_eq!(st, 303, "{at_hud}");
    let hud = at_hud.split_once("#hud-forge=").unwrap().1;
    let (st, v) = c.link("studio-u2", 4242, hud).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_token")), "{v}");

    // The platform page lists it, and the person ends it.
    let (st, v) = c.page("GET", "/api/linked-products", COOKIE).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["links"][0]["client"], CLIENT);
    assert_eq!(v["links"][0]["product"], format!("{CLIENT} (test)"));
    assert_eq!(v["links"][0]["user"], "studio-u1");
    assert_eq!(v["links"][0]["name"], "Jake G");
    assert!(v["links"][0]["granted"].as_u64().is_some(), "{v}");
    // Someone else can't end it, and sees none.
    let b = c.person("b", "5151", "someone");
    assert_eq!(c.page("GET", "/api/linked-products", &b).await.1["links"], json!([]));
    let unlink = format!("/api/linked-products/{CLIENT}/unlink");
    assert_eq!(c.page("POST", &unlink, &b).await.0, 404);
    assert_eq!(c.page("POST", &unlink, "").await.0, 401);
    assert_eq!(c.show("studio-u1").await.0, 200);
    let (st, v) = c.page("POST", &unlink, COOKIE).await;
    assert_eq!((st, v["unlinked"].as_bool()), (200, Some(true)));
    assert_eq!(c.page("POST", &unlink, COOKIE).await.0, 404, "once");
    assert_eq!(c.audit("a")[0], ("unlinked".to_owned(), "account".to_owned()));
    assert_eq!(c.page("GET", "/api/linked-products", COOKIE).await.1["links"], json!([]), "the consent went too");

    // The product sees it on its next call.
    let (st, v) = c.show("studio-u1").await;
    assert_eq!((st, v["error"].as_str()), (404, Some("not_linked")));

    // It sticks: the token from before the unlink never links again.
    let (st, v) = c.link("studio-u1", 4242, &token).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_token")), "{v}");
    assert_eq!(c.rows(), 0);

    // It comes back only with a fresh sign-in through the product, which
    // asks the person again (in a later second than the unlink: `iat` has
    // a second's grain).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (token, asked) = c.id_token_asked(COOKIE, 2).await;
    assert!(asked, "the consent screen again after an unlink");
    let (st, v) = c.link("studio-u1", 4242, &token).await;
    assert_eq!((st, v["created"].as_bool()), (200, Some(true)), "{v}");

    // The product ends it itself.
    let del = || c.links("DELETE", "/oauth/links/studio-u1", Some((CLIENT, SECRET)), None);
    assert_eq!(del().await, (200, json!({ "unlinked": true })));
    assert_eq!(del().await, (200, json!({ "unlinked": false })));
    assert_eq!(c.audit("a")[0], ("unlinked".to_owned(), "product".to_owned()));
    assert_eq!(c.rows(), 0);
    // The product's unlink sticks too.
    let (st, _) = c.link("studio-u1", 4242, &token).await;
    assert_eq!(st, 401);
    assert!(!c.app.db.issuer_grant("a", CLIENT).unwrap(), "and ends the consent");
}

/// The consent screen (M229): the first sign-in through a product asks
/// "Link <product> to your Arugula account?"; only an answer from
/// control's own page, with its ticket, in the session it was shown in,
/// counts, once; a no tells the product `access_denied`; a yes is kept until
/// the person unlinks. hud boxes never ask.
#[tokio::test]
async fn the_first_sign_in_through_a_product_asks_and_an_unlink_asks_again() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    let b = c.person("b", "5151", "bee");
    let (_, ch) = pkce(7);
    let path = c.authorize_path(CLIENT, BACK, &ch, "S256");

    // Not asked yet: prompt=none can't.
    let (st, at, _) = c.get(&format!("{path}&prompt=none"), Some(&b)).await;
    assert_eq!((st, query(&at)["error"].as_str()), (303, "consent_required"));

    // The screen: no script, no framing.
    let r = c.http.get(format!("{}{path}", c.base)).header(header::HOST, "control.test").header(header::COOKIE, &b);
    let r = r.send().await.unwrap();
    assert_eq!(r.status(), 200);
    let csp = r.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_owned();
    assert!(csp.contains("frame-ancestors 'none'") && csp.contains("default-src 'none'"), "{csp}");
    let page = r.text().await.unwrap();
    assert!(page.contains(&format!("Link {CLIENT} (test) to your Arugula account?")), "{page}");
    assert!(!page.contains("<script"));
    let ticket = |p: &str| p.split("name=\"ticket\" value=\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let t = ticket(&page);

    // From another site: refused, and the ticket still waits.
    assert_eq!(c.answer(&t, &b, "allow", Some("https://evil.example")).await.0, 403);
    // `Origin: null` (control's pages say no-referrer) counts only when
    // Sec-Fetch-Site says the post came from this origin.
    let null_from = |site: &'static str| {
        let body = format!("ticket={t}&decision=allow");
        c.http
            .post(format!("{}/oauth/consent", c.base))
            .header(header::HOST, "control.test")
            .header(header::COOKIE, &b)
            .header(header::ORIGIN, "null")
            .header("sec-fetch-site", site)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
    };
    assert_eq!(null_from("cross-site").await.unwrap().status(), 403);
    assert_eq!(null_from("same-site").await.unwrap().status(), 403);
    // In another session: refused, and the ticket is spent.
    assert_eq!(c.answer(&t, COOKIE, "allow", Some(crate::issuer_wire::CONTROL)).await.0, 403);
    assert_eq!(c.answer(&t, &b, "allow", Some(crate::issuer_wire::CONTROL)).await.0, 403, "once");
    assert!(!c.app.db.issuer_grant("b", CLIENT).unwrap());

    // No: the product hears so, and nothing is kept.
    let (st, at, asked) = c.through_consent(&path, &b, "deny").await;
    assert!(asked);
    assert_eq!((st, query(&at)["error"].as_str(), query(&at)["state"].as_str()), (303, "access_denied", "st-1"));
    assert!(!c.app.db.issuer_grant("b", CLIENT).unwrap());

    // Yes: a code, and the next sign-in goes straight through.
    let (st, at, asked) = c.through_consent(&path, &b, "allow").await;
    assert!(asked);
    assert_eq!(st, 303);
    assert!(query(&at).contains_key("code"), "{at}");
    let (st, at, asked) = c.through_consent(&path, &b, "allow").await;
    assert_eq!((st, asked), (303, false));
    assert!(query(&at).contains_key("code"));
    assert_eq!(c.get(&format!("{path}&prompt=none"), Some(&b)).await.0, 303);

    // The platform page lists the consent with no link; unlinking it is
    // revoking it, so the next sign-in asks again.
    let (_, v) = c.page("GET", "/api/linked-products", &b).await;
    assert_eq!(v["links"][0]["client"], CLIENT);
    assert!(v["links"][0]["granted"].as_u64().is_some() && v["links"][0]["user"].is_null(), "{v}");
    assert_eq!(c.page("POST", &format!("/api/linked-products/{CLIENT}/unlink"), &b).await.0, 200);
    assert_eq!(c.audit("b")[0], ("unlinked".to_owned(), "account".to_owned()));
    assert!(c.through_consent(&path, &b, "allow").await.2, "asked again");

    // A hud box never asks.
    let page = "https://box1.example/hud/";
    c.app.db.put_issuer_client(&client("box1", ClientKind::Hud, page)).unwrap();
    let enc = |u: &str| url::form_urlencoded::byte_serialize(u.as_bytes()).collect::<String>();
    let (st, at, _) =
        c.get(&format!("/oauth/hud?client_id=box1&redirect_uri={}&nonce=nonce-0002", enc(page)), Some(&b)).await;
    assert_eq!(st, 303);
    assert!(at.contains("#hud-forge="), "{at}");
}

/// Bulk unlinks or lookups from one product are logged and counted (the ops
/// counter), never refused below the product's limit.
#[tokio::test]
async fn bulk_unlinks_and_lookups_are_counted() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    c.give_secret(CLIENT, SECRET);
    for i in 0..=crate::limit::LINK_UNLINKS_ALERT.1 {
        let (st, _) = c.links("DELETE", &format!("/oauth/links/u{i}"), Some((CLIENT, SECRET)), None).await;
        assert_eq!(st, 200);
    }
    let alerts = crate::db::issuer::link_alerts(&c.app.db.conn()).unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!((alerts[0].0.as_str(), alerts[0].1.as_str(), alerts[0].2), (CLIENT, "unlinks", 1));
}

/// Logins change hands: a product user who was `jake-gh` (GitHub id 4242)
/// never links to an account that signed in as `jake-gh` with another
/// numeric id, whatever the product sends; nor to an account that never
/// signed in with GitHub.
#[tokio::test]
async fn the_same_login_with_another_numeric_id_never_links() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    c.give_secret(CLIENT, SECRET);
    let squatter = c.person("squat", "9999", "jake-gh");
    let token = c.id_token_as(&squatter, 3).await;

    let (st, v) = c.link("studio-jake", 4242, &token).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("github_id_mismatch")), "{v}");
    assert_eq!(c.rows(), 0);
    assert!(c.audit("squat").is_empty());
    assert!(c.audit("a").is_empty());

    // Passkey only: no numeric id, no link.
    let now = now_ms();
    c.app.db.account_for("passkey", "pk-1", "", "pk", now).unwrap();
    c.app.db.add_session(&hash("session-pk"), "pk", now, now + 3_600_000).unwrap();
    let token = c.id_token_as("ilg_session=session-pk", 4).await;
    let (st, v) = c.link("studio-pk", 4242, &token).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("github_id_mismatch")));
    assert_eq!(c.rows(), 0);
}

/// One account per product user, one product user per account in each
/// product: a relink to another account waits for the unlink.
#[tokio::test]
async fn a_relink_elsewhere_waits_for_the_unlink() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    c.give_secret(CLIENT, SECRET);
    let a = c.sign_in(1).await.0;
    let b = c.person("b", "5151", "bee");
    let b = c.id_token_as(&b, 5).await;

    assert_eq!(c.link("u1", 4242, &a).await.0, 200);
    let (st, v) = c.link("u1", 5151, &b).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("user_linked_elsewhere")));
    let (st, v) = c.link("u2", 4242, &a).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("account_linked_elsewhere")));
    assert_eq!(c.show("u1").await.1["platform_account"], "a");

    assert_eq!(c.links("DELETE", "/oauth/links/u1", Some((CLIENT, SECRET)), None).await.0, 200);
    let (st, v) = c.link("u1", 5151, &b).await;
    assert_eq!((st, v["platform_account"].as_str()), (200, Some("b")));
}

/// An assertion counts only for the client it was issued to, and while
/// it's fresh; a client's links are its own; a client taken out takes its
/// links and secret with it.
#[tokio::test]
async fn an_assertion_counts_only_for_its_own_client() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    c.give_secret(CLIENT, SECRET);
    c.app.db.put_issuer_client(&client("planter", ClientKind::Oidc, "https://planter.example/cb")).unwrap();
    // Registered but without a secret: can't link at all.
    let token = c.sign_in(1).await.0;
    let body = json!({ "user": "p1", "github_id": 4242, "assertion": token });
    assert_eq!(c.links("POST", "/oauth/links", Some(("planter", "")), Some(body.clone())).await.0, 401);
    c.give_secret("planter", "acs_planter");
    // Studio's token, at planter's door.
    let (st, v) = c.links("POST", "/oauth/links", Some(("planter", "acs_planter")), Some(body)).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_token")), "{v}");
    // A new secret replaces the old at once.
    c.give_secret(CLIENT, "acs_new");
    assert_eq!(c.show("u").await.0, 401);
    let (st, _) = c.links("GET", "/oauth/links/u", Some((CLIENT, "acs_new")), None).await;
    assert_eq!(st, 404);

    // Expired: an ID token lasts minutes.
    let key = IssuerKey::generate();
    let old = arugula_control_common::issuer::sign_id_token(
        &key,
        &arugula_control_common::issuer::IdClaims {
            iss: crate::issuer_wire::CONTROL.into(),
            sub: "a".into(),
            aud: CLIENT.into(),
            iat: 1,
            exp: 2,
            github_id: Some(4242),
            nonce: None,
        },
    );
    let body = json!({ "user": "u", "github_id": 4242, "assertion": old });
    let (st, v) = c.links("POST", "/oauth/links", Some((CLIENT, "acs_new")), Some(body)).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_token")));

    // Taken out: its links and its secret go too.
    let body = json!({ "user": "u", "github_id": 4242, "assertion": c.sign_in(2).await.0 });
    assert_eq!(c.links("POST", "/oauth/links", Some((CLIENT, "acs_new")), Some(body)).await.0, 200);
    assert!(crate::db::issuer::remove_client(&mut c.app.db.conn(), CLIENT).unwrap());
    assert_eq!(c.rows(), 0);
    assert_eq!(c.audit("a")[0], ("unlinked".to_owned(), "operator".to_owned()));
    assert_eq!(c.links("GET", "/oauth/links/u", Some((CLIENT, "acs_new")), None).await.0, 401);
}
