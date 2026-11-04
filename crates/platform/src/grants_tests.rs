//! Grants' who-may (Arugula #683): a project's owner revokes a teammate's
//! machine grant over the project, and nobody else does; a write power is
//! refused to a viewer's machine, and one that exists lists as idle until
//! a role change brings it back. The rig is [`seeded`]'s: a owns team t
//! (b an editor, x a viewer) and the machine geek; c alone owns u.

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::{
        issues::Actor,
        projects::{
            BOARDS_READ, BOARDS_WRITE, CLOUD_DRIVE, ISSUES_WRITE, RUNS_READ, SECRETS_WRITE, SESSIONS_REPLY,
            THREADS_POST, THREADS_READ, WRITE_POWERS,
        },
    },
    grants::VIEWER_WRITE,
};

fn project(id: &str) -> Value {
    json!({ "kind": "project", "id": id })
}

async fn revoke(base: &str, who: &str, id: &str) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/grants/{id}/revoke"), None).await
}

/// A revoke signed by a machine (`who` as the hub's gate vouches, "a
/// geek"), or with a bearer token: neither is a person.
async fn revoke_as(base: &str, header: (&str, &str), id: &str) -> u16 {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/grants/{id}/revoke"))
        .header(header.0, header.1)
        .send()
        .await
        .unwrap();
    r.status().as_u16()
}

/// t's owner revokes a grant b gave bbox over t, which says who and when;
/// not a viewer, another editor, an owner of another project, a machine or
/// a token; never a grant over an account (b's cloud.drive, b's DMs) or
/// over another project; and the giver still can, as before.
#[tokio::test]
async fn an_owner_revokes_a_teammates_grant_over_the_project_and_nobody_else() {
    let (hub, app, base, t, u) = seeded().await;
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("e", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    hub.daemon("bbox", "b", Some("t"));
    hub.daemon("cbox", "c", Some("u"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let over_t = ("project", t.as_str());

    let (st, g) = call(&base, "b", "POST", "/api/grants", give("bbox", THREADS_READ, project(&t))).await;
    assert_eq!(st, 200, "{g}");
    let id = g["id"].as_str().unwrap().to_owned();
    assert_eq!((&g["revoked_by"], &g["idle"]), (&Value::Null, &Value::Null), "{g}");

    // Who can't.
    let (st, r) = revoke(&base, "x", &id).await;
    assert_eq!(st, 403, "a viewer: {r}");
    let (st, r) = revoke(&base, "e", &id).await;
    assert_eq!(st, 403, "an editor who didn't give it: {r}");
    assert!(r["error"].as_str().unwrap().contains("owners"), "{r}");
    let (st, r) = revoke(&base, "c", &id).await;
    assert_eq!(st, 404, "an owner of another project: it isn't there for them: {r}");
    assert_eq!(revoke_as(&base, ("x-test-daemon", "a geek"), &id).await, 401, "the owner's own machine");
    assert_eq!(revoke_as(&base, ("x-test-daemon", "b bbox"), &id).await, 401, "the machine itself");
    assert_eq!(revoke_as(&base, ("authorization", "Bearer aru_agent_x"), &id).await, 401, "an agent token");
    assert!(app.db.live_grant("bbox", THREADS_READ, over_t, "b").unwrap().is_some(), "still live");

    // t's owner can: who and when are kept, and bbox's next call is refused.
    let (st, r) = revoke(&base, "a", &id).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!((&r["live"], &r["revoked_by"]), (&json!(false), &json!({ "account": "a", "name": "a" })), "{r}");
    assert!(r["revoked_at"].is_u64(), "{r}");
    assert_eq!(app.db.live_grant("bbox", THREADS_READ, over_t, "b").unwrap(), None);
    let (st, again) = revoke(&base, "a", &id).await;
    assert_eq!((st, &again["revoked_at"]), (200, &r["revoked_at"]), "again changes nothing: {again}");
    // b's own list says so.
    let (_, l) = call(&base, "b", "GET", "/api/grants", None).await;
    let mine = l["grants"].as_array().unwrap().iter().find(|g| g["id"] == id).unwrap();
    assert_eq!((&mine["live"], &mine["revoked_by"]["account"]), (&json!(false), &json!("a")), "{mine}");

    // The giver still can, the way they could: recorded as theirs.
    let (_, g2) = call(&base, "b", "POST", "/api/grants", give("bbox", THREADS_READ, project(&t))).await;
    assert_ne!(g2["id"], id, "giving it back is a new grant");
    let (st, r) = revoke(&base, "b", g2["id"].as_str().unwrap()).await;
    assert_eq!((st, &r["revoked_by"]["account"]), (200, &json!("b")), "{r}");

    // Never a grant over an account: b's cloud.drive (given on join, listed
    // first) or b's DMs. Nor over another project: c's cbox over u.
    let (_, l) = call(&base, "b", "GET", "/api/grants", None).await;
    let cloud = l["grants"].as_array().unwrap().iter().find(|g| g["power"] == CLOUD_DRIVE).unwrap()["id"].clone();
    let (st, r) = revoke(&base, "a", cloud.as_str().unwrap()).await;
    assert_eq!(st, 404, "{r}");
    assert_eq!(app.db.cloud_drive("bbox", "b").unwrap(), Some(true));
    let (_, dms) =
        call(&base, "b", "POST", "/api/grants", give("bbox", THREADS_READ, json!({ "kind": "account" }))).await;
    assert_eq!(revoke(&base, "a", dms["id"].as_str().unwrap()).await.0, 404);
    assert!(app.db.live_grant("bbox", THREADS_READ, ("account", "b"), "b").unwrap().is_some());
    let (st, other) = call(&base, "c", "POST", "/api/grants", give("cbox", THREADS_READ, project(&u))).await;
    assert_eq!(st, 200, "{other}");
    assert_eq!(revoke(&base, "a", other["id"].as_str().unwrap()).await.0, 404);
    assert!(app.db.live_grant("cbox", THREADS_READ, ("project", &u), "c").unwrap().is_some());
    assert_eq!(revoke(&base, "a", "grt_none").await.0, 404);
}

/// A write power is refused to a viewer's machine, saying why, while the
/// reads and posts are given; an editor's and an owner's are given. One
/// from before (a demotion) stays, lists as idle, is still refused at the
/// route, and is live again once x is an editor.
#[tokio::test]
async fn a_write_power_is_refused_to_a_viewers_machine_and_an_old_one_is_idle() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("xbox", "x", Some("t"));
    hub.daemon("bbox", "b", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();

    for power in WRITE_POWERS.iter().filter(|p| **p != SECRETS_WRITE) {
        let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", power, project(&t))).await;
        assert_eq!((st, r["reason"].as_str()), (403, Some(VIEWER_WRITE)), "{power}: {r}");
        let why = r["error"].as_str().unwrap();
        assert!(why.contains("you only view t") && why.contains("xbox-box's agents") && why.contains(power), "{why}");
        assert_eq!(app.db.live_grant("xbox", power, ("project", &t), "x").unwrap(), None);
    }
    for power in [THREADS_READ, THREADS_POST, BOARDS_READ, RUNS_READ, SESSIONS_REPLY] {
        let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", power, project(&t))).await;
        assert_eq!((st, &r["live"], &r["idle"]), (200, &json!(true), &Value::Null), "{power}: {r}");
    }
    for (who, machine) in [("b", "bbox"), ("a", "geek")] {
        let (st, r) = call(&base, who, "POST", "/api/grants", give(machine, ISSUES_WRITE, project(&t))).await;
        assert_eq!((st, &r["idle"]), (200, &Value::Null), "{who}: {r}");
    }

    // One x's machine held from before: as it was, idle, and refused by
    // the route's own role check as before.
    let old = app.db.give_grant("xbox", BOARDS_WRITE, ("project", &t), "x", 1).unwrap();
    let (_, l) = call(&base, "x", "GET", "/api/grants", None).await;
    let of = |l: &Value, id: &str| l["grants"].as_array().unwrap().iter().find(|g| g["id"] == id).cloned().unwrap();
    let listed = of(&l, &old.id);
    assert_eq!(listed["live"], json!(true), "{listed}");
    let why = listed["idle"].as_str().expect("idle");
    assert!(why.contains("boards.write does nothing") && why.contains("only views t"), "{why}");
    assert_eq!(of(&l, old.id.as_str())["power"], BOARDS_WRITE);
    assert!(l["grants"].as_array().unwrap().iter().filter(|g| g["power"] == THREADS_READ).all(|g| g["idle"].is_null()));
    app.db.give_grant("xbox", ISSUES_WRITE, ("project", &t), "x", 2).unwrap();
    let xbox = Actor { account: "x", agent: Some("xbox") };
    let refused = crate::issues::place(&app, xbox, &t, true).await.err().expect("refused");
    assert!(refused.0 == axum::http::StatusCode::FORBIDDEN && refused.1.contains("viewer"), "{}", refused.1);

    // x becomes an editor: the same grant is live and works; a write can be given now.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Editor)]);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let (_, l) = call(&base, "x", "GET", "/api/grants", None).await;
    assert_eq!(of(&l, &old.id)["idle"], Value::Null, "{l}");
    assert!(crate::issues::place(&app, xbox, &t, true).await.is_ok());
    let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", BOARDS_WRITE, project(&t))).await;
    assert_eq!((st, &r["id"], &r["idle"]), (200, &json!(old.id), &Value::Null), "the live one is the answer: {r}");
}
