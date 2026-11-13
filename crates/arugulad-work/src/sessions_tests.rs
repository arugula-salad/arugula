use super::*;
use arugula_control_wire::work::MAX_HELD_SESSIONS_PER_BOX;

use crate::work::sessions::default_users;

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

/// The template makes the daemon's session users (Arugula #215): one per
/// session a box holds, `ses1` to `ses6`, each in sudoers' `env_reset`,
/// and more than the three that run at once.
#[test]
fn the_template_makes_a_user_for_each_session_held() {
    let image = include_str!("../../../packaging/hosted/Dockerfile");
    let users = default_users();
    assert_eq!(users.len(), MAX_HELD_SESSIONS_PER_BOX);
    const { assert!(MAX_HELD_SESSIONS_PER_BOX > MAX_SESSIONS_PER_BOX, "parked ones keep a user each") };
    let ns: Vec<String> = users.iter().map(|u| u.trim_start_matches("ses").to_owned()).collect();
    let made = format!(r#"RUN for n in {}; do {{ id -u "ses$n""#, ns.join(" "));
    assert!(image.contains(&made), "packaging/hosted/Dockerfile: {made}");
    let sudoers = format!(r"printf 'Defaults>ses%s env_reset\n' {} > /etc/sudoers.d/sessions", ns.join(" "));
    assert!(image.contains(&sudoers), "packaging/hosted/Dockerfile: {sudoers}");
}

/// Each report is the session's next, signed, from the state the box last
/// said, and names the actions done since the last.
#[test]
fn each_report_is_the_sessions_next_signed() {
    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    let mut l = AgentSessionLedger {
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
        grant: None,
        cleanup_owed: false,
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
        model: None,
        model_now: None,
        model_said: None,
        parked: None,
        reopened: false,
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
    assert!(
        beat.actions.is_empty() && beat.tool.unwrap().chars().count() <= 200,
        "a long tool title is cut to what control keeps"
    );
}

/// Every turn's report carries the agent's last message as its reply
/// (not a review's only): trimmed, none when it said nothing, cut to
/// [`REPLY_MAX`], and cut further when the detail's JSON would be past
/// what control takes, each time from its start, so its end is kept.
#[test]
fn a_turns_report_carries_its_last_message() {
    let said = |m: &str| arugula_proto::api::TurnResult { message: m.into(), work: None };
    assert_eq!(reply_of(Some(&said("  The README is up to date.\n"))).as_deref(), Some("The README is up to date."));
    assert_eq!(reply_of(Some(&said(" \n "))), None, "nothing said, nothing to post");
    assert_eq!(reply_of(None), None);
    let long = reply_of(Some(&said(&format!("{}é{}", "b".repeat(100), "a".repeat(REPLY_MAX - 1))))).unwrap();
    assert!(long.len() <= REPLY_MAX + "…".len() && long.starts_with('…') && long.ends_with("aaa"), "{}", long.len());
    assert!(!long.contains('b'), "its start goes");

    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    let mut l = AgentSessionLedger {
        session: format!("ses_{}", "e".repeat(32)),
        project: "prj_1".into(),
        workspace: "hw_1".into(),
        asker: "acct1".into(),
        repo: None,
        branch: "arugula/x".into(),
        user: AsUser { user: "ses1".into(), home: "/home/ses1".into() },
        checkout: "/home/ses1/work/x".into(),
        block: Some(3),
        taken_at: 1,
        grant: None,
        cleanup_owed: false,
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
        turns_seen: Some(0),
        harness: None,
        review: false,
        model: None,
        model_now: None,
        model_said: None,
        parked: None,
        reopened: false,
    };
    // Quotes and newlines double in JSON: a full reply beside a full
    // diffstat would be past 16 KiB.
    let reply = "\"\n".repeat(REPLY_MAX / 2);
    let turn = |reply: &str| SessionDetail {
        branch: Some("arugula/x".into()),
        pushed: Some(false),
        diffstat: Some("d".repeat(8 * 1024)),
        commits: vec!["abc1234 fix it".into()],
        reply: Some(reply.into()),
        ..Default::default()
    };
    l.push(&keys, &me, Kind::Turn, State::Working, turn(&reply));
    l.push(&keys, &me, Kind::Turn, State::Working, turn("Done: nothing to change."));
    let d: Vec<SessionDetail> = l
        .events
        .iter()
        .map(|k| {
            assert!(k.report.detail.len() <= SESSION_DETAIL_MAX, "{}", k.report.detail.len());
            serde_json::from_str(&k.report.detail).unwrap()
        })
        .collect();
    let cut_reply = d[0].reply.as_deref().unwrap();
    assert!(cut_reply.starts_with('…') && reply.ends_with(cut_reply.trim_start_matches('…')), "{cut_reply}");
    assert!(cut_reply.len() < reply.len() && cut_reply.len() > 1024, "cut, not dropped: {}", cut_reply.len());
    assert_eq!(d[1].reply.as_deref(), Some("Done: nothing to change."), "a short one goes as it is");
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

/// Arugula #263: the default branch is what `ls-remote --symref` names,
/// and only a plain branch; a prompt says when the base is the box's clone
/// and when the disk is low.
#[test]
fn a_sessions_base_is_the_default_branch_github_names() {
    let listing = "ref: refs/heads/main\tHEAD\n0123456789abcdef0123456789abcdef01234567\tHEAD\n";
    assert_eq!(default_branch_of(listing).as_deref(), Some("main"));
    assert_eq!(default_branch_of("ref: refs/heads/release/2.x\tHEAD\n").as_deref(), Some("release/2.x"));
    for bad in [
        "0123456789abcdef0123456789abcdef01234567\tHEAD\n",
        "ref: refs/tags/v1\tHEAD\n",
        "ref: refs/heads/-x\tHEAD\n",
        "ref: refs/heads/a..b\tHEAD\n",
        "ref: refs/heads/a b\tHEAD\n",
        "ref: refs/heads/main\trefs/heads/other\n",
        "",
    ] {
        assert_eq!(default_branch_of(bad), None, "{bad:?}");
    }
    let n = stale_note("acme/app", "fatal: could not read Username");
    assert!(n.contains("the box's clone of acme/app") && n.contains("(fatal: could not read Username)"), "{n}");

    const GIB: u64 = 1 << 30;
    assert_eq!(disk_note(40 * GIB, 100 * GIB), None);
    let low = disk_note(GIB * 3 / 2, 20 * GIB).expect("under 10 GiB is low");
    assert!(low.contains("1.5 GiB free of 20 GiB") && low.contains("incremental"), "{low}");
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
        async move { push::push_bundle(&mirror, &b, &id, &own, &branch, &url, Some("ghs_not_a_real_token"), &[]).await }
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

/// A session that began before main changed CI (the v0.63.0 live test):
/// GitHub compares a new branch's tree with the default branch's, so the
/// branch's older workflow file reads as an edit, and a token without
/// `workflows` (all a session's push gets, aru-115 amended 2026-11-10) is
/// refused. The remote's hook stands in for GitHub's rule. The refusal is
/// said plainly, and nothing moved.
#[cfg(unix)]
#[tokio::test]
async fn a_branch_behind_a_main_that_changed_ci_is_refused_plainly() {
    let dir = crate::hosted::scratch("session-push-ci");
    let remote = dir.join("remote.git");
    git_in(&dir, &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
    let work = dir.join("work");
    git_in(&dir, &["clone", "-q", remote.to_str().unwrap(), work.to_str().unwrap()]);
    for (k, v) in [("user.email", "s@example.invalid"), ("user.name", "s")] {
        git_in(&work, &["config", k, v]);
    }
    std::fs::create_dir_all(work.join(".github/workflows")).unwrap();
    std::fs::write(work.join(".github/workflows/test.yml"), "on: push\n").unwrap();
    git_in(&work, &["add", "."]);
    git_in(&work, &["commit", "-qm", "init"]);
    git_in(&work, &["push", "-q", "origin", "main"]);
    // The session branches here; then main changes CI.
    let id = format!("ses_{}", "c".repeat(32));
    let own = format!("arugula/{}-fix", &id[4..12]);
    git_in(&work, &["switch", "-q", "-c", &own]);
    std::fs::write(work.join("NOTES.md"), "notes\n").unwrap();
    git_in(&work, &["add", "."]);
    git_in(&work, &["commit", "-qm", "add notes"]);
    git_in(&work, &["switch", "-q", "main"]);
    std::fs::write(work.join(".github/workflows/test.yml"), "on: [push, pull_request]\n").unwrap();
    git_in(&work, &["commit", "-qam", "test on PRs too"]);
    git_in(&work, &["push", "-q", "origin", "main"]);
    // GitHub's rule, from here on.
    let hook = remote.join("hooks/pre-receive");
    std::fs::write(
        &hook,
        "#!/bin/sh\n\
         while read old new ref; do\n\
         \x20 if [ -n \"$(git diff --name-only refs/heads/main \"$new\" -- .github/workflows)\" ] \
         && [ \"$ARUGULA_PUSH_TOKEN\" != ghs_with_workflows ]; then\n\
         \x20   echo \"refusing to allow a GitHub App to create or update workflow .github/workflows/test.yml \
         without \\`workflows\\` permission\" >&2; exit 1\n\
         \x20 fi\n\
         done\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let b = dir.join("own.bundle");
    git_in(&work, &["bundle", "create", "-q", b.to_str().unwrap(), &format!("refs/heads/{own}")]);
    let mirror = dir.join("mirror.git");
    let url = remote.display().to_string();

    // Contents only: refused, said plainly, and nothing moved.
    let e = push::push_bundle(&mirror, &b, &id, &own, &own, &url, Some("ghs_contents_only"), &[]).await.unwrap_err();
    assert_eq!(e, push::CI_BEHIND);
    assert!(e.contains("can't change workflow files") && e.contains("new session"), "{e}");
    assert_eq!(git_in(&remote, &["for-each-ref", "--format=%(refname)"]), "refs/heads/main");
}

/// A turn whose own commits change a workflow file isn't pushed: the box
/// says which files and why, before it asks for a token. Other files, or
/// none, push as before.
#[test]
fn a_turn_that_changes_workflow_files_says_why_it_isnt_pushed() {
    assert_eq!(push::changes_ci(""), None);
    assert_eq!(push::changes_ci("\n  \n"), None);
    let why = push::changes_ci(".github/workflows/ci.yml\n.github/workflows/canary.yml\n").unwrap();
    assert!(why.contains(".github/workflows/ci.yml, .github/workflows/canary.yml"), "{why}");
    assert!(why.contains("a session's push can't change workflow files"), "{why}");
    let many: String = (0..8).map(|i| format!(".github/workflows/w{i}.yml\n")).collect();
    let why = push::changes_ci(&many).unwrap();
    assert!(why.contains("w4.yml and 3 more") && !why.contains("w5.yml"), "{why}");
}

/// A stop while a push ran cleaned the slot under it, and the push made
/// its place again: once the push is back, an ended session's place goes,
/// and a session that goes on keeps its. Unix's: cleaning runs `rm` as
/// the session's user (`sudo`).
#[cfg(unix)]
#[tokio::test]
async fn a_push_back_after_an_end_cleans_again() {
    let dir = crate::hosted::scratch("session-push-end");
    let l = AgentSessionLedger {
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
        grant: None,
        cleanup_owed: false,
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
        harness: None,
        review: false,
        model: None,
        model_now: None,
        model_said: None,
        parked: None,
        reopened: false,
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

/// A question's choices go to the card as buttons (Arugula #214): one
/// question taking one answer only.
#[test]
fn a_questions_choices_go_to_the_card() {
    let one = json!({ "id": "q1", "questions": [
        { "question": "Which colour?", "multiSelect": false, "options": [{ "label": "Red" }, { "label": "Blue" }] }] });
    assert_eq!(choices(&one), ["Red", "Blue"]);
    let many = json!({ "questions": [
        { "question": "Which colour?", "multiSelect": true, "options": [{ "label": "Red" }] }] });
    assert!(choices(&many).is_empty());
    let two = json!({ "questions": [{ "question": "a", "options": [{ "label": "x" }] }, { "question": "b" }] });
    assert!(choices(&two).is_empty());
    let s = seen(&json!({ "asks": [one] }));
    assert_eq!(s.open, Some(("question", "q1".into(), "Which colour?".into(), vec!["Red".into(), "Blue".into()])));
}

/// A turn typed in the block (Arugula #215): who typed it and what, cut,
/// by the block's turn number, or its queue's head before it starts; a
/// turn the agent woke for typed nothing.
#[test]
fn what_was_typed_in_the_block_is_its_turns() {
    let long = "x".repeat(TYPED_MAX + 50);
    // `recent_turns` is newest first: turns 3, 2 and 1 of 4 (0 is gone).
    let s = seen(&json!({
        "turns": 4,
        "recent_turns": [
            { "prompt": crate::agent::WOKEN },
            { "prompt": long, "by": "ann" },
            { "prompt": "  fix the README \n", "by": "bo" },
        ],
        "queued": ["and the changelog"],
    }));
    assert_eq!((s.by_at(1), s.typed_at(1).as_deref()), (Some("bo".into()), Some("fix the README")));
    assert_eq!(s.typed_at(2).map(|t| t.chars().count()), Some(TYPED_MAX), "cut");
    assert_eq!(s.typed_at(3), None, "the agent woke itself");
    assert_eq!(s.typed_at(0), None, "too old to say");
    assert_eq!(s.typed_at(4).as_deref(), Some("and the changelog"), "not started: the queue's head");
    assert_eq!(s.typed_at(5), None);
}

/// Which model a session's block opens with (Arugula #215): the one control
/// asked for, else the strongest, said outright; Codex picks its own. Its
/// reports say the one its agent is on when that changes, and each turn's.
#[test]
fn a_session_runs_on_the_model_asked_for() {
    let w = |model: Option<&str>| -> AgentSessionWork {
        serde_json::from_value(json!({ "session": format!("ses_{}", "f".repeat(32)), "project": "prj_1",
            "workspace": "hw_1", "asker": "acct1", "branch": "arugula/x", "state": "queued", "n": 0, "model": model }))
        .unwrap()
    };
    assert_eq!(model_for(&w(None), None).as_deref(), Some(DEFAULT_MODEL), "the strongest, said outright");
    assert_eq!(DEFAULT_MODEL, "opus");
    assert_eq!(model_for(&w(Some("sonnet")), None).as_deref(), Some("sonnet"));
    assert_eq!(model_for(&w(Some("gpt-9")), None).as_deref(), Some("opus"), "not one of ours: the default");
    assert_eq!(model_for(&w(Some("sonnet")), Some("codex")), None, "Codex picks its own");

    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    let mut l: AgentSessionLedger = serde_json::from_value(json!({
        "session": format!("ses_{}", "f".repeat(32)), "project": "prj_1", "workspace": "hw_1", "asker": "acct1",
        "branch": "arugula/x", "user": { "user": "ses1", "home": "/home/ses1" }, "checkout": "/home/ses1/work/x",
        "taken_at": 1, "n": 0, "state": "queued", "model": "opus" }))
    .unwrap();
    let model = |l: &AgentSessionLedger, i: usize| -> Option<String> {
        serde_json::from_str::<SessionDetail>(&l.events[i].report.detail).unwrap().model
    };
    l.push(&keys, &me, Kind::State, State::Working, SessionDetail::default());
    assert_eq!(model(&l, 0), None, "not known yet");
    l.model_now = Some("Opus 5.5".into());
    l.push(&keys, &me, Kind::Heartbeat, State::Working, SessionDetail::default());
    l.push(&keys, &me, Kind::Heartbeat, State::Working, SessionDetail::default());
    l.push(&keys, &me, Kind::Turn, State::Working, SessionDetail::default());
    assert_eq!(
        (model(&l, 1).as_deref(), model(&l, 2), model(&l, 3).as_deref()),
        (Some("Opus 5.5"), None, Some("Opus 5.5")),
        "once it's known, again on each turn"
    );
    assert_eq!(agent_config(&l)["model"], "opus", "the block switches to it as the session opens");
}

/// A slot keeps its build caches for its next session only when its last
/// session was the same asker's: another asker's, or a slot with no
/// session before it (or none it still knows of), starts from an empty
/// home.
#[test]
fn a_slot_keeps_its_caches_only_for_the_same_asker() {
    let dir = crate::hosted::scratch("session-same-asker");
    let ledger = |n: char, asker: &str, user: &str, at: u64| AgentSessionLedger {
        session: format!("ses_{}", n.to_string().repeat(32)),
        project: "prj_1".into(),
        workspace: "hw_1".into(),
        asker: asker.into(),
        repo: None,
        branch: format!("arugula/{}-fix", n.to_string().repeat(8)),
        user: AsUser { user: user.into(), home: dir.join(user).display().to_string() },
        checkout: dir.join(user).join("work").display().to_string(),
        block: None,
        taken_at: at,
        grant: None,
        cleanup_owed: false,
        n: 0,
        state: "done".into(),
        ended: Some("done".into()),
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
        harness: None,
        review: false,
        model: None,
        model_now: None,
        model_said: None,
        parked: None,
        reopened: false,
    };
    let next = format!("ses_{}", "f".repeat(32));
    assert!(!same_asker(&dir, "ses2", &next, "acct1"), "nothing before it");
    assert!(claim(&dir, &ledger('a', "acct1", "ses2", 1)));
    assert!(claim(&dir, &ledger('b', "acct2", "ses1", 5)));
    assert!(same_asker(&dir, "ses2", &next, "acct1"));
    assert!(!same_asker(&dir, "ses2", &next, "acct2"), "another asker's slot");
    assert!(!same_asker(&dir, "ses2", &next, ""), "nobody's");
    // The slot's latest session is what counts.
    assert!(claim(&dir, &ledger('c', "acct2", "ses2", 9)));
    assert!(!same_asker(&dir, "ses2", &next, "acct1"));
    assert!(same_asker(&dir, "ses2", &next, "acct2"));
}

/// A session's ledger as a turn leaves it, in `dir`, for the push tests.
#[cfg(unix)]
fn pushing_ledger(dir: &Path, c: char) -> AgentSessionLedger {
    let l = AgentSessionLedger {
        session: format!("ses_{}", c.to_string().repeat(32)),
        project: "prj_1".into(),
        workspace: "hw_1".into(),
        asker: "acct1".into(),
        repo: Some("o/r".into()),
        branch: format!("arugula/{}-fix", c.to_string().repeat(8)),
        user: AsUser { user: "ses1".into(), home: "/home/ses1".into() },
        checkout: "/home/ses1/work/x".into(),
        block: Some(3),
        taken_at: 1,
        grant: None,
        cleanup_owed: false,
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
        turns_seen: Some(1),
        harness: None,
        review: false,
        model: None,
        model_now: None,
        model_said: None,
        parked: None,
        reopened: false,
    };
    assert!(claim(dir, &l));
    l
}

/// What a session's report says, kind by kind, with its error and its
/// `pushed`.
#[cfg(unix)]
fn said(dir: &Path, id: &str) -> Vec<(String, Option<String>, Option<bool>)> {
    let l = read(dir, id).unwrap().unwrap();
    l.events
        .iter()
        .map(|k| {
            let d: SessionDetail = serde_json::from_str(&k.report.detail).unwrap();
            (k.report.kind.clone(), d.error, d.pushed)
        })
        .collect()
}

/// A turn whose push fails (a full disk, #842) says why, as the session's
/// error, and is not pushed (#859). The turn's report that follows carries
/// `pushed: false`; the retry's test below shows what clears it.
#[cfg(unix)]
#[test]
fn a_turn_whose_push_fails_is_unpushed_with_its_reason() {
    let dir = crate::hosted::scratch("session-push-fails");
    let l = pushing_ledger(&dir, 'e');
    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    let why = "git couldn't take the session's commits: fatal: write error: No space left on device";
    let (now, pushed) = update(&dir, &l.session, |now| pushed_now(now, &keys, &me, Err(why.into()))).unwrap();
    assert!(!pushed);
    assert_eq!(now.pushed_head, None);
    assert_eq!(
        said(&dir, &l.session),
        [("error".to_owned(), Some(format!("couldn't push {}: {why}", l.branch)), None)]
    );
}

/// The retry of a failed push (#859): each failure says its newest reason,
/// and the push that gets there clears it, as a turn's report does:
/// `pushed`, and the ledger knows the head.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_push_is_tried_again_until_it_gets_there() {
    let dir = crate::hosted::scratch("session-push-retry");
    let l = pushing_ledger(&dir, 'f');
    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    retry::own(&l.session, 7);
    let tried = std::sync::atomic::AtomicUsize::new(0);
    let push = |_: AgentSessionLedger| async {
        match tried.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
            0 => Err("No space left on device".to_owned()),
            1 => Err("Out of diskspace".to_owned()),
            _ => Ok(Some("abcdef0123456789".to_owned())),
        }
    };
    let waits = [Duration::from_millis(1), Duration::from_millis(2)];
    retry::retry(&dir, &l.session, 7, &waits, &keys, &me, push).await;
    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 3);
    let b = &l.branch;
    assert_eq!(
        said(&dir, &l.session),
        [
            ("error".to_owned(), Some(format!("couldn't push {b}: No space left on device")), None),
            ("error".to_owned(), Some(format!("couldn't push {b}: Out of diskspace")), None),
            ("turn".to_owned(), None, Some(true)),
        ]
    );
    let now = read(&dir, &l.session).unwrap().unwrap();
    assert_eq!(now.pushed_head.as_deref(), Some("abcdef0123456789"));
    let turn: SessionDetail = serde_json::from_str(&now.events[2].report.detail).unwrap();
    assert_eq!((turn.branch.as_deref(), turn.head.as_deref()), (Some(b.as_str()), Some("abcdef0")));
    // It's done: nothing keeps it going, and another loop could start.
    assert!(!retry::is_running(&l.session));
}

/// A turn starting stops the retry: its own finish step pushes. So does
/// the session ending, and a newer loop taking its place; none of them
/// pushes.
#[cfg(unix)]
#[tokio::test]
async fn a_retry_stops_when_a_turn_starts() {
    let dir = crate::hosted::scratch("session-push-retry-stops");
    let keys = arugula_e2e::DeviceKeys::generate();
    let me = keys.id();
    let waits = [Duration::from_millis(1)];
    let tried = std::sync::atomic::AtomicUsize::new(0);
    let push = |_: AgentSessionLedger| async {
        tried.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err::<Option<String>, String>("No space left on device".into())
    };

    // A turn under way.
    let l = pushing_ledger(&dir, 'a');
    retry::own(&l.session, 1);
    update(&dir, &l.session, |now| {
        now.turn = Some(Turn { prompts: vec![], base: 1, sent: true, at: 1, fix: None, cancelled: false })
    })
    .unwrap();
    retry::retry(&dir, &l.session, 1, &waits, &keys, &me, &push).await;
    // Ended.
    let l = pushing_ledger(&dir, 'b');
    retry::own(&l.session, 2);
    update(&dir, &l.session, |now| now.ended = Some("stopped".into())).unwrap();
    retry::retry(&dir, &l.session, 2, &waits, &keys, &me, &push).await;
    // Replaced by a newer loop (or cancelled by a turn's end).
    let l = pushing_ledger(&dir, 'c');
    retry::own(&l.session, 4);
    retry::retry(&dir, &l.session, 3, &waits, &keys, &me, &push).await;
    retry::cancel(&l.session);
    retry::retry(&dir, &l.session, 4, &waits, &keys, &me, &push).await;

    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 0, "none of them pushed");
    for c in ['a', 'b', 'c'] {
        assert!(said(&dir, &format!("ses_{}", c.to_string().repeat(32))).is_empty());
    }
}
