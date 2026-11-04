//! A project's work (boards, the picker and the inbox), in
//! M165's world (`viewer_tests::world`): issues, PRs and runs in one list,
//! newest first; a repo the reader can't read left out; and untriaged
//! counting only this project's boards.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{links_tests::serve, viewer_tests::world};
use crate::{
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
};

async fn work(base: &str, who: &str, project: &str, query: &str) -> (u16, Vec<Value>) {
    let (s, v) = call(base, who, "GET", &format!("/api/projects/{project}/work{query}"), None).await;
    (s, v["items"].as_array().cloned().unwrap_or_default())
}

/// The pointers' ids of items, in order.
fn ids(items: &[Value]) -> Vec<&str> {
    items.iter().map(|i| i["pointer"]["id"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn one_list_of_issues_prs_and_runs_per_reader() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    {
        let c = app.db.c();
        // Another open issue on the private repo, a new one on the public
        // repo, and a closed one.
        for (id, repo, number, state, at) in
            [("fi_a4", 77u64, 4u64, "open", 25u64), ("fi_a5", 88, 5, "open", 5), ("fi_a6", 88, 6, "closed", 4)]
        {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, author_login, assignees,
                     created_at, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, 'issue', ?4, ?1, 'alice-gh', '[\"bob-gh\"]', 1, ?5, 1)",
                params![id, repo, number, state, at],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO runs (id, project_id, trigger_kind, op, op_hash, daemon, state, started_by, last_seq,
                 created_at, started_at)
             VALUES ('run_1', ?1, 'manual', 'arugula op v1\ndaemon d1\n', 'ab', 'd1', 'running', 'bob', 2, 22, 23)",
            params![team],
        )
        .unwrap();
    }
    let (s, native) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Fix login" })))
            .await;
    assert_eq!(s, 200, "{native}");
    let native = native["id"].as_str().unwrap().to_owned();
    // Assigned to bob and to an account that is gone; and exported to
    // acme/site as #5, whose mirror row (fi_a5) is there too.
    app.db
        .c()
        .execute(
            "UPDATE issues SET assignees = '[\"bob\", \"gone\"]', github_repo = 'acme/site', github_repo_id = 88,
                 github_number = 5 WHERE id = ?1",
            params![native],
        )
        .unwrap();

    // bob reads the private repo too: everything, newest first (the native
    // issue was just made).
    let (s, all) = work(&base, "bob", &team, "").await;
    assert_eq!(s, 200);
    assert_eq!(
        ids(&all),
        [&native, "fi_a1", "fi_a4", "run_1", "fi_a2", "fi_a3", "fi_a6"],
        "an exported issue is listed once: {all:?}"
    );
    assert_eq!(
        (all[0]["source"].as_str(), all[0]["assignees"].clone()),
        (Some("native"), json!(["bob", "someone who left"])),
        "a native issue's assignees are named"
    );
    assert_eq!(
        (all[1]["source"].as_str(), all[1]["kind"].as_str(), all[1]["repo"].as_str(), all[1]["number"].as_u64()),
        (Some("github"), Some("pr"), Some("acme/app"), Some(1))
    );
    assert_eq!(all[2]["assignees"], json!(["bob-gh"]));
    assert_eq!((all[3]["title"].as_str(), all[3]["state"].as_str()), (Some("manual run"), Some("running")));
    assert!(all[3]["repo"].is_null() && all[3]["number"].is_null(), "{:?}", all[3]);

    // carol has no GitHub: acme/app's items are not hers.
    let (s, carol) = work(&base, "carol", &team, "").await;
    assert_eq!(s, 200);
    assert_eq!(ids(&carol), [&native, "run_1", "fi_a2", "fi_a3", "fi_a6"], "{carol:?}");

    // kind, q and limit.
    let (_, prs) = work(&base, "bob", &team, "?kind=pr").await;
    assert_eq!(ids(&prs), ["fi_a1", "fi_a3"]);
    let (_, runs) = work(&base, "bob", &team, "?kind=run").await;
    assert_eq!(ids(&runs), ["run_1"]);
    let (_, issues) = work(&base, "bob", &team, "?kind=issue&limit=3").await;
    assert_eq!(ids(&issues), [&native, "fi_a4", "fi_a2"]);
    let (_, found) = work(&base, "bob", &team, "?q=LOGIN").await;
    assert_eq!(ids(&found), [&native]);
    let (_, found) = work(&base, "bob", &team, "?q=secret").await;
    assert_eq!(ids(&found), ["fi_a1"]);
    let (_, found) = work(&base, "bob", &team, "?q=MAN").await;
    assert_eq!(ids(&found), ["run_1"], "a run matches its trigger's name");
    let (s, _) = work(&base, "bob", &team, "?kind=board").await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn untriaged_is_open_issues_no_board_of_this_project_points_at() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, eve) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    {
        let c = app.db.c();
        for (id, repo, number, state, at) in
            [("fi_a4", 77u64, 4u64, "open", 25u64), ("fi_a5", 88, 5, "open", 5), ("fi_a6", 88, 6, "closed", 4)]
        {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, 'issue', ?4, ?1, 1, ?5, 1)",
                params![id, repo, number, state, at],
            )
            .unwrap();
        }
    }
    // The oldest native issue is loose, with the newer ones in front of it.
    let (_, c) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Older" }))).await;
    let c = c["id"].as_str().unwrap().to_owned();
    let (_, a) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "On a board" })))
            .await;
    let (_, b) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/issues"), Some(json!({ "title": "Loose" }))).await;
    let (a, b) = (a["id"].as_str().unwrap().to_owned(), b["id"].as_str().unwrap().to_owned());

    // Native issues made in one millisecond tie on `updated_at`, and a tie
    // is broken by id: give these three distinct times, newest `b`.
    for (id, at) in [(&c, 9_000_000_000_001u64), (&a, 9_000_000_000_002), (&b, 9_000_000_000_003)] {
        app.db.c().execute("UPDATE issues SET updated_at = ?2 WHERE id = ?1", params![id, at]).unwrap();
    }

    // `a` was exported to acme/site as #7, and its mirror row is there: one
    // piece of work, so the copy is off the inbox with `a`.
    {
        let c = app.db.c();
        c.execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES ('fi_a7', 88, 7, 'issue', 'open', 'fi_a7', 1, 3, 1)",
            params![],
        )
        .unwrap();
        c.execute(
            "UPDATE issues SET github_repo = 'acme/site', github_repo_id = 88, github_number = 7 WHERE id = ?1",
            params![a],
        )
        .unwrap();
    }

    // Nothing on a board: every open issue, and no PR or run.
    let (s, v) = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(s, 200);
    assert_eq!(ids(&v), [&b, &a, &c, "fi_a4", "fi_a2", "fi_a5"], "{v:?}");

    // Eve's board, in another project, points at the team's fi_a2 and a:
    // that triages nothing here.
    {
        let c = app.db.c();
        c.execute(
            "INSERT INTO boards (id, project_id, title, created_by, created_at, updated_at)
             VALUES ('brd_eve', ?1, 'Mine', 'eve', 1, 1)",
            params![eve],
        )
        .unwrap();
        for (n, kind, target) in [(0, "forge_item", "fi_a2"), (1, "issue", a.as_str())] {
            c.execute(
                "INSERT INTO board_entries (id, board_id, kind, position, note, target_kind, target,
                     created_at, updated_at)
                 VALUES (?1, 'brd_eve', 'row', ?2, '', ?3, ?4, 1, 1)",
                params![format!("ber_eve{n}"), n, kind, target],
            )
            .unwrap();
        }
    }
    let (_, v) = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(ids(&v), [&b, &a, &c, "fi_a4", "fi_a2", "fi_a5"], "another project's board triages nothing: {v:?}");

    // The team's own board points at a and fi_a5; closing b takes it out
    // (as does the closed fi_a6, throughout).
    let (_, brd) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Launch" }))).await;
    let id = brd["board"]["id"].as_str().unwrap().to_owned();
    let put = |body: Value| {
        let path = format!("/api/boards/{id}/entries");
        let base = base.clone();
        async move { call(&base, "bob", "PUT", &path, Some(body)).await }
    };
    for ptr in [json!({ "kind": "issue", "id": a }), json!({ "kind": "forge_item", "id": "fi_a5" })] {
        let (s, v) = put(json!({ "entry": "row", "note": "n", "pointer": ptr })).await;
        assert_eq!(s, 200, "{v}");
    }
    let (s, _) = call(&base, "bob", "PATCH", &format!("/api/issues/{b}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(s, 200);
    let (_, v) = work(&base, "bob", &team, "?untriaged=true").await;
    assert_eq!(ids(&v), [&c, "fi_a4", "fi_a2"], "{v:?}");

    // carol can't read acme/app, so fi_a4 isn't in her inbox.
    let (_, v) = work(&base, "carol", &team, "?untriaged=true").await;
    assert_eq!(ids(&v), [&c, "fi_a2"], "{v:?}");
    // A limit is kept after the boards' rows are taken out.
    let (_, v) = work(&base, "bob", &team, "?untriaged=true&limit=1").await;
    assert_eq!(ids(&v), [&c], "{v:?}");
    // A stranger.
    let (s, _) = work(&base, "eve", &team, "").await;
    assert_eq!(s, 404);
}

// ---- the untriaged list at scale (#661)

/// What a thousand-ticket project looks like, in small: 240 mirrored issues
/// with a label or two each, ties in `updated_at` among them, and ten
/// native ones. Each as (id, title, labels).
async fn crowd(app: &crate::Platform, base: &str, team: &str) -> Vec<(String, String, Vec<&'static str>)> {
    let mut all: Vec<(String, String, Vec<&'static str>)> = vec![("fi_a2".into(), "public fix".into(), vec![])];
    let c = app.db.c();
    for i in 0..240u64 {
        let mut labels = vec![];
        if i % 3 == 0 {
            labels.push("bug");
        }
        if i % 5 == 0 {
            labels.push("docs");
        }
        let (id, title) = (format!("fi_p{i:03}"), format!("ticket p{i:03}"));
        c.execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, labels, created_at, updated_at, synced_at)
             VALUES (?1, 88, ?2, 'issue', 'open', ?3, ?4, 1, ?5, 1)",
            params![id, 100 + i, title, serde_json::to_string(&labels).unwrap(), 1000 + i / 3],
        )
        .unwrap();
        all.push((id, title, labels));
    }
    drop(c);
    for n in 0..10 {
        let labels = if n % 2 == 0 { vec!["bug"] } else { vec![] };
        let title = format!("ticket n{n}");
        let (s, v) = call(
            base,
            "bob",
            "POST",
            &format!("/api/projects/{team}/issues"),
            Some(json!({ "title": title, "labels": labels })),
        )
        .await;
        assert_eq!(s, 200, "{v}");
        all.push((v["id"].as_str().unwrap().to_owned(), title, labels));
    }
    all
}

/// Every page of an untriaged query, followed by its cursors: the items in
/// order, and the first page's answer.
async fn paged(base: &str, team: &str, query: &str, limit: u32) -> (Vec<Value>, Value) {
    let (mut items, mut first, mut cursor) = (vec![], None, None::<String>);
    loop {
        let after = cursor.as_deref().map(|c| format!("&cursor={c}")).unwrap_or_default();
        let (s, v) = call(
            base,
            "bob",
            "GET",
            &format!("/api/projects/{team}/work?untriaged=true&limit={limit}{query}{after}"),
            None,
        )
        .await;
        assert_eq!(s, 200, "{v}");
        items.extend(v["items"].as_array().unwrap().iter().cloned());
        cursor = v["next"].as_str().map(str::to_owned);
        first.get_or_insert(v);
        if cursor.is_none() {
            return (items, first.unwrap());
        }
        assert!(items.len() < 1000, "a cursor that never ends");
    }
}

#[tokio::test]
async fn untriaged_pages_hold_their_place_and_count_what_they_list() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    let all = crowd(&app, &base, &team).await;
    let (_, brd) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Launch" }))).await;
    let board = brd["board"]["id"].as_str().unwrap().to_owned();

    // Every item once, newest first, while two leave between pages: one
    // already seen and one still ahead.
    let first = call(&base, "bob", "GET", &format!("/api/projects/{team}/work?untriaged=true&limit=100"), None).await.1;
    let page1 = ids(first["items"].as_array().unwrap()).iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(page1.len(), 100);
    assert_eq!(first["total"], all.len());
    let ahead = "fi_p000";
    assert!(!page1.iter().any(|i| i == ahead), "the oldest is last: {page1:?}");
    for gone in [page1[0].as_str(), ahead] {
        let ptr = if gone.starts_with("iss_") {
            json!({ "kind": "issue", "id": gone })
        } else {
            json!({ "kind": "forge_item", "id": gone })
        };
        let (s, v) = call(
            &base,
            "bob",
            "PUT",
            &format!("/api/boards/{board}/entries"),
            Some(json!({ "entry": "row", "pointer": ptr })),
        )
        .await;
        assert_eq!(s, 200, "{v}");
    }
    let mut seen = page1.clone();
    let mut cursor = first["next"].as_str().map(str::to_owned);
    while let Some(c) = cursor {
        assert!(seen.len() < 1000, "a cursor that never ends");
        let (_, v) =
            call(&base, "bob", "GET", &format!("/api/projects/{team}/work?untriaged=true&limit=100&cursor={c}"), None)
                .await;
        seen.extend(ids(v["items"].as_array().unwrap()).iter().map(|s| (*s).to_owned()));
        cursor = v["next"].as_str().map(str::to_owned);
    }
    let mut once = seen.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), seen.len(), "nothing twice");
    assert_eq!(seen.len(), all.len() - 1, "all but the one that left before it was reached");
    assert!(!seen.iter().any(|i| i == ahead));

    // The total is what paging to the end gives, for each query; the labels
    // count the words' matches whatever `label` says.
    let now: Vec<_> = all.iter().filter(|(id, ..)| id != ahead && *id != page1[0]).collect();
    for (query, text, label) in [
        ("", "", ""),
        ("&q=ticket", "ticket", ""),
        ("&q=ticket&label=bug", "ticket", "bug"),
        ("&label=docs", "", "docs"),
        ("&q=public", "public", ""),
        ("&label=nope", "", "nope"),
    ] {
        let (items, head) = paged(&base, &team, query, 37).await;
        let want: Vec<_> =
            now.iter().filter(|(_, t, l)| t.contains(text) && (label.is_empty() || l.contains(&label))).collect();
        assert_eq!(items.len(), want.len(), "{query}");
        assert_eq!(head["total"], want.len(), "{query}: the total is what paging gives");
        let mut counts = std::collections::BTreeMap::<&str, u64>::new();
        for (_, t, l) in now.iter().filter(|(_, t, _)| t.contains(text)) {
            let _ = t;
            for x in l {
                *counts.entry(x).or_default() += 1;
            }
        }
        let mut facet: Vec<_> = counts.into_iter().collect();
        facet.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let got: Vec<_> = head["labels"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|l| (l["name"].as_str().unwrap().to_owned(), l["count"].as_u64().unwrap()))
            .collect();
        let want_facet: Vec<_> = facet.iter().map(|(n, c)| ((*n).to_owned(), *c)).collect();
        assert_eq!(got, want_facet, "{query}");
    }
    let (items, _) = paged(&base, &team, "&label=bug", 100).await;
    assert!(
        items.iter().all(|i| i["labels"].as_array().unwrap().iter().any(|l| l == "bug")),
        "labels are on the items"
    );

    // Oldest first is the same list backwards, ties and all.
    let (new, _) = paged(&base, &team, "", 50).await;
    let (old, _) = paged(&base, &team, "&order=old", 50).await;
    let mut back = ids(&new).iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    back.reverse();
    let old_ids = ids(&old).iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(old.len(), new.len());
    let at = |v: &[Value]| v.iter().map(|i| i["updated_at"].as_u64().unwrap()).collect::<Vec<_>>();
    assert_eq!(at(&old), at(&new).into_iter().rev().collect::<Vec<_>>());
    assert_eq!(
        old_ids.iter().collect::<std::collections::BTreeSet<_>>(),
        back.iter().collect::<std::collections::BTreeSet<_>>()
    );

    // What is refused, and what a list that isn't untriaged leaves out.
    for q in
        ["?untriaged=true&order=sideways", "?untriaged=true&cursor=nonsense", "?cursor=1:x", "?label=bug", "?order=old"]
    {
        assert_eq!(call(&base, "bob", "GET", &format!("/api/projects/{team}/work{q}"), None).await.0, 400, "{q}");
    }
    let (_, plain) = call(&base, "bob", "GET", &format!("/api/projects/{team}/work"), None).await;
    assert!(plain.get("total").is_none() && plain.get("labels").is_none() && plain["next"].is_null(), "{plain}");
}

#[tokio::test]
async fn add_many_takes_pointers_or_a_query_but_some_and_is_capped() {
    let hub = Arc::new(TestHub::default());
    let (_gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &_gh, &app).await;
    hub.flag("team:t1", true);
    let all = crowd(&app, &base, &team).await;
    let (_, brd) =
        call(&base, "bob", "POST", &format!("/api/projects/{team}/boards"), Some(json!({ "title": "Launch" }))).await;
    let board = brd["board"]["id"].as_str().unwrap().to_owned();
    let many = format!("/api/boards/{board}/entries/many");
    let item = |id: &str| {
        if id.starts_with("iss_") {
            json!({ "kind": "issue", "id": id })
        } else {
            json!({ "kind": "forge_item", "id": id })
        }
    };
    let rows = |v: &Value| {
        v["board"]["zones"].as_array().unwrap().iter().map(|z| z["rows"].as_array().unwrap().len()).sum::<usize>()
    };

    // Pointers: each becomes a row; those there already are not counted.
    let (s, v) =
        call(&base, "bob", "POST", &many, Some(json!({ "pointers": [item("fi_p001"), item("fi_p002")] }))).await;
    assert_eq!((s, v["added"].as_u64(), v["attached"].as_u64(), rows(&v)), (200, Some(2), Some(0), 2), "{v}");
    let (_, v) =
        call(&base, "bob", "POST", &many, Some(json!({ "pointers": [item("fi_p002"), item("fi_p003")] }))).await;
    assert_eq!((v["added"].as_u64(), rows(&v)), (Some(1), 3), "{v}");

    // A query but two: the rest go onto the board and off the list.
    let docs: Vec<_> =
        all.iter().filter(|(id, _, l)| l.contains(&"docs") && id != "fi_p000").map(|(id, ..)| id.clone()).collect();
    let spare = [docs[0].clone(), docs[1].clone()];
    let untriaged = json!({ "project": team, "label": "docs" });
    let (s, v) = call(
        &base,
        "bob",
        "POST",
        &many,
        Some(json!({ "untriaged": untriaged, "except": [item(&spare[0]), item(&spare[1])] })),
    )
    .await;
    // fi_p000 is a docs one too.
    assert_eq!((s, v["added"].as_u64()), (200, Some(docs.len() as u64 - 2 + 1)), "{v}");
    let (items, head) = paged(&base, &team, "&label=docs", 50).await;
    let mut left: Vec<_> = ids(&items).iter().map(|s| (*s).to_owned()).collect();
    left.sort();
    let mut want = spare.to_vec();
    want.sort();
    assert_eq!((left, head["total"].as_u64()), (want, Some(2)), "they left the untriaged list");

    // Refused: a viewer; both forms or neither; except alone; another
    // project; and a query that matches more than a call adds.
    assert_eq!(call(&base, "carol", "POST", &many, Some(json!({ "pointers": [item("fi_p004")] }))).await.0, 403);
    for body in [
        json!({}),
        json!({ "pointers": [], "untriaged": { "project": team } }),
        json!({ "pointers": [item("fi_p004")], "except": [item("fi_p005")] }),
        json!({ "untriaged": { "project": "personal" } }),
    ] {
        assert_eq!(call(&base, "bob", "POST", &many, Some(body.clone())).await.0, 400, "{body}");
    }
    {
        let c = app.db.c();
        for i in 0..520u64 {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
                 VALUES (?1, 88, ?2, 'issue', 'open', 'bulk', 1, 5, 1)",
                params![format!("fi_z{i:03}"), 1000 + i],
            )
            .unwrap();
        }
    }
    let (s, v) =
        call(&base, "bob", "POST", &many, Some(json!({ "untriaged": { "project": team, "q": "bulk" } }))).await;
    assert_eq!(s, 400, "{v}");
    assert!(v["error"].to_string().contains("narrow"), "{v}");
    let over: Vec<_> = (0..501).map(|i| item(&format!("fi_z{i:03}"))).collect();
    assert_eq!(call(&base, "bob", "POST", &many, Some(json!({ "pointers": over }))).await.0, 400);
    let (_, v) = call(&base, "bob", "GET", &format!("/api/boards/{board}"), None).await;
    assert_eq!(rows(&json!({ "board": v })), 3 + docs.len() - 2 + 1, "a refused call adds nothing: {v}");
}
