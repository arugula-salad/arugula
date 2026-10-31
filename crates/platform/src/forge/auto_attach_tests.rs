//! A PR attaches itself to the row of the issue it names (Arugula #368),
//! through the mirror's own webhook, in M165's world
//! (`viewer_tests::world`): opened, the row is in Review with no board
//! write; edited to name nothing, it is let go; merged green and the issue
//! closed, Done and proven by CI. A PR stays within its project, and an
//! attachment a person made is never taken off.

use std::time::Instant;

use serde_json::{Value, json};

use super::{
    lanes_tests::{add, attach, check, fi, item, native, on, read, rows, start},
    viewer_tests::world,
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

/// A `pull_request` webhook of `repo` (88 is `acme/site`, 99 is eve's):
/// the PR as GitHub sends it at second `at` of the hour, merged or not.
/// `hook_with` also says its body.
#[allow(clippy::too_many_arguments)]
fn hook(app: &Platform, repo: u64, action: &str, number: u64, title: &str, branch: &str, at: u32, merged: bool) {
    hook_with(app, repo, action, number, (title, ""), branch, at, merged)
}

#[allow(clippy::too_many_arguments)]
fn hook_with(
    app: &Platform,
    repo: u64,
    action: &str,
    number: u64,
    (title, body): (&str, &str),
    branch: &str,
    at: u32,
    merged: bool,
) {
    let name = if repo == 88 { "acme/site" } else { "eve-gh/x" };
    let stamp = |s: u32| format!("2026-10-30T01:{:02}:{:02}Z", s / 60, s % 60);
    let v = json!({
        "action": action,
        "repository": { "id": repo, "full_name": name },
        "sender": { "login": "bob-gh", "id": 1002 },
        "pull_request": {
            "id": 1000 + number, "number": number, "title": title, "body": body, "state": if merged { "closed" } else { "open" },
            "user": { "login": "bob-gh", "id": 1002 }, "labels": [], "assignees": [],
            "requested_reviewers": [], "requested_teams": [], "draft": false,
            "merged_at": if merged { json!(stamp(at)) } else { Value::Null },
            "head": { "ref": branch, "sha": format!("h{number}") }, "base": { "ref": "main", "sha": "base" },
            "created_at": stamp(1), "updated_at": stamp(at),
        },
    });
    crate::forge::ingest::webhook(app, "pull_request", "d", &v, Instant::now());
}

fn pr_id(app: &Platform, repo: u64, number: u64) -> String {
    app.db
        .c()
        .query_row("SELECT id FROM forge_items WHERE repo_id = ?1 AND number = ?2", params![repo, number], |r| r.get(0))
        .unwrap()
}

/// How many attachments `board` has, and how many of them are automatic.
fn attachments(app: &Platform, board: &str) -> (u64, u64) {
    let c = app.db.c();
    let all = c.query_row("SELECT count(*) FROM board_attachments WHERE board_id = ?1", params![board], |r| r.get(0));
    let auto = c.query_row(
        "SELECT count(*) FROM board_attachments WHERE board_id = ?1 AND source = 'auto'",
        params![board],
        |r| r.get(0),
    );
    (all.unwrap(), auto.unwrap())
}

/// Opened naming the issue (in its title, or its branch), a PR moves its
/// row to Review with no board write; naming nothing, it lets go; merged
/// green and the issue closed, the row is Done, proven by CI. Running it
/// again changes nothing.
#[tokio::test]
async fn a_pr_attaches_to_its_issues_row_and_lets_go() {
    let (base, app, team, id) = start().await;
    let (issue, n) = native(&base, &team, "Lanes").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": issue })).await;
    assert_eq!(on(&v, &issue).0, "lane_next");

    hook(&app, 88, "opened", 40, &format!("Arugula #{n}: lanes"), "feat/x", 10, false);
    let pr = pr_id(&app, 88, 40);
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &issue).0, "lane_review", "{v}");
    assert_eq!(on(&v, &issue).1["attachments"][0]["pointer"], fi(&pr), "{v}");
    assert_eq!(attachments(&app, &id), (1, 1));

    // Again, and a newer write of the same PR: nothing changes.
    hook(&app, 88, "synchronize", 40, &format!("Arugula #{n}: lanes"), "feat/x", 11, false);
    assert_eq!(attachments(&app, &id), (1, 1), "idempotent");

    // Edited to name nothing: let go. Named by its branch instead: back.
    hook(&app, 88, "edited", 40, "Plumbing", "feat/x", 12, false);
    assert_eq!(attachments(&app, &id), (0, 0), "the reference is edited away");
    assert_eq!(on(&read(&base, "bob", &id).await, &issue).0, "lane_next");
    hook(&app, 88, "edited", 40, "Plumbing", &format!("feat/{n}-plumbing"), 13, false);
    assert_eq!(attachments(&app, &id), (1, 1), "named by its branch");

    // Merged, CI green: the row waits for its issue to close, then is Done
    // and proven.
    check(&app, 88, "h40", "build", "completed", Some("success"));
    hook(&app, 88, "closed", 40, "Plumbing", &format!("feat/{n}-plumbing"), 14, true);
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &issue).1["marks"], json!([{ "kind": "merged_not_closed" }]), "{v}");
    app.db
        .c()
        .execute(
            "UPDATE issues SET state = 'closed', close_reason = 'completed', closed_at = 5 WHERE id = ?1",
            params![issue],
        )
        .unwrap();
    let v = read(&base, "bob", &id).await;
    assert_eq!(on(&v, &issue).0, "lane_done", "{v}");
    assert_eq!(on(&v, &issue).1["proof"], json!({ "by": "ci", "pointer": fi(&pr) }), "{v}");

    // Closed without merging stays attached.
    hook(&app, 88, "opened", 41, &format!("Arugula #{n}: other"), "feat/y", 15, false);
    hook(&app, 88, "closed", 41, &format!("Arugula #{n}: other"), "feat/y", 16, false);
    assert_eq!(attachments(&app, &id), (2, 2));
}

/// A PR names only issues of its own repo's project: eve's PR naming the
/// number of a team issue, or the team's naming the number of an issue of
/// eve's, touches nobody else's board.
#[tokio::test]
async fn a_pr_never_reaches_another_projects_board() {
    let hub = std::sync::Arc::new(TestHub::default());
    let (gh, app, base) = super::links_tests::serve(&hub).await;
    let (team, eve) = world(&hub, &gh, &app).await;
    for owner in ["team:t1", "account:eve"] {
        hub.flag(owner, true);
        hub.flag_named(owner, crate::boards::FLAG, true);
    }
    let board = |who: &str, project: &str| {
        let (who, project) = (who.to_owned(), project.to_owned());
        let base = base.clone();
        async move {
            let (s, b) =
                call(&base, &who, "POST", &format!("/api/projects/{project}/boards"), Some(json!({ "title": "B" })))
                    .await;
            assert_eq!(s, 200, "{b}");
            b["board"]["id"].as_str().unwrap().to_owned()
        }
    };
    let (theirs, mine) = (board("eve", &eve).await, board("bob", &team).await);
    let (e_issue, e_n) = native_of(&base, "eve", &eve, "Eve's").await;
    let (t_issue, t_n) = native_of(&base, "bob", &team, "Team's").await;
    assert_eq!(e_n, t_n, "the same number in two projects");
    for (who, b, i) in [("eve", &theirs, &e_issue), ("bob", &mine, &t_issue)] {
        let (s, v) = call(
            &base,
            who,
            "PUT",
            &format!("/api/boards/{b}/entries"),
            Some(json!({ "entry": "row", "note": "n", "pointer": { "kind": "issue", "id": i } })),
        )
        .await;
        assert_eq!(s, 200, "{v}");
    }

    hook(&app, 99, "opened", 50, &format!("Arugula #{e_n}: eve's"), "x", 10, false);
    assert_eq!((attachments(&app, &theirs).0, attachments(&app, &mine).0), (1, 0), "eve's repo: eve's project");
    hook(&app, 88, "opened", 51, &format!("Arugula #{t_n}: team's"), "y", 10, false);
    assert_eq!((attachments(&app, &theirs).0, attachments(&app, &mine).0), (1, 1), "the team's repo: the team's");
}

async fn native_of(base: &str, who: &str, project: &str, title: &str) -> (String, u64) {
    let (s, i) =
        call(base, who, "POST", &format!("/api/projects/{project}/issues"), Some(json!({ "title": title }))).await;
    assert_eq!(s, 200, "{i}");
    (i["id"].as_str().unwrap().to_owned(), i["number"].as_u64().unwrap())
}

/// A person attaches a PR by hand; the PR then names the issue and drops
/// it again: the attachment is theirs, and stays. The body is read too
/// when the content cache has it.
#[tokio::test]
async fn a_person_attachment_is_never_taken_off() {
    let (base, app, team, id) = start().await;
    let (issue, n) = native(&base, &team, "Lanes").await;
    let v = add(&base, &id, json!({ "kind": "issue", "id": issue })).await;
    let row = on(&v, &issue).1["id"].as_str().unwrap().to_owned();

    hook(&app, 88, "opened", 60, "Plumbing", "chore/x", 10, false);
    let pr = pr_id(&app, 88, 60);
    attach(&base, &id, &row, "work", fi(&pr)).await;
    assert_eq!(attachments(&app, &id), (1, 0));

    // Its body names the issue: already attached, so nothing is added.
    app.db.c().execute("UPDATE linked_repos SET mirror_content = ?1 WHERE repo_id = 88", params![true]).unwrap();
    let edit = |at, body: &str| hook_with(&app, 88, "edited", 60, ("Plumbing", body), "chore/x", at, false);
    edit(11, &format!("Arugula #{n}"));
    assert_eq!(attachments(&app, &id), (1, 0), "theirs, not ours");

    // The body drops the reference: it stays.
    edit(12, "nothing");
    assert_eq!(attachments(&app, &id), (1, 0), "a person's attachment stays");
    assert_eq!(on(&read(&base, "bob", &id).await, &issue).0, "lane_review");

    // Only the body names it: attached by us; the body drops it: let go.
    hook_with(&app, 88, "opened", 61, ("More", &format!("Closes #{n}")), "chore/y", 13, false);
    assert_eq!(attachments(&app, &id), (2, 1), "the body names it");
    hook_with(&app, 88, "edited", 61, ("More", "nothing"), "chore/y", 14, false);
    assert_eq!(attachments(&app, &id), (1, 0), "the body drops it");
}

/// A second board of the team's project.
async fn another_board(base: &str, team: &str) -> String {
    let (s, b) =
        call(base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Two" }))).await;
    assert_eq!(s, 200, "{b}");
    b["board"]["id"].as_str().unwrap().to_owned()
}

/// The pointers attached to the row of `item`.
fn attached_to(v: &Value, item: &str) -> Vec<String> {
    on(v, item).1["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["pointer"]["id"].as_str().unwrap().to_owned())
        .collect()
}

/// A board with a native #n and an unrelated GitHub #n of acme/site (88):
/// "Closes #n" from a PR of acme/site is the GitHub issue's, "Arugula #n" is
/// the native one's from any repo, and "Closes #n" from another repo
/// (acme/app, 77) falls to the native issue. Auto-attach and a hand add say
/// the same.
#[tokio::test]
async fn a_bare_number_is_the_prs_repos_issue_and_arugula_is_the_native_one() {
    let (base, app, team, id) = start().await;
    let (nat, n) = native(&base, &team, "Native").await;
    item(&app, "fi_g", 88, n, "issue", "open", "GitHub's own", ("", ""));
    add(&base, &id, json!({ "kind": "issue", "id": nat })).await;
    let v = add(&base, &id, fi("fi_g")).await;
    assert_eq!(rows(&v).len(), 2, "two issues, two rows: {v}");

    hook(&app, 88, "opened", 70, &format!("Closes #{n}"), "x/a", 10, false);
    let v = read(&base, "bob", &id).await;
    assert_eq!((attached_to(&v, &nat).len(), attached_to(&v, "fi_g")), (0, vec![pr_id(&app, 88, 70)]), "{v}");

    hook(&app, 88, "opened", 71, &format!("Arugula #{n}: native"), "x/b", 11, false);
    let v = read(&base, "bob", &id).await;
    assert_eq!(attached_to(&v, &nat), vec![pr_id(&app, 88, 71)], "{v}");
    assert_eq!(attached_to(&v, "fi_g").len(), 1, "the GitHub row got nothing more: {v}");

    hook(&app, 77, "opened", 72, &format!("Closes #{n}"), "x/c", 12, false);
    let v = read(&base, "bob", &id).await;
    assert_eq!(attached_to(&v, &nat), vec![pr_id(&app, 88, 71), pr_id(&app, 77, 72)], "another repo's PR: native: {v}");
    assert_eq!(attached_to(&v, "fi_g").len(), 1, "{v}");

    // A hand add of the same three PRs, on a board of its own.
    let two = another_board(&base, &team).await;
    add(&base, &two, json!({ "kind": "issue", "id": nat })).await;
    add(&base, &two, fi("fi_g")).await;
    item(&app, "fi_h1", 88, 80, "pr", "open", &format!("Closes #{n}"), ("x/d", "d1"));
    item(&app, "fi_h2", 88, 81, "pr", "open", &format!("Arugula #{n}"), ("x/e", "d2"));
    item(&app, "fi_h3", 77, 82, "pr", "open", &format!("Closes #{n}"), ("x/f", "d3"));
    for pr in ["fi_h1", "fi_h2", "fi_h3"] {
        let v = add(&base, &two, fi(pr)).await;
        assert_eq!(rows(&v).len(), 2, "{pr} was attached, not added: {v}");
    }
    let v = read(&base, "bob", &two).await;
    assert_eq!(attached_to(&v, "fi_g"), ["fi_h1"], "{v}");
    assert_eq!(attached_to(&v, &nat), ["fi_h2", "fi_h3"], "{v}");
}

/// A native issue exported to GitHub and mirrored is one row, whichever is
/// added first, the picker lists it once (the native one, then not at all
/// once it is on a board), and a PR finds the row by either number.
#[tokio::test]
async fn an_exported_issue_is_one_row_and_listed_once() {
    let (base, app, team, id) = start().await;
    let (nat, n) = native(&base, &team, "Exported").await;
    item(&app, "fi_x", 88, 500, "issue", "open", "Exported", ("", ""));
    app.db
        .c()
        .execute(
            "UPDATE issues SET github_repo = 'acme/site', github_repo_id = 88, github_number = 500 WHERE id = ?1",
            params![nat],
        )
        .unwrap();
    let listed = |untriaged: bool| {
        let (base, team, nat) = (base.clone(), team.clone(), nat.clone());
        async move {
            let q = if untriaged { "&untriaged=true" } else { "" };
            let (_, v) =
                call(&base, "bob", "GET", &format!("/api/projects/{team}/work?kind=issue&limit=100{q}"), None).await;
            v["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["pointer"]["id"].as_str().unwrap().to_owned())
                .filter(|i| *i == "fi_x" || *i == nat)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(listed(false).await, std::slice::from_ref(&nat), "the picker lists the native issue, not its copy");

    // The native issue first, then its copy: the same row.
    add(&base, &id, json!({ "kind": "issue", "id": nat })).await;
    let v = add(&base, &id, fi("fi_x")).await;
    assert_eq!(rows(&v).len(), 1, "{v}");
    let row = on(&v, &nat).1["id"].as_str().unwrap().to_owned();
    assert!(v["said"].as_str().unwrap().contains(&row), "the answer says which row: {v}");
    assert!(listed(true).await.is_empty(), "on a board, by either id: not untriaged");

    // The copy first, then the native issue.
    let two = another_board(&base, &team).await;
    add(&base, &two, fi("fi_x")).await;
    let v = add(&base, &two, json!({ "kind": "issue", "id": nat })).await;
    assert_eq!(rows(&v).len(), 1, "{v}");
    assert!(v["said"].is_string(), "{v}");

    // A PR finds the row by the GitHub number or by the native one.
    hook(&app, 88, "opened", 90, "Closes #500", "x/g", 10, false);
    hook(&app, 88, "opened", 91, &format!("Arugula #{n}"), "x/h", 11, false);
    let (a, b) = (pr_id(&app, 88, 90), pr_id(&app, 88, 91));
    assert_eq!(attached_to(&read(&base, "bob", &id).await, &nat), [a.clone(), b.clone()]);
    assert_eq!(
        attached_to(&read(&base, "bob", &two).await, "fi_x"),
        [a, b],
        "the other board's row points at the copy"
    );
}

/// The trap: two work items that turn out to be one, both on boards. The
/// GitHub issue is a row first; the native issue is a row of another board
/// before it is linked; then the link is recorded and the native issue is
/// added where the copy is. One work item, one row on each board, no ref
/// left pointing at a work item that is gone.
#[tokio::test]
async fn a_native_issue_that_gains_its_link_merges_with_the_copy_already_on_boards() {
    let (base, app, team, id) = start().await;
    let (nat, _) = native(&base, &team, "Linked later").await;
    item(&app, "fi_t", 88, 600, "issue", "open", "Linked later", ("", ""));
    let two = another_board(&base, &team).await;
    add(&base, &id, fi("fi_t")).await;
    add(&base, &two, json!({ "kind": "issue", "id": nat })).await;
    let count = |sql: &str| -> u64 { app.db.c().query_row(sql, params![], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT count(*) FROM work_items"), 2, "not linked: two");

    app.db
        .c()
        .execute(
            "UPDATE issues SET github_repo = 'acme/site', github_repo_id = 88, github_number = 600 WHERE id = ?1",
            params![nat],
        )
        .unwrap();
    let v = add(&base, &id, json!({ "kind": "issue", "id": nat })).await;
    assert_eq!(rows(&v).len(), 1, "the copy's row is the native issue's too: {v}");
    assert!(v["said"].is_string(), "{v}");

    assert_eq!(count("SELECT count(*) FROM work_items"), 1);
    assert_eq!(count("SELECT count(*) FROM work_item_refs"), 2);
    assert_eq!(count("SELECT count(*) FROM work_item_refs WHERE work_item NOT IN (SELECT id FROM work_items)"), 0);
    assert_eq!(count("SELECT count(DISTINCT work_item) FROM board_entries WHERE work_item IS NOT NULL"), 1);
    assert_eq!(count("SELECT count(*) FROM board_entries WHERE kind = 'row'"), 2, "one row on each board");
    let v = read(&base, "bob", &two).await;
    assert_eq!(rows(&v).len(), 1, "{v}");
}
