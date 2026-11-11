//! A merged PR closes the native issues it names (Arugula #432), through the
//! mirror's own webhook, in M165's world (`viewer_tests::world`): the same
//! links the boards read (`Arugula #N` in the body, a branch `feat/N-…`)
//! close on merge, as completed, naming the PR and its words, and the board's
//! row is Done, proven by CI, and says which PR closed it, with nobody
//! writing a keyword. `part of Arugula #N` (even from a branch `feat/N-…`),
//! `(#N)` in passing in the body, a PR closed unmerged and an issue of
//! another project close nothing; one closed already stays as it was; a PR a
//! person attached to a row closes that row's issue; a second pass adds no
//! event. A ticket marked Not finished (Arugula #841) isn't closed by a merge,
//! and closes when the mark comes off.

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
    let by = format!("\"Arugula #{n}\" in the body");
    assert_eq!(events[0].detail, json!({ "reason": "completed", "pr": "acme/site#70", "by": by }));
    let (s, got) = call(&base, "bob", "GET", &format!("/api/issues/{issue}"), None).await;
    assert_eq!((s, got["state"].as_str(), got["close_reason"].as_str()), (200, Some("closed"), Some("completed")));
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &issue).0, "lane_done", "{v}");
    assert_eq!(on(&v, &issue).1["proof"], json!({ "by": "ci", "pointer": fi(&pr_id(&app, 70)) }), "{v}");
    let live = &on(&v, &issue).1["live"];
    assert_eq!((&live["closed_with"], &live["closed_by"]), (&json!("acme/site#70"), &json!(by)), "{v}");

    // The same delivery again, and the poller's pass over the PR.
    deliver(&app, "closed", &p, 11, true, BOB).await;
    closes::merged(&app, 88, &[70]).await;
    assert_eq!(closed_events(&app, &issue).len(), 1, "no second event");
}

/// The branch, a `(#N)` in the title and a GitHub-style `Closes #N` name too,
/// with no board, each kept as its words; a `(#N)` in passing in the body
/// only links (Arugula #743); a `#N` that is a GitHub issue of the repo is
/// not the native one; a merger who is not in the project leaves the App to
/// close it; an issue closed already stays as it was; the poller's pass
/// closes a PR the webhook never told of.
#[tokio::test]
async fn every_link_names_an_issue_and_the_rest_stays_as_it_was() {
    let (base, app, team, _) = start().await;
    mirror_bodies(&app);
    let mut ns = vec![];
    for t in ["A", "B", "C", "D", "E", "F", "G"] {
        ns.push(native(&base, &team, t).await);
    }
    let (a, b, c, d, e) = (&ns[0], &ns[1], &ns[2], &ns[3], &ns[4]);
    let (f, g) = (&ns[5], &ns[6]);
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
    let body = format!("closes #{} and Arugula #{}, as the platform had it (#{}). Closes #{}", b.1, c.1, d.1, f.1);
    let title = format!("Wire (#{})", g.1);
    let p = Pr { number: 71, title: &title, body: &body, branch: &branch };
    open_and_merge(&app, &p, ("eve-gh", 1005)).await;
    let ea = closed_events(&app, &a.0);
    assert_eq!(ea.len(), 1, "named by its branch");
    assert_eq!(ea[0].actor, closes::APP, "a merger outside the project: the App");
    assert_eq!(ea[0].detail["pr"], "acme/site#71");
    assert_eq!(ea[0].detail["by"], format!("the branch {branch}"));
    assert!(closed_events(&app, &b.0).is_empty(), "#2 is the repo's GitHub issue");
    let ec = closed_events(&app, &c.0);
    assert_eq!(ec.len(), 1, "its own close only: {ec:?}");
    assert_eq!(ec[0].detail, json!({ "reason": "not_planned" }), "not changed to completed");
    assert!(closed_events(&app, &d.0).is_empty(), "(#N) in passing in the body only links");
    let ef = closed_events(&app, &f.0);
    assert_eq!(ef[0].detail["by"], format!("\"Closes #{}\" in the body", f.1));
    let eg = closed_events(&app, &g.0);
    assert_eq!(eg[0].detail["by"], format!("\"(#{})\" in the title", g.1));

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

    // A branch that names it, with `part of` in the text (Arugula #755).
    let branch = format!("feat/{n}-page-header");
    let part = format!("Part of Arugula #{n}");
    open_and_merge(&app, &Pr { number: 77, title: &part, body: &part, branch: &branch }, BOB).await;
    assert!(closed_events(&app, &issue).is_empty(), "the text's part of wins over the branch");

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

/// Whether the repo's content is kept, and what it holds.
fn kept(app: &Platform) -> (bool, i64) {
    let on = app.db.linked_repo(88).unwrap().unwrap().mirror_content;
    let rows = app.db.c().query_row("SELECT count(*) FROM forge_content", params![], |r| r.get(0)).unwrap();
    (on, rows)
}

/// With the project's content off the mirror keeps no body (#467), so the
/// webhook's own is what names the issue: the title names nothing and the
/// branch is `chore/x`. The body is not kept by the merge. The GitHub issue
/// with the same number (the world's #2) does not stand in the way of
/// `Arugula #2`, and a bare `#2` still closes nothing.
#[tokio::test]
async fn the_webhooks_body_closes_the_issue_with_content_off() {
    let (base, app, team, _) = start().await;
    let (a, b) = (native(&base, &team, "A").await, native(&base, &team, "B").await);
    assert_eq!(b.1, 2, "the number of a GitHub issue the mirror has");
    assert_eq!(kept(&app), (false, 0));

    let body = format!("Does it.\n\nArugula #{}", a.1);
    open_and_merge(&app, &pr(70, "Plumbing", &body), BOB).await;
    assert_eq!(closed_events(&app, &a.0).len(), 1);
    assert_eq!(kept(&app), (false, 0), "the body is used, not kept");

    open_and_merge(&app, &pr(71, "Plumbing", "closes #2"), BOB).await;
    assert!(closed_events(&app, &b.0).is_empty(), "a bare #2 is the GitHub issue's");
    open_and_merge(&app, &pr(72, "Plumbing", "Arugula #2"), BOB).await;
    assert_eq!(closed_events(&app, &b.0).len(), 1, "named as Arugula's, whatever GitHub has at #2");
    assert_eq!(kept(&app), (false, 0));
}

/// The row of `issue` on `board`.
fn row_of(v: &Value, issue: &str) -> String {
    on(v, issue).1["id"].as_str().unwrap().to_owned()
}

/// A hold on an issue (Arugula #841): a merged PR that names it plainly
/// attaches and doesn't close it, its row stays in Review marked Not
/// finished with the reason, and taking the mark off closes it at once,
/// naming the PR that merged.
#[tokio::test]
async fn a_held_issue_is_not_closed_by_a_merge_and_closes_when_released() {
    use arugula_control_wire::boards::board_row_hold;
    let (base, app, team, board) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Two PRs").await;
    let v = add(&base, &board, json!({ "kind": "issue", "id": issue })).await;
    let row = row_of(&v, &issue);
    let path = board_row_hold(&board, &row);

    // Held with nothing merged: its own lane, marked, and not merged.
    let (s, v) = call(&base, "bob", "PUT", &path, Some(json!({ "reason": "  second PR for the API " }))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(on(&v, &issue).0, "lane_next");
    let marks = &on(&v, &issue).1["marks"];
    assert_eq!(marks[0]["kind"], "not_finished", "{marks}");
    assert_eq!(
        (&marks[0]["reason"], &marks[0]["merged"], &marks[0]["by"]),
        (&json!("second PR for the API"), &json!(false), &json!("bob"))
    );
    assert!(marks[0]["at"].is_u64());

    // Merged, naming it plainly.
    let body = format!("Does it.\n\nArugula #{n}");
    open_and_merge(&app, &pr(70, "First", &body), BOB).await;
    assert!(closed_events(&app, &issue).is_empty(), "held: not closed");
    assert_eq!(app.db.issue(&issue).unwrap().unwrap().state.as_str(), "open");
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &issue).0, "lane_review", "{v}");
    assert_eq!(on(&v, &issue).1["marks"][0]["merged"], true, "{v}");
    assert_eq!(on(&v, &issue).1["marks"][0]["reason"], "second PR for the API");
    let attached = on(&v, &issue).1["attachments"].to_string();
    assert!(attached.contains(&pr_id(&app, 70)), "the PR still attaches: {attached}");
    // Redelivered, and the poller's pass: still open.
    closes::merged(&app, 88, &[70]).await;
    assert!(closed_events(&app, &issue).is_empty());

    // Reasons are checked.
    for bad in ["", "   ", "two\nlines", &"x".repeat(201)] {
        let (s, _) = call(&base, "bob", "PUT", &path, Some(json!({ "reason": bad }))).await;
        assert_eq!(s, 400, "{bad:?}");
    }

    // Released: closed now, as completed, naming the PR that merged.
    let (s, v) = call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!(s, 200, "{v}");
    let events = closed_events(&app, &issue);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].detail["pr"], "acme/site#70");
    assert_eq!(events[0].detail["reason"], "completed");
    assert_eq!(events[0].detail["by"], format!("\"Arugula #{n}\" in the body"));
    assert_eq!(on(&v, &issue).0, "lane_done", "{v}");
    assert!(on(&v, &issue).1["marks"].as_array().is_none_or(|m| m.is_empty()), "{v}");
    assert!(app.db.hold_of(&app.db.issue(&issue).unwrap().unwrap().project, &issue).unwrap().is_none());
    // Released again: nothing more.
    let (s, _) = call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!(s, 200);
    assert_eq!(closed_events(&app, &issue).len(), 1);
}

/// Releasing an issue nothing merged for leaves it open; an issue merged
/// with `part of` only is not closed by its release either, and shows Not
/// finished with no reason while no one has marked it.
#[tokio::test]
async fn a_release_with_nothing_merged_closes_nothing() {
    use arugula_control_wire::boards::board_row_hold;
    let (base, app, team, board) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Nothing yet").await;
    let (other, m) = native(&base, &team, "Only linked").await;
    let v = add(&base, &board, json!({ "kind": "issue", "id": issue })).await;
    let row = row_of(&v, &issue);
    let v = add(&base, &board, json!({ "kind": "issue", "id": other })).await;
    let other_row = row_of(&v, &other);

    // `part of`: merged, still open, unmarked: Review, no reason given.
    open_and_merge(&app, &pr(72, "Part", &format!("Part of Arugula #{m}")), BOB).await;
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &other).0, "lane_review", "{v}");
    assert_eq!(on(&v, &other).1["marks"], json!([{ "kind": "not_finished", "merged": true }]), "{v}");

    // Held after that merge, then released: it only linked, so it stays open.
    let path = board_row_hold(&board, &other_row);
    let (s, _) = call(&base, "bob", "PUT", &path, Some(json!({ "reason": "verify on staging" }))).await;
    assert_eq!(s, 200);
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &other).1["marks"][0]["reason"], "verify on staging", "{v}");
    let (s, _) = call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!(s, 200);
    assert!(closed_events(&app, &other).is_empty());

    // Held and released with nothing merged at all.
    let path = board_row_hold(&board, &row);
    call(&base, "bob", "PUT", &path, Some(json!({ "reason": "wait" }))).await;
    call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!(app.db.issue(&issue).unwrap().unwrap().state.as_str(), "open");
    let _ = n;
}

/// A hold set after a merge that closed the issue and then reopened: the
/// release finds the merged PR by its references and closes it again, and
/// a hold on a closed issue is ignored by the board (Done).
#[tokio::test]
async fn a_release_finds_a_merged_pr_by_its_references() {
    use arugula_control_wire::boards::board_row_hold;
    let (base, app, team, board) = start().await;
    mirror_bodies(&app);
    let (issue, n) = native(&base, &team, "Found by text").await;
    let v = add(&base, &board, json!({ "kind": "issue", "id": issue })).await;
    let row = row_of(&v, &issue);
    open_and_merge(&app, &pr(73, "Wire", &format!("Arugula #{n}")), BOB).await;
    assert_eq!(closed_events(&app, &issue).len(), 1);

    // A hold on a closed issue: Done, no mark.
    let path = board_row_hold(&board, &row);
    let (s, v) = call(&base, "bob", "PUT", &path, Some(json!({ "reason": "later" }))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(on(&v, &issue).0, "lane_done", "{v}");
    assert!(on(&v, &issue).1["marks"].as_array().is_none_or(|m| m.is_empty()), "{v}");

    // Reopened, still held: Review, not finished; released: closed again by the PR.
    let (s, _) = call(&base, "bob", "PATCH", &format!("/api/issues/{issue}"), Some(json!({ "state": "open" }))).await;
    assert_eq!(s, 200);
    let v = read(&base, "bob", &board).await;
    assert_eq!(on(&v, &issue).1["marks"][0]["reason"], "later", "{v}");
    let (s, _) = call(&base, "bob", "DELETE", &path, None).await;
    assert_eq!(s, 200);
    let events = closed_events(&app, &issue);
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[1].detail["pr"], "acme/site#73");
}

/// With the project's content off the mirror keeps no body, so a release
/// can't read the PR's words again: the hold remembered the merge it
/// skipped, and the release closes the issue naming that PR and its words.
#[tokio::test]
async fn a_release_closes_by_the_merge_the_hold_skipped_with_content_off() {
    let (base, app, team, _) = start().await;
    let (issue, n) = native(&base, &team, "No bodies").await;
    let project = app.db.issue(&issue).unwrap().unwrap().project;
    app.db.hold_set(&project, &issue, "second PR", crate::db::issues::Actor::person("bob"), 5).unwrap();

    open_and_merge(&app, &pr(79, "Plumbing", &format!("Arugula #{n}")), BOB).await;
    assert!(closed_events(&app, &issue).is_empty(), "held");
    let skipped = app.db.hold_of(&project, &issue).unwrap().unwrap().skipped.unwrap();
    assert_eq!((skipped.reference.as_str(), skipped.merged_by), ("acme/site#79", Some(1002)));

    closes::released(&app, &project, &issue, Some(skipped)).await;
    let events = closed_events(&app, &issue);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].detail["pr"], "acme/site#79");
    assert_eq!(events[0].detail["by"], format!("\"Arugula #{n}\" in the body"));
    assert_eq!(events[0].actor, "bob", "closed as the person who merged it");
}
