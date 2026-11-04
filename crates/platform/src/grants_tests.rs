//! Grants' who-may (Arugula #683): a project's owner revokes a teammate's
//! grant over the project, and nobody else does; a write power that does
//! nothing for a viewer's agents is refused to a viewer's machine, one that
//! still does something is given and says what it does, and one from
//! before stays until a role change brings the rest back. The rig is
//! [`seeded`]'s: a owns team t (b an editor, x a viewer) and the machine
//! geek; c alone owns u.

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::{
        issues::Actor,
        projects::{
            BOARDS_READ, BOARDS_WRITE, CLOUD_DRIVE, DEPLOYS_PLAN, ISSUES_WRITE, RUNS_READ, SECRETS_WRITE,
            SESSIONS_REPLY, SESSIONS_START, THREADS_POST, THREADS_READ, ViewerUse, WRITE_POWERS,
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

fn of(l: &Value, id: &str) -> Value {
    l["grants"].as_array().unwrap().iter().find(|g| g["id"] == id).cloned().unwrap_or_default()
}

/// t's owner revokes a grant b gave bbox over t, which says who and when,
/// and b's devices are told; not a viewer, another editor, an owner of
/// another project, a machine or a token; never a grant over an account
/// (b's cloud.drive, b's DMs) or over another project; and the giver
/// still can, as before, and can give it again.
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
    assert_eq!((&g["revoked_by"], &g["limited"]), (&Value::Null, &Value::Null), "{g}");

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
    assert!(hub.pushes().is_empty());

    // t's owner can: who and when are kept, b is told, and bbox's next
    // call is refused.
    let (st, r) = revoke(&base, "a", &id).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!((&r["live"], &r["revoked_by"]), (&json!(false), &json!({ "account": "a", "name": "a" })), "{r}");
    assert!(r["revoked_at"].is_u64(), "{r}");
    assert_eq!(app.db.live_grant("bbox", THREADS_READ, over_t, "b").unwrap(), None);
    let pushed = waits_for_pushes(&hub, 1).await;
    assert_eq!(pushed.len(), 1, "{pushed:?}");
    let (to, msg, _) = &pushed[0];
    assert_eq!(to, "b");
    let body = msg["body"].as_str().unwrap();
    assert!(
        body.contains("a, an owner of t, revoked bbox-box's threads.read") && body.contains("give it again"),
        "{body}"
    );
    let (st, again) = revoke(&base, "a", &id).await;
    assert_eq!((st, &again["revoked_at"]), (200, &r["revoked_at"]), "again changes nothing: {again}");
    assert_eq!(hub.pushes().len(), 1, "told once");
    // b's own list says so.
    let (_, l) = call(&base, "b", "GET", "/api/grants", None).await;
    let mine = of(&l, &id);
    assert_eq!((&mine["live"], &mine["revoked_by"]["account"]), (&json!(false), &json!("a")), "{mine}");

    // The giver still can, the way they could: recorded as theirs, and
    // nobody is told. And a revoke isn't a ban: b gives it again.
    let (_, g2) = call(&base, "b", "POST", "/api/grants", give("bbox", THREADS_READ, project(&t))).await;
    assert_ne!(g2["id"], id, "giving it back is a new grant");
    let (st, r) = revoke(&base, "b", g2["id"].as_str().unwrap()).await;
    assert_eq!((st, &r["revoked_by"]["account"]), (200, &json!("b")), "{r}");
    assert_eq!(hub.pushes().len(), 1);

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

/// Pushes the hub took, waited for.
async fn waits_for_pushes(hub: &crate::chat_tests::TestHub, n: usize) -> Vec<(String, Value, crate::hub::PushOptions)> {
    for _ in 0..50 {
        if hub.pushes().len() >= n {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    hub.pushes()
}

/// An owner's standing is asked of the hub now: a former owner, demoted or
/// removed, can't revoke; while the team is locked only its owners read
/// it, so an editor's attempt finds nothing and an owner's goes through.
/// A grant over a personal project is its owner's alone.
#[tokio::test]
async fn a_former_owner_a_locked_out_member_and_an_outsider_cannot_revoke() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.team("t", &[("a", TeamRole::Owner), ("o", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    hub.daemon("bbox", "b", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let gives = |n: usize| {
        let base = base.clone();
        let t = t.clone();
        async move {
            let power = [THREADS_READ, THREADS_POST, BOARDS_READ, RUNS_READ][n];
            let (st, g) = call(&base, "b", "POST", "/api/grants", give("bbox", power, project(&t))).await;
            assert_eq!(st, 200, "{g}");
            g["id"].as_str().unwrap().to_owned()
        }
    };
    let g0 = gives(0).await;

    // o was an owner a moment ago: demoted to editor, 403; out of the team, 404.
    hub.team("t", &[("a", TeamRole::Owner), ("o", TeamRole::Editor), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    assert_eq!(revoke(&base, "o", &g0).await.0, 403, "demoted, on the hub's word, not the cache's");
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Viewer)]);
    assert_eq!(revoke(&base, "o", &g0).await.0, 404, "removed");
    assert!(app.db.live_grant("bbox", THREADS_READ, ("project", &t), "b").unwrap().is_some());

    // Locked: b (an editor) doesn't read t now, so their own revoke finds
    // nothing of theirs there either way; a, an owner, still revokes.
    hub.lock("t", true);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(revoke(&base, "x", &g0).await.0, 404, "a viewer, locked out");
    let (st, r) = revoke(&base, "a", &g0).await;
    assert_eq!((st, &r["revoked_by"]["account"]), (200, &json!("a")), "{r}");
    hub.lock("t", false);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();

    // a's personal project: a gives geek a power over it; nobody else
    // sees the grant, and a revokes it as its giver.
    let (_, me) = call(&base, "a", "GET", "/api/projects", None).await;
    let personal = me["projects"].as_array().unwrap().iter().find(|p| p["team"].is_null()).unwrap()["id"].clone();
    let personal = personal.as_str().unwrap();
    let (st, g) = call(&base, "a", "POST", "/api/grants", give("geek", ISSUES_WRITE, project(personal))).await;
    assert_eq!(st, 200, "{g}");
    let pid = g["id"].as_str().unwrap();
    assert_eq!(revoke(&base, "b", pid).await.0, 404);
    assert_eq!(revoke(&base, "c", pid).await.0, 404);
    let (st, r) = revoke(&base, "a", pid).await;
    assert_eq!((st, &r["revoked_by"]["account"]), (200, &json!("a")), "{r}");
}

/// A write power that does nothing for a viewer's agents is refused to a
/// viewer's machine, saying why; one that still does something is given
/// and says what; the reads and posts are given as they are; an editor's
/// and an owner's writes are given whole. One from before (a demotion)
/// stays, lists with its limit, is still refused at the route, and is
/// whole again once x is an editor.
#[tokio::test]
async fn a_viewers_machine_gets_what_still_works_and_says_so() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("xbox", "x", Some("t"));
    hub.daemon("bbox", "b", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();

    for (power, use_) in WRITE_POWERS {
        if power == SECRETS_WRITE {
            continue; // a token's, never a machine's: 400 whoever asks
        }
        let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", power, project(&t))).await;
        match use_ {
            ViewerUse::Nothing => {
                assert_eq!((st, r["reason"].as_str()), (403, Some(VIEWER_WRITE)), "{power}: {r}");
                let why = r["error"].as_str().unwrap();
                assert!(
                    why.contains("you only view t") && why.contains("xbox-box's agents") && why.contains(power),
                    "{why}"
                );
                assert_eq!(app.db.live_grant("xbox", power, ("project", &t), "x").unwrap(), None);
            }
            ViewerUse::Only(label, does) => {
                assert_eq!((st, &r["live"]), (200, &json!(true)), "{power}: {r}");
                assert_eq!(r["limited"]["label"], label, "{r}");
                let why = r["limited"]["why"].as_str().unwrap();
                assert!(why.contains(does) && why.contains("only views t"), "{why}");
            }
        }
    }
    for power in [THREADS_READ, THREADS_POST, BOARDS_READ, RUNS_READ, SESSIONS_REPLY] {
        let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", power, project(&t))).await;
        assert_eq!((st, &r["live"], &r["limited"]), (200, &json!(true), &Value::Null), "{power}: {r}");
    }
    for (who, machine) in [("b", "bbox"), ("a", "geek")] {
        for power in [ISSUES_WRITE, DEPLOYS_PLAN, SESSIONS_START] {
            let (st, r) = call(&base, who, "POST", "/api/grants", give(machine, power, project(&t))).await;
            assert_eq!((st, &r["limited"]), (200, &Value::Null), "{who} {power}: {r}");
        }
    }

    // One x's machine held from before: as it was, idle, and refused by
    // the route's own role check as before.
    let old = app.db.give_grant("xbox", ISSUES_WRITE, ("project", &t), "x", 1).unwrap();
    let (_, l) = call(&base, "x", "GET", "/api/grants", None).await;
    let listed = of(&l, &old.id);
    assert_eq!((&listed["live"], &listed["limited"]["label"]), (&json!(true), &json!("idle")), "{listed}");
    let why = listed["limited"]["why"].as_str().unwrap();
    assert!(why.contains("issues.write does nothing here") && why.contains("only views t"), "{why}");
    assert!(
        l["grants"].as_array().unwrap().iter().filter(|g| g["power"] == THREADS_READ).all(|g| g["limited"].is_null())
    );
    assert_eq!(of(&l, &old.id)["power"], ISSUES_WRITE);
    let xbox = Actor { account: "x", agent: Some("xbox") };
    let refused = crate::issues::place(&app, xbox, &t, true).await.err().expect("refused");
    assert!(refused.0 == axum::http::StatusCode::FORBIDDEN && refused.1.contains("viewer"), "{}", refused.1);

    // x becomes an editor: the same grants are whole, and a write can be
    // given now.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Editor), ("x", TeamRole::Editor)]);
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let (_, l) = call(&base, "x", "GET", "/api/grants", None).await;
    assert!(l["grants"].as_array().unwrap().iter().all(|g| g["limited"].is_null()), "{l}");
    assert!(crate::issues::place(&app, xbox, &t, true).await.is_ok());
    let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", BOARDS_WRITE, project(&t))).await;
    assert_eq!((st, &r["limited"]), (200, &Value::Null), "the live one, whole: {r}");
}
