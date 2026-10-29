//! hud boxes as clients (M237, `boxes.rs`) with the hub of chat's tests
//! ([`TestHub`]): a bound box's roster is its project's members now, and
//! its review lands in the thread on an object in that project for a
//! member, kept as posted via the box, and nowhere else.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    PlatformPort as _,
    boxes::{BoxMember, BoxPost, BoxPosted},
    chat_tests::{TestHub, call, serve},
    db::sql::{Sql as _, params},
};

fn member(account: &str, role: &str) -> BoxMember {
    BoxMember { account: account.into(), role: role.into(), name: account.into() }
}

fn post(project: &str, target: &str, author: &str, text: &str) -> BoxPost {
    BoxPost {
        project: project.into(),
        via: "hud box one".into(),
        target: target.into(),
        author: author.into(),
        text: text.into(),
    }
}

#[tokio::test]
async fn a_bound_box_reads_its_members_and_posts_reviews_in_its_project_only() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    // Team t (a owns it, b views it) and team u (c); chat on for both.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.flag("team:u", true);
    let team = app.db.team_project("t", "t", 1).unwrap();
    let own = app.db.personal_project("a", 1).unwrap();
    {
        let c = app.db.c();
        for (repo, name, project) in [(7, "o/public", &team), (9, "a/mine", &own)] {
            c.execute(
                "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at)
                 VALUES (?1, ?2, FALSE, ?3, 1, 1)",
                params![repo, name, project],
            )
            .unwrap();
        }
        for (id, repo) in [("fi_pub", 7), ("fi_own", 9)] {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
                 VALUES (?1, ?2, 3, 'pr', 'open', 'a title', 1, 1, 1)",
                params![id, repo],
            )
            .unwrap();
        }
    }

    // The roster: the team's members with their roles; a personal
    // project's account; nothing for a project that isn't there.
    assert_eq!(app.box_members(&team).await.unwrap(), Some(vec![member("a", "owner"), member("b", "viewer")]));
    assert_eq!(app.box_members(&own).await.unwrap(), Some(vec![member("a", "owner")]));
    assert_eq!(app.box_members("prj_nothere").await.unwrap(), None);
    // Someone who leaves the team is gone from the next read the cache
    // brings (the roster's change feed).
    hub.team("t", &[("a", TeamRole::Owner)]);
    app.cache.drop_all();
    assert_eq!(app.box_members(&team).await.unwrap(), Some(vec![member("a", "owner")]));
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer)]);
    app.cache.drop_all();

    // b's review on the team's PR: in its thread, via the box.
    let posted = app.box_post(&post(&team, "fi_pub", "b", "1. the button overlaps the logo")).await.unwrap();
    let BoxPosted::Posted { room, seq } = posted else { panic!("{posted:?}") };
    assert_eq!(seq, 1);
    let (st, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    assert_eq!(st, 200, "{v}");
    let m = &v["messages"][0];
    assert_eq!(
        (&m["author"], &m["text"], &m["via"]),
        (&json!("b"), &json!("1. the button overlaps the logo"), &json!("hud box one"))
    );
    // The room is the object's, as when a member opens it.
    let (_, opened) =
        call(&base, "a", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": "fi_pub" }))).await;
    assert_eq!(opened["id"], json!(room));
    // A message posted on the page carries no via.
    let (st, _) =
        call(&base, "a", "POST", &format!("/api/chat/rooms/{room}/messages"), Some(json!({ "text": "on it" }))).await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    assert_eq!(v["messages"][1]["via"], Value::Null);
    assert!(v["messages"][1].get("via").is_none(), "left out when there's none");

    // An object in another project is no such object, even for its own
    // member; so is one that isn't there.
    let refused = |status: u16, error: &str| BoxPosted::Refused { status, error: error.into() };
    assert_eq!(app.box_post(&post(&team, "fi_own", "a", "x")).await.unwrap(), refused(404, "no such object"));
    assert_eq!(app.box_post(&post(&team, "iss_00", "a", "x")).await.unwrap(), refused(404, "no such object"));
    // Someone outside the project can't post there through a box.
    assert_eq!(app.box_post(&post(&team, "fi_pub", "c", "x")).await.unwrap(), refused(404, "no such object"));
    // Nothing to say.
    assert_eq!(app.box_post(&post(&team, "fi_pub", "a", "  ")).await.unwrap(), refused(400, "say something"));
    let (_, v) = call(&base, "a", "GET", &format!("/api/chat/rooms/{room}/messages"), None).await;
    assert_eq!(v["messages"].as_array().unwrap().len(), 2, "nothing else posted");
}
