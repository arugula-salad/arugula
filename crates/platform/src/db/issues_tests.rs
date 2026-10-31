use super::*;

fn project(db: &Db, id: &str) {
    db.c()
        .execute(
            "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES (?1, ?1, ?1, FALSE, 1)",
            params![id],
        )
        .unwrap();
}

fn open(db: &Db, project: &str, title: &str, body: &str, labels: &[&str]) -> Issue {
    let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
    db.create_issue(&NewIssue { project, title, body, labels: &labels, assignees: &[] }, Actor::person("a1"), 10)
        .unwrap()
        .unwrap()
}

fn words(q: &str) -> Vec<Word> {
    q.split_whitespace().map(|w| Word { text: w.trim_end_matches('*').into(), prefix: w.ends_with('*') }).collect()
}

fn numbers(issues: &[Issue]) -> Vec<u64> {
    issues.iter().map(|i| i.number).collect()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 100 issues opened at once in one project get the numbers 1 to 100,
/// each once, and search finds them (M194's done-when). On Postgres they
/// come from two handles, as two of the platform's machines.
#[test]
fn numbers_have_no_gaps_or_duplicates_under_load() {
    let db = Db::test();
    project(&db, "prj_a");
    project(&db, "prj_b");
    let second = db.second();
    let handles: Vec<&Db> = std::iter::once(&db).chain(second.as_ref()).collect();
    let made: Vec<Issue> = std::thread::scope(|s| {
        let threads: Vec<_> = (0..100)
            .map(|i| {
                let db = handles[i % handles.len()];
                s.spawn(move || {
                    let title = format!("flaky deploy {i}");
                    db.create_issue(
                        &NewIssue {
                            project: "prj_a",
                            title: &title,
                            body: "the canary fails",
                            labels: &[],
                            assignees: &[],
                        },
                        Actor::person("a1"),
                        i as u64,
                    )
                    .unwrap()
                    .unwrap()
                })
            })
            .collect();
        threads.into_iter().map(|t| t.join().unwrap()).collect()
    });
    let mut got = numbers(&made);
    got.sort();
    assert_eq!(got, (1..=100).collect::<Vec<u64>>());
    assert!(made.iter().all(|i| i.id.starts_with("iss_") && i.id.len() == 36), "{}", made[0].id);
    let n: u64 =
        db.c().query_row("SELECT last_issue FROM projects WHERE id = 'prj_a'", params![], |r| r.get(0)).unwrap();
    assert_eq!(n, 100);
    // Another project counts from 1.
    assert_eq!(open(&db, "prj_b", "first", "", &[]).number, 1);

    assert_eq!(db.search_issues("prj_a", &words("canary"), 200).unwrap().len(), 100, "every body");
    assert_eq!(numbers(&db.search_issues("prj_a", &words("deploy 42"), 200).unwrap()), vec![made_number(&made, 42)]);
    assert_eq!(db.search_issues("prj_a", &words("flak*"), 3).unwrap().len(), 3);
    assert!(db.search_issues("prj_b", &words("canary"), 10).unwrap().is_empty(), "only the project asked");
    // Every issue's timeline starts with `opened`.
    let one = db.issue_by_number("prj_a", 7).unwrap().unwrap();
    assert_eq!(db.issue(&one.id).unwrap().unwrap(), one);
    let ev = db.issue_events(&one.id).unwrap();
    assert_eq!(ev.iter().map(|e| e.kind).collect::<Vec<_>>(), [EventKind::Opened]);
}

/// The number the thread that titled it `flaky deploy {i}` got.
fn made_number(made: &[Issue], i: usize) -> u64 {
    made.iter().find(|m| m.title == format!("flaky deploy {i}")).unwrap().number
}

#[test]
fn no_project_no_issue() {
    let db = Db::test();
    let new = NewIssue { project: "prj_gone", title: "t", body: "", labels: &[], assignees: &[] };
    assert!(db.create_issue(&new, Actor::person("a1"), 1).unwrap().is_none());
    assert!(db.issue("iss_nope").unwrap().is_none());
    assert!(db.patch_issue("iss_nope", &Patch::default(), Actor::person("a1"), 1).unwrap().is_none());
}

/// Each change is its own event, in the patch's transaction; a patch that
/// changes nothing writes none and leaves `updated_at`.
#[test]
fn patches_write_their_events() {
    let db = Db::test();
    project(&db, "prj_a");
    let i = open(&db, "prj_a", "Crash on start", "it crashes", &["bug"]);
    let agent = Actor { account: "a2", agent: Some("box1") };
    let kinds = |e: &[Event]| e.iter().map(|e| (e.kind, e.detail.clone())).collect::<Vec<_>>();

    let (now, ev) = db
        .patch_issue(
            &i.id,
            &Patch {
                title: Some("Crash on cold boot"),
                body: Some("it crashes"),
                labels: Some(ListEdit::Add(&strings(&["p1", "bug"]))),
                assignees: Some(ListEdit::Set(&strings(&["a2", "a3", "a2"]))),
                ..Default::default()
            },
            agent,
            20,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        kinds(&ev),
        [
            (EventKind::Edited, json!({ "title": "Crash on start" })),
            (EventKind::Labelled, json!({ "label": "p1" })),
            (EventKind::Assigned, json!({ "assignee": "a2" })),
            (EventKind::Assigned, json!({ "assignee": "a3" })),
        ],
        "an unchanged body isn't an edit; labels and assignees are each once"
    );
    assert!(ev.iter().all(|e| e.actor == "a2" && e.actor_agent.as_deref() == Some("box1") && e.at == 20));
    assert_eq!(
        (now.title.as_str(), now.labels.clone(), now.assignees.clone()),
        ("Crash on cold boot", strings(&["bug", "p1"]), strings(&["a2", "a3"]))
    );
    assert_eq!(now.updated_at, 20);
    assert!(search(&db, "start").is_empty(), "an edit leaves no old words in the index");
    assert_eq!(search(&db, "boot"), vec![1]);
    assert_eq!(db.issue(&i.id).unwrap().unwrap(), now);

    let close = Patch { state: Some(State::Closed(CloseReason::NotPlanned)), ..Default::default() };
    let (now, ev) = db.patch_issue(&i.id, &close, Actor::person("a1"), 30).unwrap().unwrap();
    assert_eq!(kinds(&ev), [(EventKind::Closed, json!({ "reason": "not_planned" }))]);
    assert_eq!((now.state, now.closed_at), (State::Closed(CloseReason::NotPlanned), Some(30)));
    let (same, ev) = db.patch_issue(&i.id, &close, Actor::person("a1"), 35).unwrap().unwrap();
    assert!(ev.is_empty() && same.updated_at == 30, "closing a closed issue changes nothing");
    let dup = Patch { state: Some(State::Closed(CloseReason::Duplicate)), ..Default::default() };
    let (now, ev) = db.patch_issue(&i.id, &dup, Actor::person("a1"), 40).unwrap().unwrap();
    assert_eq!(kinds(&ev), [(EventKind::Closed, json!({ "reason": "duplicate" }))]);
    assert_eq!(now.closed_at, Some(30), "a new reason keeps when it closed");
    let by_pr =
        Patch { state: Some(State::Closed(CloseReason::Completed)), by_pr: Some("o/r#9"), ..Default::default() };
    let (now, ev) = db.patch_issue(&i.id, &by_pr, Actor::person("a1"), 45).unwrap().unwrap();
    assert!(
        ev.is_empty() && now.state == State::Closed(CloseReason::Duplicate),
        "a merged PR closes an open issue only"
    );

    let reopen = Patch {
        state: Some(State::Open),
        body: Some("it crashes on a cold start"),
        labels: Some(ListEdit::Remove(&strings(&["bug", "nope"]))),
        assignees: Some(ListEdit::Set(&strings(&["a3"]))),
        ..Default::default()
    };
    let (now, ev) = db.patch_issue(&i.id, &reopen, Actor::person("a1"), 50).unwrap().unwrap();
    assert_eq!(
        kinds(&ev),
        [
            (EventKind::Edited, json!({ "body": true })),
            (EventKind::Reopened, json!({})),
            (EventKind::Unlabelled, json!({ "label": "bug" })),
            (EventKind::Unassigned, json!({ "assignee": "a2" })),
        ]
    );
    assert_eq!((now.state, now.closed_at, now.labels.clone()), (State::Open, None, strings(&["p1"])));
    assert_eq!(search(&db, "start"), vec![1], "the index follows edits");

    let all = db.issue_events(&i.id).unwrap();
    assert_eq!(all.len(), 1 + 4 + 1 + 1 + 4);
    assert!(all.windows(2).all(|w| w[0].id < w[1].id), "oldest first");
    assert_eq!(all[0].kind, EventKind::Opened);
}

fn search(db: &Db, q: &str) -> Vec<u64> {
    numbers(&db.search_issues("prj_a", &words(q), 50).unwrap())
}

/// A list narrows by state, labels (every one), assignee and author,
/// pages newest first, and takes search words too.
#[test]
fn lists_filter_and_page() {
    let db = Db::test();
    project(&db, "prj_a");
    project(&db, "prj_b");
    for n in 1..=6 {
        let labels: &[&str] = match n % 3 {
            0 => &["bug", "p1"],
            1 => &["bug"],
            _ => &[],
        };
        open(&db, "prj_a", &format!("issue {n}"), if n == 5 { "mentions src/main.rs" } else { "" }, labels);
    }
    open(&db, "prj_b", "elsewhere", "", &["bug"]);
    let id = |n| db.issue_by_number("prj_a", n).unwrap().unwrap().id;
    let set = |n, p: Patch| db.patch_issue(&id(n), &p, Actor::person("a1"), 20).unwrap().unwrap();
    set(2, Patch { state: Some(State::Closed(CloseReason::Completed)), ..Default::default() });
    set(3, Patch { state: Some(State::Closed(CloseReason::Completed)), ..Default::default() });
    set(4, Patch { assignees: Some(ListEdit::Set(&strings(&["a9"]))), ..Default::default() });
    db.create_issue(
        &NewIssue { project: "prj_a", title: "by someone else", body: "", labels: &[], assignees: &strings(&["a9"]) },
        Actor::person("a2"),
        30,
    )
    .unwrap();

    let list = |f: Filter, before, limit| {
        let (v, more) = db.issues("prj_a", &f, before, limit).unwrap();
        (numbers(&v), more)
    };
    assert_eq!(list(Filter::default(), None, 50), (vec![7, 6, 5, 4, 3, 2, 1], false));
    assert_eq!(list(Filter { open: Some(true), ..Default::default() }, None, 50), (vec![7, 6, 5, 4, 1], false));
    assert_eq!(list(Filter { open: Some(false), ..Default::default() }, None, 50), (vec![3, 2], false));
    let bug = strings(&["bug"]);
    let bug_p1 = strings(&["bug", "p1"]);
    assert_eq!(list(Filter { labels: &bug, ..Default::default() }, None, 50), (vec![6, 4, 3, 1], false));
    assert_eq!(list(Filter { labels: &bug_p1, ..Default::default() }, None, 50), (vec![6, 3], false));
    assert_eq!(list(Filter { labels: &bug_p1, open: Some(true), ..Default::default() }, None, 50), (vec![6], false));
    assert_eq!(list(Filter { assignee: Some("a9"), ..Default::default() }, None, 50), (vec![7, 4], false));
    assert_eq!(list(Filter { author: Some("a2"), ..Default::default() }, None, 50), (vec![7], false));
    let w = words("main.rs");
    assert_eq!(list(Filter { words: &w, ..Default::default() }, None, 50), (vec![5], false));
    let w = words("issue");
    assert_eq!(list(Filter { words: &w, open: Some(false), ..Default::default() }, None, 50), (vec![3, 2], false));
    let w = words("--");
    assert_eq!(list(Filter { words: &w, ..Default::default() }, None, 50), (vec![], false), "no letters, no hits");
    // Pages.
    assert_eq!(list(Filter::default(), None, 3), (vec![7, 6, 5], true));
    assert_eq!(list(Filter::default(), Some(5), 3), (vec![4, 3, 2], true));
    assert_eq!(list(Filter::default(), Some(2), 3), (vec![1], false));
}

/// An export is every issue in the project with its timeline, by number.
#[test]
fn exports_every_issue_with_its_timeline() {
    let db = Db::test();
    project(&db, "prj_a");
    project(&db, "prj_b");
    let one = open(&db, "prj_a", "one", "", &[]);
    open(&db, "prj_b", "not this", "", &[]);
    let two = open(&db, "prj_a", "two", "", &[]);
    db.patch_issue(&one.id, &Patch { title: Some("uno"), ..Default::default() }, Actor::person("a1"), 20).unwrap();
    let out = db.export_issues("prj_a").unwrap();
    assert_eq!(out.iter().map(|e| e.issue.title.as_str()).collect::<Vec<_>>(), ["uno", "two"]);
    assert_eq!(out[0].events.iter().map(|e| e.kind).collect::<Vec<_>>(), [EventKind::Opened, EventKind::Edited]);
    assert_eq!(out[1].events, db.issue_events(&two.id).unwrap());
    assert!(db.export_issues("prj_none").unwrap().is_empty());
}

/// Deleting an account takes the issues of the projects that go with it;
/// elsewhere its issues stay, naming no one, and it leaves assignees.
#[test]
fn deleting_an_account_takes_its_projects_issues_and_its_name() {
    let db = Db::test();
    let mine = db.personal_project("alice", 1).unwrap();
    project(&db, "prj_team");
    let gone = Actor { account: "alice", agent: Some("alices-box") };
    let new = |project, title, assignees: &[String]| {
        db.create_issue(&NewIssue { project, title, body: "words", labels: &[], assignees }, gone, 1).unwrap().unwrap()
    };
    let private = new(&mine, "my own", &[]);
    let shared = new("prj_team", "the team's", &strings(&["bob"]));
    let bobs = db
        .create_issue(
            &NewIssue { project: "prj_team", title: "bob's", body: "", labels: &[], assignees: &strings(&["bob"]) },
            Actor::person("bob"),
            1,
        )
        .unwrap()
        .unwrap();
    let assign = Patch { assignees: Some(ListEdit::Add(&strings(&["alice"]))), ..Default::default() };
    db.patch_issue(&bobs.id, &assign, Actor::person("bob"), 2).unwrap();
    // Inbox entries (M202) on an issue that goes, and on one that stays.
    db.notice_inbox("bob", &private.id, "mentioned", 2).unwrap();
    db.notice_inbox("bob", &shared.id, "assigned", 2).unwrap();
    let d = Deleted { account: "alice".into(), machines: vec!["alices-box".into()], disbanded: vec![] };
    for _ in 0..2 {
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        crate::db::projects::erase_account(&tx, &d).unwrap();
        erase_account(&tx, &d).unwrap();
        tx.commit().unwrap();
    }
    assert!(db.issue(&private.id).unwrap().is_none(), "its personal project's issues go");
    assert!(db.issue_events(&private.id).unwrap().is_empty());
    let entries: Vec<String> = db.c().query_map("SELECT item FROM inbox_items", params![], |r| r.get(0)).unwrap();
    assert_eq!(entries, std::slice::from_ref(&shared.id), "the gone issue's entries go");
    let shared = db.issue(&shared.id).unwrap().unwrap();
    assert_eq!(
        (shared.author.as_str(), shared.author_agent.as_deref(), shared.title.as_str()),
        ("", None, "the team's")
    );
    assert_eq!(db.issue(&bobs.id).unwrap().unwrap().assignees, strings(&["bob"]));
    let ev = db.issue_events(&bobs.id).unwrap();
    assert_eq!(ev.last().unwrap().detail, json!({ "assignee": "" }));
    assert!(db.mentions("alice").is_empty(), "{:?}", db.mentions("alice"));
    // Nothing of the deleted issue's words is left in FTS5's segments.
    if db.dialect() == crate::db::migrate::Dialect::Sqlite {
        let left: u64 = db
            .c()
            .query_row(
                "SELECT count(*) FROM issues_fts_data WHERE instr(block, CAST('own' AS BLOB)) > 0",
                params![],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "merged out of the index");
    }
}
