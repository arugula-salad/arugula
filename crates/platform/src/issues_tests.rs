//! Native issues through the platform's routes (M195), with the hub of
//! chat's tests: each role on a team's project and on a personal one,
//! patches, lists, export and the issue home.

use std::sync::Arc;

use arugula_control_wire::issues::IssueExport;
use arugula_e2e::team::TeamRole;
use serde_json::{Value, json};

use super::*;
use crate::{
    chat_tests::{TestHub, call, serve},
    db::sql::{Sql as _, params},
};

/// Team t: a owns it, e edits, v views. c is in team u, not t. Chat (and
/// so issues) is on for both teams. t's project id, and a's own.
fn seed(app: &Platform, hub: &TestHub) -> (String, String) {
    hub.team("t", &[("a", TeamRole::Owner), ("e", TeamRole::Editor), ("v", TeamRole::Viewer)]);
    hub.team("u", &[("c", TeamRole::Owner)]);
    hub.flag("team:t", true);
    hub.flag("team:u", true);
    (app.db.team_project("t", "t", 1).unwrap(), app.db.personal_project("a", 1).unwrap())
}

async fn new_issue(base: &str, who: &str, project: &str, body: Value) -> (u16, Value) {
    call(base, who, "POST", &format!("/api/projects/{project}/issues"), Some(body)).await
}

async fn patch(base: &str, who: &str, id: &str, body: Value) -> (u16, Value) {
    call(base, who, "PATCH", &format!("/api/issues/{id}"), Some(body)).await
}

async fn get(base: &str, who: &str, path: &str) -> (u16, Value) {
    call(base, who, "GET", path, None).await
}

/// A GET's status and body as text (an export in Markdown).
async fn text(base: &str, who: &str, path: &str) -> (u16, String, String) {
    let r = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .header("cookie", format!("ilg_session=session-{who}"))
        .send()
        .await
        .unwrap();
    let kind = r.headers().get("content-type").map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default();
    (r.status().as_u16(), kind, r.text().await.unwrap())
}

/// Owners and editors open, edit and close; a viewer reads, searches,
/// exports, and is refused changes; someone outside the team gets 404
/// everywhere, as if the project weren't there.
#[tokio::test]
async fn each_role_on_a_teams_issues() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);

    let (st, one) = new_issue(&base, "a", &p, json!({ "title": "The canary fails", "body": "on deploy" })).await;
    assert_eq!(
        (st, &one["number"], &one["state"], &one["author"]),
        (200, &json!(1), &json!("open"), &json!("a")),
        "{one}"
    );
    let id = one["id"].as_str().unwrap().to_owned();
    let (st, two) = new_issue(&base, "e", &p, json!({ "title": "Flaky test" })).await;
    assert_eq!((st, &two["number"]), (200, &json!(2)), "an editor opens one: {two}");
    let (st, v) = new_issue(&base, "v", &p, json!({ "title": "nope" })).await;
    assert_eq!(st, 403, "a viewer doesn't: {v}");
    assert_eq!(new_issue(&base, "c", &p, json!({ "title": "nope" })).await.0, 404);

    for who in ["a", "e", "v"] {
        let (st, v) = get(&base, who, &format!("/api/projects/{p}/issues")).await;
        assert_eq!((st, v["issues"].as_array().map(Vec::len)), (200, Some(2)), "{who}: {v}");
        assert_eq!(get(&base, who, &format!("/api/issues/{id}")).await.1["title"], json!("The canary fails"));
        assert_eq!(get(&base, who, &format!("/api/projects/{p}/issues/1")).await.1["id"], json!(id));
        let (st, v) = get(&base, who, &format!("/api/projects/{p}/issues?q=canary")).await;
        assert_eq!((st, &v["issues"][0]["number"]), (200, &json!(1)), "{who} searches: {v}");
        assert_eq!(get(&base, who, &format!("/api/issues/{id}/events")).await.0, 200);
        assert_eq!(get(&base, who, &format!("/api/projects/{p}/issues/export")).await.0, 200);
        assert_eq!(get(&base, who, &format!("/api/projects/{p}/issue-home")).await.0, 200);
    }
    for path in [
        format!("/api/projects/{p}/issues"),
        format!("/api/projects/{p}/issues/1"),
        format!("/api/projects/{p}/issues?q=canary"),
        format!("/api/issues/{id}"),
        format!("/api/issues/{id}/events"),
        format!("/api/projects/{p}/issues/export"),
        format!("/api/projects/{p}/issue-home"),
    ] {
        assert_eq!(get(&base, "c", &path).await.0, 404, "{path}");
    }

    let (st, v) = patch(&base, "e", &id, json!({ "state": "closed" })).await;
    assert_eq!((st, &v["issue"]["close_reason"]), (200, &json!("completed")), "the reason by default: {v}");
    assert_eq!(v["events"][0]["kind"], json!("closed"));
    assert_eq!(patch(&base, "v", &id, json!({ "state": "open" })).await.0, 403, "a viewer doesn't reopen");
    assert_eq!(patch(&base, "c", &id, json!({ "state": "open" })).await.0, 404);
    let (st, v) = patch(&base, "a", &id, json!({ "state": "open" })).await;
    assert_eq!((st, &v["issue"]["state"]), (200, &json!("open")), "the owner reopens: {v}");

    let home = Some(json!({ "home": "native" }));
    let path = format!("/api/projects/{p}/issue-home");
    assert_eq!(call(&base, "v", "PUT", &path, home.clone()).await.0, 403);
    assert_eq!(call(&base, "c", "PUT", &path, home.clone()).await.0, 404);
    assert_eq!(call(&base, "e", "PUT", &path, home).await.0, 200);
    assert_eq!(get(&base, "a", "/api/issues/iss_0123456789abcdef0123456789abcdef").await.0, 404);
    assert_eq!(get(&base, "a", "/api/projects/prj_nope/issues").await.0, 404);
}

/// A personal project's issues are its account's alone: by its id or by
/// `personal`; a teammate gets 404.
#[tokio::test]
async fn a_personal_projects_issues_are_its_accounts() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (_, own) = seed(&app, &hub);
    let (st, i) = new_issue(&base, "a", "personal", json!({ "title": "mine", "assignees": ["a"] })).await;
    assert_eq!((st, &i["project"], &i["assignees"]), (200, &json!(own), &json!(["a"])), "{i}");
    let id = i["id"].as_str().unwrap();
    assert_eq!(get(&base, "a", &format!("/api/projects/{own}/issues")).await.1["issues"][0]["id"], json!(id));
    assert_eq!(patch(&base, "a", id, json!({ "labels": { "add": ["p1"] } })).await.0, 200);
    let (st, v) = new_issue(&base, "a", "personal", json!({ "title": "x", "assignees": ["e"] })).await;
    assert_eq!(st, 400, "only the project's people are assigned: {v}");
    for who in ["e", "c"] {
        assert_eq!(get(&base, who, &format!("/api/projects/{own}/issues")).await.0, 404, "{who}");
        assert_eq!(get(&base, who, &format!("/api/issues/{id}")).await.0, 404, "{who}");
        assert_eq!(patch(&base, who, id, json!({ "title": "theirs" })).await.0, 404, "{who}");
        assert_eq!(new_issue(&base, who, &own, json!({ "title": "x" })).await.0, 404, "{who}");
        assert_eq!(get(&base, who, &format!("/api/projects/{own}/issues/export")).await.0, 404, "{who}");
    }
    // `personal` is each reader's own.
    let (st, v) = get(&base, "e", "/api/projects/personal/issues").await;
    assert_eq!((st, v["issues"].as_array().map(Vec::len)), (200, Some(0)), "{v}");
}

/// Issues are a Labs feature, with chat.
#[tokio::test]
async fn issues_come_with_chat() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    hub.flag("account:e", false);
    let (st, v) = get(&base, "e", &format!("/api/projects/{p}/issues")).await;
    assert_eq!((st, &v["reason"]), (403, &json!(crate::chat::CHAT_OFF)), "{v}");
}

/// What a patch takes and what it refuses; lists filter and page.
#[tokio::test]
async fn patches_lists_and_what_they_refuse() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    for n in 1..=5 {
        let labels = if n % 2 == 0 { json!(["bug"]) } else { json!([]) };
        new_issue(&base, "a", &p, json!({ "title": format!("issue {n}"), "labels": labels })).await;
    }
    let id = get(&base, "a", &format!("/api/projects/{p}/issues/2")).await.1["id"].as_str().unwrap().to_owned();

    let (st, v) = patch(
        &base,
        "e",
        &id,
        json!({ "title": "  renamed  ", "state": "closed", "reason": "not_planned",
                "labels": { "remove": ["bug"] }, "assignees": { "set": ["v", "e"] } }),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (&v["issue"]["title"], &v["issue"]["close_reason"], &v["issue"]["labels"], &v["issue"]["assignees"]),
        (&json!("renamed"), &json!("not_planned"), &json!([]), &json!(["v", "e"]))
    );
    let kinds: Vec<&str> = v["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["edited", "closed", "unlabelled", "assigned", "assigned"]);
    assert!(v["events"].as_array().unwrap().iter().all(|e| e["actor"] == json!("e") && e["agent"].is_null()));
    let (_, ev) = get(&base, "v", &format!("/api/issues/{id}/events")).await;
    assert_eq!(ev["events"].as_array().unwrap().len(), 6, "opened, then the five: {ev}");

    for (body, why) in [
        (json!({ "title": " " }), "an empty title"),
        (json!({ "title": "x".repeat(257) }), "a long title"),
        (json!({ "body": "x".repeat(65_537) }), "a long body"),
        (json!({ "state": "shut" }), "a state"),
        (json!({ "reason": "duplicate" }), "a reason without closing"),
        (json!({ "state": "closed", "reason": "bored" }), "a reason"),
        (json!({ "labels": { "add": ["a,b"] } }), "a label with a comma"),
        (json!({ "assignees": { "add": ["c"] } }), "someone outside the team"),
    ] {
        assert_eq!(patch(&base, "a", &id, body).await.0, 400, "{why}");
    }
    assert_eq!(patch(&base, "a", &id, json!({ "assignees": { "remove": ["c"] } })).await.0, 200, "taking off anyone");

    let numbers = |v: &Value| -> Vec<u64> {
        v["issues"].as_array().unwrap().iter().map(|i| i["number"].as_u64().unwrap()).collect()
    };
    let list = |q: &str| {
        let path = format!("/api/projects/{p}/issues?{q}");
        let base = base.clone();
        async move { get(&base, "v", &path).await }
    };
    assert_eq!(numbers(&list("").await.1), [5, 4, 3, 1], "open ones by default");
    assert_eq!(numbers(&list("state=closed").await.1), [2]);
    assert_eq!(numbers(&list("state=all").await.1), [5, 4, 3, 2, 1]);
    assert_eq!(numbers(&list("labels=bug").await.1), [4]);
    assert_eq!(numbers(&list("state=all&assignee=v").await.1), [2]);
    assert_eq!(numbers(&list("author=a&q=issu*").await.1), [5, 4, 3, 1]);
    let (_, page) = list("state=all&limit=2").await;
    assert_eq!((numbers(&page), &page["more"]), (vec![5, 4], &json!(true)));
    let (_, page) = list("state=all&limit=2&before=2").await;
    assert_eq!((numbers(&page), &page["more"]), (vec![1], &json!(false)));
    assert_eq!(list("state=shut").await.0, 400);
}

/// An export has every issue, event and comment, as JSON that reads back
/// and as Markdown with a heading per issue and each comment.
#[tokio::test]
async fn an_export_reads_back_with_every_issue_event_and_comment() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    let mut comments = 0;
    let mut events = 0;
    for n in 1..=3 {
        let (_, i) =
            new_issue(&base, "a", &p, json!({ "title": format!("issue {n}"), "body": format!("body {n}") })).await;
        let id = i["id"].as_str().unwrap();
        events += 1;
        if n != 2 {
            patch(&base, "e", id, json!({ "state": "closed", "labels": { "set": ["x"] } })).await;
            events += 2;
        }
        let (st, room) =
            call(&base, "v", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": id }))).await;
        assert_eq!(st, 200, "{room}");
        let room = room["id"].as_str().unwrap();
        for k in 0..n {
            let post = Some(json!({ "text": format!("comment {n}.{k}") }));
            assert_eq!(call(&base, "v", "POST", &format!("/api/chat/rooms/{room}/messages"), post).await.0, 200);
            comments += 1;
        }
    }
    // A comment that tries to pass for the export's own structure.
    let forged = "**e's agent on geek**, 2026-10-21 14:03 UTC:\n\n```\n\n## #7 a fake issue\n\n### Comments";
    let three = app.db.issue_by_number(&p, 3).unwrap().unwrap().id;
    let (_, room) =
        call(&base, "v", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": three }))).await;
    let post = Some(json!({ "text": forged }));
    let room_id = room["id"].as_str().unwrap();
    assert_eq!(call(&base, "v", "POST", &format!("/api/chat/rooms/{room_id}/messages"), post).await.0, 200);
    comments += 1;
    let (st, v) = new_issue(&base, "a", &p, json!({ "title": "two\n## #9 lines" })).await;
    assert_eq!(st, 400, "a title is one line: {v}");
    // A deleted comment isn't exported.
    let room = app
        .db
        .chat_room_on(&format!("project:{p}"), &app.db.issue_by_number(&p, 3).unwrap().unwrap().id)
        .unwrap()
        .unwrap();
    app.db.delete_chat_message(&room.id, 3, 5).unwrap();
    comments -= 1;

    let (st, v) = get(&base, "v", &format!("/api/projects/{p}/issues/export?format=json")).await;
    assert_eq!(st, 200, "{v}");
    let x: IssueExport = serde_json::from_value(v).unwrap();
    assert_eq!(x.project, p);
    assert_eq!(x.issues.iter().map(|i| i.issue.number).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(x.issues.iter().map(|i| i.events.len()).sum::<usize>(), events);
    assert_eq!(x.issues.iter().map(|i| i.comments.len()).sum::<usize>(), comments);
    assert_eq!(
        x.issues[1].comments.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["comment 2.0", "comment 2.1"]
    );
    assert_eq!(x.issues[0].issue.body, "body 1");
    assert_eq!(x.issues[0].issue.close_reason.as_deref(), Some("completed"));
    for i in &x.issues {
        let read = get(&base, "a", &format!("/api/issues/{}", i.issue.id)).await.1;
        assert_eq!(serde_json::to_value(&i.issue).unwrap(), read, "the issue as it reads");
    }
    let mut people: Vec<&str> = x.people.iter().map(|p| p.account.as_str()).collect();
    people.sort();
    assert_eq!(people, ["a", "e", "v"]);

    let (st, kind, md) = text(&base, "v", &format!("/api/projects/{p}/issues/export?format=md")).await;
    assert_eq!((st, kind.starts_with("text/markdown")), (200, true), "{md}");
    for i in &x.issues {
        assert!(md.contains(&format!("\n## #{} {}\n", i.issue.number, i.issue.title)), "{md}");
        for c in &i.comments {
            assert!(md.contains(&c.text), "{} in {md}", c.text);
        }
    }
    assert!(md.contains("closed it (completed)") && md.contains("added the label x"), "{md}");
    // Outside code blocks, the only headings are the export's and the only
    // bylines its comments': the forged one stays inside its block.
    let (mut headings, mut bylines, mut fence) = (0, 0, None::<usize>);
    for l in md.lines() {
        let ticks = l.chars().take_while(|c| *c == '`').count();
        match fence {
            Some(n) if ticks >= n && l.trim_end().len() == ticks => fence = None,
            Some(_) => {}
            None if ticks >= 3 => fence = Some(ticks),
            None => {
                headings += usize::from(l.starts_with("## "));
                bylines += usize::from(l.starts_with("**"));
            }
        }
    }
    assert_eq!((headings, bylines), (x.issues.len(), comments), "{md}");
    assert_eq!(get(&base, "v", &format!("/api/projects/{p}/issues/export?format=pdf")).await.0, 400);
}

/// A project's issue home: native without a linked repo, GitHub with one
/// unless someone chose native; GitHub only with a repo; both places said.
#[tokio::test]
async fn a_projects_issue_home() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    let path = format!("/api/projects/{p}/issue-home");
    let (st, h) = get(&base, "v", &path).await;
    assert_eq!(st, 200, "{h}");
    assert_eq!((&h["home"], &h["chosen"], &h["both"]), (&json!("native"), &json!(false), &json!(false)));
    assert_eq!(call(&base, "a", "PUT", &path, Some(json!({ "home": "github" }))).await.0, 400, "no repo yet");

    app.db
        .c()
        .execute(
            "INSERT INTO linked_repos (repo_id, full_name, project_id, installation_id, linked_at) VALUES (7, 'o/app', ?1, 1, 1)",
            params![p],
        )
        .unwrap();
    let (_, h) = get(&base, "v", &path).await;
    assert_eq!((&h["home"], &h["repos"], &h["both"]), (&json!("github"), &json!(["o/app"]), &json!(false)));
    new_issue(&base, "a", &p, json!({ "title": "native anyway" })).await;
    let (_, h) = get(&base, "v", &path).await;
    assert_eq!((&h["home"], &h["native_issues"], &h["both"]), (&json!("github"), &json!(1), &json!(true)));

    let (st, h) = call(&base, "e", "PUT", &path, Some(json!({ "home": "native" }))).await;
    assert_eq!((st, &h["home"], &h["chosen"]), (200, &json!("native"), &json!(true)), "{h}");
    assert_eq!(call(&base, "e", "PUT", &path, Some(json!({ "home": "gitlab" }))).await.0, 400);
    let (st, h) = call(&base, "a", "PUT", &path, Some(json!({ "home": null }))).await;
    assert_eq!((st, &h["home"], &h["chosen"]), (200, &json!("github"), &json!(false)), "the default again: {h}");
    let (st, h) = call(&base, "a", "PUT", &path, Some(json!({ "home": "github" }))).await;
    assert_eq!((st, &h["home"], &h["chosen"]), (200, &json!("github"), &json!(true)), "{h}");
    app.db.c().execute("UPDATE linked_repos SET unlinked_at = 5", params![]).unwrap();
    let (_, h) = get(&base, "v", &path).await;
    assert_eq!(
        (&h["home"], &h["chosen"], &h["repos"]),
        (&json!("native"), &json!(false), &json!([])),
        "GitHub chosen, and its repo unlinked since, isn't a home: {h}"
    );
}

#[test]
fn times_read_as_utc() {
    assert_eq!(when(0), "1970-01-01 00:00 UTC");
    assert_eq!(when(1_792_591_380_000), "2026-10-21 14:03 UTC");
}

/// An agent (M197) acts on issues for its owner through the functions
/// M200's tools call: refused without its machine's grant, never past its
/// owner's role, and labelled as theirs in reads, timelines and exports
/// (its comments too).
#[tokio::test]
async fn an_agent_acts_through_its_grant_and_is_labelled() {
    use crate::grants::{AGENT_UNGRANTED, AgentPrincipal};
    use arugula_control_wire::chat::ChatPost;
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    hub.daemon("geek", "a", Some("t"));
    hub.daemon("vbox", "v", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    let geek = Actor { account: "a", agent: Some("geek") };
    let new =
        wire::NewIssue { title: "found by an agent".into(), body: String::new(), labels: vec![], assignees: vec![] };

    let e = create(&app, geek, &p, &new).await.unwrap_err();
    assert_eq!((e.0, e.2), (StatusCode::FORBIDDEN, Some(AGENT_UNGRANTED)), "off by default: {}", e.1);
    let e = list(&app, geek, &p, &Default::default()).await.unwrap_err();
    assert_eq!(e.2, Some(AGENT_UNGRANTED), "reading needs threads.read");

    let give = |who: &'static str, machine: &'static str, power: &'static str| {
        let body = json!({ "daemon": machine, "power": power, "scope": { "kind": "project", "id": p } });
        let base = base.clone();
        async move { call(&base, who, "POST", "/api/grants", Some(body)).await }
    };
    assert_eq!(give("a", "geek", "issues.write").await.0, 200);
    let made = create(&app, geek, &p, &new).await.unwrap();
    assert_eq!(made.author, "a");
    assert_eq!(made.agent.as_ref().map(|a| (a.machine.as_str(), a.machine_name.as_str())), Some(("geek", "geek-box")));
    assert!(list(&app, geek, &p, &Default::default()).await.is_err(), "issues.write isn't threads.read");
    assert_eq!(give("a", "geek", "threads.read").await.0, 200);
    // A machine that only reads doesn't write, though its owner may.
    hub.daemon("reader", "a", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(give("a", "reader", "threads.read").await.0, 200);
    let reader = Actor { account: "a", agent: Some("reader") };
    assert_eq!(read(&app, reader, &made.id).await.unwrap().id, made.id);
    let retitle = wire::IssuePatch { title: Some("retitled by a reader".into()), ..Default::default() };
    let e = change(&app, reader, &made.id, &retitle).await.unwrap_err();
    assert_eq!((e.0, e.2), (StatusCode::FORBIDDEN, Some(AGENT_UNGRANTED)), "threads.read isn't issues.write: {}", e.1);
    assert_eq!(list(&app, geek, &p, &Default::default()).await.unwrap().issues.len(), 1);

    // A viewer's agent never writes, granted or not.
    assert_eq!(give("v", "vbox", "issues.write").await.0, 200);
    let e = create(&app, Actor { account: "v", agent: Some("vbox") }, &p, &new).await.unwrap_err();
    assert_eq!(e.0, StatusCode::FORBIDDEN, "{}", e.1);

    let shut = wire::IssuePatch { state: Some("closed".into()), ..Default::default() };
    let changed = change(&app, geek, &made.id, &shut).await.unwrap();
    assert_eq!(changed.events[0].agent.as_ref().map(|a| a.machine.as_str()), Some("geek"));
    let (_, read) = get(&base, "v", &format!("/api/issues/{}", made.id)).await;
    assert_eq!(read["agent"], json!({ "machine": "geek", "machine_name": "geek-box" }), "{read}");

    // Its comment, in the issue's thread.
    let (_, room) =
        call(&base, "a", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": made.id }))).await;
    let room = room["id"].as_str().unwrap();
    let agent = AgentPrincipal { machine: "geek".into(), machine_name: "geek".into(), owner: "a".into() };
    crate::chat::post_as(
        &app,
        "a",
        Some(&agent),
        room,
        ChatPost { text: "repro attached".into(), ..Default::default() },
    )
    .await
    .unwrap();
    let x = export(&app, Actor::person("v"), &p).await.unwrap();
    let c = &x.issues[0].comments[0];
    assert_eq!((c.text.as_str(), c.agent.as_ref().map(|a| a.machine.as_str())), ("repro attached", Some("geek")));
    assert!(x.issues[0].events.iter().all(|e| e.agent.is_some()), "opened and closed by the agent");
    let md = markdown(&x);
    assert!(md.contains("agent on geek-box"), "{md}");
}

/// A write asks the hub for the team's roster, not the cache: an editor
/// made a viewer is refused at once, though the cache still says editor.
#[tokio::test]
async fn a_demoted_editor_stops_writing_at_once() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    let (_, i) = new_issue(&base, "e", &p, json!({ "title": "mine for now" })).await;
    let id = i["id"].as_str().unwrap();
    hub.team("t", &[("a", TeamRole::Owner), ("e", TeamRole::Viewer), ("v", TeamRole::Viewer)]);
    assert_eq!(patch(&base, "e", id, json!({ "state": "closed" })).await.0, 403);
    assert_eq!(new_issue(&base, "e", &p, json!({ "title": "another" })).await.0, 403);
    assert_eq!(get(&base, "e", &format!("/api/issues/{id}")).await.0, 200, "still reads");
}

/// A change to an issue is heard live by those who see its thread (M203:
/// a pinned issue block, a phone and the page update together); nobody
/// outside the project hears it, and an issue nobody opened a thread on
/// sends nothing.
#[tokio::test]
async fn a_change_is_heard_live_by_the_threads_readers() {
    use std::time::Duration;

    use crate::chat_tests::{hears, open_live};

    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    let (p, _) = seed(&app, &hub);
    let (_, one) = new_issue(&base, "a", &p, json!({ "title": "The canary fails" })).await;
    let id = one["id"].as_str().unwrap().to_owned();
    let v = open_live(&base, "v", None).await;
    let c = open_live(&base, "c", None).await;
    hears(&v, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    hears(&c, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    // No thread yet: nothing to say it on.
    assert_eq!(patch(&base, "a", &id, json!({ "title": "The canary fails twice" })).await.0, 200);
    let (st, room) =
        call(&base, "v", "POST", "/api/chat/targets", Some(json!({ "kind": "object", "target": id }))).await;
    assert_eq!(st, 200, "{room}");
    let room = room["id"].as_str().unwrap().to_owned();
    assert_eq!(patch(&base, "e", &id, json!({ "state": "closed" })).await.0, 200);
    let said = format!("\"type\":\"object\",\"room\":\"{room}\",\"target\":\"{id}\"");
    hears(&v, &said, Duration::from_secs(4)).await;
    assert_eq!(v.lock().unwrap().matches("\"type\":\"object\"").count(), 1, "once, for the change after the thread");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!c.lock().unwrap().contains("\"type\":\"object\""), "c isn't in the project");
}
