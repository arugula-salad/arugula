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
