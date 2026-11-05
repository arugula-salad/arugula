//! A machine's Docs copied up (#418) over the platform's routes: the first
//! put makes the copy in its owner's personal project, where no one else
//! (a teammate included) can read it or its pictures; the machine is in the
//! key, so no machine reaches another's; a copy that sits in a team's
//! project (moved there before the move route went, native #673) takes
//! the next put there while its owner may write there, and is refused
//! once they can't. Run on Postgres too by `just test-platform-postgres`.

use arugula_control_wire::project_docs::{ASSETS_MAX, machine_doc, machine_doc_asset, project_doc_asset};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{
    agents_tests::seeded,
    chat_tests::call,
    db::sql::{Sql as _, params},
};

/// A call signed by `who`'s machine (`"<account> <machine>"`).
async fn signed(base: &str, who: &str, method: &str, path: &str, body: Option<Vec<u8>>, json: bool) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", who);
    if let Some(b) = body {
        req = if json { req.header("content-type", "application/json").body(b) } else { req.body(b) };
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

async fn put_doc(base: &str, who: &str, md: &str, title: &str, bodies: &[(&str, Option<&str>)]) -> (u16, Value) {
    let sections: Vec<Value> =
        bodies.iter().map(|(id, b)| json!({ "id": id, "heading": format!("Heading {id}"), "body": b })).collect();
    let body = serde_json::to_vec(&json!({ "title": title, "sections": sections })).unwrap();
    signed(base, who, "PUT", &machine_doc(md), Some(body), true).await
}

async fn put_png(base: &str, who: &str, md: &str, name: &str, tail: &[u8]) -> (u16, Value) {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(tail);
    signed(base, who, "PUT", &machine_doc_asset(md, name), Some(bytes), false).await
}

fn id_of(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

async fn raw_get(base: &str, who: &str, path: &str) -> (u16, String, Vec<u8>) {
    let r = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .header("cookie", format!("ilg_session=session-{who}"))
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    (st, mime, r.bytes().await.unwrap().to_vec())
}

/// The first put copies the Doc into its owner's personal project (not the
/// team's), the same content again changes nothing, and new content moves
/// its rev. Another account, a teammate in the owner's team included, gets
/// 404 on the Doc and on its pictures; the owner reads both.
#[tokio::test]
async fn a_copy_lands_in_the_owners_personal_project_and_stays_private() {
    let (hub, app, base, t, u) = seeded().await;
    pad(&hub, &app).await;
    hub.flag("team:u", true);
    let (st, p) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("Ship it.")), ("s2", None)]).await;
    assert_eq!(st, 200, "{p}");
    let (id, personal) = (id_of(&p), p["project"].as_str().unwrap().to_owned());
    assert!(id.starts_with("pdc_") && personal != t, "{p}");
    assert!(p["link"].as_str().unwrap().ends_with(&format!("/p/{personal}/docs/{id}")), "{p}");

    // The same content again is the same Doc, untouched.
    let (_, again) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("Ship it.")), ("s2", None)]).await;
    assert_eq!((id_of(&again), &again["rev"]), (id.clone(), &p["rev"]), "{again}");
    let (_, newer) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("Shipped.")), ("s2", None)]).await;
    assert_eq!(newer["rev"], json!(p["rev"].as_u64().unwrap() + 1), "{newer}");

    let (st, a) = put_png(&base, "a apad", "d1", "pic.png", b"one").await;
    assert_eq!((st, &a["mime"], &a["size"]), (200, &json!("image/png"), &json!(11)), "{a}");

    // The owner reads it, with its section and its picture.
    let path = format!("/api/projects/{personal}/docs/{id}");
    let (st, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["sections"][0]["body"], &v["doc"]["machine"], &v["assets"][0]["name"]),
        (&json!("Shipped."), &json!("apad"), &json!("pic.png"))
    );
    let (st, mime, bytes) = raw_get(&base, "a", &project_doc_asset(&personal, &id, "pic.png")).await;
    assert_eq!((st, mime.as_str(), bytes.len()), (200, "image/png", 11));

    // Everyone else: 404, the owner's teammates and another team's owner too.
    for who in ["b", "x", "c"] {
        for project in [&personal, &t, &u] {
            let (st, _) = call(&base, who, "GET", &format!("/api/projects/{project}/docs/{id}"), None).await;
            assert_eq!(st, 404, "{who} reads the Doc under {project}");
            let (st, _, _) = raw_get(&base, who, &project_doc_asset(project, &id, "pic.png")).await;
            assert_eq!(st, 404, "{who} reads the picture under {project}");
        }
        let (st, _) = call(&base, who, "GET", &format!("/api/projects/{personal}/docs"), None).await;
        assert_eq!(st, 404, "{who} lists the personal project");
    }
    // And the team's list doesn't have it.
    let (_, l) = call(&base, "b", "GET", &format!("/api/projects/{t}/docs"), None).await;
    assert!(l["docs"].as_array().unwrap().is_empty(), "{l}");
}

/// A machine is in the key: another machine putting the same
/// `machine_doc` (even one of the same account) makes a Doc of its own and
/// never touches the first's, and can't delete it.
#[tokio::test]
async fn another_machine_with_the_same_doc_id_gets_its_own_doc() {
    let (hub, app, base, _t, _u) = seeded().await;
    pad(&hub, &app).await;
    hub.daemon("lap", "a", None);
    hub.daemon("bgeek", "b", None);
    let (_, x) = put_doc(&base, "a apad", "d1", "X's", &[("s1", Some("from geek"))]).await;
    put_png(&base, "a apad", "d1", "pic.png", b"x").await;

    let (st, y) = put_doc(&base, "a lap", "d1", "Y's", &[("s1", Some("from lap"))]).await;
    assert_eq!(st, 200, "{y}");
    assert_ne!(id_of(&y), id_of(&x));
    let (_, z) = put_doc(&base, "b bgeek", "d1", "Z's", &[]).await;
    assert!(z["project"] != x["project"] && id_of(&z) != id_of(&x), "{z}");

    // X's is as it was, pictures and all; Y has none.
    let path = format!("/api/projects/personal/docs/{}", id_of(&x));
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(
        (&v["doc"]["title"], &v["sections"][0]["body"], v["assets"].as_array().unwrap().len()),
        (&json!("X's"), &json!("from geek"), 1),
        "{v}"
    );
    let (st, _) = put_png(&base, "a lap", "d9", "pic.png", b"y").await;
    assert_eq!(st, 404, "a picture needs its Doc first");

    // Y takes its copy down; X's stays.
    let (st, _) = signed(&base, "a lap", "DELETE", &machine_doc("d1"), None, false).await;
    assert_eq!(st, 200);
    let (st, _) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(st, 200);
    let (st, _) = signed(&base, "a lap", "DELETE", &machine_doc("d1"), None, false).await;
    assert_eq!(st, 404);
}

/// A copy in a team's project (one moved there while the move route
/// existed) takes the machine's next put there, with its owner's right to
/// write there checked: the key and the pictures stay, no second copy is
/// made at home, and once the owner may no longer write there the put is
/// refused with a reason, and nothing changes.
#[tokio::test]
async fn a_copy_in_a_team_project_takes_puts_while_its_owner_may_write_there() {
    let (hub, app, base, t, _u) = seeded().await;
    pad(&hub, &app).await;
    hub.daemon("bgeek", "b", None);
    let (_, p) = put_doc(&base, "b bgeek", "d1", "Plan", &[("s1", Some("v1"))]).await;
    let id = id_of(&p);
    put_png(&base, "b bgeek", "d1", "pic.png", b"one").await;
    moved_before(&app, &id, &t);

    // b is an editor of t: the put lands there, picture included, and
    // nothing comes back to b's personal project.
    let (st, q) = put_doc(&base, "b bgeek", "d1", "Plan", &[("s1", Some("v2"))]).await;
    assert_eq!((st, id_of(&q), &q["project"]), (200, id.clone(), &json!(t)), "{q}");
    let (st, _) = put_png(&base, "b bgeek", "d1", "more.png", b"two").await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "a", "GET", &format!("/api/projects/{t}/docs/{id}"), None).await;
    assert_eq!((&v["sections"][0]["body"], v["assets"].as_array().unwrap().len()), (&json!("v2"), 2), "{v}");
    let (st, _, bytes) = raw_get(&base, "x", &project_doc_asset(&t, &id, "pic.png")).await;
    assert_eq!((st, bytes.len()), (200, 11), "the team reads its pictures");
    let (_, l) = call(&base, "b", "GET", "/api/projects/personal/docs", None).await;
    assert!(l["docs"].as_array().unwrap().is_empty(), "no second copy at home: {l}");

    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer), ("x", TeamRole::Viewer)]);
    let (st, e) = put_doc(&base, "b bgeek", "d1", "Plan", &[("s1", Some("v2"))]).await;
    assert!(st == 403 && e["error"].as_str().unwrap().contains("no longer write"), "{st} {e}");
    let (st, _) = put_png(&base, "b bgeek", "d1", "pic.png", b"x").await;
    assert_eq!(st, 403);
    let (_, v) = call(&base, "a", "GET", &format!("/api/projects/{t}/docs/{id}"), None).await;
    assert_eq!((&v["sections"][0]["body"], v["assets"].as_array().unwrap().len()), (&json!("v2"), 2), "{v}");
}

/// A copy as the move route left it before it went (native #673): in
/// another project, with its key.
fn moved_before(app: &crate::Platform, id: &str, to: &str) {
    let n = app.db.c().execute("UPDATE project_docs SET project_id = ?1 WHERE id = ?2", params![to, id]).unwrap();
    assert_eq!(n, 1);
}

/// A picture is a PNG, JPEG, GIF or WebP by its bytes, named for what it
/// is; the same bytes again change nothing; a Doc keeps 20.
#[tokio::test]
async fn pictures_are_checked_by_their_bytes() {
    let (hub, app, base, _t, _u) = seeded().await;
    pad(&hub, &app).await;
    put_doc(&base, "a apad", "d1", "Plan", &[]).await;
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec();
    let (st, _) = signed(&base, "a apad", "PUT", &machine_doc_asset("d1", "a.png"), Some(svg), false).await;
    assert_eq!(st, 400, "an SVG isn't a picture");
    let (st, _) = put_png(&base, "a apad", "d1", "a.jpg", b"x").await;
    assert_eq!(st, 400, "named for another type");
    let (st, _) = put_png(&base, "a apad", "d1", "../a.png", b"x").await;
    assert!(st == 400 || st == 404, "{st}");

    let (_, first) = put_png(&base, "a apad", "d1", "a.png", b"x").await;
    let (_, same) = put_png(&base, "a apad", "d1", "a.png", b"x").await;
    assert_eq!(same["rev"], first["rev"]);
    let (_, other) = put_png(&base, "a apad", "d1", "a.png", b"y").await;
    assert!(other["rev"].as_u64() > first["rev"].as_u64(), "{other}");

    for i in 1..ASSETS_MAX {
        let (st, v) = put_png(&base, "a apad", "d1", &format!("p{i}.png"), b"x").await;
        assert_eq!(st, 200, "{v}");
    }
    let (st, v) = put_png(&base, "a apad", "d1", "one-more.png", b"x").await;
    assert!(st == 400 && v["error"].as_str().unwrap().contains("at most 20"), "{v}");
}

/// The machine's library Doc is the truth and its copy here is read-only
/// (Arugula #678): a person's write to it, a section or its title, is
/// refused naming the machine, its view has no writer, and the machine's
/// next put still lands. (Before: the write went through, and the next put
/// silently put the edit back and dropped the added section.)
#[tokio::test]
async fn a_copy_is_read_only_and_the_machines_puts_still_land() {
    let (hub, app, base, t, _u) = seeded().await;
    pad(&hub, &app).await;
    let (_, p) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("v1")), ("s2", None)]).await;
    let (id, personal) = (id_of(&p), p["project"].as_str().unwrap().to_owned());
    let path = format!("/api/projects/{personal}/docs/{id}");

    let (st, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["writer"], &v["doc"]["machine"], &v["doc"]["machine_name"]),
        (&json!(false), &json!("apad"), &json!("apad-box")),
        "{v}"
    );
    let (_, l) = call(&base, "a", "GET", &format!("/api/projects/{personal}/docs"), None).await;
    assert_eq!((&l["docs"][0]["machine_name"], &l["writer"]), (&json!("apad-box"), &json!(true)), "{l}");

    for edit in [
        json!({ "section": "s1", "markdown": "edited on the web" }),
        json!({ "heading": "Added on the web", "markdown": "new" }),
        json!({ "title": "Renamed on the web" }),
    ] {
        let (st, e) = call(&base, "a", "PUT", &path, Some(edit.clone())).await;
        assert_eq!(st, 403, "{edit}: {e}");
        let why = e["error"].as_str().unwrap();
        assert!(why.contains("copy") && why.contains("apad-box"), "{why}");
    }
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!((&v["doc"]["title"], bodies_of(&v)), (&json!("Plan"), vec![json!("v1"), Value::Null]), "{v}");

    // The machine's next put lands, as it did.
    let (st, q) = put_doc(&base, "a apad", "d1", "Plan, v2", &[("s1", Some("v2"))]).await;
    assert_eq!((st, &q["id"]), (200, &json!(id)), "{q}");
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(
        (&v["doc"]["title"], bodies_of(&v), &v["writer"]),
        (&json!("Plan, v2"), vec![json!("v2")], &json!(false))
    );

    // In the team's project (moved there before the move route went),
    // it's a copy there too: an editor can't write it, and the machine's
    // put still lands there.
    moved_before(&app, &id, &t);
    let team_path = format!("/api/projects/{t}/docs/{id}");
    let (st, m) = call(&base, "b", "GET", &team_path, None).await;
    assert_eq!((st, &m["writer"], &m["doc"]["machine_name"]), (200, &json!(false), &json!("apad-box")), "{m}");
    let (st, e) = call(&base, "b", "PUT", &team_path, Some(json!({ "section": "s1", "markdown": "b's" }))).await;
    assert!(st == 403 && e["error"].as_str().unwrap().contains("apad-box"), "{st} {e}");
    let (st, q) = put_doc(&base, "a apad", "d1", "Plan, v3", &[("s1", Some("v3"))]).await;
    assert_eq!((st, &q["project"]), (200, &json!(t)), "{q}");
    let (_, v) = call(&base, "b", "GET", &team_path, None).await;
    assert_eq!((bodies_of(&v), &v["writer"]), (vec![json!("v3")], &json!(false)), "{v}");
}

/// a's personal machine, apad: a team's machine copies nothing (native #673).
async fn pad(hub: &crate::testing::hub::TestHub, app: &crate::Platform) {
    hub.daemon("apad", "a", None);
    crate::cache::catch_up(app).await.unwrap();
}

fn bodies_of(v: &Value) -> Vec<Value> {
    v["sections"].as_array().unwrap().iter().map(|s| s["body"].clone()).collect()
}

/// A copy goes with its Doc (native #673): the machine takes it down when
/// the Doc is deleted there, wherever the copy sits, and a copy gone
/// already is 404. And a copy is its project's to delete, as any Doc
/// there: the owner in their personal project, an editor where a move
/// left it; the machine's next change makes a new copy, under a new id.
#[tokio::test]
async fn a_copy_goes_with_its_doc_and_is_its_projects_to_delete() {
    let (hub, app, base, t, _u) = seeded().await;
    pad(&hub, &app).await;
    let (_, p) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("v1"))]).await;
    let (id, personal) = (id_of(&p), p["project"].as_str().unwrap().to_owned());
    let path = format!("/api/projects/{personal}/docs/{id}");
    let (st, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!((st, &v["deleter"], &v["writer"]), (200, &json!(true), &json!(false)), "{v}");
    assert_eq!(call(&base, "a", "DELETE", &path, None).await.0, 200);
    assert_eq!(call(&base, "a", "GET", &path, None).await.0, 404);
    // The machine's next change: a new copy, its own Doc again.
    let (st, q) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("v2"))]).await;
    assert_eq!(st, 200, "{q}");
    let again = id_of(&q);
    assert_ne!(again, id, "a new copy: {q}");
    assert_eq!(call(&base, "a", "GET", &format!("/api/projects/{personal}/docs/{again}"), None).await.0, 200);

    // Where a move left it, the team's project: an editor may delete it,
    // and the machine takes it down when its Doc goes.
    {
        use crate::db::sql::{Sql as _, params};
        let n = app.db.c().execute("UPDATE project_docs SET project_id = ?1 WHERE id = ?2", params![t, again]).unwrap();
        assert_eq!(n, 1);
    }
    let team_path = format!("/api/projects/{t}/docs/{again}");
    let (_, v) = call(&base, "b", "GET", &team_path, None).await;
    assert_eq!((&v["deleter"], &v["writer"]), (&json!(true), &json!(false)), "{v}");
    let (_, v) = call(&base, "x", "GET", &team_path, None).await;
    assert_eq!(v["deleter"], json!(false), "a viewer: {v}");
    assert_eq!(call(&base, "x", "DELETE", &team_path, None).await.0, 403);
    let (st, d) = signed(&base, "a apad", "DELETE", &machine_doc("d1"), None, true).await;
    assert_eq!((st, &d["deleted"]), (200, &json!(again)), "{d}");
    assert_eq!(call(&base, "b", "GET", &team_path, None).await.0, 404);
    // Gone already: 404, which the machine takes as done.
    assert_eq!(signed(&base, "a apad", "DELETE", &machine_doc("d1"), None, true).await.0, 404);
    // And a copy there is the editor's to delete, as the owner's.
    let (_, p) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("v3"))]).await;
    let third = id_of(&p);
    {
        use crate::db::sql::{Sql as _, params};
        app.db.c().execute("UPDATE project_docs SET project_id = ?1 WHERE id = ?2", params![t, third]).unwrap();
    }
    assert_eq!(call(&base, "b", "DELETE", &format!("/api/projects/{t}/docs/{third}"), None).await.0, 200);
}

/// Native #673: a machine's copies take chat or its account's own word for
/// them, and the word turns nothing else on. An account with chat off (in
/// no team that has it) can't copy until it says so; then its machine puts,
/// and it lists, opens, sees the pictures of and deletes its copies while
/// its chat stays off; a Doc of its own project that isn't a copy isn't
/// listed. A copy sitting in a team's project with chat off takes no put on
/// the word. With the word off again, nothing new is put, and the machine
/// still takes a copy down.
#[tokio::test]
async fn copies_take_the_accounts_own_word_and_turn_nothing_else_on() {
    let (hub, app, base, _t, u) = seeded().await;
    pad(&hub, &app).await;
    hub.daemon("cbox", "c", None);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, e) = put_doc(&base, "c cbox", "d1", "Plan", &[("s1", Some("v1"))]).await;
    assert_eq!(st, 403, "no chat, no word: {e}");

    hub.flag_named("account:c", "doc-copies", true);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, p) = put_doc(&base, "c cbox", "d1", "Plan", &[("s1", Some("v1"))]).await;
    assert_eq!(st, 200, "{p}");
    let path = format!("/api/projects/personal/docs/{}", id_of(&p));
    let (st, v) = call(&base, "c", "GET", &path, None).await;
    assert_eq!((st, &v["doc"]["machine"]), (200, &json!("cbox")), "c opens its own copy: {v}");
    let (st, a) = put_png(&base, "c cbox", "d1", "pic.png", b"one").await;
    assert_eq!(st, 200, "{a}");
    let (st, _, bytes) = raw_get(&base, "c", &format!("{path}/assets/pic.png")).await;
    assert_eq!((st, bytes.len()), (200, 11), "its pictures too");
    // A Doc of c's own project made there, not a copy: chat's, so not listed.
    let personal = crate::projects::personal_project(&app, "c").await.unwrap().unwrap();
    let made = crate::db::project_docs::NewDoc {
        project: &personal,
        title: "Made here",
        made_by: "c",
        made_by_session: None,
        sections: &[],
    };
    app.db.create_project_doc(&made, 1, 1_000).unwrap().unwrap();
    let (st, l) = call(&base, "c", "GET", "/api/projects/personal/docs", None).await;
    assert_eq!(st, 200, "{l}");
    let titles: Vec<&str> = l["docs"].as_array().unwrap().iter().map(|d| d["title"].as_str().unwrap()).collect();
    assert_eq!((titles, &l["writer"]), (vec!["Plan"], &json!(false)), "its copies alone, nothing to make: {l}");
    // Chat stays off for c everywhere else.
    let (st, _) = call(&base, "c", "POST", "/api/projects/personal/docs", Some(json!({ "title": "x" }))).await;
    assert_eq!(st, 403, "chat stays off for c");
    let (st, _) = call(&base, "c", "PUT", &path, Some(json!({ "section": "s1", "markdown": "x" }))).await;
    assert_eq!(st, 403, "a copy stays read-only");

    // A copy sitting in c's team u (chat off there) takes no put on the word.
    let (_, q) = put_doc(&base, "c cbox", "d4", "Team's", &[]).await;
    let n = app.db.c().execute("UPDATE project_docs SET project_id = ?1 WHERE id = ?2", params![u, id_of(&q)]).unwrap();
    assert_eq!(n, 1);
    let (st, e) = put_doc(&base, "c cbox", "d4", "Team's, v2", &[]).await;
    assert_eq!(st, 403, "the team's chat decides there: {e}");

    // Off: nothing new goes up, and the machine still takes a copy down.
    hub.flag_named("account:c", "doc-copies", false);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, _) = put_doc(&base, "c cbox", "d2", "Notes", &[]).await;
    assert_eq!(st, 403, "off: no new copies");
    let (st, d) = signed(&base, "c cbox", "DELETE", &machine_doc("d1"), None, true).await;
    assert_eq!(st, 200, "a take-down needs no word: {d}");
    assert_eq!(call(&base, "c", "GET", &path, None).await.0, 404);

    // And c deletes a copy of theirs with the word off and chat off.
    hub.flag_named("account:c", "doc-copies", true);
    crate::cache::catch_up(&app).await.unwrap();
    let (_, p) = put_doc(&base, "c cbox", "d3", "Kept", &[]).await;
    let path = format!("/api/projects/personal/docs/{}", id_of(&p));
    hub.flag_named("account:c", "doc-copies", false);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(call(&base, "c", "DELETE", &path, None).await.0, 200);
    assert_eq!(call(&base, "c", "GET", &path, None).await.0, 404);
}

/// Native #673: a team's machine copies no Docs up, whatever daemon it runs:
/// its puts and its pictures are refused saying why, and its take-down of an
/// old copy still goes. Nothing is lost: the Docs stay on the machine.
#[tokio::test]
async fn a_teams_machine_copies_nothing_and_still_takes_an_old_copy_down() {
    let (hub, app, base, _t, _u) = seeded().await;
    pad(&hub, &app).await;
    let (st, e) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("v1"))]).await;
    assert_eq!(st, 403, "{e}");
    assert!(e["error"].as_str().unwrap().contains("team's machine"), "{e}");
    assert_eq!(put_png(&base, "a geek", "d1", "pic.png", b"one").await.0, 403);
    // A copy geek made before (as a personal machine's put left it).
    let (_, p) = put_doc(&base, "a apad", "d1", "Plan", &[("s1", Some("v1"))]).await;
    let id = id_of(&p);
    let n = app.db.c().execute("UPDATE project_docs SET machine = 'geek' WHERE id = ?1", params![id]).unwrap();
    assert_eq!(n, 1);
    let (st, d) = signed(&base, "a geek", "DELETE", &machine_doc("d1"), None, true).await;
    assert_eq!((st, &d["deleted"]), (200, &json!(id)), "{d}");
}

/// Native #673: an account's explicit no to copies is a no even with chat
/// on (an older daemon puts on chat), and a machine taken out of its team
/// copies again, on its account's word.
#[tokio::test]
async fn an_explicit_no_wins_over_chat_and_a_machine_out_of_its_team_copies_again() {
    let (hub, app, base, _t, _u) = seeded().await;
    pad(&hub, &app).await;
    // a has chat on through team t, and said no.
    hub.flag_named("account:a", "doc-copies", false);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, e) = put_doc(&base, "a apad", "d1", "Plan", &[]).await;
    assert_eq!(st, 403, "{e}");
    assert!(e["error"].as_str().unwrap().contains("turned copying"), "{e}");
    hub.flag_named("account:a", "doc-copies", true);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(put_doc(&base, "a apad", "d1", "Plan", &[]).await.0, 200);

    // geek, a team's machine, is refused; taken out of the team, it copies.
    assert_eq!(put_doc(&base, "a geek", "d2", "Notes", &[]).await.0, 403);
    hub.daemon("geek", "a", None);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, p) = put_doc(&base, "a geek", "d2", "Notes", &[]).await;
    assert_eq!(
        (st, &p["project"]),
        (200, &json!(crate::projects::personal_project(&app, "a").await.unwrap().unwrap())),
        "{p}"
    );
}
