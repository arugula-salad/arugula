//! Lanes and proof on a board (aru-125), in M165's world
//! (`viewer_tests::world`): a lane follows the work for a reader who can't
//! read it, Done is proven by CI or by evidence or not at all, and one piece
//! of work is one row.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    boards::FLAG,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

/// A flagged team world, and a board of it by bob (an editor signed in with
/// GitHub): (base, app, team, board).
async fn start() -> (String, Arc<crate::Platform>, String, String) {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    hub.flag_named("team:t1", FLAG, true);
    let (s, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Lanes" }))).await;
    assert_eq!(s, 200, "{b}");
    let id = b["board"]["id"].as_str().unwrap().to_owned();
    (base, app, team, id)
}

async fn add(base: &str, board: &str, pointer: Value) -> Value {
    let (s, v) = call(
        base,
        "bob",
        "PUT",
        &format!("/api/boards/{board}/entries"),
        Some(json!({ "entry": "row", "note": "n", "pointer": pointer })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    v
}

async fn attach(base: &str, board: &str, row: &str, role: &str, pointer: Value) -> Value {
    let (s, v) = call(
        base,
        "bob",
        "POST",
        &format!("/api/boards/{board}/attachments"),
        Some(json!({ "row": row, "role": role, "pointer": pointer })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    v
}

async fn read(base: &str, who: &str, board: &str) -> Value {
    let (s, v) = call(base, who, "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(s, 200, "{v}");
    v
}

/// Every row with the lane it is in.
fn rows(v: &Value) -> Vec<(&str, &Value)> {
    v["zones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|z| z["rows"].as_array().unwrap().iter().map(|r| (z["id"].as_str().unwrap(), r)))
        .collect()
}

/// The row whose item is `id`.
fn on<'a>(v: &'a Value, id: &str) -> (&'a str, &'a Value) {
    rows(v).into_iter().find(|(_, r)| r["pointer"]["id"] == id).unwrap_or_else(|| panic!("no row on {id}: {v}"))
}

fn fi(id: &str) -> Value {
    json!({ "kind": "forge_item", "id": id })
}

async fn native(base: &str, team: &str, title: &str) -> (String, u64) {
    let (s, i) =
        call(base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": title }))).await;
    assert_eq!(s, 200, "{i}");
    (i["id"].as_str().unwrap().to_owned(), i["number"].as_u64().unwrap())
}

#[allow(clippy::too_many_arguments)]
fn item(
    app: &crate::Platform,
    id: &str,
    repo: u64,
    number: u64,
    kind: &str,
    state: &str,
    title: &str,
    head: (&str, &str),
) {
    app.db
        .c()
        .execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, head_ref, head_sha, created_at, updated_at,
                 synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULLIF(?7, ''), NULLIF(?8, ''), 1, 1, 1)",
            params![id, repo, number, kind, state, title, head.0, head.1],
        )
        .unwrap();
}

fn check(app: &crate::Platform, repo: u64, sha: &str, name: &str, status: &str, conclusion: Option<&str>) {
    app.db
        .c()
        .execute(
            "INSERT INTO forge_checks (repo_id, head_sha, kind, name, status, conclusion, updated_at)
             VALUES (?1, ?2, 'check_run', ?5, ?3, ?4, 1)",
            params![repo, sha, status, conclusion, name],
        )
        .unwrap();
}

/// A reader who can't read a private repo sees the row in the lane its PR
/// puts it in, and the attachment says nothing: no title, no state, no
/// repo. The same for a row whose own item is that PR.
#[tokio::test]
async fn a_reader_without_access_sees_the_lane_and_not_the_pr() {
    let (base, _app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    let v = attach(&base, &id, &row, "work", fi("fi_a1")).await;
    assert_eq!(on(&v, &login).0, "lane_review", "an open PR: Review");

    // bob reads acme/app: the chip is there.
    let v = read(&base, "bob", &id).await;
    let att = &on(&v, &login).1["attachments"][0];
    assert_eq!(
        (att["live"]["title"].as_str(), att["live"]["repo"].as_str()),
        (Some("secret plan"), Some("acme/app")),
        "{att}"
    );

    // carol (a viewer, without GitHub) doesn't: the row is in Review all the
    // same, the attachment is there, and nothing of the PR is.
    let v = read(&base, "carol", &id).await;
    let (lane, r) = on(&v, &login);
    assert_eq!(lane, "lane_review", "the lane moves for her too: {v}");
    assert_eq!(r["attachments"][0]["pointer"], fi("fi_a1"));
    assert!(r["attachments"][0].get("live").is_none(), "the chip says nothing: {v}");
    assert!(!v.to_string().contains("secret plan") && !v.to_string().contains("acme/app"), "{v}");
    assert_eq!(v["zones"][2]["rows"][0]["id"], json!(row));
}

/// The same for a row whose own item is the private PR: Review, no title.
#[tokio::test]
async fn a_row_on_a_private_pr_is_in_review_for_everyone() {
    let (base, _app, _team, id) = start().await;
    add(&base, &id, fi("fi_a1")).await;
    for who in ["bob", "carol"] {
        let v = read(&base, who, &id).await;
        let (lane, r) = on(&v, "fi_a1");
        assert_eq!(lane, "lane_review", "{who}: {v}");
        assert_eq!(r.get("live").is_some(), who == "bob", "{who}: only the reader of acme/app has a chip: {v}");
        assert!(who == "bob" || !v.to_string().contains("secret plan"), "{v}");
        assert_eq!(v["board"]["progress"], json!({ "done": 0, "total": 1, "proven": 0 }), "{who}");
    }
}

/// Done is not proven: a merge with every check completed and none red is
/// proven by CI, whether the row's own PR or an attached one; a red, an
/// unfinished or a missing check isn't; evidence on a Done row proves it;
/// and a row not Done is never proven.
#[tokio::test]
async fn done_is_proven_by_ci_or_evidence_or_not() {
    let (base, app, team, id) = start().await;
    for (fid, number, sha) in
        [("fi_ok", 20u64, "h_ok"), ("fi_red", 21, "h_red"), ("fi_run", 22, "h_run"), ("fi_none", 23, "h_none")]
    {
        item(&app, fid, 88, number, "pr", "merged", fid, ("", sha));
    }
    check(&app, 88, "h_ok", "build", "completed", Some("success"));
    check(&app, 88, "h_ok", "docs", "completed", Some("skipped"));
    check(&app, 88, "h_red", "build", "completed", Some("success"));
    check(&app, 88, "h_red", "tests", "completed", Some("failure"));
    check(&app, 88, "h_run", "build", "completed", Some("success"));
    check(&app, 88, "h_run", "tests", "in_progress", None);
    for p in ["fi_ok", "fi_red", "fi_run", "fi_none"] {
        add(&base, &id, fi(p)).await;
    }
    let v = read(&base, "bob", &id).await;
    for p in ["fi_ok", "fi_red", "fi_run", "fi_none"] {
        assert_eq!(on(&v, p).0, "lane_done", "{p} merged: Done");
    }
    assert_eq!(on(&v, "fi_ok").1["proof"], json!({ "by": "ci", "pointer": fi("fi_ok") }), "{v}");
    for p in ["fi_red", "fi_run", "fi_none"] {
        assert!(on(&v, p).1.get("proof").is_none(), "{p} is not proven: {}", on(&v, p).1);
    }
    assert_eq!(v["board"]["progress"], json!({ "done": 4, "total": 4, "proven": 1 }), "{v}");

    // Evidence: a closed issue with a link proves it by evidence, one
    // without isn't proven, and a link on a row that isn't Done proves
    // nothing.
    let (shut, _) = native(&base, &team, "Shut").await;
    let (bare, _) = native(&base, &team, "Bare").await;
    let (open, _) = native(&base, &team, "Open").await;
    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id IN (?1, ?2)",
            params![shut, bare],
        )
        .unwrap();
    let link = json!({ "kind": "link", "url": "https://ci.example.com/1" });
    let mut rows_of = std::collections::HashMap::new();
    for i in [&shut, &bare, &open] {
        let v = add(&base, &id, json!({ "kind": "issue", "id": i })).await;
        rows_of.insert(i.clone(), on(&v, i).1["id"].as_str().unwrap().to_owned());
    }
    attach(&base, &id, &rows_of[&shut], "proof", link.clone()).await;
    let v = attach(&base, &id, &rows_of[&open], "proof", json!({ "kind": "link", "url": "https://ci.example.com/2" }))
        .await;
    assert_eq!(on(&v, &shut).0, "lane_done");
    assert_eq!(on(&v, &shut).1["proof"], json!({ "by": "evidence", "pointer": link }), "{v}");
    assert!(on(&v, &bare).1.get("proof").is_none(), "closed with nothing to show: not proven");
    assert_eq!(on(&v, &open).0, "lane_next");
    assert!(on(&v, &open).1.get("proof").is_none(), "proof on a row that isn't Done is no mark");

    // A PR attached to a closed issue and merged green proves it by CI.
    item(&app, "fi_att", 88, 24, "pr", "merged", "att", ("", "h_att"));
    check(&app, 88, "h_att", "build", "completed", Some("success"));
    let v = attach(&base, &id, &rows_of[&bare], "work", fi("fi_att")).await;
    assert_eq!(on(&v, &bare).1["proof"], json!({ "by": "ci", "pointer": fi("fi_att") }), "{v}");
}

/// One row is one piece of work: a PR that names an issue that is a row is
/// attached to it (by its title or its branch), an issue that PR rows name
/// takes them in, and an item that is on the board already is not added.
#[tokio::test]
async fn one_row_is_one_piece_of_work() {
    let (base, app, team, id) = start().await;
    let (issue, n) = native(&base, &team, "Lanes").await;
    item(&app, "fi_t", 88, 30, "pr", "open", &format!("Arugula #{n}: lanes"), ("feat/x", "abc"));
    item(&app, "fi_b", 88, 31, "pr", "open", "Plumbing", (&format!("feat/{n}-plumbing"), "abd"));
    item(&app, "fi_o", 88, 32, "pr", "open", "Unrelated (#9999) (#32)", ("chore/tidy", "abe"));

    let v = add(&base, &id, json!({ "kind": "issue", "id": issue })).await;
    let row = on(&v, &issue).1["id"].as_str().unwrap().to_owned();
    // By its title.
    let v = add(&base, &id, fi("fi_t")).await;
    assert_eq!(rows(&v).len(), 1, "attached, not added: {v}");
    assert!(v["said"].as_str().unwrap().contains(&row), "the answer says so: {v}");
    assert_eq!(
        on(&v, &issue).1["attachments"][0],
        json!({ "id": on(&v, &issue).1["attachments"][0]["id"], "role": "work", "pointer": fi("fi_t"), "live": on(&v, &issue).1["attachments"][0]["live"] })
    );
    assert_eq!(on(&v, &issue).0, "lane_review", "its PR is open");
    // By its branch.
    let v = add(&base, &id, fi("fi_b")).await;
    assert_eq!((rows(&v).len(), on(&v, &issue).1["attachments"].as_array().unwrap().len()), (1, 2), "{v}");
    // A PR that names no issue of the board is a row of its own (its own
    // number never counts).
    let v = add(&base, &id, fi("fi_o")).await;
    assert_eq!(rows(&v).len(), 2, "{v}");
    assert!(v.get("said").is_none(), "{v}");
    // What is there already is a no-op that says where.
    let v = add(&base, &id, fi("fi_t")).await;
    assert!(v["said"].as_str().unwrap().contains("attached") && v["said"].as_str().unwrap().contains(&row), "{v}");
    let v = add(&base, &id, json!({ "kind": "issue", "id": issue })).await;
    assert!(v["said"].as_str().unwrap().contains(&row), "{v}");
    let v = add(&base, &id, fi("fi_o")).await;
    assert_eq!((rows(&v).len(), v["said"].is_string()), (2, true), "{v}");
    let (s, w) = call(
        &base,
        "bob",
        "POST",
        &format!("/api/boards/{id}/attachments"),
        Some(json!({ "row": row, "role": "work", "pointer": fi("fi_o") })),
    )
    .await;
    assert_eq!((s, w["said"].is_string()), (200, true), "attaching what is a row: a no-op that says where: {w}");

    // The other way round: the PR is a row first, then its issue is added.
    let (_, board2) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Two" }))).await;
    let id2 = board2["board"]["id"].as_str().unwrap().to_owned();
    let (second, m) = native(&base, &team, "Second").await;
    item(&app, "fi_early", 88, 40, "pr", "open", &format!("Closes #{m}"), ("feat/early", "abf"));
    let v = add(&base, &id2, fi("fi_early")).await;
    assert_eq!(on(&v, "fi_early").0, "lane_review", "a PR row with no issue row yet: {v}");
    let v = add(&base, &id2, json!({ "kind": "issue", "id": second })).await;
    assert_eq!(rows(&v).len(), 1, "the PR row became the issue's work: {v}");
    assert!(v["said"].is_string(), "{v}");
    assert_eq!(on(&v, &second).1["attachments"][0]["pointer"], fi("fi_early"));
    assert_eq!(on(&v, &second).0, "lane_review");
}

/// A GitHub issue is named from a PR of its own repo only.
#[tokio::test]
async fn a_github_issue_is_named_from_its_own_repo() {
    let (base, app, _team, id) = start().await;
    // fi_a2 is issue #2 of acme/site (88).
    item(&app, "fi_same", 88, 50, "pr", "open", "Fixes #2", ("feat/s", "aca"));
    item(&app, "fi_other", 77, 51, "pr", "open", "Fixes #2", ("feat/o", "acb"));
    add(&base, &id, fi("fi_a2")).await;
    let v = add(&base, &id, fi("fi_other")).await;
    assert_eq!(rows(&v).len(), 2, "another repo's #2 is not this issue: {v}");
    let v = add(&base, &id, fi("fi_same")).await;
    assert_eq!(rows(&v).len(), 2, "this repo's is: {v}");
    assert_eq!(on(&v, "fi_a2").1["attachments"][0]["pointer"], fi("fi_same"));
}
