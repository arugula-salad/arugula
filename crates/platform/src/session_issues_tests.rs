//! A session's agent and its project's native issues (aru-115, amended):
//! the hosted route reads and comments as the session's asker, in the
//! session's project alone, and only for the machine the session is on.

use arugula_control_wire::work::{SessionIssuesAsk as Ask, SessionIssuesGiven as Given, SessionIssuesOp as Op};
use serde_json::json;

use crate::{
    PlatformPort as _,
    agents_tests::seeded,
    chat_tests::call,
    db::sessions::{NewPrompt, NewSession, Source},
    work::HostedMachine,
};

/// A session of project `project`, asked by `asker`, on a new workspace
/// whose machine is `box1`.
fn session(app: &crate::Platform, project: &str, ws: &str, asker: &str) -> String {
    use crate::db::sql::{Sql as _, params};
    let crate::db::hosted::Added::Made(w) =
        app.db.add_hosted_workspace(ws, project, ws, &Default::default(), asker, 10, 1).unwrap()
    else {
        panic!("no workspace");
    };
    let new = NewSession {
        project,
        room: "ch_general",
        parent_seq: ws.len() as u64 + ws.bytes().last().map_or(0, u64::from),
        asker,
        workspace: &w.id,
        repo: None,
        slug: "look",
        prompt: NewPrompt { author: asker, source: Source::Member, association: None, text: "look" },
    };
    let (s, _) = app.db.create_session(&new, 1).unwrap().unwrap();
    app.db
        .c()
        .execute("UPDATE agent_sessions SET machine = 'box1', state = 'working' WHERE id = ?1", params![s.id])
        .unwrap();
    s.id
}

fn machine(project: &str, ws: &str, machine: &str) -> HostedMachine {
    HostedMachine { project: project.into(), workspace: ws.into(), machine: machine.into(), sign: String::new() }
}

async fn ask(app: &crate::Platform, m: &HostedMachine, ses: &str, op: Op) -> Given {
    app.hosted_session_issues(m, &Ask { session: ses.into(), op }).await.unwrap()
}

fn why(g: &Given) -> &str {
    match g {
        Given::Refused { why } => why,
        g => panic!("not refused: {g:?}"),
    }
}

async fn file(base: &str, by: &str, project: &str, title: &str) -> serde_json::Value {
    let (st, v) =
        call(base, by, "POST", &format!("/api/projects/{project}/issues"), Some(json!({ "title": title }))).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// The asker's agent lists, reads and comments on its own project's
/// issues, and a comment is the session's own (no author, `via`
/// `said:<ses>`, as its answers are), in the issue's thread, where people
/// read it.
#[tokio::test]
async fn a_sessions_agent_reads_and_comments_in_its_project() {
    let (hub, app, base, t, u) = seeded().await;
    hub.flag("team:u", true);
    let one = file(&base, "a", &t, "the canary fails").await;
    file(&base, "c", &u, "u's secret").await;
    let ses = session(&app, &t, "hw_1", "b");
    let m = machine(&t, "hw_1", "box1");

    let Given::List { list } = ask(&app, &m, &ses, Op::List { query: Default::default() }).await else { panic!() };
    let titles: Vec<&str> = list.issues.iter().map(|i| i.title.as_str()).collect();
    assert_eq!((list.project.as_str(), titles), (t.as_str(), vec!["the canary fails"]), "never u's");

    let Given::Shown { issue, events, comments, more_comments } = ask(&app, &m, &ses, Op::Show { number: 1 }).await
    else {
        panic!()
    };
    assert_eq!(
        (issue.id.as_str(), events.len(), comments.len(), more_comments),
        (one["id"].as_str().unwrap(), 1, 0, false)
    );

    let comment = Op::Comment { number: 1, text: "bisected it to Tuesday".into() };
    let Given::Commented { seq } = ask(&app, &m, &ses, comment).await else { panic!() };
    let Given::Shown { comments, .. } = ask(&app, &m, &ses, Op::Show { number: 1 }).await else { panic!() };
    let c = &comments[0];
    let said = arugula_control_wire::sessions::said_via(&ses);
    assert_eq!(
        (c.seq, c.author.as_str(), c.text.as_str(), c.via.as_deref()),
        (seq, "", "bisected it to Tuesday", Some(said.as_str())),
        "the session's, not b's"
    );
    assert!(c.agent.is_none(), "not a machine's grant");

    // It's in the issue's thread, where people read it: the session's.
    let id = one["id"].as_str().unwrap();
    let room = app.db.chat_room_on(&format!("project:{t}"), id).unwrap().expect("the thread");
    let (ms, _) = app.db.chat_messages(&room.id, crate::db::chat::Page::Newest, 10).unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!((ms[0].author.as_str(), ms[0].via.as_deref(), ms[0].agent.clone()), ("", Some(said.as_str()), None));
}

/// Another project's issue is never reached: a number is the session's own
/// project's, and an issue that isn't there is the same as none.
#[tokio::test]
async fn a_session_never_reaches_another_projects_issue() {
    let (hub, app, base, t, u) = seeded().await;
    hub.flag("team:u", true);
    file(&base, "c", &u, "u's secret").await;
    let ses = session(&app, &t, "hw_1", "b");
    let m = machine(&t, "hw_1", "box1");
    for op in [Op::Show { number: 1 }, Op::Comment { number: 1, text: "hi".into() }] {
        assert!(why(&ask(&app, &m, &ses, op).await).contains("no such issue"));
    }
}

/// The asker's role is the ceiling: a viewer's session reads and comments;
/// and nobody's session acts for a person who isn't in the project.
#[tokio::test]
async fn a_session_acts_as_its_asker_and_no_more() {
    let (_hub, app, base, t, _u) = seeded().await;
    file(&base, "a", &t, "one").await;
    let m = machine(&t, "hw_1", "box1");
    let viewer = session(&app, &t, "hw_1", "x");
    assert!(matches!(ask(&app, &m, &viewer, Op::Show { number: 1 }).await, Given::Shown { .. }));
    assert!(matches!(
        ask(&app, &m, &viewer, Op::Comment { number: 1, text: "seen".into() }).await,
        Given::Commented { .. }
    ));
    // Not in the project (c is team u's): it's no project at all for them.
    let stranger = session(&app, &t, "hw_2", "c");
    let m2 = machine(&t, "hw_2", "box1");
    let g = ask(&app, &m2, &stranger, Op::List { query: Default::default() }).await;
    assert!(matches!(g, Given::Refused { .. }), "{g:?}");
}

/// Only the session's own live machine; a review comments on nothing; a
/// note isn't a document.
#[tokio::test]
async fn a_session_call_is_checked_against_the_session() {
    use crate::db::sql::{Sql as _, params};
    let (_hub, app, base, t, u) = seeded().await;
    file(&base, "a", &t, "one").await;
    let ses = session(&app, &t, "hw_1", "b");
    let show = || Op::Show { number: 1 };
    let ok = machine(&t, "hw_1", "box1");
    assert!(matches!(ask(&app, &ok, &ses, show()).await, Given::Shown { .. }));

    for (m, what) in [
        (machine(&t, "hw_1", "box2"), "another machine"),
        (machine(&t, "hw_9", "box1"), "another workspace"),
        (machine(&u, "hw_1", "box1"), "another project"),
    ] {
        assert!(why(&ask(&app, &m, &ses, show()).await).contains("isn't this machine's"), "{what}");
    }
    assert!(why(&ask(&app, &ok, "ses_nope", show()).await).contains("no such session"));

    let long = Op::Comment { number: 1, text: "x".repeat(super::sessions::issues::COMMENT_MAX + 1) };
    assert!(why(&ask(&app, &ok, &ses, long).await).contains("at most"));
    assert!(why(&ask(&app, &ok, &ses, Op::Comment { number: 1, text: "  ".into() }).await).contains("say something"));

    app.db.c().execute("UPDATE agent_sessions SET role = 'review' WHERE id = ?1", params![ses]).unwrap();
    assert!(matches!(ask(&app, &ok, &ses, show()).await, Given::Shown { .. }), "a review reads");
    let c = Op::Comment { number: 1, text: "hi".into() };
    assert!(why(&ask(&app, &ok, &ses, c).await).contains("review"));

    app.db
        .c()
        .execute("UPDATE agent_sessions SET role = 'thread', state = 'stopped' WHERE id = ?1", params![ses])
        .unwrap();
    assert!(why(&ask(&app, &ok, &ses, show()).await).contains("stopped"));
}

/// An agent's comment pings nobody: `@channel`, `@here` and a member's
/// `@login` are text, kept with no mentions, nobody is pushed, and no
/// session starts or is prompted by it.
#[tokio::test]
async fn an_agents_comment_mentions_and_pushes_nobody() {
    use crate::db::sql::Sql as _;
    let (hub, app, base, t, _u) = seeded().await;
    let one = file(&base, "b", &t, "one").await;
    let ses = session(&app, &t, "hw_1", "a");
    let m = machine(&t, "hw_1", "box1");
    let sessions = || app.db.c().query_row("SELECT COUNT(*) FROM agent_sessions", &[], |r| r.get::<i64>(0)).unwrap();
    let (before, pushed) = (sessions(), hub.pushes().len());

    let text = "@channel @here @b @arugula please look".to_owned();
    let Given::Commented { seq } = ask(&app, &m, &ses, Op::Comment { number: 1, text: text.clone() }).await else {
        panic!()
    };
    let room = app.db.chat_room_on(&format!("project:{t}"), one["id"].as_str().unwrap()).unwrap().expect("the thread");
    let (ms, _) = app.db.chat_messages(&room.id, crate::db::chat::Page::Newest, 10).unwrap();
    let c = ms.iter().find(|m| m.seq == seq).expect("in the issue's thread");
    let said = arugula_control_wire::sessions::said_via(&ses);
    assert_eq!((c.author.as_str(), c.text.as_str(), c.via.as_deref()), ("", text.as_str(), Some(said.as_str())));
    assert!(c.mentions.is_empty(), "no mentions: {:?}", c.mentions);
    assert_eq!(hub.pushes().len(), pushed, "nobody was pushed");
    assert_eq!(sessions(), before, "and @arugula started no session");
}

/// A session's agent opens an issue in its project as itself: no author,
/// the session as its agent (named as chat names what it said), its labels
/// from the project's, and a link to its page. Its body's mentions are
/// text: nobody gets an inbox entry or a push, and no session starts.
#[tokio::test]
async fn a_sessions_agent_opens_an_issue_as_itself_and_tells_nobody() {
    use crate::db::sql::Sql as _;
    let (hub, app, base, t, _u) = seeded().await;
    let (st, v) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{t}/issues"),
        Some(json!({ "title": "one", "labels": ["bug", "ci"] })),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let ses = session(&app, &t, "hw_1", "b");
    let m = machine(&t, "hw_1", "box1");
    let count =
        |table: &str| app.db.c().query_row(&format!("SELECT COUNT(*) FROM {table}"), &[], |r| r.get::<i64>(0)).unwrap();
    let (sessions, inbox, pushed) = (count("agent_sessions"), count("inbox_items"), hub.pushes().len());

    let body = format!("@channel @here @a @b @{} the canary flakes", crate::chat::sessions::MENTION);
    let new = Op::Create { title: "the canary flakes".into(), body: body.clone(), labels: vec!["ci".into()] };
    let Given::Created { issue, url } = ask(&app, &m, &ses, new).await else { panic!() };
    assert_eq!(
        (issue.project.as_str(), issue.number, issue.author.as_str(), issue.body.as_str(), issue.labels.clone()),
        (t.as_str(), 2, "", body.as_str(), vec!["ci".to_owned()]),
        "the session's, not b's"
    );
    let agent = issue.agent.clone().expect("the session as its agent");
    let short = &ses["ses_".len().."ses_".len() + 8];
    assert_eq!((agent.machine.as_str(), agent.machine_name), (ses.as_str(), format!("Agent session {short}")));
    assert!(issue.assignees.is_empty());
    assert_eq!(url, format!("{}/issues/{}", app.cfg.page(), issue.id));

    // People read it so, and its timeline's opening is the session's too.
    let (st, v) = call(&base, "a", "GET", &format!("/api/issues/{}", issue.id), None).await;
    assert_eq!((st, &v["author"], &v["agent"]["machine"]), (200, &json!(""), &json!(ses)), "{v}");
    let (_, ev) = call(&base, "a", "GET", &format!("/api/issues/{}/events", issue.id), None).await;
    let opened = &ev["events"][0];
    assert_eq!(
        (&opened["kind"], &opened["actor"], &opened["agent"]["machine"]),
        (&json!("opened"), &json!(""), &json!(ses))
    );

    assert_eq!(count("inbox_items"), inbox, "nobody's inbox");
    assert_eq!(hub.pushes().len(), pushed, "nobody was pushed");
    assert_eq!(count("agent_sessions"), sessions, "and the mention started no session");
}

/// What a session opens is checked as its asker's write and against the
/// session: a viewer's session opens nothing, nor a review, nor an ended
/// one, nor another project's machine; labels are the project's; and it
/// assigns nobody.
#[tokio::test]
async fn a_session_opens_issues_only_with_its_askers_rights() {
    use crate::db::sql::{Sql as _, params};
    let (_hub, app, base, t, u) = seeded().await;
    let (st, _) = call(
        &base,
        "a",
        "POST",
        &format!("/api/projects/{t}/issues"),
        Some(json!({ "title": "one", "labels": ["bug"] })),
    )
    .await;
    assert_eq!(st, 200);
    let new = |labels: &[&str]| Op::Create {
        title: "two".into(),
        body: String::new(),
        labels: labels.iter().map(|l| (*l).to_owned()).collect(),
    };
    let m = machine(&t, "hw_1", "box1");
    let ses = session(&app, &t, "hw_1", "b");

    let w = why(&ask(&app, &m, &ses, new(&["urgent"])).await).to_owned();
    assert!(w.contains("isn't one of this project's labels") && w.contains("bug"), "{w}");
    assert!(matches!(ask(&app, &m, &ses, new(&[])).await, Given::Created { .. }), "no labels is fine");
    assert!(why(&ask(&app, &machine(&u, "hw_1", "box1"), &ses, new(&[])).await).contains("isn't this machine's"));

    let viewer = session(&app, &t, "hw_2", "x");
    assert!(why(&ask(&app, &machine(&t, "hw_2", "box1"), &viewer, new(&[])).await).contains("viewer"));

    app.db.c().execute("UPDATE agent_sessions SET role = 'review' WHERE id = ?1", params![ses]).unwrap();
    assert!(why(&ask(&app, &m, &ses, new(&[])).await).contains("review"));
    app.db
        .c()
        .execute("UPDATE agent_sessions SET role = 'thread', state = 'stopped' WHERE id = ?1", params![ses])
        .unwrap();
    assert!(why(&ask(&app, &m, &ses, new(&[])).await).contains("stopped"));
    assert_eq!(app.db.issue_count(&t).unwrap(), 2, "only the one it was allowed");
}
