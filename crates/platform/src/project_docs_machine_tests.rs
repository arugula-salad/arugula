//! A machine's Docs copied up (#418) over the platform's routes: the first
//! put makes the copy in its owner's personal project, where no one else
//! (a teammate included) can read it or its pictures; the machine is in the
//! key, so no machine reaches another's; a move into a team's project keeps
//! the id and the next put lands there; and a move needs the right to write
//! in both projects. Run on Postgres too by `just test-platform-postgres`.

use arugula_control_wire::project_docs::{ASSETS_MAX, machine_doc, machine_doc_asset, project_doc_asset};
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use crate::{agents_tests::seeded, chat_tests::call};

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
    let (hub, _app, base, t, u) = seeded().await;
    hub.flag("team:u", true);
    let (st, p) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("Ship it.")), ("s2", None)]).await;
    assert_eq!(st, 200, "{p}");
    let (id, personal) = (id_of(&p), p["project"].as_str().unwrap().to_owned());
    assert!(id.starts_with("pdc_") && personal != t, "{p}");
    assert!(p["link"].as_str().unwrap().ends_with(&format!("/p/{personal}/docs/{id}")), "{p}");

    // The same content again is the same Doc, untouched.
    let (_, again) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("Ship it.")), ("s2", None)]).await;
    assert_eq!((id_of(&again), &again["rev"]), (id.clone(), &p["rev"]), "{again}");
    let (_, newer) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("Shipped.")), ("s2", None)]).await;
    assert_eq!(newer["rev"], json!(p["rev"].as_u64().unwrap() + 1), "{newer}");

    let (st, a) = put_png(&base, "a geek", "d1", "pic.png", b"one").await;
    assert_eq!((st, &a["mime"], &a["size"]), (200, &json!("image/png"), &json!(11)), "{a}");

    // The owner reads it, with its section and its picture.
    let path = format!("/api/projects/{personal}/docs/{id}");
    let (st, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["sections"][0]["body"], &v["doc"]["machine"], &v["assets"][0]["name"]),
        (&json!("Shipped."), &json!("geek"), &json!("pic.png"))
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
    let (hub, _app, base, _t, _u) = seeded().await;
    hub.daemon("lap", "a", None);
    hub.daemon("bgeek", "b", Some("t"));
    let (_, x) = put_doc(&base, "a geek", "d1", "X's", &[("s1", Some("from geek"))]).await;
    put_png(&base, "a geek", "d1", "pic.png", b"x").await;

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

/// Sharing with the team moves the Doc: it keeps its id, key and pictures,
/// the team reads it, and the machine's next put lands there, not in a new
/// personal copy.
#[tokio::test]
async fn after_a_move_the_machines_next_put_lands_in_the_team_project() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let (_, p) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("v1"))]).await;
    let id = id_of(&p);
    put_png(&base, "a geek", "d1", "pic.png", b"one").await;

    let mv = format!("/api/projects/personal/docs/{id}/move");
    let (st, m) = call(&base, "a", "POST", &mv, Some(json!({ "to": t }))).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(
        (&m["doc"]["id"], &m["doc"]["project"], &m["doc"]["machine"], &m["assets"][0]["name"]),
        (&json!(id), &json!(t), &json!("geek"), &json!("pic.png"))
    );
    let (st, _) =
        call(&base, "a", "GET", &format!("/api/projects/{}/docs/{id}", p["project"].as_str().unwrap()), None).await;
    assert_eq!(st, 404, "gone from where it was");
    // The team reads it, picture included.
    let (st, _, bytes) = raw_get(&base, "x", &project_doc_asset(&t, &id, "pic.png")).await;
    assert_eq!((st, bytes.len()), (200, 11));

    let (st, q) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("v2"))]).await;
    assert_eq!((st, id_of(&q), &q["project"]), (200, id.clone(), &json!(t)), "{q}");
    let (st, _) = put_png(&base, "a geek", "d1", "more.png", b"two").await;
    assert_eq!(st, 200);
    let (_, v) = call(&base, "b", "GET", &format!("/api/projects/{t}/docs/{id}"), None).await;
    assert_eq!((&v["sections"][0]["body"], v["assets"].as_array().unwrap().len()), (&json!("v2"), 2), "{v}");
    let (_, l) = call(&base, "a", "GET", "/api/projects/personal/docs", None).await;
    assert!(l["docs"].as_array().unwrap().is_empty(), "no second copy at home: {l}");
}

/// A move needs the right to write in both projects: a stranger's project is
/// 404, a viewer's is 403, and the Doc stays where it was.
#[tokio::test]
async fn a_move_needs_write_in_both_projects() {
    let (hub, _app, base, t, u) = seeded().await;
    hub.flag("team:u", true);
    let (_, p) = put_doc(&base, "a geek", "d1", "Plan", &[]).await;
    let id = id_of(&p);
    let mv = format!("/api/projects/personal/docs/{id}/move");
    let (st, _) = call(&base, "a", "POST", &mv, Some(json!({ "to": u }))).await;
    assert_eq!(st, 404, "not a member of u");

    // x views t; x's own Doc doesn't go in.
    let (_, mine) = call(&base, "x", "POST", "/api/projects/personal/docs", Some(json!({ "title": "x's" }))).await;
    let (st, _) = call(
        &base,
        "x",
        "POST",
        &format!("/api/projects/personal/docs/{}/move", mine["doc"]["id"].as_str().unwrap()),
        Some(json!({ "to": t })),
    )
    .await;
    assert_eq!(st, 403, "a viewer doesn't write in t");

    // A teammate can't move a Doc out of someone's personal project.
    let (st, _) = call(
        &base,
        "b",
        "POST",
        &format!("/api/projects/{}/docs/{id}/move", p["project"].as_str().unwrap()),
        Some(json!({ "to": t })),
    )
    .await;
    assert_eq!(st, 404);
    let (st, _) = call(&base, "a", "GET", &format!("/api/projects/personal/docs/{id}"), None).await;
    assert_eq!(st, 200, "still at home");
}

/// A copy that sits where its owner may no longer write is refused with a
/// reason, and nothing changes.
#[tokio::test]
async fn a_put_where_the_owner_can_no_longer_write_is_refused() {
    let (hub, _app, base, t, _u) = seeded().await;
    hub.daemon("bgeek", "b", Some("t"));
    let (_, p) = put_doc(&base, "b bgeek", "d1", "Plan", &[("s1", Some("v1"))]).await;
    let id = id_of(&p);
    let (st, _) =
        call(&base, "b", "POST", &format!("/api/projects/personal/docs/{id}/move"), Some(json!({ "to": t }))).await;
    assert_eq!(st, 200);

    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer), ("x", TeamRole::Viewer)]);
    let (st, e) = put_doc(&base, "b bgeek", "d1", "Plan", &[("s1", Some("v2"))]).await;
    assert!(st == 403 && e["error"].as_str().unwrap().contains("no longer write"), "{st} {e}");
    let (st, _) = put_png(&base, "b bgeek", "d1", "pic.png", b"x").await;
    assert_eq!(st, 403);
    let (_, v) = call(&base, "a", "GET", &format!("/api/projects/{t}/docs/{id}"), None).await;
    assert_eq!((&v["sections"][0]["body"], v["assets"].as_array().unwrap().len()), (&json!("v1"), 0), "{v}");
}

/// A picture is a PNG, JPEG, GIF or WebP by its bytes, named for what it
/// is; the same bytes again change nothing; a Doc keeps 20.
#[tokio::test]
async fn pictures_are_checked_by_their_bytes() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    put_doc(&base, "a geek", "d1", "Plan", &[]).await;
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec();
    let (st, _) = signed(&base, "a geek", "PUT", &machine_doc_asset("d1", "a.png"), Some(svg), false).await;
    assert_eq!(st, 400, "an SVG isn't a picture");
    let (st, _) = put_png(&base, "a geek", "d1", "a.jpg", b"x").await;
    assert_eq!(st, 400, "named for another type");
    let (st, _) = put_png(&base, "a geek", "d1", "../a.png", b"x").await;
    assert!(st == 400 || st == 404, "{st}");

    let (_, first) = put_png(&base, "a geek", "d1", "a.png", b"x").await;
    let (_, same) = put_png(&base, "a geek", "d1", "a.png", b"x").await;
    assert_eq!(same["rev"], first["rev"]);
    let (_, other) = put_png(&base, "a geek", "d1", "a.png", b"y").await;
    assert!(other["rev"].as_u64() > first["rev"].as_u64(), "{other}");

    for i in 1..ASSETS_MAX {
        let (st, v) = put_png(&base, "a geek", "d1", &format!("p{i}.png"), b"x").await;
        assert_eq!(st, 200, "{v}");
    }
    let (st, v) = put_png(&base, "a geek", "d1", "one-more.png", b"x").await;
    assert!(st == 400 && v["error"].as_str().unwrap().contains("at most 20"), "{v}");
}

/// The machine's library Doc is the truth and its copy here is read-only
/// (Arugula #678): a person's write to it, a section or its title, is
/// refused naming the machine, its view has no writer, and the machine's
/// next put still lands. (Before: the write went through, and the next put
/// silently put the edit back and dropped the added section.)
#[tokio::test]
async fn a_copy_is_read_only_and_the_machines_puts_still_land() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let (_, p) = put_doc(&base, "a geek", "d1", "Plan", &[("s1", Some("v1")), ("s2", None)]).await;
    let (id, personal) = (id_of(&p), p["project"].as_str().unwrap().to_owned());
    let path = format!("/api/projects/{personal}/docs/{id}");

    let (st, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["writer"], &v["doc"]["machine"], &v["doc"]["machine_name"]),
        (&json!(false), &json!("geek"), &json!("geek-box")),
        "{v}"
    );
    let (_, l) = call(&base, "a", "GET", &format!("/api/projects/{personal}/docs"), None).await;
    assert_eq!((&l["docs"][0]["machine_name"], &l["writer"]), (&json!("geek-box"), &json!(true)), "{l}");

    for edit in [
        json!({ "section": "s1", "markdown": "edited on the web" }),
        json!({ "heading": "Added on the web", "markdown": "new" }),
        json!({ "title": "Renamed on the web" }),
    ] {
        let (st, e) = call(&base, "a", "PUT", &path, Some(edit.clone())).await;
        assert_eq!(st, 403, "{edit}: {e}");
        let why = e["error"].as_str().unwrap();
        assert!(why.contains("copy") && why.contains("geek-box"), "{why}");
    }
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!((&v["doc"]["title"], bodies_of(&v)), (&json!("Plan"), vec![json!("v1"), Value::Null]), "{v}");

    // The machine's next put lands, as it did.
    let (st, q) = put_doc(&base, "a geek", "d1", "Plan, v2", &[("s1", Some("v2"))]).await;
    assert_eq!((st, &q["id"]), (200, &json!(id)), "{q}");
    let (_, v) = call(&base, "a", "GET", &path, None).await;
    assert_eq!(
        (&v["doc"]["title"], bodies_of(&v), &v["writer"]),
        (&json!("Plan, v2"), vec![json!("v2")], &json!(false))
    );

    // Moved into the team, it's a copy there too: an editor can't write it,
    // and the machine's put still lands there.
    let (st, m) = call(&base, "a", "POST", &format!("{path}/move"), Some(json!({ "to": t }))).await;
    assert_eq!((st, &m["writer"], &m["doc"]["machine_name"]), (200, &json!(false), &json!("geek-box")), "{m}");
    let team_path = format!("/api/projects/{t}/docs/{id}");
    let (st, e) = call(&base, "b", "PUT", &team_path, Some(json!({ "section": "s1", "markdown": "b's" }))).await;
    assert!(st == 403 && e["error"].as_str().unwrap().contains("geek-box"), "{st} {e}");
    let (st, q) = put_doc(&base, "a geek", "d1", "Plan, v3", &[("s1", Some("v3"))]).await;
    assert_eq!((st, &q["project"]), (200, &json!(t)), "{q}");
    let (_, v) = call(&base, "b", "GET", &team_path, None).await;
    assert_eq!((bodies_of(&v), &v["writer"]), (vec![json!("v3")], &json!(false)), "{v}");
}

fn bodies_of(v: &Value) -> Vec<Value> {
    v["sections"].as_array().unwrap().iter().map(|s| s["body"].clone()).collect()
}
