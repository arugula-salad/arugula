//! The CLI's side of telemetry (M105, #987): the one question it asks
//! before a command, the TUI's hint, and `arugula telemetry on|off|status`.
//! The answer is the daemon's (`telemetry.get` and `telemetry.set`). With no
//! daemon running it's the consent file (`arugula_proto::telemetry`),
//! written as `arugulad telemetry` writes it. `docs/telemetry.md` says it
//! for people.

use std::{
    io::{self, BufRead, Write},
    path::Path,
};

use anyhow::{Context, bail};
use arugula_proto::{
    api::Empty,
    op::ops::{TelemetryGet, TelemetrySet},
    telemetry::{self as consent, TelemetryInfo, TelemetrySetRequest, Why},
};

use crate::{
    cmd::Ctx,
    http::{self, Target},
    util::{now_ms, print_json},
};

/// The question, on stderr, before the command runs.
pub const QUESTION: &str = "Help improve Arugula? Send usage counts and crash reports from this machine (never \
                            terminal contents, file names, commands or prompts). Details: arugula telemetry status. \
                            [y/N] ";

/// What the TUI shows in its status line instead of asking.
pub const HINT: &str = "Usage reports are off: `arugula telemetry on` to help improve Arugula";

/// What the question is for: where the answer is recorded.
const VIA: &str = "cli";

#[derive(clap::Subcommand)]
pub enum TelemetryCmd {
    /// Send usage counts and crash reports from this machine.
    On,
    /// Send nothing, and delete the install id and anything not yet sent.
    Off,
    /// On or off, why, and the next report exactly as it would be sent.
    Status,
}

/// What decides whether `arugula` asks before a command.
#[derive(Debug, Clone, Copy)]
pub struct Facts {
    /// stdin and stderr are terminals: a person is there to answer.
    pub tty: bool,
    /// The local daemon, as its owner: no `--host` or `--ssh`.
    pub local: bool,
    /// An agent runs this (`CLAUDECODE`, `AI_AGENT`): the daemon would
    /// refuse its answer, and it isn't the agent's to give.
    pub agent: bool,
    /// The command is one a person reads at a terminal: not `--json`, MCP,
    /// `tmux -CC`, the TUI, `arugula telemetry` itself, or a hook.
    pub command: bool,
    /// What the environment and the consent file say here: only
    /// [`Why::NotAsked`] asks. `DO_NOT_TRACK`, `ARUGULA_TELEMETRY=off`, `CI`
    /// and a test run decide it before the file does.
    pub why: Why,
}

/// Whether to ask. The daemon has the last word ([`first_run`]).
pub fn should_ask(f: &Facts) -> bool {
    f.tty && f.local && !f.agent && f.command && f.why == Why::NotAsked
}

/// `y` or `yes`, any case. Enter, and anything else, is no.
pub fn is_yes(line: &str) -> bool {
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Whether the daemon at `target` has an answer yet: `Ok(true)` not asked,
/// `Ok(false)` answered, off for a reason of its own, or refused (an older
/// daemon, a guest), and `Err` when there's no daemon to ask.
fn daemon_not_asked(target: &Target) -> anyhow::Result<bool> {
    match http::call_raw_conn::<TelemetryGet>(target, &(), &Empty {})? {
        Ok((info, _)) => Ok(info.why == Why::NotAsked),
        Err(_) => Ok(false),
    }
}

/// Ask once, before the command, and record the answer. The caller checked
/// [`should_ask`]. Nothing here stops the command: a failed write is one
/// line on stderr.
pub fn first_run(target: &Target, state_dir: &Path) {
    let daemon = match daemon_not_asked(target) {
        Ok(true) => true,
        Ok(false) => return,
        // No daemon: the file said not asked, and takes the answer.
        Err(_) => false,
    };
    let mut err = io::stderr();
    let _ = write!(err, "{QUESTION}");
    let _ = err.flush();
    let mut line = String::new();
    let on = io::stdin().lock().read_line(&mut line).is_ok_and(|_| is_yes(&line));
    if line.is_empty() {
        // End of input: the line the answer would have ended.
        let _ = writeln!(err);
    }
    let saved = if daemon { set_daemon(target, on).map(drop) } else { set_file(state_dir, on) };
    let _ = match saved {
        Ok(()) if on => writeln!(err, "Usage reports are on. `arugula telemetry off` turns them off."),
        Ok(()) => writeln!(err, "Usage reports stay off. `arugula telemetry on` turns them on."),
        Err(e) => writeln!(err, "arugula: couldn't save your answer: {e:#}"),
    };
}

/// The TUI's hint, if it should show one: the local daemon, its owner in
/// person, and nobody has answered there yet.
pub fn tui_hint(target: &Target, state_dir: Option<&Path>) -> Option<&'static str> {
    let local = matches!(target, Target::Socket(_));
    let why = consent::enabled(state_dir?).why;
    let f = Facts { tty: true, local, agent: http::agent(), command: true, why };
    (should_ask(&f) && daemon_not_asked(target).unwrap_or(false)).then_some(HINT)
}

fn set_daemon(target: &Target, on: bool) -> anyhow::Result<TelemetryInfo> {
    http::call::<TelemetrySet>(target, &(), &TelemetrySetRequest { on, via: Some(VIA.into()) })
}

fn set_file(dir: &Path, on: bool) -> anyhow::Result<()> {
    consent::set(dir, on, VIA, now_ms()).with_context(|| format!("writing {}", dir.join(consent::FILE).display()))?;
    Ok(())
}

/// The file's answer as `telemetry.get` gives it, for when no daemon runs:
/// no preview (the daemon makes it) and not signed (it would know).
fn from_file(dir: &Path) -> TelemetryInfo {
    let s = consent::enabled(dir);
    let c = s.consent.clone().unwrap_or_default();
    TelemetryInfo {
        on: s.on,
        why: s.why,
        asked: s.consent.as_ref().map(|c| c.asked),
        via: c.via,
        install: s.install().map(str::to_owned),
        url: consent::url_in(&|k| std::env::var(k).ok()),
        signed: false,
        preview: None,
    }
}

/// `arugula telemetry on|off|status`, through the daemon, or on the file
/// when the local daemon isn't running.
pub fn run(cmd: TelemetryCmd, ctx: Ctx, state_dir: Option<&Path>) -> anyhow::Result<i32> {
    if http::agent() {
        bail!("telemetry is the owner's to see and answer, not an agent's");
    }
    let Ctx { sock, json_out, .. } = ctx;
    let set = match cmd {
        TelemetryCmd::On => Some(true),
        TelemetryCmd::Off => Some(false),
        TelemetryCmd::Status => None,
    };
    let answer = match set {
        Some(on) => http::call_raw_conn::<TelemetrySet>(&sock, &(), &TelemetrySetRequest { on, via: Some(VIA.into()) }),
        None => http::call_raw_conn::<TelemetryGet>(&sock, &(), &Empty {}),
    };
    let (info, raw, running) = match answer {
        Ok(got) => {
            let (info, raw) = got?;
            (info, raw, true)
        }
        // The local daemon isn't running: its state dir has the answer.
        Err(e) if matches!(sock, Target::Socket(_)) => {
            let Some(dir) = state_dir else { return Err(e) };
            if let Some(on) = set {
                set_file(dir, on)?;
            }
            let info = from_file(dir);
            let raw = serde_json::to_value(&info)?;
            (info, raw, false)
        }
        Err(e) => return Err(e),
    };
    if json_out {
        print_json(&raw);
        return Ok(0);
    }
    println!("Telemetry is {}", info.why.line());
    match set {
        Some(true) if !info.on => println!("  Nothing is sent while that holds."),
        Some(true) => println!("  See what is sent with `arugula telemetry status`."),
        Some(false) => println!("  The install id and anything not yet sent are deleted."),
        None => {
            if let Some(install) = &info.install {
                println!("  install  {install}");
            }
            match (running, info.signed) {
                (false, _) => println!("  sent to  {}", info.url),
                (true, signed) => {
                    println!("  sent to  {} ({})", info.url, if signed { "signed as this machine" } else { "unsigned" })
                }
            }
            match &info.preview {
                Some(p) => {
                    let head = if info.on {
                        "The next report, as it would be sent:"
                    } else {
                        "Nothing is sent. With `arugula telemetry on`, the first report would be (docs/telemetry.md):"
                    };
                    println!("\n{head}\n{}", serde_json::to_string_pretty(p)?);
                }
                None if !running => println!("\nThe daemon isn't running: start it to see the next report."),
                None => {}
            }
        }
    }
    if set.is_some() && !running {
        println!("  The daemon isn't running; it follows when it starts.");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts { tty: true, local: true, agent: false, command: true, why: Why::NotAsked }
    }

    /// Only a person at a terminal, on the local daemon, with nothing
    /// answered and nothing keeping it off, is asked.
    #[test]
    fn asks_only_a_person_at_a_terminal_who_hasnt_answered() {
        assert!(should_ask(&facts()));
        assert!(!should_ask(&Facts { tty: false, ..facts() }), "a pipe");
        assert!(!should_ask(&Facts { local: false, ..facts() }), "--host or --ssh");
        assert!(!should_ask(&Facts { agent: true, ..facts() }), "an agent");
        assert!(!should_ask(&Facts { command: false, ..facts() }), "--json, mcp, tmux, tui, telemetry");
        for why in [Why::Yes, Why::No, Why::DoNotTrack, Why::Env, Why::Ci, Why::Test] {
            assert!(!should_ask(&Facts { why, ..facts() }), "{why:?}");
        }
    }

    #[test]
    fn only_y_or_yes_is_yes() {
        for yes in ["y\n", "Y", " yes\r\n", "YES"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "yeah", "y es", "ok"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }
}
