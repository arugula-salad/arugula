//! Drafts on linked repos (M170) through the platform's routes, in M165's
//! world (`viewer_tests::world`): team t1's project links `acme/app` (77,
//! private: alice and bob have access on GitHub, carol didn't sign in with
//! GitHub) and `acme/site` (88, public); eve's own project links `eve-gh/x`.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

/// A call signed as a machine (`<account> <machine>`).
async fn machine(base: &str, who: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req =
        reqwest::Client::new().request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", who);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

fn ids(v: &Value) -> Vec<String> {
    v["drafts"].as_array().into_iter().flatten().map(|d| d["id"].as_str().unwrap().to_owned()).collect()
}

fn rows(app: &crate::Platform) -> u64 {
    app.db.c().query_row("SELECT count(*) FROM drafts", params![], |r| r.get(0)).unwrap()
}

/// A person drafts for themselves; a machine drafts for its owner, naming
/// itself and the agent. Both show to the project's readers, oldest first.
#[tokio::test]
async fn a_person_and_an_agent_on_their_machine_draft() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);

    let (s, mine) = call(
        &base,
        "bob",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "acme/site", "number": 2, "kind": "comment", "body": "on it" })),
    )
    .await;
    assert_eq!(s, 200, "{mine}");
    let d = &mine["draft"];
    assert!(mine["created"] == true && d["id"].as_str().unwrap().starts_with("dft_"), "{mine}");
    assert_eq!((d["author"].as_str(), d["agent"].as_str(), d["machine"].as_str()), (Some("bob"), None, None));
    assert_eq!(
        (d["project"].as_str(), d["repo"].as_str(), d["state"].as_str()),
        (Some(team.as_str()), Some("acme/site"), Some("waiting"))
    );
    assert_eq!((d["item"].as_str(), d["item_title"].as_str()), (Some("fi_a2"), Some("public fix")));

    let (s, agent) = machine(
        &base,
        "alice geek",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "ACME/app", "number": 1, "kind": "review", "event": "REQUEST_CHANGES",
                     "body": "split it", "agent": "mcp:claude-code" })),
    )
    .await;
    assert_eq!(s, 200, "{agent}");
    let a = &agent["draft"];
    assert_eq!(
        (a["author"].as_str(), a["agent"].as_str(), a["machine"].as_str(), a["machine_name"].as_str()),
        (Some("alice"), Some("mcp:claude-code"), Some("geek"), Some("geek-box"))
    );
    assert_eq!(
        (a["kind"].as_str(), a["event"].as_str(), a["repo"].as_str()),
        (Some("review"), Some("REQUEST_CHANGES"), Some("acme/app"))
    );
    // A machine's with no agent named is still an agent's.
    let (_, unnamed) = machine(
        &base,
        "alice geek",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "acme/site", "number": 3, "kind": "comment", "body": "hm" })),
    )
    .await;
    assert_eq!(unnamed["draft"]["agent"], "an agent");

    let (s, v) = call(&base, "alice", "GET", &format!("/api/projects/{team}/drafts"), None).await;
    assert_eq!(s, 200);
    let mut got = ids(&v);
    got.sort();
    let mut want = [&d["id"], &a["id"], &unnamed["draft"]["id"]].map(|x| x.as_str().unwrap().to_owned());
    want.sort();
    assert_eq!(got, want);
    let (_, v) = call(&base, "alice", "GET", &format!("/api/projects/{team}/drafts?mine=true"), None).await;
    assert_eq!(ids(&v).len(), 2, "{v}");
    let (_, v) =
        call(&base, "alice", "GET", &format!("/api/projects/{team}/drafts?repo=acme/site&number=2"), None).await;
    assert_eq!(ids(&v), [d["id"].as_str().unwrap()]);
    let (_, v) = call(&base, "bob", "GET", "/api/forge/items/fi_a1/drafts", None).await;
    assert_eq!(ids(&v), [a["id"].as_str().unwrap()]);
    let (_, v) = call(&base, "bob", "GET", "/api/forge/drafts", None).await;
    assert_eq!(ids(&v), [d["id"].as_str().unwrap()], "bob's own");
    let (s, one) = call(&base, "bob", "GET", &format!("/api/forge/drafts/{}", a["id"].as_str().unwrap()), None).await;
    assert_eq!((s, one["body"].as_str()), (200, Some("split it")));

    // What a draft must say.
    for bad in [
        json!({ "repo": "acme/site", "number": 2, "kind": "comment", "body": " " }),
        json!({ "repo": "acme/site", "kind": "comment", "body": "x" }),
        json!({ "repo": "acme/site", "number": 2, "kind": "review", "event": "LGTM", "body": "x" }),
        json!({ "repo": "acme/site", "kind": "issue", "body": "x" }),
        json!({ "repo": "acme/site", "number": 2, "kind": "merge", "body": "x" }),
        json!({ "repo": "acme/site", "number": 2, "kind": "comment", "body": "x", "import_key": "k" }),
    ] {
        assert_eq!(call(&base, "bob", "POST", "/api/forge/drafts", Some(bad.clone())).await.0, 400, "{bad}");
    }
    let (s, issue) = call(
        &base,
        "bob",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "acme/site", "kind": "issue", "title": "Docs", "body": "" })),
    )
    .await;
    assert_eq!((s, issue["draft"]["number"].clone()), (200, Value::Null), "{issue}");
}

/// A private repo's drafts are no drafts to a member GitHub wouldn't let
/// read it, nor to anyone outside the project: not in a list, not one by
/// one, and they can't draft there or discard them.
#[tokio::test]
async fn leak_a_private_repos_drafts() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, eve_project) = world(&hub, &gh, &app).await;
    let (_, d) = call(
        &base,
        "alice",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "the secret plan's flaw" })),
    )
    .await;
    let id = d["draft"]["id"].as_str().unwrap().to_owned();

    // carol is on the team, but didn't sign in with GitHub.
    let (s, v) = call(&base, "carol", "GET", &format!("/api/projects/{team}/drafts?state=all"), None).await;
    assert_eq!((s, ids(&v).len()), (200, 0), "{v}");
    assert!(!v.to_string().contains("secret"), "{v}");
    assert_eq!(call(&base, "carol", "GET", &format!("/api/forge/drafts/{id}"), None).await.0, 404);
    assert_eq!(call(&base, "carol", "POST", &format!("/api/forge/drafts/{id}/discard"), None).await.0, 404);
    let (s, v) = call(
        &base,
        "carol",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "x" })),
    )
    .await;
    assert_eq!(s, 404, "{v}");
    // eve isn't on the team at all.
    assert_eq!(call(&base, "eve", "GET", &format!("/api/projects/{team}/drafts"), None).await.0, 404);
    assert_eq!(call(&base, "eve", "GET", &format!("/api/forge/drafts/{id}"), None).await.0, 404);
    let (s, v) = call(&base, "eve", "GET", "/api/forge/drafts?state=all", None).await;
    assert_eq!((s, ids(&v).len()), (200, 0));
    // ...and alice isn't in eve's project, though she's an admin of its
    // repo on GitHub.
    let (s, _) = call(
        &base,
        "alice",
        "POST",
        "/api/forge/drafts",
        Some(json!({ "repo": "eve-gh/x", "number": 3, "kind": "comment", "body": "x" })),
    )
    .await;
    assert_eq!(s, 404);
    assert_eq!(call(&base, "alice", "GET", &format!("/api/projects/{eve_project}/drafts"), None).await.0, 404);
    // bob has access: he reads it.
    let (_, v) = call(&base, "bob", "GET", &format!("/api/projects/{team}/drafts"), None).await;
    assert_eq!(ids(&v), std::slice::from_ref(&id));
}

/// Whether a repo is linked, as a machine asks before it keeps a block's
/// drafts there: another project's is no linked repo, and one of the
/// reader's own projects they may not read says why.
#[tokio::test]
async fn a_machine_asks_whether_a_repo_is_linked() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);
    hub.daemon("box", "carol", None);
    hub.daemon("eves", "eve", None);
    let (s, v) = machine(&base, "alice geek", "GET", "/api/forge/repos/Acme/App", None).await;
    assert_eq!(s, 200);
    // M172: and who the reader is on GitHub, for a block pinned to one of
    // its items.
    assert_eq!(
        v,
        json!({ "repo": "Acme/App", "linked": true, "project": team, "repo_id": 77, "why": null, "login": "alice-gh" })
    );
    let (_, v) = machine(&base, "carol box", "GET", "/api/forge/repos/acme/app", None).await;
    assert_eq!((v["linked"].as_bool(), v["why"].as_str()), (Some(false), Some("no_github")));
    let (_, v) = machine(&base, "eve eves", "GET", "/api/forge/repos/acme/app", None).await;
    assert_eq!(
        v,
        json!({ "repo": "acme/app", "linked": false, "project": null, "repo_id": null, "why": null, "login": null })
    );
    let (_, v) = machine(&base, "alice geek", "GET", "/api/forge/repos/acme/elsewhere", None).await;
    assert_eq!(v["linked"], false);
}

/// A machine's upload lands once: the same key again (a restart before it
/// dropped its copy) answers with the first draft. Another machine's key
/// is its own, and a key is never another author's.
#[tokio::test]
async fn an_upload_lands_once() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);
    hub.daemon("mini", "alice", None);
    let upload = json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "kept in layout.json",
                         "agent": "mcp:claude-code", "import_key": "4/d1700000000000" });
    let (s, first) = machine(&base, "alice geek", "POST", "/api/forge/drafts", Some(upload.clone())).await;
    assert_eq!((s, first["created"].as_bool()), (200, Some(true)), "{first}");
    let (s, again) = machine(&base, "alice geek", "POST", "/api/forge/drafts", Some(upload.clone())).await;
    assert_eq!((s, again["created"].as_bool()), (200, Some(false)), "{again}");
    assert_eq!(again["draft"], first["draft"]);
    assert_eq!(rows(&app), 1);
    let key: String = app.db.c().query_row("SELECT import_key FROM drafts", params![], |r| r.get(0)).unwrap();
    assert_eq!(key, "geek:4/d1700000000000", "the machine's own");
    // Discarded since: still the same one, not a new one.
    let id = first["draft"]["id"].as_str().unwrap();
    assert_eq!(call(&base, "alice", "POST", &format!("/api/forge/drafts/{id}/discard"), None).await.0, 200);
    let (_, after) = machine(&base, "alice geek", "POST", "/api/forge/drafts", Some(upload.clone())).await;
    assert_eq!((after["created"].as_bool(), after["draft"]["state"].as_str()), (Some(false), Some("discarded")));
    // Another machine's same key is another draft.
    let (_, other) = machine(&base, "alice mini", "POST", "/api/forge/drafts", Some(upload)).await;
    assert_eq!(other["created"], true);
    assert_eq!(rows(&app), 2);
}

/// Its author discards a draft, or anyone who may read it; again is no
/// change. An author cut off from the repo can still discard their own,
/// but not read it.
#[tokio::test]
async fn discarding() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let new = |who: &'static str| {
        let base = base.clone();
        async move {
            let (_, d) = call(
                &base,
                who,
                "POST",
                "/api/forge/drafts",
                Some(json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "x" })),
            )
            .await;
            d["draft"]["id"].as_str().unwrap().to_owned()
        }
    };
    let (a, b1, b2) = (new("alice").await, new("bob").await, new("bob").await);
    let discard = |who: &'static str, id: String| {
        let base = base.clone();
        async move { call(&base, who, "POST", &format!("/api/forge/drafts/{id}/discard"), None).await }
    };
    let (s, v) = discard("alice", a.clone()).await;
    assert_eq!((s, v), (200, json!({ "id": a, "state": "discarded" })));
    assert_eq!(discard("alice", a.clone()).await.0, 200, "again: no change");
    assert_eq!(discard("alice", b1.clone()).await.0, 200, "a reader discards another's");
    let (_, v) = call(&base, "alice", "GET", &format!("/api/projects/{team}/drafts"), None).await;
    assert_eq!(ids(&v), std::slice::from_ref(&b2), "only the waiting one");
    let (_, v) = call(&base, "alice", "GET", &format!("/api/projects/{team}/drafts?state=discarded"), None).await;
    let mut got = ids(&v);
    got.sort();
    let mut want = [a.clone(), b1];
    want.sort();
    assert_eq!(got, want);
    // A sent one stays sent (M168 sends).
    app.db.c().execute("UPDATE drafts SET state = 'sent' WHERE id = ?1", params![a]).unwrap();
    assert_eq!(discard("alice", a).await.0, 409);

    // bob is removed on GitHub: his own draft he discards, unread.
    gh.lock().unwrap().roles.remove(&("acme/app".to_owned(), "bob-gh".to_owned()));
    let removed = json!({ "action": "removed", "member": { "id": 1002, "login": "bob-gh" },
        "repository": { "id": 77, "full_name": "acme/app", "owner": { "login": "acme" } }, "installation": { "id": 9 } });
    assert_eq!(super::links_tests::hook(&base, "member", "m1", &removed).await, 202);
    assert_eq!(call(&base, "bob", "GET", &format!("/api/forge/drafts/{b2}"), None).await.0, 404);
    let (s, v) = discard("bob", b2.clone()).await;
    assert_eq!((s, v["state"].as_str()), (200, Some("discarded")), "{v}");
    assert!(!v.to_string().contains("secret"));
}
