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
        pushed_head: None,
        turns_seen: Some(0),
        harness: None,
        review: false,
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

/// What a session may push: its own branch exactly, never `main` or
/// another session's; a bundle's one head; the push's remote is GitHub's.
#[test]
fn a_session_pushes_only_its_own_branch() {
    let id = format!("ses_{}", "a".repeat(32));
    let own = format!("arugula/{}-fix", &id[4..12]);
    assert!(push::may_push(&id, &own, &own));
    let other = format!("arugula/{}-fix", "b".repeat(8));
    for bad in ["main", other.as_str(), &format!("{own}x"), &format!("refs/heads/{own}")] {
        assert!(!push::may_push(&id, &own, bad), "{bad}");
    }
    // Its own name, but not a session's shape: refused too.
    assert!(!push::may_push(&id, "main", "main"));
    let sha = "a".repeat(40);
    assert_eq!(push::only_head(&format!("{sha} refs/heads/{own}\n"), &own), Some(sha.clone()));
    for bad in [
        format!("{sha} refs/heads/main\n"),
        format!("{sha} refs/heads/{own}\n{sha} refs/heads/main\n"),
        format!("{sha} refs/tags/{own}\n"),
        String::new(),
    ] {
        assert_eq!(push::only_head(&bad, &own), None, "{bad}");
    }
    assert_eq!(push::remote_for("acme/app").as_deref(), Some("https://github.com/acme/app.git"));
    for bad in ["acme", "acme/app/x", "../x", "acme/.git", "acme/a b"] {
        assert_eq!(push::remote_for(bad), None, "{bad}");
    }
}

#[cfg(unix)]
fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The push from a bundle (S42 q2, end to end with git, a local bare repo
/// standing in for GitHub): the session's own branch reaches the remote,
/// and only it. A bundle of `main`, of another session's branch, or of
/// more than the one ref is refused, and nothing the agent planted in its
/// checkout runs.
#[cfg(unix)]
#[tokio::test]
async fn a_push_from_a_bundle_moves_one_ref() {
    let dir = crate::hosted::scratch("session-push");
    let remote = dir.join("remote.git");
    git_in(&dir, &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
    let work = dir.join("work");
    git_in(&dir, &["clone", "-q", remote.to_str().unwrap(), work.to_str().unwrap()]);
    for (k, v) in [("user.email", "s@example.invalid"), ("user.name", "s")] {
        git_in(&work, &["config", k, v]);
    }
    std::fs::write(work.join("README.md"), "# app\n").unwrap();
    git_in(&work, &["add", "."]);
    git_in(&work, &["commit", "-qm", "init"]);
    git_in(&work, &["push", "-q", "origin", "main"]);
    let id = format!("ses_{}", "a".repeat(32));
    let own = format!("arugula/{}-fix", &id[4..12]);
    let other = format!("arugula/{}-x", "b".repeat(8));
    git_in(&work, &["switch", "-q", "-c", &own]);
    std::fs::write(work.join("NOTES.md"), "notes\n").unwrap();
    git_in(&work, &["add", "."]);
    git_in(&work, &["commit", "-qm", "add notes"]);
    let head = git_in(&work, &["rev-parse", "HEAD"]);
    git_in(&work, &["branch", &other]);
    git_in(&work, &["branch", "-f", "main", "HEAD"]);
    // What the agent plants for a careless push to run or follow.
    let marks = dir.join("marks");
    std::fs::create_dir_all(&marks).unwrap();
    let hook = work.join(".git/hooks/pre-push");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}/hook\n", marks.display())).unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    git_in(&work, &["config", "core.fsmonitor", &format!("touch {}/fsmonitor; false", marks.display())]);
    git_in(&work, &["config", "remote.origin.push", "+refs/heads/*:refs/heads/*"]);
    let bundle = |name: &str, refs: &[&str]| {
        let b = dir.join(name);
        let mut args = vec!["bundle", "create", "-q", b.to_str().unwrap()];
        args.extend(refs);
        let refs: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
        git_in(&work, &refs);
        b
    };
    let mirror = dir.join("mirror.git");
    let url = remote.display().to_string();
    let push = |b: PathBuf, branch: String| {
        let (mirror, url, id, own) = (mirror.clone(), url.clone(), id.clone(), own.clone());
        async move { push::push_bundle(&mirror, &b, &id, &own, &branch, &url, Some("ghs_not_a_real_token")).await }
    };

    // main, and another session's branch: refused before git runs.
    let b = bundle("main.bundle", &["refs/heads/main"]);
    let e = push(b, "main".into()).await.unwrap_err();
    assert!(e.contains("its own branch and nothing else"), "{e}");
    let b = bundle("other.bundle", &[&format!("refs/heads/{other}")]);
    assert!(push(b, other.clone()).await.is_err());
    // Its own name asked for, but the bundle names more, or something
    // else: refused.
    let b = bundle("two.bundle", &[&format!("refs/heads/{own}"), "refs/heads/main"]);
    let e = push(b, own.clone()).await.unwrap_err();
    assert!(e.contains("names more than its branch"), "{e}");
    let b = bundle("main-as-own.bundle", &["refs/heads/main"]);
    assert!(push(b, own.clone()).await.is_err());
    assert_eq!(git_in(&remote, &["for-each-ref", "--format=%(refname)"]), "refs/heads/main", "nothing moved");

    // Its own branch, alone: it reaches the remote, and nothing else does.
    let b = bundle("own.bundle", &[&format!("refs/heads/{own}")]);
    assert_eq!(push(b, own.clone()).await.unwrap(), head);
    let refs = git_in(&remote, &["for-each-ref", "--format=%(refname) %(objectname)"]);
    let main = git_in(&remote, &["rev-parse", "refs/heads/main"]);
    assert_ne!(main, head, "main stayed where it was");
    assert_eq!(refs, format!("refs/heads/{own} {head}\nrefs/heads/main {main}"));
    assert!(std::fs::read_dir(&marks).unwrap().next().is_none(), "nothing the agent planted ran");
    // The token reached no file of the push's.
    let mut found = vec![];
    let mut todo = vec![mirror.clone()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if std::fs::read(&p).unwrap().windows(9).any(|w| w == b"ghs_not_a") {
                found.push(p);
            }
        }
    }
    assert!(found.is_empty(), "{found:?}");
}

/// A stop while a push ran cleaned the slot under it, and the push made
/// its place again: once the push is back, an ended session's place goes,
/// and a session that goes on keeps its. Unix's: cleaning runs `rm` as
/// the session's user (`sudo`).
#[cfg(unix)]
#[tokio::test]
async fn a_push_back_after_an_end_cleans_again() {
    let dir = crate::hosted::scratch("session-push-end");
    let l = Session {
        session: format!("ses_{}", "d".repeat(32)),
        project: "prj_1".into(),
        workspace: "hw_1".into(),
        asker: "acct1".into(),
        repo: Some("o/r".into()),
        branch: format!("arugula/{}-fix", "d".repeat(8)),
        user: AsUser { user: "arugula-no-such-user".into(), home: dir.join("home").display().to_string() },
        checkout: dir.join("home/work").display().to_string(),
        block: None,
        taken_at: 1,
        n: 0,
        state: "working".into(),
        ended: None,
        handed: vec![],
        turn: None,
        notes: None,
        acted: vec![],
        unnamed: vec![],
        error: None,
        last_at: 0,
        events: vec![],
        pushed_head: None,
        turns_seen: None,
    };
    assert!(claim(&dir, &l));
    let place = dir.join(super::super::DIR).join(DIR).join(format!("{}.push", l.session));
    std::fs::create_dir_all(place.join("repo.git")).unwrap();
    assert!(!push::clean_if_ended(&dir, &l).await);
    assert!(place.exists(), "it goes on: kept for its next push");
    update(&dir, &l.session, |now| now.ended = Some("control says it ended".into())).unwrap();
    assert!(push::clean_if_ended(&dir, &l).await);
    assert!(!place.exists(), "the push's place went with the session");
}
