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

/// A session on team t1's project (M259), asked for by alice, working in
/// `acme/app` on the workspace whose box is `box1`, with one turn that
/// pushed a commit (`turn` adds more). The session's id.
pub(super) async fn pushed_session(app: &crate::Platform, gh: &super::links_tests::Shared, team: &str) -> String {
    session_on(app, gh, team, "hw_pr", true).await
}

/// [`pushed_session`] on workspace `ws`, its one turn `pushed` or not.
pub(super) async fn session_on(
    app: &crate::Platform,
    gh: &super::links_tests::Shared,
    team: &str,
    ws: &str,
    pushed: bool,
) -> String {
    use crate::db::sessions::{NewPrompt, NewSession, Source};
    {
        // As `arugula-io` is installed (S42 q2): it writes contents and
        // workflows too.
        let mut g = gh.lock().unwrap();
        g.repos.insert("acme/app".into(), (77, true));
        let mut grant = super::links_tests::granted();
        grant["contents"] = "write".into();
        grant["workflows"] = "write".into();
        g.grants.insert("acme/app".into(), grant);
    }
    let crate::db::hosted::Added::Made(ws) =
        app.db.add_hosted_workspace(ws, team, ws, &Default::default(), "alice", 10, 1).unwrap()
    else {
        panic!("no workspace");
    };
    let room = format!("rm_{}", ws.id);
    let new = NewSession {
        project: team,
        room: &room,
        parent_seq: 1,
        asker: "alice",
        workspace: &ws.id,
        repo: Some("acme/app"),
        slug: "add-notes",
        prompt: NewPrompt { author: "alice", source: Source::Member, association: None, text: "add notes" },
    };
    let (s, _) = app.db.create_session(&new, 1).unwrap().unwrap();
    app.db
        .c()
        .execute("UPDATE agent_sessions SET machine = 'box1', state = 'done' WHERE id = ?1", params![s.id])
        .unwrap();
    turn(app, &s.id, pushed);
    s.id
}

/// A turn of session `ses`, as its box reported it: one commit, pushed or
/// not.
pub(super) fn turn(app: &crate::Platform, ses: &str, pushed: bool) {
    let c = app.db.c();
    let seq: u64 =
        c.query_row("SELECT last_seq + 1 FROM agent_sessions WHERE id = ?1", params![ses], |r| r.get(0)).unwrap();
    let detail = json!({ "branch": "x", "head": "abc1234", "pushed": pushed, "commits": ["abc1234 Add notes"],
                         "diffstat": " NOTES.md | 1 +\n 1 file changed, 1 insertion(+)" });
    c.execute(
        "INSERT INTO session_events (session_id, seq, kind, from_state, to_state, at, detail)
         VALUES (?1, ?2, 'turn', 'working', 'working', 5, ?3)",
        params![ses, seq, detail.to_string()],
    )
    .unwrap();
    c.execute("UPDATE agent_sessions SET last_seq = ?2 WHERE id = ?1", params![ses, seq]).unwrap();
}

/// A session's PR (M259): its asker reads it prefilled from the repo's PR
/// template and the turns (the one commit's subject for its title, the
/// commits and diffstat in its body, the repo's default branch for its
/// base), edits it and keeps it. Nobody else drafts it: another member is
/// told only its asker does, and someone who can't read the repo, or isn't
/// in the project, finds no session.
#[tokio::test]
async fn a_sessions_pr_is_drafted_by_its_asker() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    gh.lock().unwrap().files.insert("acme/app:.github/pull_request_template.md".into(), "## What\n\n## Why\n".into());
    let ses = pushed_session(&app, &gh, &team).await;
    let branch = app.db.session(&ses).unwrap().unwrap().branch;
    let path = format!("/api/sessions/{ses}/pr");
    // A later turn whose finish step failed (M261) wasn't pushed: its
    // commit isn't on GitHub, so the PR leaves it out.
    {
        let c = app.db.c();
        let detail = json!({ "branch": "x", "head": "def5678", "pushed": false, "commits": ["def5678 Break the build"],
                             "diffstat": " BROKEN.md | 1 +\n 1 file changed, 1 insertion(+)",
                             "finish": { "ok": false, "command": "just check", "output_tail": "error" } });
        c.execute(
            "INSERT INTO session_events (session_id, seq, kind, from_state, to_state, at, detail)
             SELECT id, last_seq + 1, 'turn', 'working', 'working', 6, ?2 FROM agent_sessions WHERE id = ?1",
            params![ses, detail.to_string()],
        )
        .unwrap();
        c.execute("UPDATE agent_sessions SET last_seq = last_seq + 1 WHERE id = ?1", params![ses]).unwrap();
    }

    let (s, v) = call(&base, "alice", "GET", &path, None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["state"].as_str(), v["id"].as_str()), (Some("new"), None));
    assert_eq!((v["title"].as_str(), v["base"].as_str()), (Some("Add notes"), Some("main")));
    assert_eq!((v["head"].as_str(), v["repo"].as_str()), (Some(branch.as_str()), Some("acme/app")));
    assert_eq!(v["template"], ".github/pull_request_template.md");
    assert_eq!(v["draft"], true);
    let body = v["body"].as_str().unwrap();
    assert!(body.starts_with("## What\n\n## Why\n\n---\n\nOpened from Arugula agent session"), "{body}");
    assert!(body.contains("- abc1234 Add notes") && body.contains("NOTES.md | 1 +"), "{body}");
    assert!(!body.contains("def5678") && !body.contains("BROKEN.md"), "only what's on GitHub: {body}");

    // bob edits the project but didn't ask: he's told; carol (no GitHub
    // sign-in, so no read of the private repo) and eve (not in the
    // project) find no session.
    let (s, v) = call(&base, "bob", "GET", &path, None).await;
    assert_eq!(s, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("only the session's asker"), "{v}");
    for who in ["carol", "eve"] {
        assert_eq!(call(&base, who, "GET", &path, None).await.0, 404, "{who}");
    }
    let edit = json!({ "title": "Notes, at last", "body": "Adds NOTES.md.", "draft": false });
    assert_eq!(call(&base, "bob", "PUT", &path, Some(edit.clone())).await.0, 403);

    // Its asker keeps it, and reads it back as kept.
    let (s, v) = call(&base, "alice", "PUT", &path, Some(edit)).await;
    assert_eq!(s, 200, "{v}");
    assert!(v["id"].as_str().unwrap().starts_with("pdr_"), "{v}");
    assert_eq!(
        (v["state"].as_str(), v["title"].as_str(), v["draft"].as_bool()),
        (Some("waiting"), Some("Notes, at last"), Some(false))
    );
    let (_, again) = call(&base, "alice", "GET", &path, None).await;
    assert_eq!((again["id"].clone(), again["body"].clone()), (v["id"].clone(), json!("Adds NOTES.md.")));
    // A PR into its own branch, or with no title, isn't one.
    let bad = json!({ "title": "x", "body": "", "base": branch });
    assert_eq!(call(&base, "alice", "PUT", &path, Some(bad)).await.0, 400);
    assert_eq!(call(&base, "alice", "PUT", &path, Some(json!({ "title": " ", "body": "" }))).await.0, 400);
}

/// The push token (M259, S42 q2): the platform mints one only for the
/// session's own box, for exactly the session's branch, on its linked repo,
/// with `contents: write` and no `workflows`; a push outside `arugula/*`
/// (`main`), another session's branch, another machine, or a session that
/// ended gets none.
#[tokio::test]
async fn a_push_outside_its_branch_is_refused() {
    use arugula_control_wire::work::{PushAsk, PushGiven};

    use crate::{PlatformPort as _, work::HostedMachine};

    let hub = Arc::new(TestHub::default());
    let (gh, app, _) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let ses = pushed_session(&app, &gh, &team).await;
    let branch = app.db.session(&ses).unwrap().unwrap().branch;
    let m = |machine: &str| HostedMachine {
        project: team.clone(),
        workspace: "hw_pr".into(),
        machine: machine.into(),
        sign: String::new(),
    };
    let ask = |b: &str| PushAsk { session: ses.clone(), branch: b.into() };
    let other = format!("arugula/{}-x", "f".repeat(8));
    for (machine, b) in [("box1", "main"), ("box1", other.as_str()), ("box1", "arugula/"), ("box2", branch.as_str())] {
        let got = app.hosted_session_push(&m(machine), &ask(b)).await.unwrap();
        assert!(matches!(got, PushGiven::Refused { .. }), "{machine} {b}: {got:?}");
    }
    assert!(gh.lock().unwrap().minted.is_empty(), "nothing minted for them");

    let got = app.hosted_session_push(&m("box1"), &ask(&branch)).await.unwrap();
    let PushGiven::Given { token, revoke, .. } = &got else { panic!("{got:?}") };
    assert_eq!(token, "ghs_fake");
    assert!(revoke.ends_with("/installation/token"), "{revoke}");
    assert!(!format!("{got:?}").contains("ghs_fake"), "never printed");
    assert_eq!(gh.lock().unwrap().minted, [json!({ "metadata": "read", "contents": "write" })], "no workflows");
    let logged: Vec<(String, String, String)> = app
        .db
        .c()
        .query_map(
            "SELECT caller, purpose, permissions FROM github_token_mints WHERE kind = 'installation' AND repo_id = 77",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert_eq!((logged[0].0.as_str(), logged[0].1.as_str()), ("alice", "session:push"));
    assert!(!logged[0].2.contains("workflows"), "{logged:?}");

    // Once it ended, no more.
    app.db.c().execute("UPDATE agent_sessions SET state = 'stopped' WHERE id = ?1", params![ses]).unwrap();
    let got = app.hosted_session_push(&m("box1"), &ask(&branch)).await.unwrap();
    assert!(matches!(got, PushGiven::Refused { .. }), "{got:?}");
}
