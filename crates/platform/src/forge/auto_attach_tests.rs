//! A PR attaches itself to the row of the issue it names (Arugula #368),
//! through the mirror's own webhook, in M165's world
//! (`viewer_tests::world`): opened, the row is in Review with no board
//! write; edited to name nothing, it is let go; merged green and the issue
//! closed, Done and proven by CI. A PR stays within its project, and an
//! attachment a person made is never taken off.

use std::time::Instant;

use serde_json::{Value, json};

use super::{
    lanes_tests::{add, attach, check, fi, native, on, read, start},
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
