//! M56 (#219): arugulad runs panes on Windows. The real binary, its
//! named pipe and its TCP port, and PowerShell on a pseudoconsole.

#![cfg(windows)]

use std::time::Duration;

use arugula_testkit::{Builder, Daemon, arugulad};
use serde_json::json;

/// How long a wait here lasts. GitHub's Windows runner has 4 CPUs, and the
/// first tests start PowerShell 7 cold in several daemons at once (each with
/// its session's first pane too): a prompt has taken longer than the 15 s
/// the other tests wait (#524).
const WAIT: u64 = 60;

fn daemon(tag: &str) -> Builder {
    arugulad!(tag).wait_secs(WAIT)
}

/// What the pane shows, or nothing while its thread is busy: starting its
/// host, or adopting one after a restart, the pane answers nothing for a
/// while (504 after 5 s), and the test waits on.
fn capture(d: &Daemon, pane: u64) -> String {
    let path = format!("/api/panes/{pane}/capture");
    let (status, text) = d.raw("GET", &path, None);
    match status {
        200 => text,
        504 => String::new(),
        _ => panic!("{path}: {status} {text}"),
    }
}

#[test]
fn a_command_runs_in_powershell_with_its_output_and_exit_code() {
    let d = daemon("win-run").start();
    let sock = std::fs::read_to_string(d.state.join("sock.path")).unwrap();
    assert!(sock.starts_with(r"\\.\pipe\arugula-"), "{sock}");
    let pane =
        d.post("/api/run", json!({"command": "Write-Output ('from' + 'windows'); exit 4"}))["pane"].as_u64().unwrap();
    let waited = d.get(&format!("/api/panes/{pane}/wait?until=exit&timeout={WAIT}"));
    assert_eq!(waited["code"], 4, "{waited}");
    assert!(capture(&d, pane).contains("fromwindows"));
}

#[test]
fn a_cmdlet_that_fails_exits_1_and_a_native_programs_code_is_kept() {
    let d = daemon("win-codes").start();
    let failed = d.post("/api/run", json!({"command": "Get-Item C:\\no\\such\\path"}))["pane"].as_u64().unwrap();
    assert_eq!(d.get(&format!("/api/panes/{failed}/wait?until=exit&timeout={WAIT}"))["code"], 1);
    let native = d.post("/api/run", json!({"command": "cmd /c exit 9"}))["pane"].as_u64().unwrap();
    assert_eq!(d.get(&format!("/api/panes/{native}/wait?until=exit&timeout={WAIT}"))["code"], 9);
}

#[test]
fn an_interactive_pane_takes_input_and_splits_and_closes() {
    let d = daemon("win-shell").start();
    let pane = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    d.wait_for("the prompt", || capture(&d, pane).contains("PS "));
    d.post(&format!("/api/panes/{pane}/send"), json!({"text": "'ab' + 'cd'\r"}));
    d.wait_for("the answer", || capture(&d, pane).contains("abcd"));

    let split = d.post("/api/run", json!({"split": pane}))["pane"].as_u64().unwrap();
    d.wait_for("the split's prompt", || capture(&d, split).contains("PS "));
    let ids = |d: &Daemon| -> Vec<u64> {
        let panes = d.get("/api/panes");
        let list = panes.as_array().cloned().or_else(|| panes["panes"].as_array().cloned()).unwrap_or_default();
        list.iter().filter_map(|p| p["id"].as_u64()).collect()
    };
    assert!(ids(&d).contains(&split));
    d.post(&format!("/api/panes/{split}/close"), json!({}));
    d.wait_for("the split to close", || !ids(&d).contains(&split));
    assert!(ids(&d).contains(&pane));
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
fn a_pane_outlives_its_daemon_and_the_next_one_adopts_it() {
    let mut d = daemon("win-keep").start();
    let pane = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    d.wait_for("the prompt", || capture(&d, pane).contains("PS "));
    d.post(&format!("/api/panes/{pane}/send"), json!({"text": "$x = 42; $PID\r"}));
    d.wait_for("the shell's pid", || capture(&d, pane).lines().any(|l| l.trim().parse::<u32>().is_ok()));
    let shell_pid: u32 = capture(&d, pane).lines().find_map(|l| l.trim().parse().ok()).unwrap();

    // No chance to save anything: the pane's host keeps it.
    d.kill();
    d.start();
    d.wait_for("the pane back", || capture(&d, pane).contains("PS "));
    d.post(&format!("/api/panes/{pane}/send"), json!({"text": "'v' + $x + ' ' + $PID\r"}));
    d.wait_for("the same shell, with its state", || capture(&d, pane).contains(&format!("v42 {shell_pid}")));

    // Closing it still ends it, through the host.
    d.post(&format!("/api/panes/{pane}/close"), json!({}));
    d.wait_for("the shell to end", || {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {shell_pid}"), "/NH"])
            .output()
            .map(|o| !String::from_utf8_lossy(&o.stdout).contains(&shell_pid.to_string()))
            .unwrap_or(false)
    });
}

fn pane_info(d: &Daemon, id: u64) -> serde_json::Value {
    let panes = d.get("/api/panes");
    let list = panes.as_array().cloned().or_else(|| panes["panes"].as_array().cloned()).unwrap_or_default();
    list.into_iter().find(|p| p["id"] == id).unwrap_or_default()
}

/// M60 (#223): PowerShell reports its prompts, commands, exit codes and
/// directory (the integration through `-EncodedCommand`), and what runs in
/// the pane is read from the process.
#[test]
fn powershell_reports_commands_exit_codes_and_cwd_and_the_foreground_program() {
    let d = daemon("win-shellint").start();
    let pane = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    d.wait_for("the first prompt", || pane_info(&d, pane)["cwd"].is_string());

    let send = |text: &str| d.post(&format!("/api/panes/{pane}/send"), json!({"text": text, "enter": true}));
    send(r"Set-Location C:\Windows; cmd /c exit 3");
    let w = d.get(&format!("/api/panes/{pane}/wait?until=command-end&timeout={WAIT}"));
    assert_eq!((w["result"].as_str(), w["exit"].as_i64()), (Some("command_end"), Some(3)), "{w}");
    assert_eq!(w["text"], r"Set-Location C:\Windows; cmd /c exit 3");
    d.wait_for("the cwd", || pane_info(&d, pane)["cwd"] == r"C:\Windows");
    d.wait_for("the cwd as the title", || {
        pane_info(&d, pane)["title"].as_str().is_some_and(|t| t.ends_with(r"C:\Windows"))
    });

    // A failing cmdlet is exit 1; a command with quotes and ';' comes
    // through intact.
    send("Get-Item C:\\no\\such");
    let w = d.get(&format!("/api/panes/{pane}/wait?until=command-end&timeout={WAIT}"));
    assert_eq!(w["exit"], 1, "{w}");
    send(r#"Write-Output "a;b" 'c'"#);
    let w = d.get(&format!("/api/panes/{pane}/wait?until=command-end&timeout={WAIT}"));
    assert_eq!((w["exit"].as_i64(), w["text"].as_str()), (Some(0), Some(r#"Write-Output "a;b" 'c'"#)), "{w}");

    // The program in the foreground, and the shell's own directory.
    send("ping -n 30 127.0.0.1");
    d.wait_for("ping in the foreground", || {
        d.get(&format!("/api/panes/{pane}/process"))["comm"].as_str().is_some_and(|c| c.eq_ignore_ascii_case("ping"))
    });
    let p = d.get(&format!("/api/panes/{pane}/process"));
    assert_eq!(p["argv"].as_array().and_then(|a| a.last()).and_then(|a| a.as_str()), Some("127.0.0.1"), "{p}");
    d.wait_for("the command as the title", || {
        pane_info(&d, pane)["title"].as_str().is_some_and(|t| t.ends_with("ping -n 30 127.0.0.1"))
    });
    d.post(&format!("/api/panes/{pane}/keys"), json!({"keys": ["C-c"]}));
}
