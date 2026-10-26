use super::*;

#[test]
fn a_session_names_only_its_own_branch() {
    let id = format!("ses_{}", "a".repeat(32));
    assert!(good_id(&id));
    for bad in ["ses_", "ses_../x", &format!("ses_{}", "A".repeat(32)), &format!("run_{}", "a".repeat(32))] {
        assert!(!good_id(bad), "{bad}");
    }
    let short = &id[4..12];
    assert!(good_branch(&id, &format!("arugula/{short}-fix-login")));
    assert!(good_branch(&id, &format!("arugula/{short}")));
    let other = format!("ses_{}", "b".repeat(32));
    for bad in [
        "main".to_owned(),
        format!("arugula/{}-fix", &other[4..12]),
        format!("arugula/{id}-fix"),
        format!("arugula/{short}-fix/../main"),
        format!("arugula/{short}x"),
        format!("arugula/{short}-"),
        format!("arugula/{short}-a b"),
        format!("refs/heads/arugula/{short}-x"),
    ] {
        assert!(!good_branch(&id, &bad), "{bad}");
    }
}

/// Each report is the session's next, signed, from the state the box last
/// said, and names the actions done since the last.
#[test]
fn each_report_is_the_sessions_next_signed() {
    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    let mut l = Session {
        session: format!("ses_{}", "c".repeat(32)),
        project: "prj_1".into(),
        workspace: "hw_1".into(),
        asker: "acct1".into(),
        repo: None,
        branch: "arugula/x".into(),
        user: AsUser { user: "ses1".into(), home: "/home/ses1".into() },
        checkout: "/home/ses1/work/x".into(),
        block: Some(3),
        taken_at: 1,
        n: 0,
        state: "queued".into(),
        ended: None,
        handed: vec![],
        turn: None,
        notes: None,
        acted: vec![],
        unnamed: vec!["sac_1".into()],
        error: None,
        last_at: 0,
        events: vec![],
    };
    l.push(
        &keys,
        &me,
        Kind::State,
        State::Working,
        SessionDetail { prompts: vec!["spr_1".into()], ..Default::default() },
    );
    l.push(
        &keys,
        &me,
        Kind::Heartbeat,
        State::Done,
        SessionDetail { tool: Some("x".repeat(10_000)), ..Default::default() },
    );
    l.push(&keys, &me, Kind::State, State::Unresponsive, SessionDetail::default());
    l.push(&keys, &me, Kind::State, State::Done, SessionDetail::default());
    assert_eq!(l.events.len(), 3, "the box never reports unresponsive");
    let key = hex::encode(keys.sign_public());
    let got: Vec<(u64, &str, &str, &str)> = l
        .events
        .iter()
        .map(|k| (k.report.n, k.report.kind.as_str(), k.report.from.as_str(), k.report.to.as_str()))
        .collect();
    assert_eq!(
        got,
        [(1, "state", "queued", "working"), (2, "heartbeat", "working", "working"), (3, "state", "working", "done")]
    );
    for k in &l.events {
        let r = &k.report;
        let ev = Event {
            session: &r.session,
            n: r.n,
            kind: Kind::parse(&r.kind).unwrap(),
            from: State::parse(&r.from).unwrap(),
            to: State::parse(&r.to).unwrap(),
            at: r.at,
            detail: &r.detail,
            daemon: &me,
        };
        assert!(ev.verify(&key, &r.sig), "{r:?}");
        assert!(r.detail.len() <= 16 * 1024);
    }
    let first: SessionDetail = serde_json::from_str(&l.events[0].report.detail).unwrap();
    assert_eq!(first.actions, ["sac_1"], "the actions done, named once");
    let beat: SessionDetail = serde_json::from_str(&l.events[1].report.detail).unwrap();
    assert!(beat.actions.is_empty() && beat.tool.unwrap().len() < 5000, "a long tool title is cut");
}

/// The first prompt's notes: the repo's CLAUDE.md and AGENTS.md, as text.
#[test]
fn the_repos_notes_are_text() {
    let dir = crate::hosted::scratch("session-notes");
    assert_eq!(notes(&dir), None);
    std::fs::write(dir.join("CLAUDE.md"), "Run just check.\n").unwrap();
    std::fs::write(dir.join("AGENTS.md"), "  \n").unwrap();
    let n = notes(&dir).unwrap();
    assert!(
        n.contains("its settings and hooks aren't loaded") && n.ends_with("CLAUDE.md, as text:\n\nRun just check."),
        "{n}"
    );
    assert!(!n.contains("AGENTS.md"), "an empty one is left out");
}

/// M261: the finish step's output goes to the agent fenced, in a fence its
/// own backticks can't close, and only its last 4 KiB.
#[test]
fn the_finish_steps_output_is_fenced() {
    let r = FinishResult { ok: false, command: "just check".into(), output_tail: "error:\n````\nnot a fence\n".into() };
    let p = fix_prompt(&r);
    assert!(p.contains("It ran `just check` in your checkout"), "{p}");
    assert!(p.contains("\n`````text\nerror:\n````\nnot a fence\n`````\n"), "{p}");
    let long = format!("{}é{}", "a".repeat(5000), "b".repeat(4095));
    let kept = last(&long, 4096);
    assert!(kept.len() <= 4096 && kept.ends_with('b') && kept.starts_with('b'), "on a character's edge");
}
