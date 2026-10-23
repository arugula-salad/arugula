//! hud boxes as clients (M237, `hud_box.rs`) over HTTP: a box bound to a
//! project reads its members with numeric GitHub ids, and posts a review
//! into an object's thread for a member who signed in to it through the
//! hub, kept as posted via the box; an unbound box, another kind's secret,
//! someone who never signed in to the box, and a box rebound since are
//! refused.

use arugula_control_common::issuer::IssuerKey;
use arugula_e2e::now_ms;
use serde_json::{Value, json};

use crate::{
    db::issuer::{Client, ClientKind},
    issuer_wire::{CLIENT, COOKIE, Control, client, control, keys},
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
            .header(reqwest::header::COOKIE, cookie)
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

    /// `cookie`'s browser signs in to the box through the hub.
    async fn sign_in_to_box(&self, cookie: &str) {
        let enc = url::form_urlencoded::byte_serialize(PAGE.as_bytes()).collect::<String>();
        let (st, at, _) =
            self.get(&format!("/oauth/hud?client_id={BOX}&redirect_uri={enc}&nonce=nonce-0001"), Some(cookie)).await;
        assert_eq!(st, 303, "{at}");
        assert!(at.starts_with(&format!("{PAGE}#hud-forge=hudfs1.")), "{at}");
    }
}

#[tokio::test]
async fn a_bound_box_reads_its_roster_and_posts_a_members_review_to_an_issue_thread() {
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

    // Its roster: the project's members, by numeric GitHub id.
    let (st, v) = c.as_box("GET", "/oauth/hud/members", BOX_SECRET, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v,
        json!({ "project": project, "members": [
            { "account": "a", "github_id": 4242, "login": "jake-gh", "name": "jake-gh", "role": "owner" }
        ] })
    );

    // A review only for someone who signed in to the box through the hub.
    let review = json!({ "target": issue, "author": "a", "text": "1. The header overlaps the logo on a phone." });
    let (st, v) = c.as_box("POST", "/oauth/hud/threads", BOX_SECRET, Some(review.clone())).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("not_signed_in")), "{v}");
    c.sign_in_to_box(COOKIE).await;
    let (st, v) = c.as_box("POST", "/oauth/hud/threads", BOX_SECRET, Some(review.clone())).await;
    assert_eq!(st, 200, "{v}");
    let room = v["room"].as_str().unwrap().to_owned();
    assert_eq!(v["seq"], 1);
    // It's in the issue's thread on the platform page, via the box.
    let (st, opened) =
        c.page_json("POST", "/api/chat/targets", COOKIE, json!({ "kind": "object", "target": issue })).await;
    assert_eq!((st, opened["id"].as_str()), (200, Some(room.as_str())), "{opened}");
    let (st, v) = c.page("GET", &format!("/api/chat/rooms/{room}/messages"), COOKIE).await;
    assert_eq!(st, 200, "{v}");
    let m = &v["messages"][0];
    assert_eq!((&m["author"], &m["text"], &m["via"]), (&json!("a"), &review["text"], &json!(format!("{BOX} (test)"))));

    // Someone who signed in to the box but isn't in the project: refused,
    // as the object isn't there for them.
    let b = c.person("b", "5151", "someone");
    c.app.db.set_labs_flag("account:b", "chat", Some(true)).unwrap();
    c.sign_in_to_box(&b).await;
    let (st, v) = c
        .as_box("POST", "/oauth/hud/threads", BOX_SECRET, Some(json!({ "target": issue, "author": "b", "text": "hi" })))
        .await;
    assert_eq!((st, v["error"].as_str()), (404, Some("refused")), "{v}");
    let (st, _) = c
        .as_box(
            "POST",
            "/oauth/hud/threads",
            BOX_SECRET,
            Some(json!({ "target": "../x", "author": "a", "text": "hi" })),
        )
        .await;
    assert_eq!(st, 400);

    // Rebound to another project: who it may post for starts over.
    c.app.db.put_issuer_client(&bound(BOX, Some("prj_0ther"))).unwrap();
    let (st, v) = c.as_box("POST", "/oauth/hud/threads", BOX_SECRET, Some(review.clone())).await;
    assert_eq!((st, v["error"].as_str()), (403, Some("not_signed_in")), "{v}");
    let (st, v) = c.as_box("GET", "/oauth/hud/members", BOX_SECRET, None).await;
    assert_eq!((st, v["error"].as_str()), (404, Some("no_such_project")), "{v}");

    // Unbound, it keeps its own roster.
    c.app.db.put_issuer_client(&bound(BOX, None)).unwrap();
    let (st, v) = c.as_box("GET", "/oauth/hud/members", BOX_SECRET, None).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("not_bound")), "{v}");
    let (st, _) = c.as_box("POST", "/oauth/hud/threads", BOX_SECRET, Some(review)).await;
    assert_eq!(st, 409);
    // An unbound box's sign-ins aren't kept.
    c.sign_in_to_box(COOKIE).await;
    let n: i64 = c.app.db.conn().query_row("SELECT COUNT(*) FROM hud_signins", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);

    // Only a box is bound to a project.
    assert!(
        c.app
            .db
            .put_issuer_client(&Client { project: Some(project.clone()), ..client("p2", ClientKind::Oidc, PAGE) })
            .is_err()
    );
    assert!(c.app.db.put_issuer_client(&bound("box3", Some("not-a-project"))).is_err());
}
