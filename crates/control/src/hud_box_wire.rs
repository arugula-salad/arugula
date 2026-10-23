//! hud boxes as clients (M237, `hud_box.rs`) over HTTP: a box bound to a
//! project reads its members with numeric GitHub ids; it asks for a review
//! to be posted, and only its author, on the hub's own page, posts it, into
//! the object's thread, kept as posted via the box. Another page, another
//! person, a second answer, a cancel and an expired review post nothing; an
//! unbound box, another kind's secret and a client that changed kind are
//! refused.

use arugula_control_common::issuer::IssuerKey;
use arugula_e2e::now_ms;
use reqwest::header;
use serde_json::{Value, json};

use crate::{
    db::issuer::{Client, ClientKind},
    hud_box::{POST_MS, WAITING_PER_BOX},
    issuer_wire::{CLIENT, CONTROL, COOKIE, Control, client, control, keys},
    links_wire::SECRET,
};

const BOX: &str = "box1";
const BOX_SECRET: &str = "acs_box1-secret";
const PAGE: &str = "https://box1.example/__hud/signin/back";

fn bound(id: &str, project: Option<&str>) -> Client {
    Client { project: project.map(str::to_owned), ..client(id, ClientKind::Hud, PAGE) }
}

impl Control {
    /// A platform route with a JSON body, as the page calls it.
    async fn page_json(&self, method: &str, path: &str, cookie: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header(header::COOKIE, cookie)
            .json(&body)
            .send()
            .await
            .unwrap();
        let st = r.status().as_u16();
        (st, r.json().await.unwrap_or(Value::Null))
    }

    async fn as_box(&self, method: &str, path: &str, secret: &str, body: Option<Value>) -> (u16, Value) {
        self.links(method, path, Some((BOX, secret)), body).await
    }

    /// The box asks for `text` to be posted on `target` for `author`: the id
    /// in the hub page's address it's given.
    async fn ask(&self, target: &str, author: &str, text: &str) -> String {
        let (st, v) = self
            .as_box(
                "POST",
                "/oauth/hud/posts",
                BOX_SECRET,
                Some(json!({ "target": target, "author": author, "text": text })),
            )
            .await;
        assert_eq!(st, 200, "{v}");
        let url = v["confirm_url"].as_str().unwrap();
        assert!(url.starts_with(&format!("{CONTROL}/oauth/hud/post?id=")), "{url}");
        assert_eq!(v["expires_in"], POST_MS / 1000);
        url.split_once("?id=").unwrap().1.to_owned()
    }

    /// The person's answer on the hub's page, from `origin` (None: no Origin).
    async fn answer_post(
        &self,
        cookie: &str,
        id: &str,
        decision: &str,
        origin: Option<&str>,
        site: Option<&str>,
    ) -> (u16, String) {
        let mut r = self
            .http
            .post(format!("{}/oauth/hud/post", self.base))
            .header(header::HOST, "control.test")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs([("id", id), ("decision", decision)])
                    .finish(),
            );
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        if let Some(s) = site {
            r = r.header("sec-fetch-site", s);
        }
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.text().await.unwrap())
    }

    async fn thread(&self, cookie: &str, target: &str) -> Vec<Value> {
        let (st, room) =
            self.page_json("POST", "/api/chat/targets", cookie, json!({ "kind": "object", "target": target })).await;
        assert_eq!(st, 200, "{room}");
        let (st, v) =
            self.page("GET", &format!("/api/chat/rooms/{}/messages", room["id"].as_str().unwrap()), cookie).await;
        assert_eq!(st, 200, "{v}");
        v["messages"].as_array().unwrap().clone()
    }
}

#[tokio::test]
async fn a_bound_box_reads_its_roster_and_only_the_author_posts_its_review_on_the_hubs_page() {
    let c = control(keys(&IssuerKey::generate(), None, None)).await;
    // Account `a` (GitHub id 4242, jake-gh) has chat on and an issue in its
    // personal project.
    c.app.db.set_labs_flag("account:a", "chat", Some(true)).unwrap();
    let project = c.app.platform.inner().db.personal_project("a", now_ms()).unwrap();
    let (st, issue) =
        c.page_json("POST", &format!("/api/projects/{project}/issues"), COOKIE, json!({ "title": "The header" })).await;
    assert_eq!(st, 200, "{issue}");
    let issue = issue["id"].as_str().unwrap().to_owned();

    c.app.db.put_issuer_client(&bound(BOX, Some(&project))).unwrap();
    c.give_secret(BOX, BOX_SECRET);
    c.give_secret(CLIENT, SECRET);

    // Only the box, with its own secret.
    assert_eq!(c.links("GET", "/oauth/hud/members", None, None).await.0, 401);
    assert_eq!(c.as_box("GET", "/oauth/hud/members", "acs_wrong", None).await.0, 401);
    let (st, v) = c.links("GET", "/oauth/hud/members", Some((CLIENT, SECRET)), None).await;
    assert_eq!((st, v["error"].as_str()), (401, Some("invalid_client")), "an oidc client's secret isn't a box's");
    let (st, _) = c
        .links(
            "POST",
            "/oauth/hud/posts",
            Some((CLIENT, SECRET)),
            Some(json!({ "target": issue, "author": "a", "text": "x" })),
        )
        .await;
    assert_eq!(st, 401);

    // Its roster: the project's members, by numeric GitHub id.
    let (st, v) = c.as_box("GET", "/oauth/hud/members", BOX_SECRET, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v,
        json!({ "project": project, "members": [
            { "account": "a", "github_id": 4242, "login": "jake-gh", "name": "jake-gh", "role": "owner" }
        ] })
    );

    // The box asks; nothing is posted until the author says so on the hub's page.
    let text = "1. The header overlaps the logo.\n<script>alert(1)</script>";
    let id = c.ask(&issue, "a", text).await;
    assert!(c.thread(COOKIE, &issue).await.is_empty(), "asking posts nothing");
    // Signed out: sign in first, then back to the same page.
    let (st, at, _) = c.get(&format!("/oauth/hud/post?id={id}"), None).await;
    assert_eq!(st, 303);
    assert!(at.starts_with("/#issuer-signin="), "{at}");
    // Someone else, signed in: not theirs to see or post.
    let b = c.person("b", "5151", "someone");
    c.app.db.set_labs_flag("account:b", "chat", Some(true)).unwrap();
    let (st, _, body) = c.get(&format!("/oauth/hud/post?id={id}"), Some(&b)).await;
    assert_eq!(st, 403, "{body}");
    assert!(!body.contains("overlaps"), "the text isn't shown to anyone else");
    // The author: the box, the object and the text, word for word, escaped; no script; no frame.
    let r = c
        .http
        .get(format!("{}/oauth/hud/post?id={id}", c.base))
        .header(header::HOST, "control.test")
        .header(header::COOKIE, COOKIE)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let csp = r.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_owned();
    assert!(csp.contains("default-src 'none'") && csp.contains("frame-ancestors 'none'"), "{csp}");
    assert_eq!(r.headers()[header::X_FRAME_OPTIONS], "DENY");
    let body = r.text().await.unwrap();
    assert!(body.contains("box1 (test)") && body.contains(&issue), "{body}");
    assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;") && !body.contains("<script>"), "{body}");

    // An answer from anywhere but the hub's own page, or by anyone but the author, posts nothing.
    let own = Some("http://control.test");
    assert_eq!(c.answer_post(COOKIE, &id, "post", Some("https://box1.example"), None).await.0, 403);
    assert_eq!(c.answer_post(COOKIE, &id, "post", own, Some("cross-site")).await.0, 403);
    assert_eq!(c.answer_post(COOKIE, &id, "post", Some("null"), Some("same-site")).await.0, 403);
    assert_eq!(c.answer_post(&b, &id, "post", own, Some("same-origin")).await.0, 403);
    assert!(c.thread(COOKIE, &issue).await.is_empty());
    // The author posts it: in the issue's thread, as them, via the box. Once.
    let (st, body) = c.answer_post(COOKIE, &id, "post", own, Some("same-origin")).await;
    assert_eq!(st, 200, "{body}");
    assert!(body.contains("Posted"), "{body}");
    let msgs = c.thread(COOKIE, &issue).await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(
        (&msgs[0]["author"], &msgs[0]["text"], &msgs[0]["via"]),
        (&json!("a"), &json!(text), &json!("box1 (test)"))
    );
    assert_eq!(c.answer_post(COOKIE, &id, "post", own, Some("same-origin")).await.0, 410, "once");
    assert_eq!(c.get(&format!("/oauth/hud/post?id={id}"), Some(COOKIE)).await.0, 410);

    // Cancelled, or run out: nothing.
    let id = c.ask(&issue, "a", "no thanks").await;
    assert_eq!(c.answer_post(COOKIE, &id, "cancel", own, Some("same-origin")).await.0, 200);
    assert_eq!(c.answer_post(COOKIE, &id, "post", own, Some("same-origin")).await.0, 410);
    let id = c.ask(&issue, "a", "too late").await;
    c.app.issuer.as_ref().unwrap().box_posts.age(POST_MS);
    assert_eq!(c.answer_post(COOKIE, &id, "post", own, Some("same-origin")).await.0, 410);
    assert_eq!(c.thread(COOKIE, &issue).await.len(), 1);

    // Someone who isn't in the project, posting their own: refused as the object isn't there for them.
    let id = c.ask(&issue, "b", "hi").await;
    let (st, body) = c.answer_post(&b, &id, "post", own, Some("same-origin")).await;
    assert_eq!(st, 404, "{body}");
    assert_eq!(c.thread(COOKIE, &issue).await.len(), 1);
    // A body that isn't one.
    let (st, _) = c
        .as_box("POST", "/oauth/hud/posts", BOX_SECRET, Some(json!({ "target": "../x", "author": "a", "text": "hi" })))
        .await;
    assert_eq!(st, 400);

    // At most so many waiting per box.
    for _ in 0..WAITING_PER_BOX {
        let (st, _) = c
            .as_box(
                "POST",
                "/oauth/hud/posts",
                BOX_SECRET,
                Some(json!({ "target": issue, "author": "a", "text": "x" })),
            )
            .await;
        if st != 200 {
            assert_eq!(st, 429);
            break;
        }
    }
    let (st, v) = c
        .as_box("POST", "/oauth/hud/posts", BOX_SECRET, Some(json!({ "target": issue, "author": "a", "text": "x" })))
        .await;
    assert_eq!(st, 429, "{v}");

    // Rebound to a project that isn't there; then unbound: it keeps its own roster.
    c.app.db.put_issuer_client(&bound(BOX, Some("prj_0ther"))).unwrap();
    let (st, v) = c.as_box("GET", "/oauth/hud/members", BOX_SECRET, None).await;
    assert_eq!((st, v["error"].as_str()), (404, Some("no_such_project")), "{v}");
    c.app.db.put_issuer_client(&bound(BOX, None)).unwrap();
    let (st, v) = c.as_box("GET", "/oauth/hud/members", BOX_SECRET, None).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("not_bound")), "{v}");

    // A client that changes kind loses its secret.
    c.app.db.put_issuer_client(&client(BOX, ClientKind::Oidc, PAGE)).unwrap();
    for kind in [ClientKind::Hud, ClientKind::Oidc] {
        assert!(c.app.db.issuer_client_by_secret(BOX, BOX_SECRET, kind).unwrap().is_none());
    }

    // Only a box is bound to a project.
    assert!(
        c.app
            .db
            .put_issuer_client(&Client { project: Some(project.clone()), ..client("p2", ClientKind::Oidc, PAGE) })
            .is_err()
    );
    assert!(c.app.db.put_issuer_client(&bound("box3", Some("not-a-project"))).is_err());
}
