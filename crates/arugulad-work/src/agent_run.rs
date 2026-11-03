//! An agent run's Claude Code, followed (Arugula #552): `arugulad
//! _agent-run CLAUDE ARGS…`, which the run's pane runs as the run user.
//!
//! An agent run is one headless turn (`claude -p`): when the turn ends,
//! the process ends, and so does the run. An agent that starts something
//! in the background and ends its turn to wait for it ("the background
//! timer will report when the rate limit resets; I'll keep going from
//! there") is never woken again. The Claude Code a box brings stops what
//! it left as it exits, and newer ones wait for a while, then stop it.
//! Either way the run exited 0 with no answer, and looked green.
//!
//! So Claude Code runs with `--output-format stream-json --verbose`, and
//! this follows what it says: the background tasks it reports
//! (`background_tasks_changed`, `task_started`, `task_notification`,
//! `task_updated`) and each turn's `result`. The pane shows each turn's
//! answer, as `--output-format text` did. When Claude Code exits 0 having
//! given its last answer while background work it started was still
//! running, the run exits [`LEFT_WAITING`], with a line in its log saying
//! what was left, and the run fails with [`LEFT_WAITING_WHY`] instead of
//! succeeding.
//!
//! The agent is told so first ([`HEADLESS`], its appended system prompt),
//! so it waits in the foreground or reports what it has.

use std::{
    collections::BTreeMap,
    io::{BufRead as _, BufReader, Write as _},
    process::{Command, Stdio},
};

use serde_json::Value;

/// The daemon's subcommand that runs and follows an agent run's Claude Code.
pub const NAME: &str = "_agent-run";

/// Its exit when Claude Code exited 0 but ended with background work still
/// running (`EX_TEMPFAIL`).
pub const LEFT_WAITING: i32 = 75;

/// Why such a run failed, in its `failed` event.
pub const LEFT_WAITING_WHY: &str = "the agent ended its turn with background work still running, \
     so the run ended without what that work was for (a run is one headless turn: nothing wakes it again)";

/// What an agent run's Claude Code is told besides its instruction.
pub const HEADLESS: &str = "You are running headless, as an Arugula run (`claude -p`). Nobody reads your \
     messages until the run is over, and the end of your turn ends the run: your final message is the run's \
     whole answer. Nothing wakes you up after your turn ends, so never end it to wait for something (a \
     background task, a timer, a rate limit to reset). If you must wait, wait in the foreground (a bounded \
     `sleep`, or a command that blocks until it's done); if you can't, report what you have and what's left. \
     Before you end, stop anything you started in the background. A run whose turn ends with background work \
     still running is marked failed.";

/// What Claude Code's stream has said so far, as far as a run's end goes.
#[derive(Debug, Default)]
pub struct Follow {
    /// Background tasks running now: id, and what it is.
    running: BTreeMap<String, String>,
    /// The ones that were running when it last gave an answer.
    at_answer: BTreeMap<String, String>,
    answers: usize,
}

impl Follow {
    /// Take one line of Claude Code's `stream-json` output; what the pane
    /// shows for it, if anything. A line that isn't its JSON shows as is.
    pub fn line(&mut self, line: &str) -> Option<String> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return (!line.trim().is_empty()).then(|| line.to_owned());
        };
        let s = |k: &str| v[k].as_str().unwrap_or_default();
        match (s("type"), s("subtype")) {
            // The tasks running in the background, all of them.
            ("system", "background_tasks_changed") => {
                if let Some(tasks) = v["tasks"].as_array() {
                    self.running = tasks.iter().filter_map(task).collect();
                }
            }
            ("system", "task_started") if v["is_backgrounded"].as_bool() == Some(true) => {
                if let Some((id, what)) = task(&v) {
                    self.running.insert(id, what);
                }
            }
            ("system", "task_notification") => {
                self.running.remove(s("task_id"));
            }
            ("system", "task_updated")
                if matches!(
                    v["patch"]["status"].as_str(),
                    Some("completed" | "failed" | "killed" | "stopped" | "canceled" | "cancelled")
                ) =>
            {
                self.running.remove(s("task_id"));
            }
            ("result", subtype) => {
                self.at_answer = self.running.clone();
                self.answers += 1;
                let said = match v["result"].as_str() {
                    Some(r) => r.to_owned(),
                    None if v["is_error"].as_bool() == Some(true) || subtype.starts_with("error") => {
                        format!("Claude Code: {subtype}")
                    }
                    None => return None,
                };
                // Each answer as `--output-format text` prints it, a blank
                // line between them.
                return Some(if self.answers > 1 { format!("\n{said}") } else { said });
            }
            _ => {}
        }
        None
    }

    /// What was still running in the background when Claude Code gave its
    /// last answer: what the run's agent ended its turn waiting on (or
    /// left behind). Empty when it gave none.
    pub fn left(&self) -> Vec<&str> {
        self.at_answer.values().map(String::as_str).collect()
    }
}

/// A task in Claude Code's stream: its id, and what it is.
fn task(v: &Value) -> Option<(String, String)> {
    let id = v["task_id"].as_str()?;
    let what = v["description"].as_str().filter(|d| !d.trim().is_empty()).unwrap_or(id);
    Some((id.to_owned(), what.to_owned()))
}

/// The line a run's log ends with when it ends with background work left.
pub fn left_line(left: &[&str]) -> String {
    let what: Vec<String> = left.iter().map(|w| crate::work::printable(w)).collect();
    format!("arugula: {LEFT_WAITING_WHY}. Still running when it ended: {}.", what.join("; "))
}

/// `arugulad _agent-run CLAUDE ARGS…`: run Claude Code (which writes
/// `stream-json`), show each answer, and exit as it did, or with
/// [`LEFT_WAITING`].
pub fn cli(argv: &[String]) -> ! {
    let Some((claude, args)) = argv.split_first() else {
        eprintln!("usage: arugulad {NAME} CLAUDE ARGS…");
        std::process::exit(2);
    };
    let mut child = match Command::new(claude).args(args).stdout(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("arugula: can't start Claude Code ({claude}): {e}");
            std::process::exit(127);
        }
    };
    let mut follow = Follow::default();
    let mut out = std::io::stdout();
    if let Some(stdout) = child.stdout.take() {
        let mut r = BufReader::new(stdout);
        let mut buf = Vec::new();
        while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            let line = String::from_utf8_lossy(&buf);
            if let Some(said) = follow.line(line.trim_end_matches(['\n', '\r'])) {
                let _ = writeln!(out, "{said}");
                let _ = out.flush();
            }
            buf.clear();
        }
    }
    let code = child.wait().map_or(1, |s| s.code().unwrap_or_else(|| signaled(&s)));
    let left = follow.left();
    if code == 0 && !left.is_empty() {
        let _ = writeln!(out, "\n{}", left_line(&left));
        let _ = out.flush();
        std::process::exit(LEFT_WAITING);
    }
    std::process::exit(code)
}

/// A shell's exit for a process a signal ended.
#[cfg(unix)]
fn signaled(s: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt as _;
    128 + s.signal().unwrap_or(0)
}

#[cfg(not(unix))]
fn signaled(_: &std::process::ExitStatus) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn follow(lines: &[&str]) -> (Follow, Vec<String>) {
        let mut f = Follow::default();
        let shown = lines.iter().filter_map(|l| f.line(l)).collect();
        (f, shown)
    }

    const STARTED: &str = r#"{"type":"system","subtype":"task_started","task_id":"b1","tool_use_id":"toolu_1","description":"sleep 420; echo reset","is_backgrounded":true,"task_type":"local_bash"}"#;
    const LISTED: &str = r#"{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"b1","task_type":"local_bash","description":"sleep 420; echo reset"}]}"#;
    const NONE: &str = r#"{"type":"system","subtype":"background_tasks_changed","tasks":[]}"#;
    const WAITING: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"The background timer will report when the rate limit resets. I'll keep going from there."}"#;

    /// Arugula #552, as Claude Code 2.1.286 (the one a box's adapter
    /// brings) wrote it: the turn ends waiting on a background timer, and
    /// it stops the timer as it exits. The answer shows, and the run is
    /// left waiting.
    #[test]
    fn a_turn_that_ends_waiting_on_background_work_is_left_waiting() {
        let (f, shown) = follow(&[
            r#"{"type":"system","subtype":"init","session_id":"s"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"sleep 420; echo reset","run_in_background":true}}]}}"#,
            LISTED,
            STARTED,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Command running in background with ID: b1."}]}}"#,
            WAITING,
            NONE,
            r#"{"type":"system","subtype":"task_updated","task_id":"b1","patch":{"status":"killed"}}"#,
            r#"{"type":"system","subtype":"task_notification","task_id":"b1","tool_use_id":"toolu_1","status":"stopped"}"#,
        ]);
        assert_eq!(shown, ["The background timer will report when the rate limit resets. I'll keep going from there."]);
        assert_eq!(f.left(), ["sleep 420; echo reset"]);
        assert!(left_line(&f.left()).contains("Still running when it ended: sleep 420; echo reset."));
    }

    /// A Claude Code that waits (2.1.316): the task ends, it takes another
    /// turn with what the task said, and that answer is the run's.
    #[test]
    fn a_turn_after_the_background_work_ends_is_the_answer() {
        let (f, shown) = follow(&[
            LISTED,
            STARTED,
            WAITING,
            r#"{"type":"system","subtype":"task_updated","task_id":"b1","patch":{"status":"completed"}}"#,
            r#"{"type":"system","subtype":"task_notification","task_id":"b1","status":"completed"}"#,
            NONE,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"3 runs, 2 passed."}"#,
        ]);
        assert_eq!(shown, [WAITING_TEXT, "\n3 runs, 2 passed."]);
        assert!(f.left().is_empty(), "{:?}", f.left());
    }

    const WAITING_TEXT: &str =
        "The background timer will report when the rate limit resets. I'll keep going from there.";

    /// Any one of Claude Code's signals is enough: a task it says started
    /// in the background, until it says it's done; and its list of them.
    #[test]
    fn each_signal_counts() {
        assert_eq!(follow(&[STARTED, WAITING]).0.left(), ["sleep 420; echo reset"]);
        assert_eq!(follow(&[LISTED, WAITING]).0.left(), ["sleep 420; echo reset"]);
        assert!(follow(&[LISTED, NONE, WAITING]).0.left().is_empty());
        let done = r#"{"type":"system","subtype":"task_notification","task_id":"b1","status":"completed"}"#;
        assert!(follow(&[STARTED, done, WAITING]).0.left().is_empty());
        // A task in the foreground (a subagent it waits for) isn't one.
        let fore = STARTED.replace(r#""is_backgrounded":true"#, r#""is_backgrounded":false"#);
        assert!(follow(&[&fore, WAITING]).0.left().is_empty());
        // No answer at all: Claude Code's own exit says how it went.
        assert!(follow(&[STARTED]).0.left().is_empty());
    }

    /// The pane shows what `--output-format text` did: each answer, an
    /// error's kind when it has no text, and anything that isn't Claude
    /// Code's JSON as it is.
    #[test]
    fn the_pane_shows_each_answer_and_nothing_else() {
        let (_, shown) = follow(&[
            "not json",
            "",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"thinking out loud"}]}}"#,
            r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#,
        ]);
        assert_eq!(shown, ["not json", "Claude Code: error_max_turns"]);
    }
}
