//! A merged PR closes the native issues it names (Arugula #432), through the
//! mirror's own webhook, in M165's world (`viewer_tests::world`): the same
//! links the boards read (`Arugula #N` in the body, a branch `feat/N-…`)
//! close on merge, as completed, naming the PR, and the board's row is Done,
//! proven by CI, with nobody writing a keyword. `part of Arugula #N`, a PR
//! closed unmerged and an issue of another project close nothing; one closed
//! already stays as it was; a PR a person attached to a row closes that
//! row's issue; a second pass adds no event.

use std::time::Instant;

use serde_json::{Value, json};

use super::{
    closes,
    lanes_tests::{add, attach, check, fi, native, on, read, start},
};
use crate::{
    Platform,
    chat_tests::call,
    db::{Sql as _, issues::Event, sql::params},
};

/// A PR of acme/site (repo 88) as GitHub sends it at second `at` of the hour.
pub(super) struct Pr<'a> {
    pub(super) number: u64,
    pub(super) title: &'a str,
    pub(super) body: &'a str,
    pub(super) branch: &'a str,
}

pub(super) fn pr<'a>(number: u64, title: &'a str, body: &'a str) -> Pr<'a> {
    Pr { number, title, body, branch: "chore/x" }
}

fn payload(action: &str, pr: &Pr<'_>, at: u32, merged: bool, sender: (&str, u64)) -> Value {
    let stamp = |s: u32| format!("2026-10-30T01:{:02}:{:02}Z", s / 60, s % 60);
    json!({
        "action": action,
        "repository": { "id": 88, "full_name": "acme/site" },
        "sender": { "login": sender.0, "id": sender.1 },
        "pull_request": {
            "id": 1000 + pr.number, "number": pr.number, "title": pr.title, "body": pr.body,
            "state": if action == "opened" { "open" } else { "closed" },
            "user": { "login": "bob-gh", "id": 1002 }, "labels": [], "assignees": [],
            "requested_reviewers": [], "requested_teams": [], "draft": false,
            "merged_at": if merged { json!(stamp(at)) } else { Value::Null },
            "head": { "ref": pr.branch, "sha": format!("h{}", pr.number) }, "base": { "ref": "main", "sha": "base" },
            "created_at": stamp(1), "updated_at": stamp(at),
        },
    })
}

/// A webhook as the mirror and then the closer see it.
pub(super) async fn deliver(app: &Platform, action: &str, pr: &Pr<'_>, at: u32, merged: bool, sender: (&str, u64)) {
    let v = payload(action, pr, at, merged, sender);
    crate::forge::ingest::webhook(app, "pull_request", "d", &v, Instant::now());
    closes::webhook(app, "pull_request", &v).await;
}

/// Opened, then merged by `sender`: what a PR does.
async fn open_and_merge(app: &Platform, pr: &Pr<'_>, sender: (&str, u64)) {
    deliver(app, "opened", pr, 9, false, BOB).await;
    deliver(app, "closed", pr, 10, true, sender).await;
}

pub(super) const BOB: (&str, u64) = ("bob-gh", 1002);

fn closed_events(app: &Platform, issue: &str) -> Vec<Event> {
    app.db.issue_events(issue).unwrap().into_iter().filter(|e| e.kind.as_str() == "closed").collect()
}

pub(super) fn mirror_bodies(app: &Platform) {
    app.db.c().execute("UPDATE linked_repos SET mirror_content = ?1 WHERE repo_id = 88", params![true]).unwrap();
}

pub(super) fn pr_id(app: &Platform, number: u64) -> String {
    app.db
        .c()
        .query_row("SELECT id FROM forge_items WHERE repo_id = 88 AND number = ?1", params![number], |r| r.get(0))
        .unwrap()
}

/// `Arugula #N` in the body, no keyword: the PR opens and nothing closes; it
/// merges and the issue is closed as completed by its merger, the event names
/// the PR, and the board's row is Done, proven by CI. Redelivered, and read
/// again by the poller's pass, it adds no second event.
#[tokio::test]
async fn a_merged_pr_that_names_the_issue_closes_it() {
    let (base, app, team, board) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Close me").await;
    let v = add(&base, &board, json!({ "kind": "issue", "id": issue })).await;
    assert_eq!(on(&v, &issue).0, "lane_next");

    let body = format!("Does it.\n\nArugula #{n}");
    let p = pr(70, "Plumbing", &body);
    deliver(&app, "opened", &p, 10, false, BOB).await;
    assert!(closed_events(&app, &issue).is_empty(), "open PRs close nothing");
    assert_eq!(on(&read(&base, "bob", &board).await, &issue).0, "lane_review");

    check(&app, 88, "h70", "build", "completed", Some("success"));
    deliver(&app, "closed", &p, 11, true, BOB).await;
    let events = closed_events(&app, &issue);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!((events[0].actor.as_str(), &events[0].actor_agent), ("bob", &None));
    assert_eq!(events[0].detail, json!({ "reason": "completed", "pr": "acme/site#70" }));
    let (s, got) = call(&base, "bob", "GET", &format!("/api/issues/{issue}"), None).await;
    assert_eq!((s, got["state"].as_str(), got["close_reason"].as_str()), (200, Some("closed"), Some("completed")));
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &issue).0, "lane_done", "{v}");
    assert_eq!(on(&v, &issue).1["proof"], json!({ "by": "ci", "pointer": fi(&pr_id(&app, 70)) }), "{v}");

    // The same delivery again, and the poller's pass over the PR.
    deliver(&app, "closed", &p, 11, true, BOB).await;
    closes::merged(&app, 88, &[70]).await;
    assert_eq!(closed_events(&app, &issue).len(), 1, "no second event");
}

/// The branch, a `(#N)` in the title and a GitHub-style `Closes #N` name too,
/// with no board; a `#N` that is a GitHub issue of the repo is not the native
/// one; a merger who is not in the project leaves the App to close it; an
/// issue closed already stays as it was; the poller's pass closes a PR the
/// webhook never told of.
#[tokio::test]
async fn every_link_names_an_issue_and_the_rest_stays_as_it_was() {
    let (base, app, team, _) = start().await;
    mirror_bodies(&app);
    let mut ns = vec![];
    for t in ["A", "B", "C", "D", "E"] {
        ns.push(native(&base, &team, t).await);
    }
    let (a, b, c, d, e) = (&ns[0], &ns[1], &ns[2], &ns[3], &ns[4]);
    let (s, _) = call(
        &base,
        "bob",
        "PATCH",
        &format!("/api/issues/{}", c.0),
        Some(json!({ "state": "closed", "reason": "not_planned" })),
    )
    .await;
    assert_eq!(s, 200);

    // A's number is a GitHub issue of the repo too (the world's #2), so a
    // bare `#2` is GitHub's: native #2 (B) is left alone by it.
    assert_eq!(b.1, 2);
    let branch = format!("feat/{}-wire", a.1);
    let body = format!("closes #{} and Arugula #{}, (#{})", b.1, c.1, d.1);
    let p = Pr { number: 71, title: "Wire", body: &body, branch: &branch };
    open_and_merge(&app, &p, ("eve-gh", 1005)).await;
    let ea = closed_events(&app, &a.0);
    assert_eq!(ea.len(), 1, "named by its branch");
    assert_eq!(ea[0].actor, closes::APP, "a merger outside the project: the App");
    assert_eq!(ea[0].detail["pr"], "acme/site#71");
    assert!(closed_events(&app, &b.0).is_empty(), "#2 is the repo's GitHub issue");
    let ec = closed_events(&app, &c.0);
    assert_eq!(ec.len(), 1, "its own close only: {ec:?}");
    assert_eq!(ec[0].detail, json!({ "reason": "not_planned" }), "not changed to completed");
    assert_eq!(closed_events(&app, &d.0).len(), 1, "named by (#N) in the body");

    // The poller reads a PR the webhook never told of: its pass closes too.
    let title = format!("Arugula #{}: poll", e.1);
    let v = payload("closed", &pr(75, &title, ""), 10, true, ("ghost", 999_999));
    crate::forge::ingest::webhook(&app, "pull_request", "d", &v, Instant::now());
    assert!(closed_events(&app, &e.0).is_empty());
    closes::merged(&app, 88, &[75]).await;
    let ee = closed_events(&app, &e.0);
    assert_eq!((ee.len(), ee[0].actor.as_str()), (1, closes::APP), "a merger no account has: the App");
}

/// `part of`, `refs` and `see` only link; a PR closed unmerged closes
/// nothing; neither does one naming a number only another project has.
#[tokio::test]
async fn links_that_do_not_close() {
    let (base, app, team, _) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Stays open").await;
    let eve = app.db.personal_project("eve", 1).unwrap();
    let new = |t| crate::db::issues::NewIssue { project: &eve, title: t, body: "", labels: &[], assignees: &[] };
    let by = crate::db::issues::Actor::person("eve");
    app.db.create_issue(&new("Eve's 1"), by, 1).unwrap().unwrap();
    let theirs = app.db.create_issue(&new("Eve's 2"), by, 1).unwrap().unwrap();
    assert_eq!(theirs.number, 2);

    for (number, body, merged) in [
        (72, format!("Part of Arugula #{n}"), true),
        (73, format!("see: arugula #{n}"), true),
        (74, format!("Arugula #{n}"), false),
        (75, "Arugula #2".to_owned(), true),
    ] {
        let p = pr(number, "Links", &body);
        deliver(&app, "opened", &p, 9, false, BOB).await;
        deliver(&app, "closed", &p, 10, merged, BOB).await;
    }
    assert!(closed_events(&app, &issue).is_empty(), "linked, unmerged: open");
    assert!(closed_events(&app, &theirs.id).is_empty(), "another project's issue");
    assert_eq!(app.db.issue(&theirs.id).unwrap().unwrap().state.as_str(), "open");

    // Said plainly as well as softly, it closes.
    let both = format!("Part of Arugula #{n}\n\nArugula #{n}");
    open_and_merge(&app, &pr(76, "Both", &both), BOB).await;
    assert_eq!(closed_events(&app, &issue).len(), 1);
}

/// A PR a person attached to an issue's row as work, naming nothing itself,
/// closes the row's issue when it merges; the issue of a row it isn't
/// attached to is left.
#[tokio::test]
async fn a_pr_attached_by_hand_closes_the_rows_issue() {
    let (base, app, team, board) = start().await;
    let (issue, _) = native(&base, &team, "Row's issue").await;
    let (other, _) = native(&base, &team, "Another row").await;
    let v = add(&base, &board, json!({ "kind": "issue", "id": issue })).await;
    add(&base, &board, json!({ "kind": "issue", "id": other })).await;
    let row = on(&v, &issue).1["id"].as_str().unwrap().to_owned();

    let p = pr(77, "Plumbing", "");
    deliver(&app, "opened", &p, 9, false, BOB).await;
    attach(&base, &board, &row, "work", fi(&pr_id(&app, 77))).await;
    assert!(closed_events(&app, &issue).is_empty());
    deliver(&app, "closed", &p, 10, true, BOB).await;
    assert_eq!(closed_events(&app, &issue).len(), 1, "its issue");
    assert!(closed_events(&app, &other).is_empty(), "the other row's isn't");
}

/// Through GitHub's own door, signed: the merge closes the issue, and the
/// same merge delivered again (as GitHub does) adds no second event.
#[tokio::test]
async fn the_webhook_route_closes_on_merge() {
    let (base, app, team, _) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Over the wire").await;
    let body = format!("Arugula #{n}");
    let p = pr(78, "Wire", &body);
    let at = |action: &str, merged: bool| payload(action, &p, if merged { 10 } else { 5 }, merged, BOB);
    assert_eq!(super::links_tests::hook(&base, "pull_request", "d1", &at("opened", false)).await, 202);
    assert!(closed_events(&app, &issue).is_empty());
    for delivery in ["d2", "d3"] {
        assert_eq!(super::links_tests::hook(&base, "pull_request", delivery, &at("closed", true)).await, 202);
    }
    assert_eq!(closed_events(&app, &issue).len(), 1);
}
