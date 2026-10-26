//! M265 (thread = session, Arugula #129) through the platform's own
//! routes: a thread is done when its PR merges, Agent on this on a native
//! issue, and a project's numbers.

use serde_json::{Value, json};

use crate::{
    chat_tests::call,
    db::{
        issues::{Actor, NewIssue},
        jobs::{JobEnd, JobKind},
        session_merged::MERGED,
        sessions::State,
        sql::{Sql as _, params},
    },
    forge::ingest,
    jobs::{Clock, Handler as _, Step},
    sessions_hydrate_tests::{box_says, platform, post_in, sessions, take},
};

/// Link `o/app` (repo 7) to `project`.
fn link(app: &crate::Platform, project: &str) {
    app.db
        .c()
        .execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at)
             VALUES (7, 'o/app', FALSE, ?1, 1, 1)",
            params![project],
        )
        .unwrap();
}

/// GitHub's `pull_request` webhook for PR `number` from `head` on `o/app`:
/// opened at `opened`, and merged when `merged`.
fn pr_webhook(number: u64, head: &str, merged: bool) -> Value {
    json!({
        "action": if merged { "closed" } else { "opened" },
        "repository": { "id": 7, "full_name": "o/app" },
        "sender": { "login": "ann-gh", "id": 2 },
        "pull_request": {
            "number": number, "title": "Fix it", "state": if merged { "closed" } else { "open" },
            "merged": merged, "merged_at": if merged { json!("2026-10-03T00:00:00Z") } else { Value::Null },
            "draft": false, "user": { "login": "ann-gh", "id": 2 },
            "head": { "ref": head, "sha": "abc" }, "base": { "ref": "main", "sha": "def" },
            "created_at": "2026-10-01T00:00:00Z",
            "updated_at": if merged { "2026-10-03T00:00:00Z" } else { "2026-10-01T00:00:00Z" },
        }
    })
}

/// The mirror takes the webhook, as `forge::ingest::webhook` does after
/// its rows are written.
fn mirror(app: &crate::Platform, v: &Value) {
    let (repo, rows) = ingest::rows("pull_request", v).unwrap();
    app.db.mirror(repo, None, |tx| ingest::write(tx, &rows, arugula_e2e::now_ms())).unwrap().unwrap();
    crate::sessions::merged::seen(app, repo, &rows.items.iter().collect::<Vec<_>>());
}

/// The parents of the threads `who` sees in the Threads view.
async fn threads(base: &str, who: &str) -> Vec<i64> {
    let (st, v) = call(base, who, "GET", "/api/chat/threads", None).await;
    assert_eq!(st, 200, "{v}");
    v["threads"].as_array().unwrap().iter().map(|t| t["parent"]["seq"].as_i64().unwrap()).collect()
}

/// A done session's PR merges, opened from its branch by hand: the
/// session stops saying it merged, which frees its slot, its card says so
/// and links the PR, and its thread leaves the Threads view until
/// `@arugula` there again starts a new one. A PR from another branch, or
/// on a repo the project hasn't linked, does nothing.
#[tokio::test]
async fn a_merged_pr_ends_its_session_and_its_thread_is_done() {
    let (_hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    link(&app, &t);
    let p = post_in(&base, "ch_general", "a", "the login is slow", None).await["seq"].as_i64().unwrap();
    post_in(&base, "ch_general", "b", "@arugula speed up the login", Some(p)).await;
    let s = sessions(&app, &t).pop().expect("a session");
    take(&app, &keys, &s.id);
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");
    assert_eq!(threads(&base, "b").await, [p], "b follows the thread they asked in");

    // Another branch's PR, and the session's branch's PR still open.
    mirror(&app, &pr_webhook(11, "arugula/0000000-other", true));
    mirror(&app, &pr_webhook(12, &s.branch, false));
    assert_eq!(app.db.session(&s.id).unwrap().unwrap().merged_at, None);

    mirror(&app, &pr_webhook(12, &s.branch, true));
    let after = app.db.session(&s.id).unwrap().unwrap();
    assert_eq!(after.state, State::Stopped, "{after:?}");
    assert!(after.merged_at.is_some());
    assert_eq!(after.pr.as_deref(), Some("https://github.com/o/app/pull/12"), "the PR opened by hand is linked");
    let last = app.db.session_events(&s.id).unwrap().pop().unwrap();
    let detail: Value = serde_json::from_str(&last.detail).unwrap();
    assert_eq!((detail["reason"].as_str(), detail["pr"].as_u64()), (Some(MERGED), Some(12)));
    assert!(app.db.session_work("hw_dev").unwrap().is_empty(), "its slot on the box frees");
    let (st, v) = call(&base, "x", "GET", &format!("/api/sessions/{}", s.id), None).await;
    assert_eq!(st, 200, "{v}");
    assert!(v["card"]["merged_at"].is_u64(), "{v}");
    assert_eq!(v["events"].as_array().unwrap().last().unwrap()["text"], "Its pull request merged: done");
    assert!(threads(&base, "b").await.is_empty(), "a done thread leaves the Threads view");

    // Seen again (a redelivery, the poller): nothing moves.
    mirror(&app, &pr_webhook(12, &s.branch, true));
    assert_eq!(app.db.session(&s.id).unwrap().unwrap(), after);

    // @arugula again in the thread: a new session, and the thread is going.
    post_in(&base, "ch_general", "b", "@arugula now the signup", Some(p)).await;
    assert_eq!(sessions(&app, &t).len(), 2);
    assert_eq!(threads(&base, "b").await, [p]);
}

/// A PR that merges while a turn is under way: the turn finishes, and the
/// session's job stops it then.
#[tokio::test]
async fn a_turn_under_way_finishes_before_the_merge_ends_it() {
    let (_hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    link(&app, &t);
    let follow = crate::sessions::job::Follow { app: app.clone() };
    post_in(&base, "ch_general", "b", "@arugula tidy the logs", None).await;
    let s = sessions(&app, &t).pop().unwrap();
    take(&app, &keys, &s.id);
    mirror(&app, &pr_webhook(12, &s.branch, true));
    let s = app.db.session(&s.id).unwrap().unwrap();
    assert_eq!(s.state, State::Working, "the turn goes on");
    assert!(s.merged_at.is_some());
    box_says(&app, &keys, &s.id, 2, State::Working, State::Done, "{}");
    let now = arugula_e2e::now_ms();
    let mut job = app.db.claim_job(&[JobKind::Session], now).unwrap().expect("its job");
    assert_eq!(follow.step(&app.db, &mut job, &Clock::new(move || now)).await.unwrap(), Step::Done);
    app.db.finish_job(&job, JobEnd::Done, None, now).unwrap();
    let s = app.db.session(&s.id).unwrap().unwrap();
    assert_eq!(s.state, State::Stopped);
    let detail: Value = serde_json::from_str(&app.db.session_events(&s.id).unwrap().pop().unwrap().detail).unwrap();
    assert_eq!(detail["reason"], MERGED);
}

/// Agent on this writes `@arugula` in the issue's thread as its presser:
/// a session there, named for the issue, whose first prompt has the
/// issue's title and text. While it's going, another is refused; a viewer
/// is told why, and someone outside the project finds no issue.
#[tokio::test]
async fn agent_on_this_is_an_arugula_in_the_issues_thread() {
    let (_hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app"]).await;
    let n =
        NewIssue { project: &t, title: "Login's slow on Safari", body: "It takes 9 s.", labels: &[], assignees: &[] };
    let issue = app.db.create_issue(&n, Actor::person("a"), 1).unwrap().unwrap();
    let path = format!("/api/issues/{}/agent", issue.id);

    let (st, v) = call(&base, "x", "POST", &path, None).await;
    assert_eq!(st, 403, "a viewer: {v}");
    assert!(v["error"].as_str().unwrap().contains("can only view it"), "{v}");
    let (st, _) = call(&base, "c", "POST", &path, None).await;
    assert_eq!(st, 404, "outside the project");

    let (st, v) = call(&base, "b", "POST", &path, None).await;
    assert_eq!(st, 200, "{v}");
    let s = sessions(&app, &t).pop().expect("a session");
    assert_eq!(v["session"].as_str(), Some(s.id.as_str()));
    assert_eq!((s.room.as_str(), s.parent_seq, s.asker.as_str()), (v["room"].as_str().unwrap(), 0, "b"));
    assert!(s.branch.ends_with(&format!("-i{}-logins-slow-on-safari", issue.number)), "{}", s.branch);
    let prompt = &app.db.session_prompts(&s.id).unwrap()[0].text;
    assert!(prompt.contains(&format!("issue #{} Login's slow on Safari (open)", issue.number)), "{prompt}");
    assert!(prompt.contains("It takes 9 s."), "{prompt}");
    assert!(prompt.contains("b (editor): @arugula work on this issue\n"), "{prompt}");

    let (st, v) = call(&base, "a", "POST", &path, None).await;
    assert_eq!(st, 409, "one at a time: {v}");
    assert_eq!(sessions(&app, &t).len(), 1);
}

/// A project's numbers over the last 30 days: its sessions, the PRs they
/// led to (sent by the asker or opened from the branch), the merged, and
/// the median from the `@` to the PR. For its members only.
#[tokio::test]
async fn a_projects_session_numbers() {
    let (_hub, app, base, t, _u, keys) = platform(&["https://github.com/o/app"]).await;
    link(&app, &t);
    for ask in ["@arugula one", "@arugula two", "@arugula three"] {
        post_in(&base, "ch_general", "b", ask, None).await;
    }
    let all = sessions(&app, &t);
    let (one, two) = (&all[2], &all[1]);
    take(&app, &keys, &one.id);
    // One's PR opened from its branch and merged; two's opened only.
    mirror(&app, &pr_webhook(12, &one.branch, true));
    mirror(&app, &pr_webhook(13, &two.branch, false));
    // Asked 10 and 20 minutes before their PRs opened, a day ago; three
    // asked 40 days ago, so it isn't counted.
    let opened = arugula_e2e::now_ms() - 24 * 60 * 60_000;
    let c = app.db.c();
    c.execute("UPDATE forge_items SET created_at = ?1 WHERE repo_id = 7", params![opened]).unwrap();
    for (s, before) in [(one, 10), (two, 20), (&all[0], 40 * 24 * 60)] {
        c.execute("UPDATE agent_sessions SET created_at = ?2 WHERE id = ?1", params![s.id, opened - before * 60_000])
            .unwrap();
    }
    drop(c);
    let path = format!("/api/projects/{t}/sessions/stats");
    let (st, v) = call(&base, "x", "GET", &path, None).await;
    assert_eq!(st, 200, "a viewer reads them: {v}");
    assert_eq!(
        v,
        json!({ "days": 30, "sessions": 2, "prs_opened": 2, "prs_merged": 1, "median_to_pr_ms": 15 * 60_000 })
    );
    let (st, _) = call(&base, "c", "GET", &path, None).await;
    assert_eq!(st, 404, "outside the project");
}
