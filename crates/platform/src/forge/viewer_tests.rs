//! The viewer check (M165) through the platform's own read routes, against
//! a fake GitHub: the leak tests (`leak_*`), and what a list's filters do.
//! Rows are seeded as M164's ingest would write them. Runs on SQLite and,
//! with `ARUGULA_TEST_POSTGRES_URL` (`just test-platform-postgres`),
//! Postgres.

use std::sync::Arc;

use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::{
    GithubApp, GithubUser,
    links_tests::{SECRET, Shared, fake_github, hook, serve, throwaway_key},
    viewer::{self, KEEP_MS},
};
use crate::{
    Platform,
    chat_tests::{TestHub, call},
    db::{
        Db, Sql as _,
        forge::{AccessScope, ItemFilter},
        sql::params,
    },
};

/// The world the tests read:
/// - team t1's project: `acme/app` (77, private) and `acme/site` (88,
///   public), the App installed on acme (installation 9); alice (owner,
///   GitHub 1001, admin on acme/app), bob (editor, GitHub 1002, write on
///   it) and carol (viewer, not signed in with GitHub);
/// - eve's own project: `eve-gh/x` (99, private), the App installed on
///   her account, so she has access; alice is an admin of it on GitHub
///   too, but not in eve's project.
///
/// Items: `fi_a1` "secret plan" (a PR on 77, bob asked to review),
/// `fi_a2` "public fix" (an issue on 88), `fi_b1` "eve's secret" (a PR on
/// 99). The team's project id, and eve's.
pub(super) async fn world(hub: &TestHub, gh: &Shared, app: &Platform) -> (String, String) {
    hub.github("alice", 1001, "alice-gh");
    hub.github("bob", 1002, "bob-gh");
    hub.github("eve", 1005, "eve-gh");
    hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor), ("carol", TeamRole::Viewer)]);
    {
        let mut g = gh.lock().unwrap();
        g.installed.insert("acme/app".into(), (9, 500, "acme".into()));
        g.installed.insert("eve-gh/x".into(), (10, 1005, "eve-gh".into()));
        g.roles.insert(("acme/app".into(), "alice-gh".into()), ("admin".into(), 1001));
        g.roles.insert(("acme/app".into(), "bob-gh".into()), ("write".into(), 1002));
        g.roles.insert(("eve-gh/x".into(), "alice-gh".into()), ("admin".into(), 1001));
    }
    let team = crate::projects::team_project(app, "t1").await.unwrap().unwrap();
    let eve = app.db.personal_project("eve", 1).unwrap();
    let c = app.db.c();
    for (id, name, private, project, install) in
        [(77u64, "acme/app", true, &team, 9u64), (88, "acme/site", false, &team, 9), (99, "eve-gh/x", true, &eve, 10)]
    {
        c.execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_by, linked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'alice', 1)",
            params![id, name, private, project, install],
        )
        .unwrap();
    }
    for (id, repo, number, kind, state, title, author, reviewers, sha, at) in [
        ("fi_a1", 77u64, 1u64, "pr", "open", "secret plan", "alice-gh", r#"["bob-gh"]"#, Some("abc"), 30u64),
        ("fi_a2", 88, 2, "issue", "open", "public fix", "bob-gh", "[]", None, 20),
        ("fi_a3", 88, 3, "pr", "merged", "public 100%_done", "alice-gh", "[]", Some("def"), 10),
        ("fi_b1", 99, 3, "pr", "open", "eve's secret", "eve-gh", "[]", Some("abc"), 40),
    ] {
        c.execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, author_login, reviewers, head_sha,
                 created_at, updated_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, 1)",
            params![id, repo, number, kind, state, title, author, reviewers, sha, at],
        )
        .unwrap();
    }
    for repo in [77u64, 99] {
        c.execute(
            "INSERT INTO forge_checks (repo_id, head_sha, kind, name, status, updated_at)
             VALUES (?1, 'abc', 'check_run', 'ci', 'completed', 1)",
            params![repo],
        )
        .unwrap();
    }
    c.execute(
        "INSERT INTO forge_events (id, repo_id, number, kind, actor_login, review_state, at)
         VALUES ('fev_1', 77, 1, 'review', 'bob-gh', 'approved', 5), ('fev_2', 99, 3, 'review', 'eve-gh', NULL, 5)",
        params![],
    )
    .unwrap();
    (team, eve)
}

/// The ids of the items a list holds, in its order.
async fn listed(base: &str, who: &str, project: &str, query: &str) -> (u16, Vec<String>, Value) {
    let (s, v) = call(base, who, "GET", &format!("/api/projects/{project}/items{query}"), None).await;
    let ids = v["items"].as_array().into_iter().flatten().map(|i| i["id"].as_str().unwrap().to_owned()).collect();
    (s, ids, v)
}

/// What `who` gets for an item, its checks and its events: the statuses.
async fn reads(base: &str, who: &str, item: &str) -> [u16; 3] {
    let mut out = [0; 3];
    for (i, tail) in ["", "/checks", "/events"].iter().enumerate() {
        out[i] = call(base, who, "GET", &format!("/api/forge/items/{item}{tail}"), None).await.0;
    }
    out
}

fn titles(v: &Value) -> String {
    v["items"].as_array().into_iter().flatten().map(|i| i["title"].as_str().unwrap_or("")).collect::<Vec<_>>().join("|")
}

const OK: [u16; 3] = [200; 3];
const GONE: [u16; 3] = [404; 3];

/// An item in one project is never visible from another, in a list or a
/// read, whatever GitHub says the reader may see.
#[tokio::test]
async fn leak_across_projects() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, eve) = world(&hub, &gh, &app).await;

    let (s, ids, _) = listed(&base, "eve", "personal", "").await;
    assert_eq!((s, ids), (200, vec!["fi_b1".to_owned()]));
    assert_eq!(reads(&base, "eve", "fi_b1").await, OK);
    let (s, v) = call(&base, "eve", "GET", "/api/forge/items/fi_b1/checks", None).await;
    assert_eq!((s, v["checks"].as_array().map(Vec::len)), (200, Some(1)), "{v}");

    // eve isn't in the team: its project is no project, and its items no
    // items.
    let (s, ids, _) = listed(&base, "eve", &team, "").await;
    assert_eq!((s, ids.len()), (404, 0));
    for item in ["fi_a1", "fi_a2", "fi_a3"] {
        assert_eq!(reads(&base, "eve", item).await, GONE, "{item}");
    }
    // alice is an admin of eve-gh/x on GitHub, but not in eve's project.
    let (s, _, _) = listed(&base, "alice", &eve, "").await;
    assert_eq!(s, 404);
    assert_eq!(reads(&base, "alice", "fi_b1").await, GONE);
    let (s, ids, _) = listed(&base, "alice", &team, "?repo=eve-gh/x").await;
    assert_eq!((s, ids.len()), (404, 0), "another project's repo, by name");
    let (_, ids, v) = listed(&base, "alice", &team, "").await;
    assert_eq!(ids, ["fi_a1", "fi_a2", "fi_a3"], "{v}");
    assert!(!v.to_string().contains("eve"), "{v}");
    // Two items have head `abc`: alice's checks are 77's only.
    let (_, v) = call(&base, "alice", "GET", "/api/forge/items/fi_a1/checks", None).await;
    assert_eq!(v["checks"].as_array().map(Vec::len), Some(1), "{v}");
    let (_, v) = call(&base, "alice", "GET", "/api/forge/items/fi_a1/events", None).await;
    assert_eq!(
        v["events"].as_array().map(|e| e.iter().map(|e| e["id"].clone()).collect::<Vec<_>>()),
        Some(vec![json!("fev_1")])
    );
    // And nobody at all.
    assert_eq!(reads(&base, "mallory", "fi_a2").await, GONE);
}

/// The thread on a private repo's item (M196) is read through the same
/// check: bob, whom GitHub lets in, opens it, comments and reads it back;
/// carol, a member GitHub doesn't know, is refused, and doesn't see it in
/// her chat list or her search.
#[tokio::test]
async fn leak_object_threads() {
    use crate::chat::objects::PRIVATE_REPO;
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.flag("team:t1", true);
    let open = |who: &'static str| {
        let base = base.clone();
        async move {
            call(&base, who, "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": "fi_a1" }))).await
        }
    };

    let (st, room) = open("bob").await;
    assert_eq!(st, 200, "{room}");
    let id = room["id"].as_str().unwrap().to_owned();
    let post = Some(json!({ "text": "the secret canary" }));
    assert_eq!(call(&base, "bob", "POST", &format!("/api/chat/rooms/{id}/messages"), post).await.0, 200);
    let (st, v) = call(&base, "bob", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await;
    assert_eq!((st, v["messages"].as_array().map(Vec::len)), (200, Some(1)), "{v}");

    let (st, v) = open("carol").await;
    assert_eq!((st, &v["reason"]), (403, &json!(PRIVATE_REPO)), "{v}");
    assert_eq!(call(&base, "carol", "GET", &format!("/api/chat/rooms/{id}"), None).await.0, 404);
    assert_eq!(call(&base, "carol", "GET", &format!("/api/chat/rooms/{id}/messages"), None).await.0, 404);
    let (_, v) = call(&base, "carol", "GET", "/api/chat", None).await;
    assert!(v["rooms"].as_array().unwrap().iter().all(|r| r["id"] != json!(id)), "{v}");
    let (_, v) = call(&base, "carol", "GET", "/api/chat/search?q=canary", None).await;
    assert_eq!(v["found"], json!([]), "{v}");
}

/// GitHub's answer is kept: a collaborator removed on GitHub still reads
/// until the `member` event comes, then is refused at once, inside the 10
/// minutes. Another member's access is asked again, and holds. An
/// `organization` event lets them back in the same way.
#[tokio::test]
async fn leak_after_a_collaborator_is_removed() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;

    let (_, ids, _) = listed(&base, "bob", &team, "").await;
    assert_eq!(ids, ["fi_a1", "fi_a2", "fi_a3"]);
    assert_eq!(reads(&base, "bob", "fi_a1").await, OK);
    let asked = gh.lock().unwrap().asked;
    assert_eq!(asked, 1, "asked once, then kept");
    // Read with a token minted for bob, and logged (M167).
    let mints: u64 = app
        .db
        .c()
        .query_row(
            "SELECT count(*) FROM github_token_mints WHERE purpose = 'read' AND installation_id = 9",
            params![],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mints, 1);

    // Removed on GitHub: kept for now (no event yet).
    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    assert_eq!(reads(&base, "bob", "fi_a1").await, OK);
    assert_eq!(reads(&base, "alice", "fi_a1").await, OK);

    let removed = json!({ "action": "removed", "member": { "id": 1002, "login": "bob-gh" },
        "repository": { "id": 77, "full_name": "acme/app", "owner": { "login": "acme" } },
        "installation": { "id": 9 } });
    assert_eq!(hook(&base, "member", "m1", &removed).await, 202);
    assert_eq!(reads(&base, "bob", "fi_a1").await, GONE);
    let (_, ids, v) = listed(&base, "bob", &team, "").await;
    assert_eq!(ids, ["fi_a2", "fi_a3"], "{v}");
    assert_eq!(v["withheld"], json!([{ "repo": "acme/app", "why": "no_access" }]));
    assert_eq!(reads(&base, "alice", "fi_a1").await, OK, "asked again: still an admin");

    // Back on GitHub: the kept "no" holds until an event says otherwise.
    gh.lock().unwrap().roles.insert(("acme/app".into(), "bob-gh".into()), ("write".into(), 1002));
    assert_eq!(reads(&base, "bob", "fi_a1").await, GONE);
    let added = json!({ "action": "member_added", "organization": { "login": "acme" },
        "membership": { "user": { "id": 1002, "login": "bob-gh" } } });
    assert_eq!(hook(&base, "organization", "o1", &added).await, 202);
    assert_eq!(reads(&base, "bob", "fi_a1").await, OK);

    // A team change drops the org's answers, a redelivery nothing more.
    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    let left = json!({ "action": "removed", "scope": "team", "organization": { "login": "ACME" },
        "member": { "id": 1002 }, "team": { "slug": "devs" } });
    assert_eq!(hook(&base, "membership", "t1", &left).await, 202);
    assert_eq!(reads(&base, "bob", "fi_a1").await, GONE);
}

/// A member who didn't sign in to control with GitHub sees the public
/// repos' items and nothing private, and is told why.
#[tokio::test]
async fn leak_unlinked_github_account() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;

    let (s, ids, v) = listed(&base, "carol", &team, "").await;
    assert_eq!((s, ids), (200, vec!["fi_a2".to_owned(), "fi_a3".to_owned()]), "{v}");
    assert_eq!(v["withheld"], json!([{ "repo": "acme/app", "why": "no_github" }]));
    assert!(!v.to_string().contains("secret"), "{v}");
    assert_eq!(reads(&base, "carol", "fi_a1").await, GONE);
    assert_eq!(reads(&base, "carol", "fi_a2").await, OK);
    assert_eq!(gh.lock().unwrap().asked, 0, "nothing to ask GitHub about");
}

/// After a repo is unlinked, nothing of it is readable, by anyone, even
/// with GitHub's yes kept; a public one too.
#[tokio::test]
async fn leak_unlinked_repo() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    assert_eq!(reads(&base, "alice", "fi_a1").await, OK);

    for repo in ["acme/app", "acme/site"] {
        let (s, v) = call(&base, "alice", "DELETE", &format!("/api/projects/{team}/repos/{repo}"), None).await;
        assert_eq!(s, 200, "{v}");
    }
    for who in ["alice", "bob", "carol"] {
        for item in ["fi_a1", "fi_a2", "fi_a3"] {
            assert_eq!(reads(&base, who, item).await, GONE, "{who} {item}");
        }
        let (s, ids, v) = listed(&base, who, &team, "").await;
        assert_eq!((s, ids.len()), (200, 0), "{v}");
        assert_eq!(v["withheld"], json!([]), "an unlinked repo is in the repos list, not here");
    }
    let (s, _, _) = listed(&base, "alice", &team, "?repo=acme/app").await;
    assert_eq!(s, 200, "named: still the project's until the purge, with nothing in it");
}

/// The list's text filter is search: it never finds a private title the
/// viewer can't see, and `%` and `_` are only themselves.
#[tokio::test]
async fn leak_search() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;

    let (_, ids, _) = listed(&base, "bob", &team, "?q=SECRET").await;
    assert_eq!(ids, ["fi_a1"]);
    for who in ["carol", "eve", "mallory"] {
        let (_, ids, v) = listed(&base, who, &team, "?q=secret").await;
        assert!(ids.is_empty(), "{who}: {v}");
        assert!(!titles(&v).contains("secret"), "{who}: {v}");
    }
    let (_, ids, v) = listed(&base, "eve", "personal", "?q=secret").await;
    assert_eq!((ids, titles(&v)), (vec!["fi_b1".to_owned()], "eve's secret".to_owned()));
    // Taken away on GitHub and told: not found by search either.
    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    let removed =
        json!({ "action": "removed", "member": { "id": 1002 }, "repository": { "id": 77, "full_name": "acme/app" } });
    assert_eq!(hook(&base, "member", "m1", &removed).await, 202);
    let (_, ids, _) = listed(&base, "bob", &team, "?q=secret").await;
    assert!(ids.is_empty());

    for (q, want) in [("%25", vec!["fi_a3"]), ("_", vec!["fi_a3"]), ("100%25_", vec!["fi_a3"]), ("p%25x", vec![])] {
        let (_, ids, _) = listed(&base, "carol", &team, &format!("?q={q}")).await;
        assert_eq!(ids, want, "{q}");
    }
}

/// A repo made private on GitHub stops showing to members without access
/// at once; made public again, it shows.
#[tokio::test]
async fn leak_repo_made_private() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (_, _) = world(&hub, &gh, &app).await;
    assert_eq!(reads(&base, "carol", "fi_a2").await, OK);
    let ev = |action: &str, private: bool| json!({ "action": action, "repository": { "id": 88, "full_name": "acme/site", "private": private } });
    assert_eq!(hook(&base, "repository", "r1", &ev("privatized", true)).await, 202);
    assert!(app.db.linked_repo(88).unwrap().unwrap().private);
    assert_eq!(reads(&base, "carol", "fi_a2").await, GONE);
    assert_eq!(reads(&base, "bob", "fi_a2").await, GONE, "GitHub lists him on acme/app only");
    assert_eq!(hook(&base, "repository", "r2", &ev("publicized", false)).await, 202);
    assert_eq!(reads(&base, "carol", "fi_a2").await, OK);
}

/// A repo linked while public and made private is private here from the
/// next payload of any kind that carries it (the App isn't always told
/// `repository` events); a late one that says public doesn't open it up.
#[tokio::test]
async fn leak_repo_made_private_seen_on_any_payload() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let pr = |private: bool| {
        json!({ "action": "opened", "number": 4, "pull_request": { "number": 4 },
            "repository": { "id": 88, "full_name": "acme/site", "private": private } })
    };
    assert_eq!(hook(&base, "pull_request", "p1", &pr(false)).await, 202);
    assert_eq!(reads(&base, "carol", "fi_a2").await, OK);
    let check = json!({ "action": "completed", "check_run": { "pull_requests": [] },
        "repository": { "id": 88, "full_name": "acme/site", "private": true } });
    assert_eq!(hook(&base, "check_run", "c1", &check).await, 202);
    assert!(app.db.linked_repo(88).unwrap().unwrap().private);
    assert_eq!(reads(&base, "carol", "fi_a2").await, GONE);
    let (_, ids, v) = listed(&base, "bob", &team, "?q=public").await;
    assert!(ids.is_empty(), "{v}");
    assert_eq!(hook(&base, "pull_request", "p2", &pr(false)).await, 202);
    assert!(app.db.linked_repo(88).unwrap().unwrap().private, "only `publicized` opens it");
    assert_eq!(reads(&base, "carol", "fi_a2").await, GONE);
    assert_eq!(reads(&base, "alice", "fi_a2").await, GONE, "the App isn't installed on acme/site: GitHub says no");
}

/// Push and inbox titles go only to those who may read the item.
#[tokio::test]
async fn leak_title_readers() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let _ = world(&hub, &gh, &app).await;
    let everyone: Vec<String> = ["alice", "bob", "carol", "eve", "mallory"].map(Into::into).to_vec();
    let readers = |item: &'static str| {
        let (app, everyone) = (app.clone(), everyone.clone());
        async move { viewer::title_readers(&app, item, &everyone).await.unwrap() }
    };
    assert_eq!(readers("fi_a1").await, ["alice", "bob"]);
    assert_eq!(readers("fi_a2").await, ["alice", "bob", "carol"]);
    assert_eq!(readers("fi_b1").await, ["eve"]);
    assert!(readers("fi_nope").await.is_empty());

    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    let removed =
        json!({ "action": "removed", "member": { "id": 1002 }, "repository": { "id": 77, "full_name": "acme/app" } });
    assert_eq!(hook(&base, "member", "m1", &removed).await, 202);
    assert_eq!(readers("fi_a1").await, ["alice"]);
}

/// A machine reads for its owner with its live `cloud.drive` grant, and
/// not once it's revoked.
#[tokio::test]
async fn a_machine_reads_for_its_owner_with_its_grant() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.daemon("geek", "bob", None);
    let get = |path: String| {
        let r = reqwest::Client::new().get(format!("{base}{path}")).header("x-test-daemon", "bob geek");
        async move { r.send().await.unwrap().status().as_u16() }
    };
    assert_eq!(get(format!("/api/projects/{team}/items")).await, 200);
    assert_eq!(get("/api/forge/items/fi_a1".into()).await, 200);
    let g = app.db.grants("bob").unwrap().remove(0);
    app.db.revoke_grant("bob", &g.id, 1).unwrap();
    assert_eq!(get("/api/forge/items/fi_a1".into()).await, 403);
}

/// A list narrows by repo, kind, state, author and requested reviewer,
/// and is last updated first.
#[tokio::test]
async fn filters_narrow_a_list() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    for (q, want) in [
        ("", vec!["fi_a1", "fi_a2", "fi_a3"]),
        ("?repo=ACME/site", vec!["fi_a2", "fi_a3"]),
        ("?kind=pr", vec!["fi_a1", "fi_a3"]),
        ("?state=merged", vec!["fi_a3"]),
        ("?author=Alice-GH", vec!["fi_a1", "fi_a3"]),
        ("?review_requested=bob-gh", vec!["fi_a1"]),
        ("?review_requested=alice-gh", vec![]),
        ("?kind=pr&state=open&limit=1", vec!["fi_a1"]),
    ] {
        let (s, ids, v) = listed(&base, "alice", &team, q).await;
        assert_eq!((s, ids), (200, want.iter().map(|s| s.to_string()).collect::<Vec<_>>()), "{q}: {v}");
    }
    let (_, v) = call(&base, "alice", "GET", "/api/forge/items/fi_a1", None).await;
    assert_eq!(
        (v["repo"].as_str(), v["reviewers"].clone(), v["author"].as_str()),
        (Some("acme/app"), json!(["bob-gh"]), Some("alice-gh")),
        "{v}"
    );
    let (s, _, _) = listed(&base, "alice", &team, "?repo=acme/nope").await;
    assert_eq!(s, 404);
}

/// A kept answer is used for 10 minutes by the database's clock, unless
/// a drop for its repo, its owner or its person came at or after it was
/// asked: even one that came back from GitHub after the drop (another
/// machine's, mid-ask).
#[test]
fn kept_answers_and_drops() {
    let db = Db::test();
    let kept = || db.kept_access(1002, KEEP_MS).unwrap();
    let tick = || std::thread::sleep(std::time::Duration::from_millis(3));
    let now = db.clock_ms().unwrap();
    assert!(now.abs_diff(arugula_e2e::now_ms()) < 60_000, "milliseconds since the epoch: {now}");
    db.keep_access(1002, "Acme/App", None, now).unwrap();
    db.keep_access(1002, "eve-gh/x", Some("not a collaborator"), now).unwrap();
    db.keep_access(1002, "old/one", None, now - KEEP_MS - 1).unwrap();
    assert_eq!(kept().len(), 2, "{:?}", kept());
    assert_eq!(kept().get("acme/app"), Some(&None));
    assert_eq!(kept().get("eve-gh/x"), Some(&Some("not a collaborator".into())));

    tick();
    db.drop_access(AccessScope::Repo, "ACME/app").unwrap();
    assert_eq!(kept().get("acme/app"), None);
    assert!(kept().contains_key("eve-gh/x"));
    // Asked before the drop, answered after it: never used.
    db.keep_access(1002, "acme/app", None, now + 1).unwrap();
    assert_eq!(kept().get("acme/app"), None);
    tick();
    let later = db.clock_ms().unwrap();
    db.keep_access(1002, "acme/app", None, later).unwrap();
    assert_eq!(kept().get("acme/app"), Some(&None));
    // An older answer doesn't replace a newer one.
    db.keep_access(1002, "acme/app", Some("no"), later - 1).unwrap();
    assert_eq!(kept().get("acme/app"), Some(&None));

    tick();
    db.drop_access(AccessScope::Owner, "ACME").unwrap();
    assert_eq!(kept().get("acme/app"), None);
    tick();
    db.keep_access(1002, "acme/app", None, db.clock_ms().unwrap()).unwrap();
    tick();
    db.drop_access(AccessScope::Person, "1002").unwrap();
    assert!(kept().is_empty());
    assert_eq!(db.prune_access(KEEP_MS).unwrap(), 1, "the old answer");
    tick();
    assert_eq!(db.prune_access(0).unwrap(), 2 + 3, "the rest, and the drops");
}

/// A platform on `db` with the App, against the fake GitHub at `api`.
fn platform(db: Db, hub: &Arc<TestHub>, api: &str) -> Platform {
    let mut p = Platform::new(Default::default(), db, hub.clone(), reqwest::Client::new(), Default::default());
    p.github = Some(GithubApp::new("1".into(), "arugula-test".into(), SECRET.into(), api, throwaway_key()));
    p
}

/// What a hosted box of bob's is answered for a token to `acme/app`: 200
/// or the refusal's status.
async fn bob_token(p: &Platform, bob: &GithubUser) -> u16 {
    match super::token(p, "bob", Some(bob), "acme/app").await.unwrap() {
        super::TokenAnswer::Token(_) => 200,
        super::TokenAnswer::Refused { status, .. } => status,
    }
}

/// Whether bob's daemon gets live updates for `acme/app`.
async fn bob_live(p: &Platform, bob: &GithubUser) -> bool {
    super::standing(p, "bob", Some(bob), &["acme/app".to_owned()], false).await[0].live
}

/// Two machines on one database: a drop taken by one refuses the other's
/// next token and live updates, though that one asked GitHub itself and
/// was told yes a moment before.
#[tokio::test]
async fn leak_a_token_on_another_machine_after_a_drop() {
    let hub = Arc::new(TestHub::default());
    let gh: Shared = Arc::default();
    let api = fake_github(gh.clone()).await;
    {
        let mut g = gh.lock().unwrap();
        g.installed.insert("acme/app".into(), (9, 500, "acme".into()));
        g.roles.insert(("acme/app".into(), "bob-gh".into()), ("write".into(), 1002));
    }
    let bob = GithubUser { id: 1002, login: "bob-gh".into() };
    // Postgres's second pool on the same schema, or a SQLite file opened
    // twice.
    let first = Db::test();
    let file = std::env::temp_dir().join(format!("m165-{}.db", hex::encode(arugula_e2e::random::<8>())));
    let (a, b) = match first.second() {
        Some(other) => (first, other),
        None => (Db::open(&file).unwrap(), Db::open(&file).unwrap()),
    };
    let (a, b) = (platform(a, &hub, &api), platform(b, &hub, &api));
    assert_eq!(bob_token(&b, &bob).await, 200);
    assert!(bob_live(&b, &bob).await);
    assert_eq!(gh.lock().unwrap().asked, 1, "asked once");
    assert_eq!(bob_token(&a, &bob).await, 200, "the first machine uses the second's answer");
    assert_eq!(gh.lock().unwrap().asked, 1);

    // Removed on GitHub, and the event lands on the first machine.
    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    let removed = json!({ "action": "removed", "member": { "id": 1002 },
        "repository": { "id": 77, "full_name": "acme/app", "name": "app", "owner": { "login": "acme" } } });
    viewer::event(&a, "member", &removed).unwrap();
    assert_eq!(bob_token(&b, &bob).await, 403);
    assert!(!bob_live(&b, &bob).await);
    assert_eq!(bob_token(&a, &bob).await, 403);
    drop((a, b));
    let _ = std::fs::remove_file(&file);
}

/// A row whose reviewers aren't a JSON list matches no reviewer and
/// doesn't fail the list; and a title is matched by ASCII case the same
/// on SQLite and Postgres.
#[test]
fn a_bad_row_never_fails_a_list() {
    let db = Db::test();
    db.c()
        .execute_batch(
            r#"INSERT INTO forge_items (id, repo_id, number, kind, state, title, reviewers, created_at, updated_at, synced_at)
               VALUES ('fi_1', 1, 1, 'pr', 'open', 'École', '{"login": "bob-gh"}', 1, 3, 1),
                      ('fi_2', 1, 2, 'pr', 'open', 'two', '"bob-gh"', 1, 2, 1),
                      ('fi_3', 1, 3, 'pr', 'open', 'three', '["Bob-GH"]', 1, 1, 1);"#,
        )
        .unwrap();
    let r = viewer::Readable::of_repos(vec![1]);
    let ids = |f: ItemFilter<'_>| {
        db.forge_items(&r, &ItemFilter { limit: 10, ..f }).unwrap().into_iter().map(|i| i.id).collect::<Vec<_>>()
    };
    assert_eq!(ids(ItemFilter { review_requested: Some("bob-gh"), ..Default::default() }), ["fi_3"]);
    assert_eq!(ids(ItemFilter::default()).len(), 3);
    assert_eq!(ids(ItemFilter { text: Some("ÉCOLE"), ..Default::default() }), ["fi_1"]);
    assert!(ids(ItemFilter { text: Some("école"), ..Default::default() }).is_empty(), "É is folded on neither");
}

/// The guarded reads return rows of a readable repo only.
#[test]
fn reads_take_what_the_check_made() {
    let db = Db::test();
    db.c()
        .execute_batch(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES ('fi_1', 1, 1, 'pr', 'open', 'one', 1, 1, 1), ('fi_2', 2, 1, 'pr', 'open', 'two', 1, 2, 1);",
        )
        .unwrap();
    let only = viewer::Readable::of_repos(vec![1]);
    let all = |r| db.forge_items(r, &ItemFilter { limit: 10, ..Default::default() }).unwrap();
    assert_eq!(all(&only).iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["fi_1"]);
    assert!(all(&viewer::Readable::of_repos(vec![])).is_empty());
    assert!(db.forge_item(&only, "fi_2").unwrap().is_none());
    assert_eq!(db.forge_item_repo("fi_2").unwrap(), Some(2));
}
