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
pub(super) async fn start() -> (String, Arc<crate::Platform>, String, String) {
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

pub(super) async fn add(base: &str, board: &str, pointer: Value) -> Value {
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

pub(super) async fn attach(base: &str, board: &str, row: &str, role: &str, pointer: Value) -> Value {
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

pub(super) async fn read(base: &str, who: &str, board: &str) -> Value {
    let (s, v) = call(base, who, "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(s, 200, "{v}");
    v
}

/// Every row with the lane it is in.
pub(super) fn rows(v: &Value) -> Vec<(&str, &Value)> {
    v["zones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|z| z["rows"].as_array().unwrap().iter().map(|r| (z["id"].as_str().unwrap(), r)))
        .collect()
}

/// The row whose item is `id`.
pub(super) fn on<'a>(v: &'a Value, id: &str) -> (&'a str, &'a Value) {
    rows(v).into_iter().find(|(_, r)| r["pointer"]["id"] == id).unwrap_or_else(|| panic!("no row on {id}: {v}"))
}

pub(super) fn fi(id: &str) -> Value {
    json!({ "kind": "forge_item", "id": id })
}

pub(super) async fn native(base: &str, team: &str, title: &str) -> (String, u64) {
    let (s, i) =
        call(base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": title }))).await;
    assert_eq!(s, 200, "{i}");
    (i["id"].as_str().unwrap().to_owned(), i["number"].as_u64().unwrap())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn item(
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

pub(super) fn check(app: &crate::Platform, repo: u64, sha: &str, name: &str, status: &str, conclusion: Option<&str>) {
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

/// The bug that merged isn't done fixes: one issue row through its PR open
/// (Review), the PR merged with the issue still open (Review, marked), and
/// the issue closed (Done, proven by CI when the merge was green). It never
/// falls back to Next in between.
#[tokio::test]
async fn merged_isnt_done_until_the_issue_closes() {
    let (base, app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    assert_eq!(on(&v, &login).0, "lane_next");

    item(&app, "fi_fix", 88, 30, "pr", "open", "Fix login", ("feat/fix", "h_fix"));
    let v = attach(&base, &id, &row, "work", fi("fi_fix")).await;
    assert_eq!(on(&v, &login).0, "lane_review", "the PR is open");
    assert!(on(&v, &login).1.get("marks").is_none(), "nothing to mark yet: {v}");

    // Merged, the issue still open: Review, and the mark says why.
    app.db.c().execute("UPDATE forge_items SET state = 'merged' WHERE id = 'fi_fix'", params![]).unwrap();
    check(&app, 88, "h_fix", "build", "completed", Some("success"));
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &login).0, "lane_review", "merged is not done: {v}");
    assert_eq!(on(&v, &login).1["marks"], json!([{ "kind": "merged_not_closed" }]), "{v}");
    assert!(on(&v, &login).1.get("proof").is_none(), "{v}");
    assert_eq!(v["board"]["progress"], json!({ "done": 0, "total": 1, "proven": 0 }), "{v}");

    // The issue closed: Done, proven by the green merge, and no mark.
    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id = ?1",
            params![login],
        )
        .unwrap();
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &login).0, "lane_done", "{v}");
    assert_eq!(on(&v, &login).1["proof"], json!({ "by": "ci", "pointer": fi("fi_fix") }), "{v}");
    assert!(on(&v, &login).1.get("marks").is_none(), "the issue is closed: {v}");
    assert_eq!(v["board"]["progress"], json!({ "done": 1, "total": 1, "proven": 1 }), "{v}");
}

/// A PR stacked on another open PR is `after` it, with that PR's number;
/// the repo is named for a reader of the repo and left out for one who isn't.
/// Once the lower one merges, the mark goes.
#[tokio::test]
async fn a_stacked_pr_is_after_the_one_below() {
    let (base, app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    // fi_low is #40 on acme/app (77, private), on branch feat/low; fi_a1 (#1)
    // targets it.
    item(&app, "fi_low", 77, 40, "pr", "open", "Lower", ("feat/low", "h_low"));
    app.db.c().execute("UPDATE forge_items SET base_ref = 'feat/low' WHERE id = 'fi_a1'", params![]).unwrap();
    let v = attach(&base, &id, &row, "work", fi("fi_a1")).await;
    assert_eq!(on(&v, &login).0, "lane_review");

    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &login).1["marks"], json!([{ "kind": "after", "number": 40, "repo": "acme/app" }]), "{v}");
    let v = read(&base, "carol", &id).await;
    assert_eq!(on(&v, &login).1["marks"], json!([{ "kind": "after", "number": 40 }]), "no repo for carol: {v}");
    assert!(!v.to_string().contains("acme/app"), "{v}");

    // Not stacked on a PR that isn't open, or on a branch no PR has.
    app.db.c().execute("UPDATE forge_items SET state = 'merged' WHERE id = 'fi_low'", params![]).unwrap();
    let v = read(&base, "bob", &id).await;
    assert!(on(&v, &login).1.get("marks").is_none(), "the lower one merged: {v}");
    app.db
        .c()
        .execute("UPDATE forge_items SET state = 'open', head_ref = 'other' WHERE id = 'fi_low'", params![])
        .unwrap();
    let v = read(&base, "bob", &id).await;
    assert!(on(&v, &login).1.get("marks").is_none(), "no PR is on that branch: {v}");

    // Only an open PR waits: once the upper one merged, or the lower branch is
    // a PR of another repo, there is nothing to wait for.
    app.db.c().execute("UPDATE forge_items SET head_ref = 'feat/low' WHERE id = 'fi_low'", params![]).unwrap();
    app.db.c().execute("UPDATE forge_items SET state = 'merged' WHERE id = 'fi_a1'", params![]).unwrap();
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &login).1["marks"], json!([{ "kind": "merged_not_closed" }]), "the upper PR merged: {v}");
    app.db.c().execute("UPDATE forge_items SET state = 'open' WHERE id = 'fi_a1'", params![]).unwrap();
    app.db.c().execute("UPDATE forge_items SET repo_id = 88 WHERE id = 'fi_low'", params![]).unwrap();
    let v = read(&base, "bob", &id).await;
    assert!(on(&v, &login).1.get("marks").is_none(), "that branch is in another repo: {v}");
    app.db.c().execute("UPDATE forge_items SET repo_id = 77 WHERE id = 'fi_low'", params![]).unwrap();

    // A PR that is a row's item is marked the same way.
    app.db.c().execute("UPDATE forge_items SET head_ref = 'feat/low' WHERE id = 'fi_low'", params![]).unwrap();
    let (s, _) = call(&base, "bob", "DELETE", &format!("/api/boards/{id}/entries/{row}"), None).await;
    assert_eq!(s, 200);
    let v = add(&base, &id, fi("fi_a1")).await;
    assert_eq!(on(&v, "fi_a1").1["marks"], json!([{ "kind": "after", "number": 40, "repo": "acme/app" }]), "{v}");
}

/// The board raises an ask for red checks on an attached PR (Arugula #367),
/// as a chip says it: to a reader of the repo, and not to one without access,
/// who is told nothing of the PR, not even that it failed.
#[tokio::test]
async fn red_checks_raise_an_ask_only_for_a_reader_of_the_pr() {
    let (base, app, team, id) = start().await;
    let (login, _) = native(&base, &team, "Fix login").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": login })).await;
    let row = on(&v, &login).1["id"].as_str().unwrap().to_owned();
    attach(&base, &id, &row, "work", fi("fi_a1")).await;
    let v = read(&base, "bob", &id).await;
    assert!(on(&v, &login).1.get("ask").is_none(), "checks still running: {v}");

    check(&app, 77, "abc", "tests", "completed", Some("failure"));
    let v = read(&base, "bob", &id).await;
    let ask = &on(&v, &login).1["ask"];
    assert_eq!(
        (ask["question"].as_str(), ask["raised"].clone(), ask["mine"].clone(), ask["can_answer"].clone()),
        (Some("#1's checks failed"), json!(true), json!(true), json!(false)),
        "{v}"
    );

    let v = read(&base, "carol", &id).await;
    assert!(on(&v, &login).1.get("ask").is_none(), "no access to the PR, no ask about it: {v}");
    assert!(!v.to_string().contains("checks failed") && !v.to_string().contains("secret plan"), "{v}");
    assert_eq!(on(&v, &login).0, "lane_review", "the lane still moves for her: {v}");

    // A row whose own item is a PR raises it too, and it clears when the checks pass.
    item(&app, "fi_own", 88, 40, "pr", "open", "own", ("feat/own", "own1"));
    check(&app, 88, "own1", "tests", "completed", Some("failure"));
    let v = add(&base, &id, fi("fi_own")).await;
    assert_eq!(on(&v, "fi_own").1["ask"]["question"], json!("#40's checks failed"), "{v}");
    app.db.c().execute("UPDATE forge_checks SET conclusion = 'success' WHERE head_sha = 'own1'", params![]).unwrap();
    let v = read(&base, "bob", &id).await;
    assert!(on(&v, "fi_own").1.get("ask").is_none(), "it clears with its cause: {v}");

    // Raised asks have nothing to answer.
    let (s, _) =
        call(&base, "bob", "POST", &format!("/api/boards/{id}/asks/ask_none/answer"), Some(json!({ "text": "x" })))
            .await;
    assert_eq!(s, 404);
}

/// A PR row that asks a question and is then taken in by the issue it closes
/// hands its open ask and its history to the issue's row; the issue's row
/// keeps the one ask it has.
#[tokio::test]
async fn an_ask_follows_a_pr_row_taken_in_by_its_issue() {
    let (base, app, team, id) = start().await;
    let (issue, n) = native(&base, &team, "Lanes").await;
    item(&app, "fi_t", 88, 30, "pr", "open", &format!("Arugula #{n}: lanes"), ("feat/x", "abc"));
    let v = add(&base, &id, fi("fi_t")).await;
    let pr_row = on(&v, "fi_t").1["id"].as_str().unwrap().to_owned();
    let ask = |q: &str| json!({ "who": "bob", "question": q, "options": ["Yes", "No"] });
    let (s, v) = call(&base, "bob", "POST", &format!("/api/boards/{id}/rows/{pr_row}/ask"), Some(ask("Done?"))).await;
    assert_eq!(s, 200, "{v}");
    let old = on(&v, "fi_t").1["ask"]["id"].as_str().unwrap().to_owned();
    call(&base, "bob", "POST", &format!("/api/boards/{id}/asks/{old}/answer"), Some(json!({ "option": "Yes" }))).await;
    let (s, v) = call(&base, "bob", "POST", &format!("/api/boards/{id}/rows/{pr_row}/ask"), Some(ask("Merge?"))).await;
    assert_eq!(s, 200, "{v}");

    let v = add(&base, &id, json!({ "kind": "issue", "id": issue })).await;
    assert_eq!(rows(&v).len(), 1, "the PR row went into the issue's: {v}");
    let r = on(&v, &issue).1;
    assert_eq!(r["ask"]["question"], json!("Merge?"), "the open ask came along: {r}");
    assert_eq!(r["asked"][0]["answer"], json!("Yes"), "and so did the history: {r}");
}

/// What an agent's reference names (#366, #436): `#N` is the project's native
/// issue when it has one, else the GitHub item of the one linked repo that has
/// that number, and `owner/repo#N` is that repo's. A number that several repos
/// have, with no native issue, names none, with all said. Nothing by that
/// number is refused, as is an `iss_` id of another project.
#[tokio::test]
async fn a_reference_names_one_item_or_says_why_not() {
    let (base, app, team, _id) = start().await;
    let one = |item: &'static str| {
        let (app, team) = (app.clone(), team.clone());
        async move { crate::boards::resolve::one(&app, "bob", &team, item).await.unwrap() }
    };
    let issue = |p: &Result<arugula_control_wire::boards::Pointer, String>| {
        matches!(p, Ok(arugula_control_wire::boards::Pointer::Issue { .. }))
    };
    // acme/site has issue #2 and PR #3; acme/app (the viewer can read) has PR #1.
    assert_eq!(one("acme/site#2").await, Ok(wire_fi("fi_a2")));
    assert_eq!(one("#3").await, Ok(wire_fi("fi_a3")), "a number only one repo has");
    let nothing = one("#99").await.unwrap_err();
    assert!(nothing.contains("nothing is #99"), "{nothing}");
    // Two repos have #3 and there is no native #3: the choices are listed.
    item(&app, "fi_dup3", 77, 3, "issue", "open", "dup", ("", ""));
    let two = one("#3").await.unwrap_err();
    assert!(two.contains("acme/site#3") && two.contains("acme/app#3"), "{two}");
    assert_eq!(one("acme/app#3").await, Ok(wire_fi("fi_dup3")));
    let (iss, number) = native(&base, &team, "Native one").await;
    assert_eq!(number, 1);
    // Native #1 and acme/app's PR #1: the native issue wins, and the repo's form names the PR.
    let native_wins = one("#1").await;
    assert_eq!(native_wins, Ok(arugula_control_wire::boards::Pointer::Issue { id: iss.clone() }));
    assert_eq!(one("acme/app#1").await, Ok(wire_fi("fi_a1")), "the repo's form names the repo's item");
    let (_, _) = native(&base, &team, "Native two").await;
    assert!(issue(&one("#2").await), "#2 is native and acme/site's issue: the native one");
    let (_, _) = native(&base, &team, "Native three").await;
    assert!(issue(&one("#3").await), "native #3 wins over both repos' #3");
    assert_eq!(
        crate::boards::resolve::one(&app, "bob", &team, &iss).await.unwrap(),
        Ok(arugula_control_wire::boards::Pointer::Issue { id: iss.clone() })
    );
    let other = crate::boards::resolve::one(&app, "bob", &team, "iss_notmine").await.unwrap().unwrap_err();
    assert!(other.contains("isn't an issue of this project"), "{other}");
    let bad = one("banana").await.unwrap_err();
    assert!(bad.contains("#N, owner/repo#N"), "{bad}");
}

fn wire_fi(id: &str) -> arugula_control_wire::boards::Pointer {
    arugula_control_wire::boards::Pointer::ForgeItem { id: id.to_owned() }
}

/// Every source's issues carry a status category to the board, and Done
/// means the same for all: a native issue closed as not planned, a Plane
/// item in `cancelled` and a closed GitHub issue are Done, a Plane item
/// that is started and an open native issue are not.
#[tokio::test]
async fn every_sources_issue_has_a_category_and_done_means_the_same() {
    let (base, app, team, id) = start().await;
    app.db
        .c()
        .execute(
            "INSERT INTO tracker_connections (id, project_id, provider, workspace_slug, source_project_id, identifier,
                 name, mirror_id, created_at)
             VALUES ('trk_1', ?1, 'plane', 'acme', 'p1', 'ARUGU', 'Arugula', -1, 1)",
            params![team],
        )
        .unwrap();
    for (id, number, state, group) in
        [("fi_pl1", 1u64, "closed", "cancelled"), ("fi_pl2", 2, "open", "started"), ("fi_pl3", 3, "open", "completed")]
    {
        app.db
            .c()
            .execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, provider, connection_id, state_name,
                     state_group, created_at, updated_at, synced_at)
                 VALUES (?1, -1, ?2, 'issue', ?3, 't', 'plane', 'trk_1', ?4, ?4, 1, 1, 1)",
                params![id, number, state, group],
            )
            .unwrap();
    }
    item(&app, "fi_gc", 88, 700, "issue", "closed", "Closed on GitHub", ("", ""));
    let (dropped, _) = native(&base, &team, "Not planned").await;
    let (open, _) = native(&base, &team, "Open").await;
    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'not_planned', closed_at = 5 WHERE id = ?1",
            params![dropped],
        )
        .unwrap();
    let pointers = [
        fi("fi_pl1"),
        fi("fi_pl2"),
        fi("fi_pl3"),
        fi("fi_gc"),
        json!({ "kind": "issue", "id": dropped }),
        json!({ "kind": "issue", "id": open }),
    ];
    for p in pointers {
        add(&base, &id, p).await;
    }
    let v = read(&base, "bob", &id).await;
    let seen = |item: &str| {
        let (lane, row) = on(&v, item);
        (lane.to_owned(), row["live"]["category"].as_str().map(str::to_owned))
    };
    let is = |lane: &str, category: &str| (lane.to_owned(), Some(category.to_owned()));
    assert_eq!(seen("fi_pl1"), is("lane_done", "canceled"), "{v}");
    assert_eq!(seen("fi_pl2"), is("lane_doing", "started"), "{v}");
    assert_eq!(seen("fi_pl3"), is("lane_done", "completed"), "the group decides, not the mirrored state: {v}");
    assert_eq!(seen("fi_gc"), is("lane_done", "completed"), "{v}");
    assert_eq!(seen(&dropped), is("lane_done", "canceled"), "{v}");
    assert_eq!(seen(&open), is("lane_next", "unstarted"), "{v}");
}
