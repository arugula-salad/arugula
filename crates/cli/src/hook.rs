//! Claude Code's hooks for team answers (M29).
//!
//! `arugula hook` takes any hook event on stdin. A `PermissionRequest`
//! (a tool asking to run) becomes an approval card beside the pane, on
//! every client and as a push, and waits: the card's allow, allow always
//! (one of Claude Code's own suggestions) or deny is printed for Claude
//! Code. Claude Code shows its own dialog at the same time, and a "Yes"
//! there never reaches the hook (S18), so every other event (`PreToolUse`,
//! `PostToolUse`, `PostToolUseFailure`, `UserPromptSubmit`) is passed to
//! the daemon, which closes a card the terminal answered first; "No" or Esc
//! there stops the hook, which withdraws its card. AskUserQuestion is left
//! to `arugula ask` (M6c).
//!
//! `arugula inbox` is a background (`asyncRewake`) hook on `Stop` and
//! `SessionStart`: it waits for a follow-up someone sends the agent, and
//! exits 2 with it, which wakes Claude Code and runs it as the next
//! instruction. It never types into the prompt, so it never mixes with
//! what the driver has half typed. A session nobody drives (`claude -p`,
//! the Agent SDK) gets no follow-ups, so `inbox` leaves it at once: Claude
//! Code waits for a SessionStart hook before a headless session's first
//! turn, so waiting there would hold `claude -p` for the hook's 24 hours
//! (#124).
//!
//! On `SessionStart` (startup, resume, `/clear` and compaction alike),
//! `arugula hook` adds context instead: where the daemon answers, it prints
//! `flags::ISSUES_STEERING` as `additionalContext`, so Claude Code files a
//! new issue in Arugula, not on GitHub, and where it has `docs` on,
//! `flags::DOCS_STEERING` before it, so a report is a Doc, not an Artifact
//! (#1486). It asks the daemon for at most [`FLAGS_WAIT`], because Claude
//! Code waits for SessionStart hooks before a headless session's first
//! turn. A `PreToolUse` of Bash that runs `gh issue create` gets the issues
//! sentence again as its context: it doesn't stop the command, which may
//! be what the user asked for.
//!
//! Outside an Arugula pane all of them do nothing.

use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

use serde_json::Value;

use arugula_proto::{
    api::{Empty, InboxAnswer, PermitAnswer},
    flags::{self, DOCS_STEERING, ISSUES_STEERING},
    op::ops::{FlagsList, PaneInbox, PanePermit},
};

use crate::http::{Target, call, request, request_op, send_op};

/// How long to keep asking a daemon that doesn't answer before leaving the
/// permission to the terminal.
const GIVE_UP: Duration = Duration::from_secs(60);

/// The longest `SessionStart` waits for the daemon's flags. A session must
/// never wait on Arugula.
const FLAGS_WAIT: Duration = Duration::from_secs(2);

/// The pane and the hook's input, unless this is outside a pane, or the
/// plugin's copy (`command` is the hook's) of one settings.json has too
/// (#229).
fn stdin_hook(command: &str) -> Option<(u32, Value)> {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let pane = crate::util::env_pane()?;
    let hook: Value = serde_json::from_str(&input).ok()?;
    if crate::hooks::plugin_copy_stands_aside(command, hook["hook_event_name"].as_str()) {
        return None;
    }
    Some((pane, hook))
}

/// `arugula hook`: always exits 0; prints a decision only for a
/// permission someone answered on a card, the issues (and, where `docs` is
/// on, Doc) context on `SessionStart`, and the issues context before a
/// `gh issue create`.
pub fn run(sock: Target) -> i32 {
    let Some((pane, hook)) = stdin_hook("arugula hook") else { return 0 };
    if hook["hook_event_name"] == "SessionStart" {
        if let Some(out) = session_start(&sock, FLAGS_WAIT) {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{out}");
            let _ = stdout.flush();
        }
        return 0;
    }
    if hook["hook_event_name"] != "PermissionRequest" {
        if let Some(out) = pre_tool_use(&hook) {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{out}");
            let _ = stdout.flush();
        }
        let _ = request(&sock, "POST", &format!("/api/panes/{pane}/hook"), Some(&hook));
        return 0;
    }
    if hook["tool_name"].as_str().is_none_or(|t| t == arugula_proto::ask::ASK_USER_QUESTION) {
        return 0;
    }
    crate::ask::withdraw_on_signals(sock.clone(), pane, None);
    let mut failing_since: Option<Instant> = None;
    loop {
        match request_op::<PanePermit>(&sock, &pane, &hook) {
            Ok(res) if res.status == 503 => {}
            Ok(res) => {
                let Ok(answer) = res.parse::<PermitAnswer>() else { return 0 };
                if let PermitAnswer::Allow { output } | PermitAnswer::Deny { output } = answer {
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{output}");
                    let _ = out.flush();
                }
                return 0;
            }
            Err(_) => {}
        }
        let since = *failing_since.get_or_insert_with(Instant::now);
        if since.elapsed() > GIVE_UP {
            eprintln!("arugula hook: the daemon isn't answering; answer in the terminal");
            return 0;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// What `SessionStart` prints, if the daemon answers within `wait`: Claude
/// Code's `additionalContext`, the issues sentence, with the Doc one first
/// where `docs` is on. Nothing when the daemon doesn't answer in time, or
/// answers something else.
fn session_start(sock: &Target, wait: Duration) -> Option<Value> {
    let context = match docs_on(sock, wait)? {
        true => format!("{DOCS_STEERING} {ISSUES_STEERING}"),
        false => ISSUES_STEERING.to_owned(),
    };
    Some(serde_json::json!({
        "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": context }
    }))
}

/// Whether the daemon has `docs` on, asked on a thread of its own so that a
/// daemon that accepts and never answers still costs only `wait` (the thread
/// ends with the process). `None`: it didn't answer.
fn docs_on(sock: &Target, wait: Duration) -> Option<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let sock = sock.clone();
    std::thread::spawn(move || {
        let _ = tx.send(call::<FlagsList>(&sock, &(), &Empty {}));
    });
    match rx.recv_timeout(wait) {
        Ok(Ok(all)) => Some(all.iter().any(|f| f.name == flags::DOCS && f.on)),
        _ => None,
    }
}

/// What a `PreToolUse` prints: the issues sentence, as context, for a Bash
/// command that runs `gh issue create`. It never decides.
fn pre_tool_use(hook: &Value) -> Option<Value> {
    if hook["hook_event_name"] != "PreToolUse" || hook["tool_name"] != "Bash" {
        return None;
    }
    files_a_github_issue(hook["tool_input"]["command"].as_str()?).then(|| {
        serde_json::json!({
            "hookSpecificOutput": { "hookEventName": "PreToolUse", "additionalContext": ISSUES_STEERING }
        })
    })
}

/// Whether a shell command runs `gh issue create` (or its alias `new`).
fn files_a_github_issue(command: &str) -> bool {
    let words: Vec<&str> = command.split(|c: char| c.is_whitespace() || ";&|()".contains(c)).collect();
    words.windows(3).any(|w| w[0] == "gh" && w[1] == "issue" && matches!(w[2], "create" | "new"))
}

/// `arugula inbox`: exits 2 with a follow-up (Claude Code's
/// `asyncRewake` wakes the agent with it), or 0 when a newer waiter takes
/// over or there's nothing to wait for.
pub fn inbox(sock: Target) -> i32 {
    let Some((pane, hook)) = stdin_hook("arugula inbox") else { return 0 };
    let env = |k| std::env::var(k).ok();
    if !driven(env("CLAUDE_CODE_SESSION_ATTENDED").as_deref(), env("CLAUDE_CODE_ENTRYPOINT").as_deref()) {
        return 0;
    }
    loop {
        match send_op::<PaneInbox>(&sock, &pane, &hook) {
            Ok(res) if res.status == 503 => {}
            Ok(res) if res.status >= 400 => return 0,
            Ok(res) => {
                let Ok(InboxAnswer::FollowUp { text, by }) = res.parse() else { return 0 };
                let name = if by.name.is_empty() { "a teammate" } else { &by.name };
                let mut err = std::io::stderr().lock();
                let _ = writeln!(err, "A follow-up from {name} (sent through Arugula): {text}");
                let _ = err.flush();
                return 2;
            }
            // The daemon is restarting: wait for the next one.
            Err(_) => {}
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Whether a person drives this Claude Code session, from what Claude Code
/// (2.1.289) sets in its hooks' environment. Seen on SessionStart and Stop
/// (#124):
///
/// | session | `CLAUDE_CODE_SESSION_ATTENDED` | `CLAUDE_CODE_ENTRYPOINT` |
/// |---|---|---|
/// | `claude` in a terminal | `1` | `cli` |
/// | `claude -p` | `0` | `sdk-cli` |
/// | the Agent SDK (TypeScript) | `0` | `sdk-ts` |
///
/// The hook's stdin doesn't tell them apart reliably (the interactive one
/// adds `model` and `scratchpad_dir`, which isn't a promise). `ATTENDED`
/// says it outright, so it wins; an older Claude Code without it falls back
/// to the entrypoint, where every SDK one starts `sdk-`. With neither, it's
/// taken as driven, which is how `inbox` behaved before.
fn driven(attended: Option<&str>, entrypoint: Option<&str>) -> bool {
    match attended {
        Some("0") => false,
        Some("1") => true,
        _ => !entrypoint.is_some_and(|e| e.starts_with("sdk-")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{driven, files_a_github_issue, pre_tool_use};

    #[test]
    fn gh_issue_create_is_seen_wherever_it_is_in_the_command() {
        for c in [
            "gh issue create --title x --body-file b.md",
            "cd repo && gh issue create -t x",
            "gh issue new",
            "(gh issue create)",
        ] {
            assert!(files_a_github_issue(c), "{c}");
        }
        for c in ["gh issue view 12", "gh issue list", "gh pr create", "echo gh issue", "gh issues create"] {
            assert!(!files_a_github_issue(c), "{c}");
        }
    }

    #[test]
    fn only_bash_filing_a_github_issue_gets_the_issues_context() {
        let bash =
            |c: &str| json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": c } });
        let out = pre_tool_use(&bash("gh issue create -t x")).expect("context");
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], arugula_proto::flags::ISSUES_STEERING);
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert!(out["hookSpecificOutput"].get("permissionDecision").is_none(), "it never decides");
        assert!(pre_tool_use(&bash("gh issue view 3")).is_none());
        let mut post = bash("gh issue create");
        post["hook_event_name"] = json!("PostToolUse");
        assert!(pre_tool_use(&post).is_none());
        let mut other = bash("gh issue create");
        other["tool_name"] = json!("Write");
        assert!(pre_tool_use(&other).is_none());
    }

    #[test]
    fn only_a_session_someone_drives_waits_for_follow_ups() {
        assert!(driven(Some("1"), Some("cli")), "a terminal claude");
        assert!(!driven(Some("0"), Some("sdk-cli")), "claude -p");
        assert!(!driven(Some("0"), Some("sdk-ts")), "the Agent SDK");
        assert!(!driven(Some("0"), Some("cli")), "attended wins");
        assert!(!driven(None, Some("sdk-py")), "an older SDK");
        assert!(driven(None, Some("cli")), "an older terminal claude");
        assert!(driven(None, None), "no word either way: wait, as before");
    }
}
