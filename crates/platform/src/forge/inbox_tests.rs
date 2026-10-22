//! The inbox (M171) on the viewer tests' world (`viewer_tests::world`),
//! against a fake GitHub: each reason from seeded rows, a private item
//! never in an inbox or a push for a member GitHub doesn't let read it,
//! one push per reason, seen and done, and the shadow comparison. Runs on
//! SQLite and, with `ARUGULA_TEST_POSTGRES_URL` (`just
//! test-platform-postgres`), Postgres.

use std::sync::Arc;

use arugula_e2e::{now_ms, team::TeamRole};
use serde_json::{Value, json};

use super::{
    inbox::{self, Want},
    links_tests::{Shared, serve},
    viewer::Viewer,
    viewer_tests::world,
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Sql as _,
        forge::AccessScope,
        sql::{Param, params},
    },
    forge::GithubUser,
};

fn exec(app: &Platform, sql: &str, p: &[&dyn Param]) {
    app.db.c().execute(sql, p).unwrap();
}

/// Every entry, as `account item reason`, sorted.
fn rows(app: &Platform) -> Vec<String> {
    app.db
        .c()
        .query_map("SELECT account, item, reason FROM inbox_items ORDER BY 1, 2, 3", params![], |r| {
            Ok(format!("{} {} {}", r.get::<String>(0)?, r.get::<String>(1)?, r.get::<String>(2)?))
        })
        .unwrap()
}

/// The world, with dan in the team too: signed in with GitHub (1004), but
/// with no access to the private acme/app.
async fn setup() -> (Arc<TestHub>, Shared, Arc<Platform>, String) {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.github("dan", 1004, "dan-gh");
    hub.team(
        "t1",
        &[
            ("alice", TeamRole::Owner),
            ("bob", TeamRole::Editor),
            ("carol", TeamRole::Viewer),
            ("dan", TeamRole::Editor),
        ],
    );
    app.cache.drop_all();
    (hub, gh, app, base)
}

/// What an item wants of someone, by the platform's own read.
fn wants(app: &Platform, repo: u64, number: u64, account: &str, login: Option<&str>) -> Vec<Want> {
    let s = app.db.inbox_sources(repo, number).unwrap().unwrap();
    let github = login.map(|l| GithubUser { id: 1, login: l.into() });
    inbox::wants(&s, &Viewer { account: account.into(), github })
}

fn reasons(w: &[Want]) -> Vec<(&str, &str)> {
    w.iter().map(|w| (w.reason, w.why.as_str())).collect()
}

/// Each of the block's reasons, from rows as ingest writes them, in its
/// words; and an entry for each, only for members who may read the item.
#[tokio::test]
async fn each_reason_from_the_rows() {
    let (_hub, _gh, app, _) = setup().await;
    // acme/app#1, alice's PR: bob asked to review; he approved before.
    assert_eq!(
        reasons(&wants(&app, 77, 1, "bob", Some("bob-gh"))),
        [("review_requested", "review requested from you")]
    );
    assert!(wants(&app, 77, 1, "alice", Some("alice-gh")).is_empty(), "nothing counted on its checks yet");
    // Its checks fail.
    exec(&app, "UPDATE forge_checks SET conclusion = 'failure' WHERE repo_id = 77", params![]);
    assert_eq!(reasons(&wants(&app, 77, 1, "alice", Some("alice-gh"))), [("checks_failed", "1 check failed: ci")]);
    // A commit status passing doesn't change that; a second failing does.
    exec(
        &app,
        "INSERT INTO forge_checks (repo_id, head_sha, kind, name, status, conclusion, updated_at)
         VALUES (77, 'abc', 'status', 'lint', 'completed', 'error', 2), (77, 'abc', 'status', 'docs', 'completed', 'success', 2)",
        params![],
    );
    assert_eq!(
        reasons(&wants(&app, 77, 1, "alice", Some("alice-gh"))),
        [("checks_failed", "2 checks failed: ci, lint")]
    );
    // bob asks for changes, after approving.
    exec(
        &app,
        "INSERT INTO forge_events (id, repo_id, number, kind, actor_login, review_state, at)
         VALUES ('fev_3', 77, 1, 'review', 'bob-gh', 'changes_requested', 9)",
        params![],
    );
    assert_eq!(
        reasons(&wants(&app, 77, 1, "alice", Some("alice-gh"))),
        [("checks_failed", "2 checks failed: ci, lint"), ("changes_requested", "changes requested by bob-gh")]
    );
    // Green, his newest review approves, and GitHub says it can merge: done.
    exec(&app, "UPDATE forge_checks SET conclusion = 'success' WHERE repo_id = 77", params![]);
    exec(&app, "UPDATE forge_items SET merge_state = 'clean' WHERE id = 'fi_a1'", params![]);
    exec(&app, "UPDATE forge_events SET review_state = 'approved' WHERE id = 'fev_3'", params![]);
    assert_eq!(reasons(&wants(&app, 77, 1, "alice", Some("alice-gh"))), [("done", "approved, checks green")]);
    // Held by branch protection: not done.
    exec(&app, "UPDATE forge_items SET merge_state = 'blocked' WHERE id = 'fi_a1'", params![]);
    assert!(wants(&app, 77, 1, "alice", Some("alice-gh")).is_empty());
    // acme/site#3, alice's, merged.
    assert_eq!(reasons(&wants(&app, 88, 3, "alice", Some("alice-gh"))), [("done", "merged")]);
    // acme/site#2, bob's issue, given to alice: assigned. A mention needs
    // the text, which only a project that mirrors content has.
    exec(&app, "UPDATE forge_items SET assignees = '[\"alice-gh\"]' WHERE id = 'fi_a2'", params![]);
    assert_eq!(reasons(&wants(&app, 88, 2, "alice", Some("alice-gh"))), [("assigned", "assigned to you")]);
    assert!(wants(&app, 88, 2, "dan", Some("dan-gh")).is_empty(), "no content: no mentions");
    exec(
        &app,
        "INSERT INTO forge_content (repo_id, number, kind, github_id, author_login, body, updated_at)
         VALUES (88, 2, 'comment', 501, 'bob-gh', 'what do you think, @dan-gh?', 40)",
        params![],
    );
    assert_eq!(reasons(&wants(&app, 88, 2, "dan", Some("dan-gh"))), [("mentioned", "mentioned by bob-gh")]);
    // A draft waiting for carol, who has no GitHub sign-in: that's all
    // she can have.
    exec(
        &app,
        "INSERT INTO drafts (id, project_id, author, agent_name, repo_id, number, kind, body, created_at, updated_at)
         SELECT 'dft_c', project_id, 'carol', 'mcp:claude-code', 88, 2, 'comment', 'lgtm', 1, 1
         FROM linked_repos WHERE repo_id = 88",
        params![],
    );
    assert_eq!(reasons(&wants(&app, 88, 2, "carol", None)), [("draft", "a draft waiting for you")]);

    // As entries: every member who may read the repo, nobody else.
    exec(
        &app,
        "UPDATE forge_items SET merge_state = 'clean', reviewers = '[\"bob-gh\", \"dan-gh\"]' WHERE id = 'fi_a1'",
        params![],
    );
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    inbox::recompute(&app, 88, &[2, 3], false).await.unwrap();
    assert_eq!(
        rows(&app),
        [
            "alice fi_a1 done",
            "alice fi_a2 assigned",
            "alice fi_a3 done",
            "bob fi_a1 review_requested",
            "carol fi_a2 draft",
            "dan fi_a2 mentioned",
        ],
        "dan is asked to review the private #1 too, and may not read it"
    );
}

/// A green PR (bob approved alice's #1) is done only once the mirror knows it can
/// merge: no merge state (a list's row), `unknown` (GitHub hadn't worked
/// it out), a conflict and a hold aren't; a known state that's neither is.
/// Merged is done whatever it last said.
#[tokio::test]
async fn green_is_done_only_once_it_can_merge() {
    let (_hub, _gh, app, _) = setup().await;
    exec(&app, "UPDATE forge_checks SET conclusion = 'success' WHERE repo_id = 77", params![]);
    let alice = |app: &Platform| {
        reasons(&wants(app, 77, 1, "alice", Some("alice-gh"))).iter().map(|r| r.1.to_owned()).collect::<Vec<_>>()
    };
    let set = |state: Option<&str>| {
        exec(&app, "UPDATE forge_items SET merge_state = ?1 WHERE id = 'fi_a1'", params![state]);
    };
    for state in [None, Some("unknown"), Some("dirty"), Some("blocked")] {
        set(state);
        assert!(alice(&app).is_empty(), "{state:?}: {:?}", alice(&app));
    }
    for state in ["clean", "unstable", "has_hooks", "behind"] {
        set(Some(state));
        assert_eq!(alice(&app), ["approved, checks green"], "{state}");
    }
    set(Some("dirty"));
    exec(&app, "UPDATE forge_items SET state = 'merged' WHERE id = 'fi_a1'", params![]);
    assert_eq!(alice(&app), ["merged"]);
}

/// The rows ingest writes are marked, and worked out by the machine that
/// wrote them; a backfill's quietly.
#[tokio::test]
async fn marked_rows_are_worked_out() {
    let (hub, _gh, app, _) = setup().await;
    exec(&app, "UPDATE forge_items SET updated_at = ?1", params![now_ms()]);
    inbox::polled(&app, 77, true);
    assert_eq!(inbox::drain(&app).await, 1);
    assert_eq!(rows(&app), ["bob fi_a1 review_requested"]);
    assert!(hub.pushes().is_empty(), "a backfill pushes nothing");
    // A check run fails on its head: the PR it's the head of.
    let (_, r) = super::ingest::rows(
        "check_run",
        &json!({ "action": "completed", "repository": { "id": 77, "full_name": "acme/app" },
            "check_run": { "name": "ci", "head_sha": "abc", "status": "completed", "conclusion": "failure",
                "pull_requests": [{ "number": 1 }], "completed_at": "2026-10-02T00:00:00Z" } }),
    )
    .unwrap();
    app.db.mirror(77, None, |tx| super::ingest::write(tx, &r, now_ms())).unwrap();
    inbox::written(&app, 77, &r);
    assert_eq!(inbox::drain(&app).await, 1);
    assert_eq!(rows(&app), ["alice fi_a1 checks_failed", "bob fi_a1 review_requested"]);
    let pushes = hub.pushes();
    assert_eq!(pushes.len(), 1, "{pushes:?}");
    let (to, msg, _) = &pushes[0];
    assert_eq!(to, "alice");
    assert_eq!(msg["title"], "acme/app#1 1 check failed: ci");
    assert_eq!((msg["body"].as_str(), msg["inbox"]["item"].as_str()), (Some("secret plan"), Some("fi_a1")));
    assert_eq!(msg["url"], "https://github.com/acme/app/pull/1");
}

/// A private item is in the inbox, and pushed, only for those GitHub lets
/// read it: never dan's, asked to review it; and never bob's once he's
/// taken off the repo, nor a row someone else put there.
#[tokio::test]
async fn private_items_only_for_readers() {
    let (hub, gh, app, base) = setup().await;
    exec(&app, "UPDATE forge_items SET reviewers = '[\"bob-gh\", \"dan-gh\"]', updated_at = ?1", params![now_ms()]);
    // A stale row of dan's goes.
    exec(
        &app,
        "INSERT INTO inbox_items (account, item, reason, created_at, updated_at) VALUES ('dan', 'fi_a1', 'mentioned', 1, 1)",
        params![],
    );
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    assert_eq!(rows(&app), ["bob fi_a1 review_requested"]);
    let to: Vec<String> = hub.pushes().into_iter().map(|(a, ..)| a).collect();
    assert_eq!(to, ["bob"]);
    let (s, v) = call(&base, "bob", "GET", "/api/inbox", None).await;
    assert_eq!((s, v["entries"][0]["title"].as_str()), (200, Some("secret plan")), "{v}");

    // Put there anyway, it isn't read back to dan.
    exec(
        &app,
        "INSERT INTO inbox_items (account, item, reason, created_at, updated_at) VALUES ('dan', 'fi_a1', 'review_requested', 1, 1)",
        params![],
    );
    let (s, v) = call(&base, "dan", "GET", "/api/inbox", None).await;
    assert_eq!((s, v["entries"].as_array().map(Vec::len)), (200, Some(0)), "{v}");
    assert!(!v.to_string().contains("secret"));
    let (s, _) = call(&base, "dan", "POST", "/api/inbox/fi_a1", Some(json!({ "seen": true }))).await;
    assert_eq!(s, 404);

    // bob is taken off acme/app: GitHub's event drops what was kept.
    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    app.db.drop_access(AccessScope::Person, "1002").unwrap();
    exec(&app, "UPDATE forge_items SET updated_at = ?1", params![now_ms() + 1]);
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    assert!(rows(&app).is_empty(), "{:?}", rows(&app));
    let (_, v) = call(&base, "bob", "GET", "/api/inbox", None).await;
    assert_eq!(v["entries"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(hub.pushes().len(), 1, "no push for what he can't read");
}

/// A reason pushes once while it holds, however often it's worked out;
/// gone and back, it's new again.
#[tokio::test]
async fn one_push_per_reason() {
    let (hub, _gh, app, _) = setup().await;
    exec(&app, "UPDATE forge_items SET updated_at = ?1", params![now_ms()]);
    for _ in 0..3 {
        inbox::recompute(&app, 77, &[1], false).await.unwrap();
    }
    assert_eq!(hub.pushes().len(), 1);
    // The request taken back, the entry goes; asked again, it's back, and
    // (a minute on) pushed again.
    exec(&app, "UPDATE forge_items SET reviewers = '[]' WHERE id = 'fi_a1'", params![]);
    assert_eq!(inbox::recompute(&app, 77, &[1], false).await.unwrap(), 0);
    assert!(rows(&app).is_empty());
    exec(&app, "UPDATE forge_items SET reviewers = '[\"bob-gh\"]' WHERE id = 'fi_a1'", params![]);
    assert_eq!(inbox::recompute(&app, 77, &[1], false).await.unwrap(), 1);
    assert_eq!(hub.pushes().len(), 1, "within a minute of the last: the entry, no push");
    // Two reasons at once are one push: alice asked to review her own
    // PR, whose checks failed.
    app.inbox.forget_pushes();
    exec(&app, "UPDATE forge_checks SET conclusion = 'failure' WHERE repo_id = 77", params![]);
    exec(&app, "UPDATE forge_items SET reviewers = '[\"alice-gh\"]' WHERE id = 'fi_a1'", params![]);
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    let pushes = hub.pushes();
    assert_eq!(pushes.len(), 2, "{pushes:?}");
    assert_eq!(pushes[1].0, "alice");
    assert_eq!(pushes[1].1["title"], "acme/app#1 review requested from you; 1 check failed: ci");
    // An old failure, found by a sweep, lands without a push.
    exec(&app, "UPDATE forge_items SET updated_at = 30, reviewers = '[]' WHERE id = 'fi_a1'", params![]);
    exec(&app, "DELETE FROM inbox_items", params![]);
    app.inbox.forget_pushes();
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    assert_eq!(rows(&app), ["alice fi_a1 checks_failed"]);
    assert_eq!(hub.pushes().len(), 2);
}

/// Seen and done are the reader's, kept while the reason holds; a done
/// entry leaves the list unless asked for.
#[tokio::test]
async fn seen_and_done() {
    let (_hub, _gh, app, base) = setup().await;
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    let mark = |body: Value| call(&base, "bob", "POST", "/api/inbox/fi_a1", Some(body));
    let (s, v) = mark(json!({ "seen": true })).await;
    assert_eq!((s, v["marked"].as_u64()), (200, Some(1)), "{v}");
    let (_, v) = call(&base, "bob", "GET", "/api/inbox", None).await;
    let e = &v["entries"][0];
    assert_eq!(
        (e["reason"].as_str(), e["seen_at"].is_u64(), e["done_at"].is_null()),
        (Some("review_requested"), true, true)
    );
    assert_eq!(e["url"], "https://github.com/acme/app/pull/1");
    let (s, v) = mark(json!({ "reason": "draft", "done": true })).await;
    assert_eq!((s, v["marked"].as_u64()), (200, Some(0)), "no draft entry");
    let (s, _) = mark(json!({ "reason": "bogus", "done": true })).await;
    assert_eq!(s, 400);
    mark(json!({ "reason": "review_requested", "done": true })).await;
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    let (_, v) = call(&base, "bob", "GET", "/api/inbox", None).await;
    assert_eq!(v["entries"].as_array().map(Vec::len), Some(0), "{v}");
    let (_, v) = call(&base, "bob", "GET", "/api/inbox?done=true", None).await;
    assert!(v["entries"][0]["done_at"].is_u64(), "{v}");
    mark(json!({ "done": false, "seen": false })).await;
    let (_, v) = call(&base, "bob", "GET", "/api/inbox", None).await;
    assert!(v["entries"][0]["seen_at"].is_null(), "{v}");
}

/// A machine's blocks against the platform's inbox for its owner: what
/// agrees, what doesn't, and what isn't compared.
#[tokio::test]
async fn the_shadow_comparison() {
    let (_hub, _gh, app, base) = setup().await;
    exec(
        &app,
        "INSERT INTO drafts (id, project_id, author, agent_name, repo_id, number, kind, body, created_at, updated_at)
         SELECT 'dft_b', project_id, 'bob', 'mcp:claude-code', 77, 1, 'comment', 'lgtm', 1, 1
         FROM linked_repos WHERE repo_id = 77",
        params![],
    );
    inbox::recompute(&app, 77, &[1], false).await.unwrap();
    inbox::recompute(&app, 88, &[2], false).await.unwrap();
    let report = json!({ "blocks": [
        // Agrees, its draft and all.
        { "repo": "acme/app", "number": 1, "reasons": ["review_requested"], "drafts": ["dft_b"] },
        // The block found a mention the platform has no text for (acme/site
        // doesn't mirror content): explained, not compared.
        { "repo": "Acme/Site", "number": 2, "reasons": ["mentioned"] },
        // The block doesn't show the draft: disagrees.
        { "repo": "acme/app", "number": 1, "reasons": ["review_requested"] },
        // Not compared: eve's repo, an item the mirror doesn't have, a repo nobody linked.
        { "repo": "eve-gh/x", "number": 3, "reasons": [] },
        { "repo": "acme/app", "number": 404, "reasons": ["done"] },
        { "repo": "other/repo", "number": 1, "reasons": [] },
    ] });
    let r = reqwest::Client::new()
        .post(format!("{base}/api/inbox/shadow"))
        .header("x-test-daemon", "bob m1")
        .json(&report)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v, json!({ "compared": 2, "agree": 1, "disagree": 1, "explained": 1 }));
    // A person's session isn't a machine.
    let (s, _) = call(&base, "bob", "POST", "/api/inbox/shadow", Some(report)).await;
    assert_eq!(s, 401);
}

/// Production's first shadow report (2026-10-22): two issues Jake opened
/// and gave himself (and others), on a project that doesn't mirror
/// content. The block said nothing for one and `mentioned` for the other;
/// the platform said `assigned` for both, since the mirror doesn't keep
/// who assigned an issue. One you opened, you took: not `assigned`. The
/// mention needs the text.
#[tokio::test]
async fn your_own_issue_given_to_you() {
    let (_hub, _gh, app, base) = setup().await;
    exec(
        &app,
        "UPDATE forge_items SET author_login = 'alice-gh', assignees = '[\"alice-gh\", \"bob-gh\"]' WHERE id = 'fi_a2'",
        params![],
    );
    assert!(wants(&app, 88, 2, "alice", Some("alice-gh")).is_empty(), "she took it herself");
    assert!(wants(&app, 88, 2, "alice", Some("Alice-GH")).is_empty(), "logins ignore case");
    // bob didn't open it: someone gave it to him.
    assert_eq!(reasons(&wants(&app, 88, 2, "bob", Some("bob-gh"))), [("assigned", "assigned to you")]);
    inbox::recompute(&app, 88, &[2], false).await.unwrap();
    assert_eq!(rows(&app), ["bob fi_a2 assigned"]);
    let report = |reasons: Value| {
        let base = base.clone();
        async move {
            let body = json!({ "blocks": [{ "repo": "acme/site", "number": 2, "reasons": reasons }] });
            let r = reqwest::Client::new()
                .post(format!("{base}/api/inbox/shadow"))
                .header("x-test-daemon", "alice m1")
                .json(&body)
                .send()
                .await
                .unwrap();
            r.json::<Value>().await.unwrap()
        }
    };
    // #415: nothing on either side.
    assert_eq!(report(json!([])).await, json!({ "compared": 1, "agree": 1, "disagree": 0, "explained": 0 }));
    // #213: bob's comment mentions her. Without the text the platform
    // can't see it: explained, and left out of the score.
    assert_eq!(report(json!(["mentioned"])).await, json!({ "compared": 0, "agree": 0, "disagree": 0, "explained": 1 }));
    // The repo mirrors content, but the platform didn't find the mention:
    // that's a disagreement.
    exec(&app, "UPDATE linked_repos SET mirror_content = TRUE WHERE repo_id = 88", params![]);
    assert_eq!(report(json!(["mentioned"])).await, json!({ "compared": 1, "agree": 0, "disagree": 1, "explained": 0 }));
    exec(
        &app,
        "INSERT INTO forge_content (repo_id, number, kind, github_id, author_login, body, updated_at)
         VALUES (88, 2, 'comment', 502, 'bob-gh', '@alice-gh: lane C is merged', 50)",
        params![],
    );
    assert_eq!(reasons(&wants(&app, 88, 2, "alice", Some("alice-gh"))), [("mentioned", "mentioned by bob-gh")]);
    inbox::recompute(&app, 88, &[2], false).await.unwrap();
    assert_eq!(report(json!(["mentioned"])).await, json!({ "compared": 1, "agree": 1, "disagree": 0, "explained": 0 }));
}
