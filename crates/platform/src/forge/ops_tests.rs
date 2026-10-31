//! Typed forge operations (M169) through `POST /api/forge/ops`, in M165's
//! world (`viewer_tests::world`): team t1's project links `acme/app` (77,
//! private: alice and bob have access on GitHub, carol didn't sign in with
//! GitHub) and `acme/site` (88, public); eve's own project links `eve-gh/x`.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::chat_tests::{TestHub, call};

/// An operation at version 1, as a person.
async fn op(base: &str, who: &str, op: &str, args: Value) -> (u16, Value) {
    let mut body = args;
    body["op"] = json!(op);
    body["v"] = json!(1);
    call(base, who, "POST", "/api/forge/ops", Some(body)).await
}

/// The same, signed as a machine (`<account> <machine>`).
async fn machine_op(base: &str, who: &str, op: &str, args: Value) -> (u16, Value) {
    let mut body = args;
    body["op"] = json!(op);
    body["v"] = json!(1);
    let r = reqwest::Client::new()
        .post(format!("{base}/api/forge/ops"))
        .header("x-test-daemon", who)
        .json(&body)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

fn ids(v: &Value, list: &str) -> Vec<String> {
    v["result"][list].as_array().into_iter().flatten().map(|i| i["id"].as_str().unwrap().to_owned()).collect()
}

/// Reads by `OWNER/NAME#N`, through the viewer check: what a reader may
/// read, and nothing of what they may not.
#[tokio::test]
async fn reads_by_repo_and_number() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;

    let (s, pr) = op(&base, "bob", "pr.get", json!({ "repo": "acme/app", "number": 1 })).await;
    assert_eq!(s, 200, "{pr}");
    assert_eq!((pr["op"].as_str(), pr["v"].as_u64()), (Some("pr.get"), Some(1)));
    let r = &pr["result"];
    assert_eq!((r["item"]["id"].as_str(), r["item"]["title"].as_str()), (Some("fi_a1"), Some("secret plan")));
    assert_eq!(r["events"][0]["review_state"], "approved", "{pr}");
    assert_eq!(r["drafts"], json!([]));

    // Who may not read the private repo, or isn't in the project: no item.
    for who in ["carol", "eve"] {
        let (s, v) = op(&base, who, "pr.get", json!({ "repo": "acme/app", "number": 1 })).await;
        assert_eq!(s, 404, "{who}: {v}");
        assert!(!v.to_string().contains("secret plan"), "{v}");
        let (s, _) = op(&base, who, "checks.get", json!({ "repo": "acme/app", "number": 1 })).await;
        assert_eq!(s, 404, "{who}");
    }
    let (s, v) = op(&base, "alice", "pr.get", json!({ "repo": "eve-gh/x", "number": 3 })).await;
    assert_eq!(s, 404, "another project's: {v}");

    // The right op for the kind, and a number the mirror doesn't have.
    let (s, v) = op(&base, "bob", "issue.get", json!({ "repo": "acme/app", "number": 1 })).await;
    assert!(s == 404 && v["error"].as_str().unwrap().contains("pr.get reads it"), "{v}");
    let (s, v) = op(&base, "bob", "issue.get", json!({ "repo": "acme/site", "number": 2 })).await;
    assert_eq!((s, v["result"]["item"]["title"].as_str()), (200, Some("public fix")), "{v}");
    let (s, _) = op(&base, "bob", "pr.get", json!({ "repo": "acme/site", "number": 9 })).await;
    assert_eq!(s, 404);

    let (s, c) = op(&base, "bob", "checks.get", json!({ "repo": "acme/app", "number": 1 })).await;
    assert_eq!((s, c["result"]["checks"][0]["name"].as_str()), (200, Some("ci")), "{c}");

    // A list by repo, by project (with a filter), and the personal one.
    let (s, v) = op(&base, "carol", "item.list", json!({ "repo": "acme/site" })).await;
    assert_eq!((s, ids(&v, "items")), (200, vec!["fi_a2".to_owned(), "fi_a3".to_owned()]), "{v}");
    let (s, _) = op(&base, "carol", "item.list", json!({ "repo": "acme/app" })).await;
    assert_eq!(s, 404, "a private repo carol can't read");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "kind": "pr", "state": "open" })).await;
    assert_eq!(ids(&v, "items"), ["fi_a1"], "{v}");
    let (s, v) = op(&base, "eve", "item.list", json!({})).await;
    assert_eq!((s, ids(&v, "items")), (200, vec!["fi_b1".to_owned()]), "{v}");
}

/// An agent on its owner's machine drafts a review with no block open;
/// the person lists it, reads it and discards another.
#[tokio::test]
async fn drafts_by_operation() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);

    let review = json!({ "repo": "acme/app", "number": 1, "kind": "review", "event": "REQUEST_CHANGES",
                         "body": "split it", "agent": "mcp:claude-code" });
    let (s, made) = machine_op(&base, "alice geek", "draft.create", review).await;
    assert_eq!(s, 200, "{made}");
    let d = &made["result"]["draft"];
    let id = d["id"].as_str().unwrap().to_owned();
    assert_eq!((d["machine"].as_str(), d["item"].as_str()), (Some("geek"), Some("fi_a1")), "{made}");

    let (_, v) = op(&base, "alice", "draft.list", json!({ "repo": "acme/app", "number": 1 })).await;
    assert_eq!(ids(&v, "drafts"), std::slice::from_ref(&id), "{v}");
    let (_, v) = op(&base, "alice", "draft.list", json!({})).await;
    assert_eq!(ids(&v, "drafts"), std::slice::from_ref(&id), "her own: {v}");
    let (_, pr) = op(&base, "bob", "pr.get", json!({ "repo": "acme/app", "number": 1 })).await;
    assert_eq!(pr["result"]["drafts"][0]["id"], id.as_str(), "on the item: {pr}");
    let (s, one) = op(&base, "alice", "draft.get", json!({ "draft": id })).await;
    assert_eq!((s, one["result"]["body"].as_str()), (200, Some("split it")));
    for who in ["carol", "eve"] {
        assert_eq!(op(&base, who, "draft.get", json!({ "draft": id })).await.0, 404, "{who}");
        assert_eq!(op(&base, who, "draft.list", json!({ "repo": "acme/app" })).await.0, 404, "{who}");
    }

    let (_, other) =
        op(&base, "bob", "draft.create", json!({ "repo": "acme/site", "kind": "issue", "title": "Docs" })).await;
    let other = other["result"]["draft"]["id"].as_str().unwrap().to_owned();
    let (s, gone) = op(&base, "bob", "draft.discard", json!({ "draft": other })).await;
    assert_eq!((s, gone["result"]["state"].as_str()), (200, Some("discarded")), "{gone}");
    let (_, v) = op(&base, "bob", "draft.list", json!({ "repo": "acme/site" })).await;
    assert!(ids(&v, "drafts").is_empty(), "{v}");
}

/// Sends are a person's (M168's `send_tests` send them); versions and
/// names are checked.
#[tokio::test]
async fn sends_and_versions() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);

    let sends = [
        ("draft.send", json!({ "draft": "dft_1" })),
        ("pr.comment", json!({ "repo": "acme/app", "number": 1, "body": "x" })),
        ("pr.review", json!({ "repo": "acme/app", "number": 1, "event": "APPROVE" })),
        ("issue.create", json!({ "repo": "acme/app", "title": "x" })),
        ("issue.comment", json!({ "repo": "acme/site", "number": 2, "body": "x" })),
    ];
    for (name, args) in sends {
        let (s, v) = machine_op(&base, "alice geek", name, args.clone()).await;
        assert_eq!((s, v["reason"].as_str()), (403, Some("agents_draft")), "{name}: {v}");
        // A person's goes through one of their machines (M168): none is
        // awake here, so it's refused and what it says waits as a draft.
        let (s, v) = op(&base, "alice", name, args).await;
        assert!(s == 409 || s == 404, "{name}: {s} {v}");
    }

    let at = |v: u64| json!({ "op": "pr.get", "v": v, "repo": "acme/app", "number": 1 });
    let (s, v) = call(&base, "bob", "POST", "/api/forge/ops", Some(at(2))).await;
    assert!(s == 400 && v["error"].as_str().unwrap().contains("speaks v1"), "{v}");
    let (s, _) = call(&base, "bob", "POST", "/api/forge/ops", Some(at(0))).await;
    assert_eq!(s, 400);
    let (s, v) = call(&base, "bob", "POST", "/api/forge/ops", Some(json!({ "op": "pr.merge", "v": 1 }))).await;
    assert!(s == 400 && v["error"].as_str().unwrap().contains("no forge operation"), "{v}");
    let (s, _) = call(&base, "bob", "POST", "/api/forge/ops", Some(json!({ "op": "pr.get", "v": 1 }))).await;
    assert_eq!(s, 400, "missing arguments");
    let (s, _) = call(&base, "nobody", "POST", "/api/forge/ops", Some(at(1))).await;
    assert!(s == 401 || s == 404, "{s}");
}

/// The inbox (M171) as operations: `inbox.list` for a person or a machine
/// (for its owner), and `inbox.mark` a person's only.
#[tokio::test]
async fn the_inbox_as_operations() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    world(&hub, &gh, &app).await;
    hub.daemon("geek", "bob", None);
    super::inbox::recompute(&app, 77, &[1], true).await.unwrap();

    let (s, v) = op(&base, "bob", "inbox.list", json!({})).await;
    assert_eq!((s, v["result"]["entries"][0]["reason"].as_str()), (200, Some("review_requested")), "{v}");
    let (s, v) = machine_op(&base, "bob geek", "inbox.list", json!({})).await;
    assert_eq!((s, v["result"]["entries"][0]["title"].as_str()), (200, Some("secret plan")), "{v}");
    let (s, v) = op(&base, "carol", "inbox.list", json!({})).await;
    assert_eq!((s, &v["result"]["entries"]), (200, &json!([])), "{v}");

    let done = json!({ "repo": "acme/app", "number": 1, "done": true });
    let (s, v) = machine_op(&base, "bob geek", "inbox.mark", done.clone()).await;
    assert_eq!(s, 403, "{v}");
    let (s, v) = op(&base, "bob", "inbox.mark", done.clone()).await;
    assert_eq!((s, v["result"]["marked"].as_u64()), (200, Some(1)), "{v}");
    let (s, _) = op(&base, "carol", "inbox.mark", done).await;
    assert_eq!(s, 404, "a repo she may not read");
    let (_, v) = op(&base, "bob", "inbox.list", json!({})).await;
    assert_eq!(v["result"]["entries"], json!([]), "{v}");
    let (_, v) = op(&base, "bob", "inbox.list", json!({ "done": true })).await;
    assert!(v["result"]["entries"][0]["done_at"].is_u64(), "{v}");
}

/// Plane's two connections, one in the team's project (`ALL`, "Allcorp
/// Secret Roadmap") and one in eve's (`EVE`): their items, newest first.
fn plane_world(app: &crate::Platform, team: &str, eve: &str) {
    use crate::{db::trackers::NewConnection, trackers::fake::item};
    let put = |project: &str, id: &str, key: &str, name: &str, items: Vec<crate::trackers::Item>| {
        let n = NewConnection {
            id: id.into(),
            project_id: project.into(),
            workspace_slug: "acme-ws".into(),
            source_project_id: format!("src-{key}"),
            identifier: key.into(),
            name: name.into(),
            linked_by: "alice".into(),
        };
        let c = app.db.connect_tracker(&n, 1).unwrap().unwrap();
        app.db
            .tracker_mirror(id, None, |tx| {
                for it in &items {
                    crate::db::trackers::upsert_item(tx, id, c.mirror_id, it, 1)?;
                }
                Ok(())
            })
            .unwrap();
    };
    put(
        team,
        "tc_all",
        "ALL",
        "Allcorp Secret Roadmap",
        vec![
            item(2212, "Fix login timeout, after ALL-7", "started", 50),
            item(7, "Docs for sign in", "completed", 45),
            item(8, "Login page copy", "unstarted", 5),
        ],
    );
    put(team, "tc_ops", "OPS", "Ops", vec![item(8, "Ops rota", "started", 3)]);
    put(eve, "tc_eve", "EVE", "Eve's Plane", vec![item(1, "Eve's login plan", "started", 60)]);
}

fn numbers(v: &Value) -> Vec<(String, u64)> {
    v["result"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            (i["identifier"].as_str().or(i["repo"].as_str()).unwrap_or("?").to_owned(), i["number"].as_u64().unwrap())
        })
        .collect()
}

/// A project's Plane items are listed beside its GitHub ones, found by
/// `IDENT-N` and by title, and read one at a time; a reader outside the
/// project gets none of them, nor the connection's name.
#[tokio::test]
async fn plane_items_are_listed_found_and_read_by_members_only() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, eve) = world(&hub, &gh, &app).await;
    plane_world(&app, &team, &eve);

    // Merged, newest first; GitHub's items keep their shape and Plane's carry theirs.
    let (s, v) = op(&base, "bob", "item.list", json!({ "project": team })).await;
    assert_eq!(s, 200, "{v}");
    let got = numbers(&v);
    assert_eq!(
        got,
        [("ALL", 2212), ("ALL", 7), ("acme/app", 1), ("acme/site", 2), ("acme/site", 3), ("ALL", 8), ("OPS", 8)]
            .map(|(k, n)| (k.to_owned(), n))
    );
    let p = &v["result"]["items"][0];
    assert_eq!(
        (p["provider"].as_str(), p["repo"].is_null(), p["state_group"].as_str()),
        (Some("plane"), true, Some("started"))
    );
    assert_eq!(p["url"], "https://app.plane.so/acme-ws/browse/ALL-2212/");
    assert_eq!(v["result"]["items"][2]["provider"], "github");

    // `IDENT-N` first, then titles; filters narrow Plane as they do GitHub.
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "plane", "q": "ALL-2212" })).await;
    assert_eq!(numbers(&v), [("ALL".to_owned(), 2212)], "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "q": "all-8" })).await;
    assert_eq!(numbers(&v), [("ALL".to_owned(), 8)], "the key counts, not only the number: {v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "q": "ALL-7" })).await;
    assert_eq!(numbers(&v), [("ALL".to_owned(), 7), ("ALL".to_owned(), 2212)], "the exact one leads: {v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "q": "ops" })).await;
    assert_eq!(numbers(&v), [("OPS".to_owned(), 8)], "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "plane", "q": "login" })).await;
    assert_eq!(numbers(&v), [("ALL".to_owned(), 2212), ("ALL".to_owned(), 8)], "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "plane", "q": "login" })).await;
    assert_eq!(numbers(&v), [("ALL".to_owned(), 2212), ("ALL".to_owned(), 8)].to_vec(), "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "tracker": "ALL", "state": "closed" })).await;
    assert_eq!(numbers(&v), [("ALL".to_owned(), 7)], "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "tracker": "NOPE" })).await;
    assert_eq!(numbers(&v), vec![], "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "plane", "limit": 2 })).await;
    assert_eq!(numbers(&v).len(), 2, "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "github" })).await;
    assert!(numbers(&v).iter().all(|(k, _)| k != "ALL"), "{v}");
    let (_, v) = op(&base, "bob", "item.list", json!({ "repo": "acme/site" })).await;
    assert!(numbers(&v).iter().all(|(k, _)| k != "ALL"), "a repo is GitHub's: {v}");
    let (s, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "jira" })).await;
    assert_eq!(s, 400, "{v}");

    // A viewer who didn't sign in with GitHub reads Plane's items, as the board's picker lets her.
    let (_, v) = op(&base, "carol", "item.list", json!({ "project": team, "provider": "plane" })).await;
    assert_eq!(numbers(&v).len(), 4, "{v}");

    // One item.
    let (s, v) = op(&base, "bob", "tracker.get", json!({ "project": team, "key": "ALL", "number": 2212 })).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["result"]["item"]["title"], "Fix login timeout, after ALL-7");
    assert!(v["result"]["note"].as_str().unwrap().contains("aren't mirrored"), "{v}");
    let (s, _) = op(&base, "bob", "tracker.get", json!({ "project": team, "key": "ALL", "number": 99 })).await;
    assert_eq!(s, 404);
    let (s, _) = op(&base, "bob", "tracker.get", json!({ "project": team, "key": "EVE", "number": 1 })).await;
    assert_eq!(s, 404, "another project's key");

    // Outside the project: no Plane items, and no connection named.
    let secret = |v: &Value| {
        let t = v.to_string();
        ["Fix login", "Allcorp", "Docs for sign in"].iter().any(|w| t.contains(w))
    };
    for (what, args) in [
        ("item.list", json!({ "project": team })),
        ("item.list", json!({ "project": team, "provider": "plane", "tracker": "ALL", "q": "ALL-2212" })),
        ("tracker.get", json!({ "project": team, "key": "ALL", "number": 2212 })),
    ] {
        let (s, v) = op(&base, "eve", what, args).await;
        assert_eq!(s, 404, "{what}: {v}");
        assert!(!secret(&v), "{what} leaks: {v}");
    }
    // Her own project holds hers, and none of the team's.
    let (s, v) = op(&base, "eve", "item.list", json!({ "provider": "plane" })).await;
    assert_eq!((s, numbers(&v)), (200, vec![("EVE".to_owned(), 1)]), "{v}");
    let (_, v) = op(&base, "eve", "item.list", json!({ "tracker": "ALL" })).await;
    assert_eq!(numbers(&v), vec![], "{v}");
    assert!(!secret(&v), "{v}");

    // A disconnected connection reads nothing, to anyone.
    app.db.disconnect_tracker(&team, "tc_all", 2).unwrap().unwrap();
    let (_, v) = op(&base, "bob", "item.list", json!({ "project": team, "provider": "plane" })).await;
    assert_eq!(numbers(&v), [("OPS".to_owned(), 8)], "only the live connection: {v}");
    let (s, _) = op(&base, "bob", "tracker.get", json!({ "project": team, "key": "ALL", "number": 2212 })).await;
    assert_eq!(s, 404);
}
