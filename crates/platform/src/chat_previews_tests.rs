//! Link previews (#108) through the platform's routes, with chat's test
//! hub: who gets a card for a link to an object of a project they're in,
//! that others get nothing, that a card follows the object's state, that a
//! web link to a private address is never requested, and the author's and
//! the team's switches.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    Platform,
    chat_tests::{TestHub, call, serve},
    db::{
        issues::{Actor, CloseReason, NewIssue, Patch, State},
        sql::{Sql as _, params},
    },
};

/// Team t (a owns it, b views it) has a project with a linked public repo
/// (PR #3, `o/public`) and a private one (issue #4, `o/secret`) and a
/// native issue; team u (c owns it, a views it) has a channel. Returns the
/// issue's id and the channel's.
async fn seed(app: &Platform, hub: &TestHub, base: &str) -> (String, String) {
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner), ("a", TeamRole::Viewer)]);
    hub.flag("team:t", true);
    hub.flag("team:u", true);
    let project = app.db.team_project("t", "t", 1).unwrap();
    {
        let c = app.db.c();
        for (repo, name, private) in [(7, "o/public", false), (8, "o/secret", true)] {
            c.execute(
                "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at)
                 VALUES (?1, ?2, ?3, ?4, 1, 1)",
                params![repo, name, private, project],
            )
            .unwrap();
        }
        for (id, repo, number, kind) in [("fi_pub", 7, 3, "pr"), ("fi_sec", 8, 4, "issue")] {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, author_login, created_at, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, 'merged', 'Fix the thing', 'octo', 1, 5, 1)",
                params![id, repo, number, kind],
            )
            .unwrap();
        }
    }
    let issue = app
        .db
        .create_issue(
            &NewIssue { project: &project, title: "Previews", body: "", labels: &[], assignees: &[] },
            Actor { account: "a", agent: None },
            10,
        )
        .unwrap()
        .unwrap();
    let (st, ch) = call(base, "c", "POST", "/api/chat/teams/u/channels", Some(json!({ "name": "general" }))).await;
    assert_eq!(st, 200, "{ch}");
    (issue.id, ch["id"].as_str().unwrap().to_owned())
}

async fn say(base: &str, who: &str, room: &str, text: &str) -> i64 {
    let (st, v) =
        call(base, who, "POST", &format!("/api/chat/rooms/{room}/messages"), Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

async fn previews(base: &str, who: &str, room: &str, seqs: &[i64]) -> Value {
    let q: Vec<String> = seqs.iter().map(i64::to_string).collect();
    let (st, v) = call(base, who, "GET", &format!("/api/chat/rooms/{room}/previews?seqs={}", q.join(",")), None).await;
    assert_eq!(st, 200, "{v}");
    v
}

#[tokio::test]
async fn a_card_is_drawn_for_the_reader_and_nobody_else_gets_one() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (issue, room) = seed(&app, &hub, &base).await;
    // Web previews off in u, so nothing here reaches the network.
    let (st, _) = call(&base, "c", "PUT", "/api/chat/teams/u/previews", Some(json!({ "web": false }))).await;
    assert_eq!(st, 200);
    let m = say(
        &base,
        "a",
        &room,
        &format!(
            "https://app.arugula.io/issues/{issue} and https://github.com/O/Public/pull/3 and https://github.com/o/secret/issues/4"
        ),
    )
    .await;
    // a is in t: the issue, the PR from the mirror (any case); b is not
    // asked: the private repo's item needs a's own GitHub access, which
    // the test hub's GitHub doesn't give.
    let v = previews(&base, "a", &room, &[m]).await;
    let cards = v["previews"][m.to_string()].as_array().unwrap();
    assert_eq!(cards.len(), 2, "{v}");
    assert_eq!(
        (&cards[0]["what"], &cards[0]["state"], &cards[0]["tone"]),
        (&json!("issue"), &json!("open"), &json!("open"))
    );
    assert!(cards[0]["title"].as_str().unwrap().ends_with("Previews"));
    assert_eq!(cards[0]["who"], json!("a"));
    assert_eq!(
        (&cards[1]["what"], &cards[1]["state"], &cards[1]["tone"], &cards[1]["who"]),
        (&json!("pull request"), &json!("merged"), &json!("done"), &json!("octo"))
    );
    // c reads the same message in u's channel and is in neither project's
    // team: no cards at all, and nothing says why.
    let v = previews(&base, "c", &room, &[m]).await;
    assert_eq!(v["previews"], json!({}), "{v}");

    // The card follows the issue.
    app.db
        .patch_issue(
            &issue,
            &Patch { state: Some(State::Closed(CloseReason::NotPlanned)), ..Default::default() },
            Actor { account: "a", agent: None },
            20,
        )
        .unwrap()
        .unwrap();
    let v = previews(&base, "a", &room, &[m]).await;
    assert_eq!(v["previews"][m.to_string()][0]["state"], json!("closed, not planned"), "{v}");
    assert_eq!(v["previews"][m.to_string()][0]["tone"], json!("off"));
}

#[tokio::test]
async fn private_addresses_are_never_fetched_and_the_switches_work() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (_, room) = seed(&app, &hub, &base).await;
    let m = say(&base, "c", &room, "http://169.254.169.254/latest/ http://localhost:9/ http://[::1]/x").await;
    // Web previews are on: the fetcher is asked, and refuses all three
    // before connecting (link_fetch's tests prove none is requested).
    let v = previews(&base, "c", &room, &[m]).await;
    assert_eq!((v["previews"].clone(), v["web"].clone()), (json!({}), json!(true)), "{v}");
    for u in ["http://169.254.169.254/latest/", "http://localhost:9/", "http://[::1]/x"] {
        let kept = app.db.web_preview(u).unwrap().unwrap_or_else(|| panic!("{u} was never tried"));
        assert!(!kept.ok, "{u}");
    }

    // A page that was read shows until its author takes the preview off.
    app.db
        .put_web_preview(&crate::db::link_previews::WebPreview {
            url: "https://docs.example/a".into(),
            fetched_at: arugula_e2e::now_ms(),
            ok: true,
            site: "Docs".into(),
            title: "A page".into(),
            description: "About it".into(),
            image_url: Some("https://docs.example/i.png".into()),
        })
        .unwrap();
    let m = say(&base, "c", &room, "read https://docs.example/a#frag").await;
    let v = previews(&base, "c", &room, &[m]).await;
    let card = &v["previews"][m.to_string()][0];
    assert_eq!((&card["kind"], &card["title"], &card["site"]), (&json!("web"), &json!("A page"), &json!("Docs")));
    assert!(
        card["image"].as_str().unwrap().starts_with("/api/chat/previews/image?page="),
        "readers get our proxy, not the site: {card}"
    );

    let hide = |who: &'static str, hidden: bool| {
        let (base, room) = (base.clone(), room.clone());
        async move {
            call(
                &base,
                who,
                "POST",
                &format!("/api/chat/rooms/{room}/messages/{m}/previews"),
                Some(json!({ "hidden": hidden })),
            )
            .await
            .0
        }
    };
    assert_eq!(hide("a", true).await, 403, "only its author");
    assert_eq!(hide("c", true).await, 200);
    assert_eq!(previews(&base, "a", &room, &[m]).await["previews"], json!({}));
    assert_eq!(hide("c", false).await, 200);
    assert_eq!(previews(&base, "a", &room, &[m]).await["previews"][m.to_string()].as_array().unwrap().len(), 1);

    // The team's switch: owners only, and it hides web cards for everyone.
    let (st, _) = call(&base, "a", "PUT", "/api/chat/teams/u/previews", Some(json!({ "web": false }))).await;
    assert_eq!(st, 403, "a only views u");
    let (st, _) = call(&base, "c", "PUT", "/api/chat/teams/u/previews", Some(json!({ "web": false }))).await;
    assert_eq!(st, 200);
    let v = previews(&base, "a", &room, &[m]).await;
    assert_eq!((v["previews"].clone(), v["web"].clone()), (json!({}), json!(false)));
    let (st, v) = call(&base, "a", "GET", "/api/chat/teams/u/previews", None).await;
    assert_eq!((st, v["web"].clone()), (200, json!(false)));
}
