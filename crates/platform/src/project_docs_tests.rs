//! Project Docs (aru-118) over the platform's routes: members read, owners
//! and editors make, write and delete, viewers only read; another
//! project's Doc is never reached; and a Doc's size is capped as a machine
//! Doc's is. Run on Postgres too by `just test-platform-postgres`.

use arugula_control_wire::project_docs::{SECTION_MAX, SECTIONS_MAX};
use serde_json::{Value, json};

use crate::{agents_tests::seeded, chat_tests::call};

async fn make(base: &str, by: &str, project: &str, body: Value) -> (u16, Value) {
    call(base, by, "POST", &format!("/api/projects/{project}/docs"), Some(body)).await
}

fn path(project: &str, id: &str) -> String {
    format!("/api/projects/{project}/docs/{id}")
}

async fn put(base: &str, by: &str, project: &str, id: &str, body: Value) -> (u16, Value) {
    call(base, by, "PUT", &path(project, id), Some(body)).await
}

fn bodies(v: &Value) -> Vec<Value> {
    v["sections"].as_array().unwrap().iter().map(|s| s["body"].clone()).collect()
}

/// An owner makes a Doc with its outline, written and pending; an editor
/// writes a section and adds one; a viewer reads it, read only; the list
/// has it; and an editor deletes it, which a viewer can't.
#[tokio::test]
async fn members_read_a_doc_and_owners_and_editors_write_it() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let outline = json!({ "title": "Release plan", "sections": [
        { "heading": "Goals", "markdown": "Ship **0.64**." },
        { "heading": "Risks", "intent": "what could go wrong" },
    ] });
    let (st, v) = make(&base, "a", &t, outline).await;
    assert_eq!(st, 200, "{v}");
    let id = v["doc"]["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("pdc_") && id.len() == 36, "{id}");
    assert_eq!((&v["doc"]["author"], &v["doc"]["agent"], &v["writer"]), (&json!("a"), &Value::Null, &json!(true)));
    // Made here, not a copy from a machine (#678): its people write it.
    assert_eq!((&v["doc"]["machine"], &v["doc"]["machine_name"]), (&Value::Null, &Value::Null));
    assert_eq!((&v["doc"]["sections"], &v["doc"]["written"]), (&json!(2), &json!(1)));
    assert_eq!(bodies(&v), [json!("Ship **0.64**."), Value::Null]);
    assert_eq!((&v["sections"][1]["id"], &v["sections"][1]["intent"]), (&json!("s2"), &json!("what could go wrong")));

    // An editor fills the pending one and adds a third.
    let (st, v) = put(&base, "b", &t, &id, json!({ "section": "s2", "markdown": "The canary." })).await;
    assert_eq!((st, bodies(&v)[1].clone()), (200, json!("The canary.")), "{v}");
    let (st, v) = put(&base, "b", &t, &id, json!({ "heading": "Notes", "markdown": "```chart\n{}\n```" })).await;
    assert_eq!((st, &v["sections"][2]["id"], &v["doc"]["written"]), (200, &json!("s3"), &json!(3)), "{v}");
    let (st, v) = put(&base, "b", &t, &id, json!({ "title": "Release plan, 0.64" })).await;
    assert_eq!((st, &v["doc"]["title"]), (200, &json!("Release plan, 0.64")));

    // A viewer reads it, and can't change it or make one.
    let (st, v) = call(&base, "x", "GET", &path(&t, &id), None).await;
    assert_eq!((st, &v["writer"], bodies(&v).len()), (200, &json!(false), 3), "{v}");
    let (st, _) = put(&base, "x", &t, &id, json!({ "section": "s1", "markdown": "mine now" })).await;
    assert_eq!(st, 403);
    let (st, _) = make(&base, "x", &t, json!({ "title": "x's" })).await;
    assert_eq!(st, 403);
    let (st, l) = call(&base, "x", "GET", &format!("/api/projects/{t}/docs"), None).await;
    assert_eq!((st, &l["writer"], &l["docs"][0]["id"]), (200, &json!(false), &json!(id)), "{l}");

    // Only owners and editors delete it.
    let (st, _) = call(&base, "x", "DELETE", &path(&t, &id), None).await;
    assert_eq!(st, 403);
    let (st, _) = call(&base, "b", "DELETE", &path(&t, &id), None).await;
    assert_eq!(st, 200);
    let (st, _) = call(&base, "a", "GET", &path(&t, &id), None).await;
    assert_eq!(st, 404);
}

/// Membership and roles alone decide (Arugula #746): with chat off, an
/// editor makes and writes a Doc and a viewer reads it and changes nothing;
/// someone outside the project gets 404 though chat is on for them.
#[tokio::test]
async fn chat_off_changes_nothing_about_docs() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.flag("account:b", false);
    hub.flag("account:x", false);
    hub.flag("team:u", true);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, v) = make(&base, "b", &t, json!({ "title": "b's plan", "sections": [{ "heading": "One" }] })).await;
    assert_eq!(st, 200, "{v}");
    let id = v["doc"]["id"].as_str().unwrap().to_owned();
    let (st, _) = put(&base, "b", &t, &id, json!({ "section": "s1", "markdown": "hi" })).await;
    assert_eq!(st, 200);
    let (st, v) = call(&base, "x", "GET", &path(&t, &id), None).await;
    assert_eq!((st, &v["writer"]), (200, &json!(false)), "{v}");
    let (st, l) = call(&base, "x", "GET", &format!("/api/projects/{t}/docs"), None).await;
    assert_eq!((st, &l["docs"][0]["id"]), (200, &json!(id)), "{l}");
    assert_eq!(put(&base, "x", &t, &id, json!({ "section": "s1", "markdown": "mine" })).await.0, 403);
    assert_eq!(call(&base, "c", "GET", &path(&t, &id), None).await.0, 404);
    assert_eq!(call(&base, "c", "GET", &format!("/api/projects/{t}/docs"), None).await.0, 404);
}

/// A Doc is its project's: a stranger gets 404, and so does a member of
/// another project naming it under their own; a section that isn't there
/// is 404 too.
#[tokio::test]
async fn another_projects_doc_is_never_reached() {
    let (hub, _app, base, t, u) = seeded().await;
    hub.flag("team:u", true);
    let (_, v) = make(&base, "a", &t, json!({ "title": "t's plan", "sections": [{ "heading": "One" }] })).await;
    let id = v["doc"]["id"].as_str().unwrap().to_owned();
    for (who, project) in [("c", &t), ("c", &u)] {
        let (st, _) = call(&base, who, "GET", &path(project, &id), None).await;
        assert_eq!(st, 404, "{who} under {project}");
        let (st, _) = put(&base, who, project, &id, json!({ "section": "s1", "markdown": "hi" })).await;
        assert_eq!(st, 404, "{who} under {project}");
    }
    let (st, l) = call(&base, "c", "GET", &format!("/api/projects/{t}/docs"), None).await;
    assert_eq!(st, 404, "{l}");
    let (st, _) = put(&base, "a", &t, &id, json!({ "section": "s9", "markdown": "hi" })).await;
    assert_eq!(st, 404);
    let (st, _) = call(&base, "a", "GET", &path(&t, "pdc_nope"), None).await;
    assert_eq!(st, 404);
}

/// A section is at most 64 KB, a Doc 40 sections and 1 MiB, as a machine
/// Doc's; a title is one line.
#[tokio::test]
async fn a_docs_size_is_capped() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let big = "x".repeat(SECTION_MAX + 1);
    let (st, v) =
        make(&base, "a", &t, json!({ "title": "big", "sections": [{ "heading": "A", "markdown": big }] })).await;
    assert!(st == 400 && v["error"].as_str().unwrap().contains("64 KB"), "{v}");
    let many: Vec<Value> = (0..=SECTIONS_MAX).map(|i| json!({ "heading": format!("S{i}") })).collect();
    let (st, _) = make(&base, "a", &t, json!({ "title": "many", "sections": many })).await;
    assert_eq!(st, 400);
    let (st, _) = make(&base, "a", &t, json!({ "title": "two\nlines" })).await;
    assert_eq!(st, 400);

    let (st, v) = make(&base, "a", &t, json!({ "title": "full" })).await;
    assert_eq!(st, 200);
    let id = v["doc"]["id"].as_str().unwrap().to_owned();
    let section = "y".repeat(SECTION_MAX);
    let mut refused = None;
    for i in 0..SECTIONS_MAX {
        let (st, v) = put(&base, "a", &t, &id, json!({ "heading": format!("S{i}"), "markdown": section })).await;
        if st != 200 {
            refused = Some((i, v));
            break;
        }
    }
    let (i, v) = refused.expect("1 MiB refuses one");
    assert!(v["error"].as_str().unwrap().contains("1 MiB"), "{v}");
    assert_eq!(i, 15, "15 full sections fit in 1 MiB with their headings, the 16th doesn't");
    let (_, v) = call(&base, "a", "GET", &path(&t, &id), None).await;
    assert_eq!(v["sections"].as_array().unwrap().len(), 15, "the refused one isn't kept");

    // 40 sections, and not a 41st.
    let (_, v) = make(&base, "a", &t, json!({ "title": "forty" })).await;
    let id = v["doc"]["id"].as_str().unwrap().to_owned();
    for i in 0..SECTIONS_MAX {
        let (st, v) = put(&base, "a", &t, &id, json!({ "heading": format!("S{i}") })).await;
        assert_eq!(st, 200, "{v}");
    }
    let (st, v) = put(&base, "a", &t, &id, json!({ "heading": "one more" })).await;
    assert!(st == 400 && v["error"].as_str().unwrap().contains("40 sections"), "{v}");
}

// ------------------------------------------------------- Share with…

use arugula_control_wire::project_docs::DOCS_MAX;
use arugula_e2e::team::TeamRole;

use crate::db::project_docs::{MachineDoc, MachineKey, MachineSection};

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3];

fn section<'a>(id: &'a str, heading: &'a str, body: Option<&'a str>) -> MachineSection<'a> {
    MachineSection { id, heading, intent: "", body }
}

/// A machine's copy of one Doc for `account`, in `project`, as its puts
/// make it (title, two sections and a picture): the Doc's id.
fn copy_in(app: &crate::Platform, project: &str, account: &str, title: &str, body: &str) -> String {
    let sections = [section("a", "Goals", Some(body)), section("b", "Risks", None)];
    let key = MachineKey { machine: "geek", doc: "d1" };
    let d = MachineDoc { title, sections: &sections, layout: None };
    let (doc, _) = app.db.put_machine_doc(key, project, account, &d, 10, DOCS_MAX).unwrap().unwrap();
    app.db.put_project_doc_asset(&doc.id, "chart.png", "image/png", PNG, 11).unwrap().unwrap();
    doc.id
}

fn copies(project: &str, id: &str) -> String {
    format!("/api/projects/{project}/docs/{id}/copies")
}

async fn personal(app: &crate::Platform, account: &str) -> String {
    crate::projects::personal_project(app, account).await.unwrap().unwrap()
}

/// Share with… makes an ordinary Doc in the project from the copy as it
/// is, pictures too, once per project; the copy changing makes it stale,
/// the team editing it makes it edited; and Update copy refuses to replace
/// their edits until asked to.
#[tokio::test]
async fn sharing_a_copy_makes_a_team_doc_and_update_copy_refreshes_it() {
    let (_hub, app, base, t, _u) = seeded().await;
    let pa = personal(&app, "a").await;
    let copy = copy_in(&app, &pa, "a", "Release plan", "Ship it.");

    let (st, v) = call(&base, "a", "POST", &copies("personal", &copy), Some(json!({ "to": t }))).await;
    assert_eq!(st, 200, "{v}");
    let team = v["doc"]["id"].as_str().unwrap().to_owned();
    assert_ne!(team, copy);
    // Ordinary: editable by the team, no machine, authored by the sharer.
    assert_eq!((&v["doc"]["project"], &v["doc"]["author"], &v["writer"]), (&json!(t), &json!("a"), &json!(true)));
    assert_eq!((&v["doc"]["machine"], &v["doc"]["title"]), (&Value::Null, &json!("Release plan")));
    assert_eq!(bodies(&v), [json!("Ship it."), Value::Null]);
    assert_eq!((&v["sections"][0]["id"], &v["sections"][1]["id"]), (&json!("s1"), &json!("s2")));
    assert_eq!(v["assets"][0]["name"], "chart.png");
    assert_eq!((&v["doc"]["made_from"]["project"], &v["doc"]["made_from"]["id"]), (&json!(pa), &json!(copy)));
    let (st, bytes) = {
        let r = reqwest::Client::new()
            .get(format!("{base}/api/projects/{t}/docs/{team}/assets/chart.png"))
            .header("cookie", "ilg_session=session-b")
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.bytes().await.unwrap())
    };
    assert_eq!((st, bytes.as_ref()), (200, PNG), "the picture was copied");
    // The editor writes it: it is theirs. The copy stays read-only.
    assert_eq!(put(&base, "b", &t, &team, json!({ "section": "s1", "markdown": "Ours." })).await.0, 200);
    assert_eq!(put(&base, "a", &pa, &copy, json!({ "section": "s1", "markdown": "no" })).await.0, 403);

    // One per copy per project: 409 with the Doc that is there.
    let (st, v) = call(&base, "a", "POST", &copies("personal", &copy), Some(json!({ "to": t }))).await;
    assert_eq!((st, &v["id"]), (409, &json!(team)), "{v}");
    // Not into the personal project, and not where they only read.
    let (st, _) = call(&base, "a", "POST", &copies("personal", &copy), Some(json!({ "to": pa }))).await;
    assert_eq!(st, 400);

    // The editor's change counts as an edit; the copy hasn't changed.
    let (st, l) = call(&base, "a", "GET", &copies("personal", &copy), None).await;
    let c = &l["copies"][0];
    assert_eq!((st, l["copies"].as_array().unwrap().len()), (200, 1), "{l}");
    assert_eq!(
        (&c["id"], &c["project"], &c["stale"], &c["edited"]),
        (&json!(team), &json!(t), &json!(false), &json!(true))
    );
    assert!(c["link"].as_str().unwrap().ends_with(&format!("/p/{t}/docs/{team}")), "{c}");

    // The machine changes the copy: now it's stale too. Update copy asks first.
    let sections = [section("a", "Goals", Some("Ship it, twice.")), section("c", "Notes", Some("More."))];
    let key = MachineKey { machine: "geek", doc: "d1" };
    let d = MachineDoc { title: "Release plan 2", sections: &sections, layout: None };
    app.db.put_machine_doc(key, &pa, "a", &d, 20, DOCS_MAX).unwrap().unwrap();
    let (_, l) = call(&base, "a", "GET", &copies("personal", &copy), None).await;
    assert_eq!((&l["copies"][0]["stale"], &l["copies"][0]["edited"]), (&json!(true), &json!(true)));
    let up = format!("{}/{t}", copies("personal", &copy));
    let (st, v) = call(&base, "a", "PUT", &up, Some(json!({}))).await;
    assert_eq!((st, &v["edited"]), (409, &json!(true)), "{v}");
    let (_, v) = call(&base, "b", "GET", &path(&t, &team), None).await;
    assert_eq!((&v["doc"]["title"], bodies(&v)[0].clone()), (&json!("Release plan"), json!("Ours.")), "unchanged");

    // Replace: the team's edits go, the copy's current sections and
    // pictures come, with ids past any it had.
    let (st, c) = call(&base, "a", "PUT", &up, Some(json!({ "replace": true }))).await;
    assert_eq!((st, &c["stale"], &c["edited"], &c["id"]), (200, &json!(false), &json!(false), &json!(team)), "{c}");
    let (_, v) = call(&base, "b", "GET", &path(&t, &team), None).await;
    assert_eq!(v["doc"]["title"], "Release plan 2");
    assert_eq!(bodies(&v), [json!("Ship it, twice."), json!("More.")]);
    assert_eq!((&v["sections"][0]["id"], &v["sections"][1]["id"]), (&json!("s3"), &json!("s4")));
    assert_eq!(v["assets"].as_array().unwrap().len(), 1);
    // Fresh, nobody edited: Update copy needs no replace.
    let (st, c) = call(&base, "a", "PUT", &up, Some(json!({}))).await;
    assert_eq!((st, &c["edited"]), (200, &json!(false)), "{c}");
}

/// The trap: reading a copy is allowed to whoever reads its project, but
/// only its owner, from their personal project, shares it or updates the
/// team's. A teammate who writes in the target is refused, on a copy in
/// the owner's personal project (no such Doc to them) and on one that sits
/// in a team's project (which they read).
#[tokio::test]
async fn a_teammate_cannot_snapshot_or_update_someone_elses_copy() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.team("v", &[("b", TeamRole::Owner)]);
    crate::cache::catch_up(&app).await.unwrap();
    let v = crate::projects::team_project(&app, "v").await.unwrap().unwrap();
    let pa = personal(&app, "a").await;

    // a's copy in a's personal project: b can't even see it.
    let mine = copy_in(&app, &pa, "a", "a's plan", "x");
    for (m, p, body) in [("POST", copies(&pa, &mine), Some(json!({ "to": v }))), ("GET", copies(&pa, &mine), None)] {
        let (st, r) = call(&base, "b", m, &p, body).await;
        assert_eq!(st, 404, "{m}: {r}");
    }
    let (st, r) =
        call(&base, "b", "PUT", &format!("{}/{v}", copies(&pa, &mine)), Some(json!({ "replace": true }))).await;
    assert_eq!(st, 404, "{r}");

    // A copy of a's that sits in t, where b reads and writes (put there
    // before copies stayed personal): b may snapshot to v, which they own,
    // and still can't, nor can a: it isn't in a's personal project.
    app.db.delete_project_doc(&mine).unwrap();
    let moved = copy_in(&app, &t, "a", "moved", "y");
    let team = app.db.snapshot_project_doc(&moved, &v, "a", 30, DOCS_MAX).unwrap().unwrap();
    for who in ["a", "b"] {
        let (st, r) = call(&base, who, "POST", &copies(&t, &moved), Some(json!({ "to": v }))).await;
        assert_eq!(st, 403, "{who}: {r}");
        let (st, r) = call(&base, who, "GET", &copies(&t, &moved), None).await;
        assert_eq!(st, 403, "{who}: {r}");
        let (st, r) =
            call(&base, who, "PUT", &format!("{}/{v}", copies(&t, &moved)), Some(json!({ "replace": true }))).await;
        assert_eq!(st, 403, "{who}: {r}");
    }
    let (_, d) = call(&base, "b", "GET", &path(&v, &team.id), None).await;
    assert_eq!(d["doc"]["rev"], 1, "nothing was changed: {d}");
    // And a Doc written in a team's project isn't shared from there, as
    // it isn't anyone's own: 403 though a writes it.
    let (_, plain) = make(&base, "a", &t, json!({ "title": "plain", "sections": [] })).await;
    let id = plain["doc"]["id"].as_str().unwrap();
    for who in ["a", "b"] {
        let (st, r) = call(&base, who, "POST", &copies(&t, id), Some(json!({ "to": v }))).await;
        assert_eq!(st, 403, "{who}: {r}");
        let (st, r) = call(&base, who, "GET", &copies(&t, id), None).await;
        assert_eq!(st, 403, "{who}: {r}");
    }
}

/// A Doc written on the platform, in the person's own project, is shared
/// like a machine's copy (Arugula #821): the team Doc has its sections and
/// pictures, a second share is 409 with the same Doc, and the source is
/// editable, unlike a copy: its edit makes the team Doc stale, the team's
/// edit makes it edited and never reaches the source, and Update copy
/// brings the source's edit once the team's is replaced.
#[tokio::test]
async fn a_doc_written_on_the_platform_is_shared_once_and_updated() {
    let (_hub, app, base, t, _u) = seeded().await;
    let pa = personal(&app, "a").await;
    let outline = json!({ "title": "Notes", "sections": [
        { "heading": "Goals", "markdown": "Ship it." },
        { "heading": "Risks", "intent": "what could go wrong" },
    ] });
    let (st, mine) = make(&base, "a", &pa, outline).await;
    assert_eq!(st, 200, "{mine}");
    let src = mine["doc"]["id"].as_str().unwrap().to_owned();
    assert_eq!(mine["doc"]["machine"], Value::Null);
    app.db.put_project_doc_asset(&src, "chart.png", "image/png", PNG, 11).unwrap().unwrap();

    let (st, v) = call(&base, "a", "POST", &copies(&pa, &src), Some(json!({ "to": t }))).await;
    assert_eq!(st, 200, "{v}");
    let team = v["doc"]["id"].as_str().unwrap().to_owned();
    assert_eq!((&v["doc"]["project"], &v["doc"]["machine"], &v["writer"]), (&json!(t), &Value::Null, &json!(true)));
    assert_eq!((&v["doc"]["made_from"]["project"], &v["doc"]["made_from"]["id"]), (&json!(pa), &json!(src)));
    assert_eq!(v["doc"]["made_from"]["machine_name"], Value::Null);
    assert_eq!(bodies(&v), [json!("Ship it."), Value::Null]);
    assert_eq!(v["assets"][0]["name"], "chart.png", "the picture came: {v}");

    // Again: 409 with the team Doc, and the team has the one.
    let (st, v) = call(&base, "a", "POST", &copies(&pa, &src), Some(json!({ "to": t }))).await;
    assert_eq!((st, &v["id"]), (409, &json!(team)), "{v}");
    let (_, l) = call(&base, "a", "GET", &format!("/api/projects/{t}/docs"), None).await;
    assert_eq!(l["docs"].as_array().unwrap().len(), 1, "{l}");
    let (_, l) = call(&base, "a", "GET", &copies(&pa, &src), None).await;
    let c = &l["copies"][0];
    assert_eq!((l["copies"].as_array().unwrap().len(), &c["stale"], &c["edited"]), (1, &json!(false), &json!(false)));

    // The owner edits the Mine Doc: the team Doc is stale, not edited.
    let w = json!({ "section": "s1", "markdown": "Ship it, twice." });
    assert_eq!(put(&base, "a", &pa, &src, w).await.0, 200);
    let (_, l) = call(&base, "a", "GET", &copies(&pa, &src), None).await;
    assert_eq!((&l["copies"][0]["stale"], &l["copies"][0]["edited"]), (&json!(true), &json!(false)), "{l}");

    // The team edits theirs: edited, and Mine is as it was.
    let (_, before) = call(&base, "a", "GET", &path(&pa, &src), None).await;
    assert_eq!(put(&base, "b", &t, &team, json!({ "section": "s2", "markdown": "Ours." })).await.0, 200);
    let (_, after) = call(&base, "a", "GET", &path(&pa, &src), None).await;
    assert_eq!(after, before, "editing the team's Doc leaves Mine's alone");
    let (_, l) = call(&base, "a", "GET", &copies(&pa, &src), None).await;
    assert_eq!((&l["copies"][0]["stale"], &l["copies"][0]["edited"]), (&json!(true), &json!(true)), "{l}");

    // Update asks first, then replaces with Mine's current sections.
    let up = format!("{}/{t}", copies(&pa, &src));
    let (st, v) = call(&base, "a", "PUT", &up, Some(json!({}))).await;
    assert_eq!((st, &v["edited"]), (409, &json!(true)), "{v}");
    let (st, c) = call(&base, "a", "PUT", &up, Some(json!({ "replace": true }))).await;
    assert_eq!((st, &c["stale"], &c["edited"]), (200, &json!(false), &json!(false)), "{c}");
    let (_, v) = call(&base, "b", "GET", &path(&t, &team), None).await;
    assert_eq!(bodies(&v), [json!("Ship it, twice."), Value::Null]);
    assert_eq!(v["assets"].as_array().unwrap().len(), 1);

    // A teammate can't reach it: another person's Mine Doc isn't theirs.
    for (m, p) in [("POST", copies(&pa, &src)), ("GET", copies(&pa, &src)), ("PUT", up.clone())] {
        let (st, r) = call(&base, "b", m, &p, Some(json!({ "to": t }))).await;
        assert_eq!(st, 404, "{m}: {r}");
    }
}

/// Update copy needs the right to write in the team's project still: the
/// owner removed from it is refused, and doesn't hear of its Docs; one
/// made a viewer there is refused too. Sharing to where they only read is
/// refused as well.
#[tokio::test]
async fn an_owner_removed_from_the_team_cannot_update_its_copy() {
    let (hub, app, base, _t, _u) = seeded().await;
    hub.team("w", &[("c", TeamRole::Owner), ("a", TeamRole::Editor)]);
    hub.team("r", &[("c", TeamRole::Owner), ("a", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    let w = crate::projects::team_project(&app, "w").await.unwrap().unwrap();
    let r = crate::projects::team_project(&app, "r").await.unwrap().unwrap();
    let pa = personal(&app, "a").await;
    let copy = copy_in(&app, &pa, "a", "Plan", "v1");

    let (st, b) = call(&base, "a", "POST", &copies("personal", &copy), Some(json!({ "to": r }))).await;
    assert_eq!(st, 403, "a viewer there: {b}");
    let (st, b) = call(&base, "a", "POST", &copies("personal", &copy), Some(json!({ "to": w }))).await;
    assert_eq!(st, 200, "{b}");
    let team = b["doc"]["id"].as_str().unwrap().to_owned();
    // The machine moves on.
    let sections = [section("a", "Goals", Some("v2"))];
    let d = MachineDoc { title: "Plan", sections: &sections, layout: None };
    app.db.put_machine_doc(MachineKey { machine: "geek", doc: "d1" }, &pa, "a", &d, 20, DOCS_MAX).unwrap().unwrap();
    let up = format!("{}/{w}", copies("personal", &copy));

    // Demoted to a viewer: reads, can't update.
    hub.team("w", &[("c", TeamRole::Owner), ("a", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, b) = call(&base, "a", "PUT", &up, Some(json!({ "replace": true }))).await;
    assert_eq!(st, 403, "{b}");
    let (_, l) = call(&base, "a", "GET", &copies("personal", &copy), None).await;
    assert_eq!(l["copies"].as_array().unwrap().len(), 1, "a viewer still reads it: {l}");

    // Removed: no such project; and its Doc isn't listed to them.
    hub.team("w", &[("c", TeamRole::Owner)]);
    crate::cache::catch_up(&app).await.unwrap();
    let (st, b) = call(&base, "a", "PUT", &up, Some(json!({ "replace": true }))).await;
    assert_eq!(st, 404, "{b}");
    let (_, l) = call(&base, "a", "GET", &copies("personal", &copy), None).await;
    assert_eq!(l["copies"].as_array().unwrap().len(), 0, "{l}");
    let (_, d) = call(&base, "c", "GET", &path(&w, &team), None).await;
    assert_eq!(bodies(&d), [json!("v1"), Value::Null], "the team's Doc is as it was: {d}");
}

/// A Doc made as `tabs` reads back as it, a plain one names no layout, an
/// edit that omits it leaves it and one that names `doc` clears it, and a
/// layout that isn't one is refused (#826).
#[tokio::test]
async fn a_doc_keeps_its_layout_through_make_read_and_write() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let (st, v) = make(&base, "a", &t, json!({ "title": "Plain" })).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["doc"].get("layout").is_none(), "a plain Doc's body is what it was: {v}");
    let (st, v) =
        make(&base, "a", &t, json!({ "title": "Tabbed", "layout": "tabs", "sections": [{ "heading": "A" }] })).await;
    assert_eq!((st, &v["doc"]["layout"]), (200, &json!("tabs")), "{v}");
    let id = v["doc"]["id"].as_str().unwrap().to_owned();
    let (_, v) = call(&base, "b", "GET", &path(&t, &id), None).await;
    assert_eq!(v["doc"]["layout"], "tabs");
    let (_, l) = call(&base, "b", "GET", &format!("/api/projects/{t}/docs"), None).await;
    assert!(l["docs"].as_array().unwrap().iter().any(|d| d["id"] == json!(id) && d["layout"] == "tabs"), "{l}");

    // Omitted: left as it is. Alone, a layout is a change.
    let (st, v) = put(&base, "b", &t, &id, json!({ "title": "Tabbed, renamed" })).await;
    assert_eq!((st, &v["doc"]["layout"]), (200, &json!("tabs")), "{v}");
    let (st, v) = put(&base, "b", &t, &id, json!({ "layout": "prototype" })).await;
    assert_eq!((st, &v["doc"]["layout"]), (200, &json!("prototype")), "{v}");
    let (st, v) = put(&base, "b", &t, &id, json!({ "layout": "doc" })).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["doc"].get("layout").is_none(), "doc is plain: {v}");
    // A name nobody draws is refused, on make and on write.
    assert_eq!(put(&base, "b", &t, &id, json!({ "layout": "grid" })).await.0, 400);
    assert_eq!(make(&base, "a", &t, json!({ "title": "X", "layout": "grid" })).await.0, 400);
}

/// A machine's put is the whole Doc: its layout is set, changed, and, when a
/// put doesn't name one (an older daemon), `doc` and not what it was. Share
/// with… and Update copy carry it (#826).
#[tokio::test]
async fn a_machines_put_sets_the_layout_and_a_missing_one_is_doc() {
    let (_hub, app, base, t, _u) = seeded().await;
    let pa = personal(&app, "a").await;
    let sections = [section("a", "Goals", Some("Ship it.")), section("b", "Risks", None)];
    let key = MachineKey { machine: "geek", doc: "d1" };
    let put_as = |layout: Option<&str>, at: u64| {
        let d = MachineDoc { title: "Plan", sections: &sections, layout };
        app.db.put_machine_doc(key, &pa, "a", &d, at, DOCS_MAX).unwrap().unwrap()
    };
    let (doc, changed) = put_as(Some("tabs"), 10);
    assert_eq!((doc.layout.as_deref(), changed), (Some("tabs"), true));
    let (same, changed) = put_as(Some("tabs"), 11);
    assert_eq!((same.layout.as_deref(), changed, same.rev), (Some("tabs"), false, doc.rev));

    // Share with… makes the team Doc as it is laid out.
    let (st, v) = call(&base, "a", "POST", &copies("personal", &doc.id), Some(json!({ "to": t }))).await;
    assert_eq!((st, &v["doc"]["layout"]), (200, &json!("tabs")), "{v}");
    let team = v["doc"]["id"].as_str().unwrap().to_owned();

    // The machine puts the layout again as prototype; Update copy carries it.
    let (doc, changed) = put_as(Some("prototype"), 20);
    assert_eq!((doc.layout.as_deref(), changed, doc.rev > same.rev), (Some("prototype"), true, true));
    let up = format!("{}/{t}", copies("personal", &doc.id));
    let (st, v) = call(&base, "a", "PUT", &up, Some(json!({}))).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "b", "GET", &path(&t, &team), None).await;
    assert_eq!(v["doc"]["layout"], "prototype");

    // An older daemon's put names none: the copy is `doc` again.
    let (doc, changed) = put_as(None, 30);
    assert_eq!((doc.layout, changed), (None, true));
    let (st, v) = call(&base, "a", "PUT", &up, Some(json!({ "replace": true }))).await;
    assert_eq!(st, 200, "{v}");
    let (_, v) = call(&base, "b", "GET", &path(&t, &team), None).await;
    assert!(v["doc"].get("layout").is_none(), "{v}");
}
