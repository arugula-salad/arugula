//! Relations between work items (Arugula #403) in M165's world
//! (`viewer_tests::world`): the ready set follows `blocks` edges, a cycle is
//! refused at write time, and a blocker the reader can't read is "something
//! you can't see", with neither its title nor its repo.

use serde_json::{Value, json};

use super::lanes_tests::{add, native, on, read, start};
use crate::chat_tests::call;
use crate::db::{Sql as _, sql::params};
use arugula_control_wire::boards::{project_relation, project_relations};

async fn relate(base: &str, who: &str, team: &str, from: &str, kind: &str, to: &str) -> (u16, Value) {
    call(base, who, "POST", &project_relations(team), Some(json!({ "from": from, "to": to, "kind": kind }))).await
}

fn ready(v: &Value) -> Vec<&str> {
    v["ready"].as_array().map(|a| a.iter().map(|r| r.as_str().unwrap()).collect()).unwrap_or_default()
}

fn row_id<'a>(v: &'a Value, item: &str) -> &'a str {
    on(v, item).1["id"].as_str().unwrap()
}

/// A blocks B and B blocks C: the ready set is A. When A closes, it is B,
/// and B's row loses its mark.
#[tokio::test]
async fn a_chain_has_one_ready_row_and_closing_the_blocker_moves_it() {
    let (base, app, team, id) = start().await;
    let (a, a_no) = native(&base, &team, "Design").await;
    let (b, _) = native(&base, &team, "Build").await;
    let (c, _) = native(&base, &team, "Ship").await;
    for i in [&a, &b, &c] {
        add(&base, &id, json!({ "kind": "issue", "id": i })).await;
    }
    for (from, to) in [(&a, &b), (&b, &c)] {
        let (s, v) = relate(&base, "bob", &team, from, "blocks", to).await;
        assert_eq!(s, 200, "{v}");
        assert_eq!((v["kind"].as_str(), v["source"].as_str()), (Some("blocks"), Some("person")), "{v}");
    }

    let v = read(&base, "bob", &id).await;
    assert_eq!(ready(&v), [row_id(&v, &a)], "only A can start: {v}");
    let (lane, rb) = on(&v, &b);
    assert_eq!(lane, "lane_next", "a blocked row stays in its lane: {v}");
    assert_eq!(rb["marks"], json!([{ "kind": "blocked_by", "number": a_no, "title": "Design" }]), "{rb}");
    assert_eq!(rb["relations"][0]["direction"], json!("in"), "{rb}");
    assert_eq!(rb["relations"][0]["other"]["title"], json!("Design"));
    assert_eq!(rb["relations"][0]["removable"], json!(true));
    assert_eq!(on(&v, &b).1["relations"][1]["direction"], json!("out"));
    assert!(on(&v, &a).1.get("marks").is_none(), "{v}");

    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id = ?1",
            params![a],
        )
        .unwrap();
    let v = read(&base, "bob", &id).await;
    assert_eq!(ready(&v), [row_id(&v, &b)], "A is done, so B can start: {v}");
    assert!(on(&v, &b).1.get("marks").is_none(), "{v}");
    assert!(on(&v, &c).1["marks"].is_array(), "C still waits for B: {v}");
    let text = arugula_control_wire::boards::render(&serde_json::from_value(v.clone()).unwrap());
    assert!(text.contains("Ready (1): "), "{text}");
}

/// A cycle of `blocks` is refused when written, of two or of three; so is an
/// edge to the item itself and the same edge twice. `relates` has no such
/// rule about direction.
#[tokio::test]
async fn a_cycle_of_blocks_is_refused() {
    let (base, _app, team, _id) = start().await;
    let (a, _) = native(&base, &team, "A").await;
    let (b, _) = native(&base, &team, "B").await;
    let (c, _) = native(&base, &team, "C").await;

    let (s, v) = relate(&base, "bob", &team, &a, "blocks", &b).await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = relate(&base, "bob", &team, &b, "blocks", &a).await;
    assert_eq!(s, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("cycle"), "{v}");

    let (s, _) = relate(&base, "bob", &team, &b, "blocks", &c).await;
    assert_eq!(s, 200);
    let (s, v) = relate(&base, "bob", &team, &c, "blocks", &a).await;
    assert_eq!(s, 409, "a cycle of three: {v}");
    let (s, v) = relate(&base, "bob", &team, &c, "blocks", &c).await;
    assert_eq!(s, 400, "{v}");
    let (s, v) = relate(&base, "bob", &team, &a, "blocks", &b).await;
    assert_eq!(s, 409, "the same edge twice: {v}");

    // The other way round is fine for kinds that don't gate.
    let (s, v) = relate(&base, "bob", &team, &c, "discovered_from", &a).await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = relate(&base, "bob", &team, &a, "relates", &c).await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = relate(&base, "bob", &team, &c, "relates", &a).await;
    assert_eq!(s, 409, "relates reads both ways: {v}");
}

/// A blocker the reader can't read: the row is blocked for everyone, but
/// carol is told only "something you can't see", with no number, title or
/// repo, in the marks and in the drawer.
#[tokio::test]
async fn a_blocker_the_reader_cant_read_has_no_title() {
    let (base, app, team, id) = start().await;
    let (b, _) = native(&base, &team, "Build").await;
    add(&base, &id, json!({ "kind": "issue", "id": b })).await;
    // fi_a1 is the private PR "secret plan" (#1 on acme/app); bob reads it.
    let (s, v) = relate(&base, "bob", &team, "fi_a1", "blocks", &b).await;
    assert_eq!(s, 200, "{v}");
    // carol reads neither end of a PR she can't see, so she can't make the edge.
    let (s, v) = relate(&base, "carol", &team, "fi_a1", "relates", &b).await;
    assert_eq!(s, 403, "a viewer changes nothing: {v}");

    let v = read(&base, "bob", &id).await;
    let (_, r) = on(&v, &b);
    assert_eq!(
        r["marks"],
        json!([{ "kind": "blocked_by", "number": 1, "title": "secret plan", "repo": "acme/app" }]),
        "{v}"
    );
    assert!(ready(&v).is_empty(), "{v}");

    let v = read(&base, "carol", &id).await;
    let (lane, r) = on(&v, &b);
    assert_eq!(lane, "lane_next", "{v}");
    assert_eq!(r["marks"], json!([{ "kind": "blocked_by" }]), "something you can't see: {v}");
    assert_eq!(r["relations"][0]["direction"], json!("in"));
    assert!(r["relations"][0].get("other").is_none(), "{r}");
    assert_eq!(r["relations"][0]["removable"], json!(false), "carol is a viewer");
    for leak in ["secret plan", "acme/app", "fi_a1"] {
        assert!(!v.to_string().contains(leak), "{leak} leaked to carol: {v}");
    }
    assert!(ready(&v).is_empty(), "blocked for her too: {v}");
    let text = arugula_control_wire::boards::render(&serde_json::from_value(v.clone()).unwrap());
    assert!(text.contains("blocked by something you can't see") && !text.contains("secret"), "{text}");

    // The PR merges: it blocks nothing any more, for anyone.
    app.db.c().execute("UPDATE forge_items SET state = 'merged' WHERE id = 'fi_a1'", params![]).unwrap();
    for who in ["bob", "carol"] {
        let v = read(&base, who, &id).await;
        assert!(on(&v, &b).1.get("marks").is_none(), "{who}: {v}");
        assert_eq!(ready(&v).len(), 1, "{who}: {v}");
    }
}

/// Who may write: both ends must be items of the project that the writer can
/// read; an edge made here comes off; a viewer can't make or remove one.
#[tokio::test]
async fn writes_are_for_owners_and_editors_between_items_of_the_project() {
    let (base, app, team, _id) = start().await;
    let (a, _) = native(&base, &team, "A").await;
    let (b, _) = native(&base, &team, "B").await;

    let (s, v) = relate(&base, "carol", &team, &a, "blocks", &b).await;
    assert_eq!(s, 403, "{v}");
    let (s, v) = relate(&base, "bob", &team, &a, "blocks", "iss_nothing").await;
    assert_eq!(s, 400, "an item that isn't there: {v}");
    let (s, v) = relate(&base, "bob", &team, &a, "blocks", "nonsense").await;
    assert_eq!(s, 400, "{v}");
    // An item of another project is not an item of this one.
    app.db
        .c()
        .execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES ('fi_other', 999, 5, 'issue', 'open', 'elsewhere', 1, 1, 1)",
            params![],
        )
        .unwrap();
    let (s, v) = relate(&base, "bob", &team, &a, "blocks", "fi_other").await;
    assert_eq!(s, 400, "{v}");

    let (s, made) = relate(&base, "bob", &team, &a, "blocks", &b).await;
    assert_eq!(s, 200, "{made}");
    let rel = made["id"].as_str().unwrap();
    assert!(rel.starts_with("rel_"));
    let (s, v) = call(&base, "carol", "DELETE", &project_relation(&team, rel), None).await;
    assert_eq!(s, 403, "{v}");
    let (s, v) = call(&base, "bob", "DELETE", &project_relation(&team, rel), None).await;
    assert_eq!(s, 200, "{v}");
    let (s, v) = call(&base, "bob", "DELETE", &project_relation(&team, rel), None).await;
    assert_eq!(s, 404, "{v}");
    // Gone, so the same edge can be made again, and in the other direction.
    let (s, v) = relate(&base, "bob", &team, &b, "blocks", &a).await;
    assert_eq!(s, 200, "{v}");
}
