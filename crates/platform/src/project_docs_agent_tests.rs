//! A machine's agents on a team's Docs (Arugula #862) over the platform's
//! routes: `docs.write` and the owner's role are both asked on every write
//! (so a demoted owner's machine is refused with its grant row still
//! there), reading needs only the owner's reach, a write is the person's
//! (the same revs and counts), and a team's machine's issue home is the
//! team's default project.

use arugula_control_wire::project_docs::{
    ASSET_MAX, agent_project_doc, agent_project_doc_asset, agent_project_docs, project_doc, project_doc_asset,
};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::projects::DOCS_WRITE,
    grants::AGENT_UNGRANTED,
};

/// A call signed by `who`'s machine (`"<account> <machine>"`).
async fn signed(base: &str, who: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", who);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

async fn put_png(base: &str, who: &str, path: &str, tail: &[u8]) -> (u16, Value) {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(tail);
    let r = reqwest::Client::new().put(format!("{base}{path}")).header("x-test-daemon", who).body(bytes).send().await;
    let r = r.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

fn project(id: &str) -> Value {
    json!({ "kind": "project", "id": id })
}

fn new_doc() -> Value {
    json!({
        "title": "Plan",
        "layout": "tabs",
        "sections": [
            { "heading": "Goal", "markdown": "Ship it." },
            { "heading": "Risks", "intent": "what could go wrong" },
        ],
    })
}

fn id_of(v: &Value) -> String {
    v["doc"]["id"].as_str().unwrap().to_owned()
}

/// Without `docs.write` a machine's write is refused saying how to give it,
/// and with it the Doc is the owner's, labelled with the machine. It lands in
/// the team's project, where a teammate reads it, and its revs and counts
/// are a person's make and write of the same. Reading needs no power, and a
/// revoke stops the very next write.
#[tokio::test]
async fn a_machine_with_docs_write_makes_and_writes_its_owners_team_docs() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let docs = agent_project_docs(&t);

    let (st, v) = signed(&base, "a geek", "POST", &docs, Some(new_doc())).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    assert!(v["error"].as_str().unwrap().contains("docs.write"), "{v}");

    let (st, g) = call(&base, "a", "POST", "/api/grants", give("geek", DOCS_WRITE, project(&t))).await;
    assert_eq!(st, 200, "{g}");
    let (st, made) = signed(&base, "a geek", "POST", &docs, Some(new_doc())).await;
    assert_eq!(st, 200, "{made}");
    let id = id_of(&made);
    assert_eq!(
        (&made["doc"]["project"], &made["doc"]["author"], &made["doc"]["layout"], &made["doc"]["agent"]["machine"]),
        (&json!(t), &json!("a"), &json!("tabs"), &json!("geek")),
        "{made}"
    );
    assert!(!made["doc"]["agent"]["machine_name"].as_str().unwrap().is_empty(), "{made}");
    assert_eq!((&made["doc"]["sections"], &made["doc"]["written"]), (&json!(2), &json!(1)), "{made}");

    // The team reads it; an editor sees it as theirs to write.
    let (st, seen) = call(&base, "b", "GET", &project_doc(&t, &id), None).await;
    assert_eq!((st, &seen["writer"], &seen["doc"]["agent"]["machine"]), (200, &json!(true), &json!("geek")), "{seen}");

    // The same make by a person: the same rev and counts.
    let (_, theirs) = call(&base, "a", "POST", &format!("/api/projects/{t}/docs"), Some(new_doc())).await;
    assert_eq!(
        (&made["doc"]["rev"], &made["doc"]["sections"], &made["doc"]["written"]),
        (&theirs["doc"]["rev"], &theirs["doc"]["sections"], &theirs["doc"]["written"]),
        "{made} / {theirs}"
    );

    // A write is the person's: a section's body, and the rev moves once.
    let write = json!({ "section": "s2", "markdown": "Slow CI." });
    let (st, w) = signed(&base, "a geek", "PUT", &agent_project_doc(&t, &id), Some(write.clone())).await;
    assert_eq!(st, 200, "{w}");
    let (_, theirs) = call(&base, "a", "PUT", &project_doc(&t, &id_of(&theirs)), Some(write)).await;
    assert_eq!((&w["sections"][1]["body"], &w["doc"]["written"]), (&json!("Slow CI."), &json!(2)), "{w}");
    assert_eq!(
        (&w["doc"]["rev"], &w["sections"][1]["rev"]),
        (&theirs["doc"]["rev"], &theirs["sections"][1]["rev"]),
        "{w} / {theirs}"
    );
    let (_, again) =
        signed(&base, "a geek", "PUT", &agent_project_doc(&t, &id), Some(json!({ "title": "Plan B" }))).await;
    assert_eq!(again["doc"]["rev"].as_u64().unwrap(), w["doc"]["rev"].as_u64().unwrap() + 1, "{again}");

    // The revoke stops the next write, and reading still works: no power.
    let (st, _) = call(&base, "a", "POST", &format!("/api/grants/{}/revoke", g["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200);
    let (st, v) = signed(&base, "a geek", "PUT", &agent_project_doc(&t, &id), Some(json!({ "title": "No" }))).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let (st, v) = signed(&base, "a geek", "GET", &agent_project_doc(&t, &id), None).await;
    assert_eq!((st, &v["doc"]["title"]), (200, &json!("Plan B")), "{v}");
}

/// A machine acts for its owner and never past them: another team's project
/// and a Doc that isn't there are 404, and a person's session isn't a machine.
#[tokio::test]
async fn a_machine_reaches_only_what_its_owner_reads() {
    let (_hub, _app, base, t, u) = seeded().await;
    let (st, _) = call(&base, "a", "POST", "/api/grants", give("geek", DOCS_WRITE, project(&t))).await;
    assert_eq!(st, 200);
    let (_, made) = signed(&base, "a geek", "POST", &agent_project_docs(&t), Some(new_doc())).await;
    let id = id_of(&made);

    let (st, v) = signed(&base, "a geek", "POST", &agent_project_docs(&u), Some(new_doc())).await;
    assert_eq!(st, 404, "{v}");
    assert_eq!(signed(&base, "a geek", "GET", &agent_project_doc(&u, &id), None).await.0, 404, "t's Doc under u");
    let missing = "pdc_00000000000000000000000000000000";
    assert_eq!(signed(&base, "a geek", "GET", &agent_project_doc(&t, missing), None).await.0, 404);
    assert_eq!(call(&base, "a", "GET", &agent_project_doc(&t, &id), None).await.0, 401, "a session isn't a machine");
}

/// The trap: a machine's grant row outlives its owner's role. An editor's
/// machine writes while they're an editor; once they're a viewer it's refused
/// (a make and a write), with the grant still live, and it still reads.
#[tokio::test]
async fn a_demoted_owners_machine_is_refused_with_its_grant_still_there() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("bgeek", "b", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let (st, g) = call(&base, "b", "POST", "/api/grants", give("bgeek", DOCS_WRITE, project(&t))).await;
    assert_eq!(st, 200, "{g}");
    let (st, made) = signed(&base, "b bgeek", "POST", &agent_project_docs(&t), Some(new_doc())).await;
    assert_eq!(st, 200, "{made}");
    let id = id_of(&made);

    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer), ("x", TeamRole::Viewer)]);
    assert!(app.db.live_grant("bgeek", DOCS_WRITE, ("project", &t), "b").unwrap().is_some(), "the row is still live");

    let (st, v) = signed(&base, "b bgeek", "POST", &agent_project_docs(&t), Some(new_doc())).await;
    assert_eq!(st, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("viewer"), "{v}");
    assert!(v["reason"].is_null(), "a viewer, not a missing grant: {v}");
    let (st, v) =
        signed(&base, "b bgeek", "PUT", &agent_project_doc(&t, &id), Some(json!({ "title": "Mine still" }))).await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = put_png(&base, "b bgeek", &agent_project_doc_asset(&t, &id, "pic.png"), b"x").await;
    assert_eq!(st, 403, "{v}");
    let (st, v) = signed(&base, "b bgeek", "GET", &agent_project_doc(&t, &id), None).await;
    assert_eq!((st, &v["doc"]["title"], &v["writer"]), (200, &json!("Plan"), &json!(false)), "{v}");
}

/// A picture goes in as a machine copy's does (type by the bytes, 2 MB), is
/// listed on the Doc and read back through the person's route; it needs the
/// grant like any write.
#[tokio::test]
async fn a_machine_uploads_a_picture_with_a_copys_limits() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let (_, made) = signed(&base, "a geek", "POST", &agent_project_docs(&t), Some(new_doc())).await;
    assert!(made["error"].as_str().is_some(), "no grant yet: {made}");
    let (st, _) = call(&base, "a", "POST", "/api/grants", give("geek", DOCS_WRITE, project(&t))).await;
    assert_eq!(st, 200);
    let (_, made) = signed(&base, "a geek", "POST", &agent_project_docs(&t), Some(new_doc())).await;
    let id = id_of(&made);
    let path = |name: &str| agent_project_doc_asset(&t, &id, name);

    let (st, a) = put_png(&base, "a geek", &path("pic.png"), b"one").await;
    assert_eq!((st, &a["mime"], &a["size"]), (200, &json!("image/png"), &json!(11)), "{a}");
    let (_, v) = signed(&base, "a geek", "GET", &agent_project_doc(&t, &id), None).await;
    assert_eq!(v["assets"][0]["name"], "pic.png", "{v}");
    let r = reqwest::Client::new()
        .get(format!("{base}{}", project_doc_asset(&t, &id, "pic.png")))
        .header("cookie", "ilg_session=session-b")
        .send()
        .await
        .unwrap();
    assert_eq!((r.status().as_u16(), r.bytes().await.unwrap().len()), (200, 11), "a teammate reads it");

    assert_eq!(put_png(&base, "a geek", &path("pic.jpg"), b"x").await.0, 400, "named for another type");
    assert_eq!(put_png(&base, "a geek", &path("pic.txt"), b"x").await.0, 400, "not a picture's name");
    let big = vec![0u8; ASSET_MAX];
    assert_eq!(put_png(&base, "a geek", &path("big.png"), &big).await.0, 413, "past 2 MB");
    let (st, _) =
        signed(&base, "a geek", "GET", &agent_project_doc(&t, "pdc_00000000000000000000000000000000"), None).await;
    assert_eq!(st, 404);
}

/// A team's machine's issue home, with no repo or one linked to nothing,
/// is the team's default project, asked of no power; a personal machine's is
/// the owner's personal one, as it was; a repo linked to a project of the
/// owner's still says that one.
#[tokio::test]
async fn a_team_machines_home_is_the_teams_default_project() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("apad", "a", None);
    crate::cache::catch_up(&app).await.unwrap();
    let personal = crate::projects::personal_project(&app, "a").await.unwrap().unwrap();
    assert_ne!(personal, t);

    // Without a grant of any kind: the team's project is named, no more.
    let (st, h) = signed(&base, "a geek", "GET", "/api/daemon/issue-home", None).await;
    assert_eq!((st, &h["project"]), (200, &json!(t)), "{h}");
    let (st, h) = signed(&base, "a geek", "GET", "/api/daemon/issue-home?repo=acme/none", None).await;
    assert_eq!((st, &h["project"]), (200, &json!(t)), "linked to nothing: {h}");
    // A personal machine's, with its grant, is as it was.
    let (st, _) = call(&base, "a", "POST", "/api/grants", give("apad", "threads.read", project(&personal))).await;
    assert_eq!(st, 200);
    let (st, h) = signed(&base, "a apad", "GET", "/api/daemon/issue-home", None).await;
    assert_eq!((st, &h["project"]), (200, &json!(personal)), "{h}");
    let (st, h) = signed(&base, "a apad", "GET", "/api/daemon/issue-home?repo=acme/none", None).await;
    assert_eq!(st, 404, "a personal machine's unlinked repo is still refused: {h}");

    // A repo linked to a project the owner is in wins over the default.
    let i = crate::db::forge::Installation {
        id: 9,
        account_id: 500,
        login: "acme".into(),
        account_type: "Organization".into(),
        permissions: "{}".into(),
    };
    let l = crate::db::forge::LinkedRepo {
        repo_id: 7,
        full_name: "acme/app".into(),
        private: true,
        project_id: personal.clone(),
        installation_id: 9,
        linked_by: Some("a".into()),
        linked_at: 0,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    app.db.link_repo(&l, &i, "a", 1).unwrap();
    let (st, _) = call(&base, "a", "POST", "/api/grants", give("geek", "threads.read", project(&personal))).await;
    assert_eq!(st, 200);
    let (st, h) = signed(&base, "a geek", "GET", "/api/daemon/issue-home?repo=acme/app", None).await;
    assert_eq!((st, &h["project"]), (200, &json!(personal)), "{h}");
}

/// A machine's copy of a Doc (kept for its machine until copies go) is
/// read-only to an agent as it is to a person: a write or a picture is
/// refused, naming the machine that holds it.
#[tokio::test]
async fn a_machines_copy_is_read_only_to_an_agent() {
    let (hub, app, base, _t, _u) = seeded().await;
    hub.daemon("apad", "a", None);
    crate::cache::catch_up(&app).await.unwrap();
    let body = json!({ "title": "Copy", "sections": [{ "id": "s1", "heading": "One", "body": "x" }] });
    let (st, placed) = signed(&base, "a apad", "PUT", "/api/daemon/docs/d1", Some(body)).await;
    assert_eq!(st, 200, "{placed}");
    let (home, id) = (placed["project"].as_str().unwrap(), placed["id"].as_str().unwrap());
    let (st, _) = call(&base, "a", "POST", "/api/grants", give("geek", DOCS_WRITE, project(home))).await;
    assert_eq!(st, 200);

    let (st, v) = signed(&base, "a geek", "PUT", &agent_project_doc(home, id), Some(json!({ "title": "Mine" }))).await;
    assert!(st == 403 && v["error"].as_str().unwrap().contains("read-only"), "{st} {v}");
    let (st, v) = put_png(&base, "a geek", &agent_project_doc_asset(home, id, "pic.png"), b"x").await;
    assert!(st == 403 && v["error"].as_str().unwrap().contains("read-only"), "{st} {v}");
    let (st, v) = signed(&base, "a geek", "GET", &agent_project_doc(home, id), None).await;
    assert_eq!((st, &v["doc"]["title"], &v["writer"]), (200, &json!("Copy"), &json!(false)), "{v}");
}

/// A viewer's machine isn't given `docs.write`: it would do nothing for it.
#[tokio::test]
async fn a_viewers_machine_is_not_given_docs_write() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("xbox", "x", Some("t"));
    crate::cache::catch_up(&app).await.unwrap();
    let (st, r) = call(&base, "x", "POST", "/api/grants", give("xbox", DOCS_WRITE, project(&t))).await;
    assert_eq!((st, r["reason"].as_str()), (403, Some(crate::grants::VIEWER_WRITE)), "{r}");
    // Never over a person's DMs, and a token isn't given it.
    let (st, _) = call(&base, "a", "POST", "/api/grants", give("geek", DOCS_WRITE, json!({ "kind": "account" }))).await;
    assert_eq!(st, 400);
}
