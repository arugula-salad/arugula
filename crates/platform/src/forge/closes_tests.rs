//! A merged PR closes the native issues it says it closes (Arugula #432),
//! through the mirror's own webhook, in M165's world (`viewer_tests::world`):
//! `Closes Arugula #N` closes on merge, as completed, naming the PR, and the
//! board's row is Done, proven by CI. A bare `Arugula #N`, a PR closed
//! unmerged and an issue of another project close nothing; one closed
//! already stays as it was; a second pass adds no event.

use std::time::Instant;

use serde_json::{Value, json};

use super::{
    closes,
    lanes_tests::{add, check, fi, native, on, read, start},
};
use crate::{
    Platform,
    chat_tests::call,
    db::{Sql as _, issues::Event, sql::params},
};

/// A `pull_request` webhook of acme/site (repo 88) as the mirror and then
/// the closer see it: `sender` merged or closed it at second `at`.
async fn deliver(app: &Platform, action: &str, number: u64, (title, body): (&str, &str), at: u32, merged: bool, sender: (&str, u64)) {
    let stamp = |s: u32| format!("2026-10-30T01:{:02}:{:02}Z", s / 60, s % 60);
    let v = json!({
        "action": action,
        "repository": { "id": 88, "full_name": "acme/site" },
        "sender": { "login": sender.0, "id": sender.1 },
        "pull_request": {
            "id": 1000 + number, "number": number, "title": title, "body": body,
            "state": if action == "opened" { "open" } else { "closed" },
            "user": { "login": "bob-gh", "id": 1002 }, "labels": [], "assignees": [],
            "requested_reviewers": [], "requested_teams": [], "draft": false,
            "merged_at": if merged { json!(stamp(at)) } else { Value::Null },
            "head": { "ref": "feat/x", "sha": format!("h{number}") }, "base": { "ref": "main", "sha": "base" },
            "created_at": stamp(1), "updated_at": stamp(at),
        },
    });
    crate::forge::ingest::webhook(app, "pull_request", "d", &v, Instant::now());
    closes::webhook(app, "pull_request", &v).await;
}

const BOB: (&str, u64) = ("bob-gh", 1002);

fn closed_events(app: &Platform, issue: &str) -> Vec<Event> {
    app.db.issue_events(issue).unwrap().into_iter().filter(|e| e.kind.as_str() == "closed").collect()
}

fn mirror_bodies(app: &Platform) {
    app.db.c().execute("UPDATE linked_repos SET mirror_content = ?1 WHERE repo_id = 88", params![true]).unwrap();
}

/// `Closes Arugula #N` in the body: the PR opens and nothing closes; it
/// merges and the issue is closed as completed by its merger, the event
/// names the PR, and the board's row is Done, proven by CI. Redelivered, and
/// read again by the poller's pass, it adds no second event.
#[tokio::test]
async fn a_merged_pr_that_says_closes_closes_the_issue() {
    let (base, app, team, board) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Close me").await;
    let v = add(&base, &board, json!({ "kind": "issue", "id": issue })).await;
    assert_eq!(on(&v, &issue).0, "lane_next");

    let body = format!("Does it.\n\nCloses Arugula #{n}");
    deliver(&app, "opened", 70, ("Plumbing", &body), 10, false, BOB).await;
    assert!(closed_events(&app, &issue).is_empty(), "open PRs close nothing");
    assert_eq!(on(&read(&base, "bob", &board).await, &issue).0, "lane_review");

    check(&app, 88, "h70", "build", "completed", Some("success"));
    deliver(&app, "closed", 70, ("Plumbing", &body), 11, true, BOB).await;
    let events = closed_events(&app, &issue);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!((events[0].actor.as_str(), &events[0].actor_agent), ("bob", &None));
    assert_eq!(events[0].detail, json!({ "reason": "completed", "pr": "acme/site#70" }));
    let (s, got) = call(&base, "bob", "GET", &format!("/api/issues/{issue}"), None).await;
    assert_eq!((s, got["state"].as_str(), got["close_reason"].as_str()), (200, Some("closed"), Some("completed")));
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &issue).0, "lane_done", "{v}");
    let pr: String = app
        .db
        .c()
        .query_row("SELECT id FROM forge_items WHERE repo_id = 88 AND number = 70", params![], |r| r.get(0))
        .unwrap();
    assert_eq!(on(&v, &issue).1["proof"], json!({ "by": "ci", "pointer": fi(&pr) }), "{v}");

    // The same delivery again, and the poller's pass over the PR.
    deliver(&app, "closed", 70, ("Plumbing", &body), 11, true, BOB).await;
    closes::merged(&app, 88, &[70]).await;
    assert_eq!(closed_events(&app, &issue).len(), 1, "no second event");
}

/// The title says it too; an `iss_` id names the issue as well as its
/// number; a merger who is not in the project (or unknown) leaves the App
/// to close it; an issue closed already stays as it was.
#[tokio::test]
async fn titles_ids_other_mergers_and_issues_closed_already() {
    let (base, app, team, _) = start().await;
    let (a, na) = native(&base, &team, "A").await;
    let (b, _) = native(&base, &team, "B").await;
    let (c, nc) = native(&base, &team, "C").await;
    let (s, _) = call(
        &base,
        "bob",
        "PATCH",
        &format!("/api/issues/{c}"),
        Some(json!({ "state": "closed", "reason": "not_planned" })),
    )
    .await;
    assert_eq!(s, 200);
    mirror_bodies(&app);

    let title = format!("Fixes: Arugula #{na}");
    let body = format!("resolves {b}, closed ARUGULA #{nc}");
    deliver(&app, "opened", 71, (&title, &body), 9, false, BOB).await;
    deliver(&app, "closed", 71, (&title, &body), 10, true, ("eve-gh", 1005)).await;
    let ea = closed_events(&app, &a);
    assert_eq!(ea.len(), 1);
    assert_eq!(ea[0].actor, closes::APP, "a merger outside the project: the App");
    assert_eq!(ea[0].detail["pr"], "acme/site#71");
    assert_eq!(closed_events(&app, &b).len(), 1, "named by its id");
    let ec = closed_events(&app, &c);
    assert_eq!(ec.len(), 1, "its own close only: {ec:?}");
    assert_eq!(ec[0].detail, json!({ "reason": "not_planned" }), "not changed to completed");
    assert_eq!(app.db.issue(&c).unwrap().unwrap().state.reason().map(|r| r.as_str()), Some("not_planned"));

    // The poller reads a PR the webhook never told of: its pass closes too.
    let (d, nd) = native(&base, &team, "D").await;
    let v = json!({
        "action": "closed", "repository": { "id": 88, "full_name": "acme/site" }, "sender": BOB.0,
        "pull_request": {
            "id": 1075, "number": 75, "title": format!("Closes Arugula #{nd}"), "state": "closed",
            "user": { "login": "bob-gh", "id": 1002 }, "labels": [], "assignees": [],
            "requested_reviewers": [], "requested_teams": [], "draft": false,
            "merged_at": "2026-10-30T01:00:10Z", "head": { "ref": "x", "sha": "h75" },
            "base": { "ref": "main", "sha": "base" }, "created_at": "2026-10-30T01:00:01Z",
            "updated_at": "2026-10-30T01:00:10Z",
        },
    });
    crate::forge::ingest::webhook(&app, "pull_request", "d", &v, Instant::now());
    assert!(closed_events(&app, &d).is_empty());
    closes::merged(&app, 88, &[75]).await;
    assert_eq!(closed_events(&app, &d).len(), 1);
}

/// A bare `Arugula #N` only links; a PR closed unmerged closes nothing;
/// neither does one naming another project's issue, by number or by id.
#[tokio::test]
async fn only_a_merge_that_says_closes_in_its_own_project() {
    let (base, app, team, _) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Stays open").await;
    let eve = app.db.personal_project("eve", 1).unwrap();
    let new = crate::db::issues::NewIssue { project: &eve, title: "Eve's", body: "", labels: &[], assignees: &[] };
    let theirs = app.db.create_issue(&new, crate::db::issues::Actor::person("eve"), 1).unwrap().unwrap();
    let theirs_id = theirs.id.as_str();

    for (number, title, body) in [
        (72, "Links", format!("Arugula #{n}")),
        (73, "Unmerged", format!("Closes Arugula #{n}")),
        (74, "Elsewhere", format!("Closes {theirs_id}")),
    ] {
        deliver(&app, "opened", number, (title, &body), 9, false, BOB).await;
        deliver(&app, "closed", number, (title, &body), 10, number != 73, BOB).await;
    }
    assert!(closed_events(&app, &issue).is_empty(), "linked, unmerged: open");
    assert!(closed_events(&app, theirs_id).is_empty(), "another project's issue");
    assert_eq!(app.db.issue(theirs_id).unwrap().unwrap().state.as_str(), "open");
}

/// Through GitHub's own door, signed: the merge closes the issue, and the
/// same merge delivered again (as GitHub does) adds no second event.
#[tokio::test]
async fn the_webhook_route_closes_on_merge() {
    let (base, app, team, _) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Over the wire").await;
    let pr = |action: &str, merged: bool| {
        json!({
            "action": action, "repository": { "id": 88, "full_name": "acme/site" }, "sender": { "login": "bob-gh", "id": 1002 },
            "pull_request": {
                "id": 1076, "number": 76, "title": "Wire", "body": format!("Closes Arugula #{n}"),
                "state": if merged || action == "closed" { "closed" } else { "open" },
                "user": { "login": "bob-gh", "id": 1002 }, "labels": [], "assignees": [],
                "requested_reviewers": [], "requested_teams": [], "draft": false,
                "merged_at": if merged { json!("2026-10-30T01:00:10Z") } else { Value::Null },
                "head": { "ref": "x", "sha": "h76" }, "base": { "ref": "main", "sha": "base" },
                "created_at": "2026-10-30T01:00:01Z", "updated_at": if merged { "2026-10-30T01:00:10Z" } else { "2026-10-30T01:00:05Z" },
            },
        })
    };
    assert_eq!(super::links_tests::hook(&base, "pull_request", "d1", &pr("opened", false)).await, 202);
    assert!(closed_events(&app, &issue).is_empty());
    for delivery in ["d2", "d3"] {
        assert_eq!(super::links_tests::hook(&base, "pull_request", delivery, &pr("closed", true)).await, 202);
    }
    assert_eq!(closed_events(&app, &issue).len(), 1);
}
