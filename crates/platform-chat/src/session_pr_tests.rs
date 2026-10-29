//! A session's PR (M259) through the platform's routes, in M165's world
//! (`forge::viewer_tests::world`): its asker drafts it, prefilled from the
//! repo's template and the session's turns, and sends it as themselves; a
//! machine can't, and a box's push outside the session's branch gets no
//! token. Moved from the base's `forge/drafts_tests.rs` and
//! `forge/send_tests.rs` with the sessions (#314).

use std::sync::Arc;

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use serde_json::{Value, json};

use crate::{
    PlatformPort as _,
    chat_tests::{TestHub, call},
    db::{Sql as _, sql::params},
    forge::{
        send::UserToken,
        send_tests::{Logs, posted, user_mints},
        viewer_tests::world,
    },
    testing::forge_serve as serve,
};

const TOKEN: &str = "ghu_alices_one_use_token";

/// A session on team t1's project (M259), asked for by alice, working in
/// `acme/app` on the workspace whose box is `box1`, with one turn that
/// pushed a commit (`turn` adds more). The session's id.
async fn pushed_session(app: &crate::Platform, gh: &crate::forge::links_tests::Shared, team: &str) -> String {
    session_on(app, gh, team, "hw_pr", true).await
}

/// [`pushed_session`] on workspace `ws`, its one turn `pushed` or not;
/// GitHub's compare of its branch has that turn's commit.
async fn session_on(
    app: &crate::Platform,
    gh: &crate::forge::links_tests::Shared,
    team: &str,
    ws: &str,
    pushed: bool,
) -> String {
    let ses = session_with(app, gh, team, ws).await;
    turn(app, &ses, pushed);
    gh.lock().unwrap().files.insert("acme/app:compare-commits".into(), "abc1234 Add notes".into());
    ses
}

/// [`session_on`] with no turn yet.
async fn session_with(app: &crate::Platform, gh: &crate::forge::links_tests::Shared, team: &str, ws: &str) -> String {
    use crate::db::sessions::{NewPrompt, NewSession, Source};
    {
        // As `arugula-io` is installed (S42 q2): it writes contents and
        // workflows too.
        let mut g = gh.lock().unwrap();
        g.repos.insert("acme/app".into(), (77, true));
        let mut grant = crate::forge::links_tests::granted();
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
    s.id
}

/// A turn of session `ses`, as its box reported it: one commit, pushed or
/// not.
fn turn(app: &crate::Platform, ses: &str, pushed: bool) {
    let detail = json!({ "branch": "x", "head": "abc1234", "pushed": pushed, "commits": ["abc1234 Add notes"],
                         "diffstat": " NOTES.md | 1 +\n 1 file changed, 1 insertion(+)" });
    turn_with(app, ses, &detail);
}

/// A turn of session `ses` whose report's detail is `detail`.
fn turn_with(app: &crate::Platform, ses: &str, detail: &Value) {
    let c = app.db.c();
    let seq: u64 =
        c.query_row("SELECT last_seq + 1 FROM agent_sessions WHERE id = ?1", params![ses], |r| r.get(0)).unwrap();
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
    // Its summary under the template's first section, the rest kept, and
    // a footer linking the session.
    assert!(body.starts_with("## What\n\n- Add notes\n\nThe last turn's changes:\n"), "{body}");
    assert!(body.contains("NOTES.md | 1 +") && body.contains("\n```\n\n## Why\n\n---\n\n"), "{body}");
    assert!(
        body.contains(&format!(
            "Opened from Arugula agent session [`{}`](https://app.example/p/{team}/sessions/{ses})",
            &ses[4..12]
        )),
        "{body}"
    );
    assert!(!body.contains("def5678") && !body.contains("Break the build") && !body.contains("BROKEN.md"), "{body}");

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

/// A session whose pushed turns' reports name no commits (they were made
/// in a turn typed in its block, which 0.62.1's box didn't follow) still
/// gets them in its draft, from GitHub's compare of the branch, newest
/// first, with the newest's subject as its title.
#[tokio::test]
async fn a_sessions_pr_reads_its_commits_from_github_when_its_turns_name_none() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let ses = session_with(&app, &gh, &team, "hw_pr").await;
    turn_with(&app, &ses, &json!({ "branch": "x", "head": "abc1234", "pushed": true }));
    let (s, v) = call(&base, "alice", "GET", &format!("/api/sessions/{ses}/pr"), None).await;
    assert_eq!(s, 200, "{v}");
    let body = v["body"].as_str().unwrap();
    assert!(body.starts_with("## Summary\n\n- Add a test for farewell\n- Add farewell(name)\n"), "{body}");
    assert_eq!(v["title"], "Add a test for farewell");

    // On an Arugula issue's thread, the issue's title is the PR's, and the
    // footer links the thread: the issue.
    use crate::db::issues::{Actor, NewIssue};
    let n = NewIssue { project: &team, title: "Add a farewell function", body: "", labels: &[], assignees: &[] };
    let issue = app.db.create_issue(&n, Actor::person("alice"), 1).unwrap().unwrap();
    let room = crate::db::chat::Room {
        id: "rm_hw_pr".into(),
        scope: "team:t1".into(),
        kind: "object".into(),
        name: "issue #1".into(),
        machine: None,
        target: Some(issue.id.clone()),
        created_by: "alice".into(),
        created: 1,
        archived: None,
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &["alice"], Some(&team)).unwrap();
    let (_, v) = call(&base, "alice", "GET", &format!("/api/sessions/{ses}/pr"), None).await;
    assert_eq!(v["title"], "Add a farewell function", "{v}");
    let body = v["body"].as_str().unwrap();
    assert!(body.ends_with(&format!(" in [its thread](https://app.example/issues/{}).\n", issue.id)), "{body}");
}

/// A box that committed, failed to push and was rebuilt lost its commit;
/// the new box made it again and pushed. Both turns' reports name the
/// commit, by different SHAs, but the PR holds one: its draft lists
/// GitHub's compare, so the commit once (0.63.1's live test).
#[tokio::test]
async fn a_sessions_pr_lists_what_github_has_not_a_lost_commit() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let ses = session_with(&app, &gh, &team, "hw_pr").await;
    turn_with(
        &app,
        &ses,
        &json!({ "branch": "x", "head": "dc275ff", "pushed": false, "commits": ["dc275ff Add shout(name)"] }),
    );
    turn_with(
        &app,
        &ses,
        &json!({ "branch": "x", "head": "6561b1e", "pushed": true, "commits": ["6561b1e Add shout(name)"] }),
    );
    gh.lock().unwrap().files.insert("acme/app:compare-commits".into(), "6561b1e Add shout(name)".into());
    let (s, v) = call(&base, "alice", "GET", &format!("/api/sessions/{ses}/pr"), None).await;
    assert_eq!(s, 200, "{v}");
    let body = v["body"].as_str().unwrap();
    assert!(body.starts_with("## Summary\n\n- Add shout(name)\n\n"), "{body}");
    assert_eq!(body.matches("Add shout(name)").count(), 1, "{body}");
}

/// A session's branch that changes a CI workflow (which its push token
/// may, aru-115 amended) gets a warning at the top of its PR's summary,
/// from GitHub's compare of the branch; one that doesn't, none.
#[tokio::test]
async fn a_sessions_pr_says_when_it_changes_ci() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    let ses = pushed_session(&app, &gh, &team).await;
    let path = format!("/api/sessions/{ses}/pr");
    gh.lock().unwrap().files.insert("acme/app:compare-files".into(), "NOTES.md".into());
    let (s, v) = call(&base, "alice", "GET", &path, None).await;
    assert_eq!(s, 200, "{v}");
    assert!(!v["body"].as_str().unwrap().contains("CI workflows"), "{v}");
    gh.lock().unwrap().files.insert("acme/app:compare-files".into(), "NOTES.md\n.github/workflows/test.yml".into());
    let (_, v) = call(&base, "alice", "GET", &path, None).await;
    let body = v["body"].as_str().unwrap();
    assert!(
        body.starts_with(
            "## Summary\n\n**⚠ This PR changes CI workflows** (`.github/workflows/`): check them before merging.\n\n- Add notes\n"
        ),
        "{body}"
    );
}

/// The push token (M259, S42 q2): the platform mints one only for the
/// session's own box, for exactly the session's branch, on its linked repo,
/// with `contents: write` and `workflows: write` (aru-115, amended
/// 2026-10-26: a branch that began before main changed CI needs it); a
/// push outside `arugula/*`
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
    let ask = |b: &str| PushAsk { session: ses.clone(), branch: b.into(), clone: false };
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
    assert_eq!(
        gh.lock().unwrap().minted,
        [json!({ "metadata": "read", "contents": "write", "workflows": "write" })],
        "with workflows"
    );
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
    assert!(logged[0].2.contains(r#""workflows":"write""#), "{logged:?}");

    // A clone of its repo (Fleet 6): the same checks, contents read alone.
    let clone = |b: &str| PushAsk { clone: true, ..ask(b) };
    let got = app.hosted_session_push(&m("box2"), &clone(&branch)).await.unwrap();
    assert!(matches!(got, PushGiven::Refused { .. }), "another machine's: {got:?}");
    let got = app.hosted_session_push(&m("box1"), &clone(&branch)).await.unwrap();
    assert!(matches!(got, PushGiven::Given { .. }), "{got:?}");
    assert_eq!(gh.lock().unwrap().minted[1], json!({ "metadata": "read", "contents": "read" }));

    // Once it ended, no more.
    app.db.c().execute("UPDATE agent_sessions SET state = 'stopped' WHERE id = ?1", params![ses]).unwrap();
    let got = app.hosted_session_push(&m("box1"), &ask(&branch)).await.unwrap();
    assert!(matches!(got, PushGiven::Refused { .. }), "{got:?}");
}

/// A session's PR (M259), opened as its asker: the browser hop first, then
/// the hub hands the one-use user token back (`send_draft` knows a PR
/// draft by its `pdr_`); `GET /user` is alice's GitHub id; the one call is
/// `POST repos/acme/app/pulls` as her, from the session's branch into
/// main, as a draft. The session links the PR, and once the mirror has
/// it, the PR's thread says where it came from, once. The token is in no
/// row and no log line.
#[tokio::test]
async fn a_sessions_pr_is_opened_as_its_asker() {
    let (logs, _guard) = Logs::capture();
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.authorizes(true);
    gh.lock().unwrap().users.insert(TOKEN.into(), 1001);
    let ses = pushed_session(&app, &gh, &team).await;
    let branch = app.db.session(&ses).unwrap().unwrap().branch;

    // Kept and sent in one: GitHub's page, nothing sent yet.
    let edit = json!({ "title": "Add notes", "body": "Adds NOTES.md." });
    let (s, v) = call(
        &base,
        "alice",
        "POST",
        &format!("/api/sessions/{ses}/pr/send"),
        Some(json!({ "edit": edit, "via": "browser", "back": "/p/t1/sessions/x" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let id = v["pr"]["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("pdr_"));
    assert!(v["authorize"].as_str().unwrap().contains(&format!("send.alice.{id}&back=/p/t1/sessions/x")), "{v}");
    assert_eq!(v["pr"]["state"], "waiting");
    assert!(posted(&gh).is_empty());

    // GitHub came back to the hub, which hands the token over.
    let o = app.send_draft("alice", &id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    let url = "https://github.com/repos/acme/app/pulls#posted";
    assert_eq!((o.pr.as_deref(), o.refused.as_deref()), (Some(url), None), "{o:?}");
    let body = json!({ "title": "Add notes", "body": "Adds NOTES.md.", "head": branch, "base": "main", "draft": true });
    assert_eq!(posted(&gh), [("/repos/acme/app/pulls".into(), format!("Bearer {TOKEN}"), body)]);
    assert_eq!(user_mints(&app), [("alice".into(), Some(77), "send:pr".into())]);
    let d = app.db.pr_draft(&id).unwrap().unwrap();
    assert_eq!((d.state.as_str(), d.number, d.sent_url.as_deref()), ("sent", Some(12), Some(url)));
    assert_eq!(app.db.session(&ses).unwrap().unwrap().pr.as_deref(), Some(url), "the card shows the PR");
    let (_, card) = call(&base, "alice", "GET", &format!("/api/sessions/{ses}"), None).await;
    assert_eq!((card["card"]["pr"].as_str(), card["card"]["pushed"].as_bool()), (Some(url), Some(true)), "{card}");
    assert_eq!(card["asked"], true);

    // Sent is sent: again is refused, and nothing goes to GitHub.
    let o = app.send_draft("alice", &id, Some(UserToken::new(TOKEN.into()))).await.unwrap();
    assert!(o.refused.unwrap().contains("sent already"));
    assert_eq!(posted(&gh).len(), 1);

    // The mirror has no PR 12 yet: no thread line. When its webhook's row
    // lands, the PR's thread (made now) gets one, by alice, naming the
    // session; and only one.
    assert_eq!(d.item, None);
    app.db
        .c()
        .execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES ('fi_pr12', 77, 12, 'pr', 'open', 'Add notes', 1, 1, 1)",
            params![],
        )
        .unwrap();
    crate::sessions::pr::noted(&app, 77);
    crate::sessions::pr::noted(&app, 77);
    let room = app.db.chat_room_on(&format!("project:{team}"), "fi_pr12").unwrap().expect("the PR's thread");
    let (said, _) = app.db.chat_messages(&room.id, crate::db::chat::Page::Newest, 50).unwrap();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!((said[0].author.as_str(), said[0].text.as_str()), ("alice", ses.as_str()));
    let system = app.db.chat_system(&room.id, said[0].seq, said[0].seq).unwrap();
    assert_eq!(system[&said[0].seq].kind, "session_pr");

    // Never in a row, never in a log line.
    let rows: Vec<String> = app
        .db
        .c()
        .query_map("SELECT title || body || coalesce(error, '') FROM pr_drafts", params![], |r| r.get(0))
        .unwrap();
    assert!(!format!("{rows:?} {}", crate::forge::send_tests::rows(&app)).contains(TOKEN));
    let logged = logs.text();
    assert!(logged.contains("a session's PR opened as its asker"), "the capture works");
    assert!(!logged.contains(TOKEN), "{logged}");
}

/// A machine can't send a session's PR, grant or not, nor can anyone but
/// its asker; nor can its asker before a turn pushed the branch. With no
/// browser hop, one of the asker's own machines opens it with their `gh`.
#[tokio::test]
async fn a_machine_cant_send_a_sessions_pr() {
    let hub = Arc::new(TestHub::default());
    let (gh, app, base) = serve(&hub).await;
    let (team, _) = world(&hub, &gh, &app).await;
    hub.daemon("geek", "alice", None);
    hub.answers_ops("geek", OpAnswer::Done { url: Some("https://github.com/acme/app/pull/7".into()) });
    let ses = pushed_session(&app, &gh, &team).await;
    let path = format!("/api/sessions/{ses}/pr/send");
    let edit = json!({ "edit": { "title": "Add notes", "body": "b" } });

    let r = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .header("x-test-daemon", "alice geek")
        .json(&edit)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["reason"], "agents_draft", "{v}");
    assert!(app.db.pr_draft_of(&ses).unwrap().is_none(), "nothing kept");
    assert!(hub.ops_asked().is_empty());
    let (s, v) = call(&base, "bob", "POST", &path, Some(edit.clone())).await;
    assert_eq!(s, 403, "{v}");
    assert_eq!(call(&base, "eve", "POST", &path, Some(edit.clone())).await.0, 404);

    // A session whose turns pushed nothing: no PR yet.
    let unpushed = session_on(&app, &gh, &team, "hw_pr2", false).await;
    let (s, v) = call(&base, "alice", "POST", &format!("/api/sessions/{unpushed}/pr/send"), Some(edit.clone())).await;
    assert_eq!(s, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("isn't on GitHub yet"), "{v}");

    // Its asker, with no browser hop: her machine opens it as her.
    let (s, v) = call(&base, "alice", "POST", &path, Some(edit)).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        (v["via"].as_str(), v["pr"]["state"].as_str(), v["pr"]["number"].as_u64()),
        (Some("machine"), Some("sent"), Some(7))
    );
    let asked = hub.ops_asked();
    let DaemonOp::ForgeSend(op) = &asked[0].1 else { panic!("{asked:?}") };
    assert_eq!((asked[0].0.as_str(), op.kind.as_str(), op.account.as_str()), ("geek", "pr", "alice"));
    assert_eq!(
        (op.head.as_deref(), op.base.as_deref(), op.draft_pr),
        (Some(app.db.session(&ses).unwrap().unwrap().branch.as_str()), Some("main"), true)
    );
    assert!(posted(&gh).is_empty(), "nothing as the App");
}
