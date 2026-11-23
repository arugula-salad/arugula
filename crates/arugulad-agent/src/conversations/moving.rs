//! Move here: a Claude Code conversation running in a terminal outside
//! Arugula, stopped there so Continue here can go on with it. Gently:
//! SIGINT, then SIGTERM after a short wait, never SIGKILL; and only the
//! owner's own process, on this machine. One that won't stop is left
//! running, and the person is asked to quit it there.

use std::time::{Duration, Instant};

/// How long each signal gets to work, and how often to look.
#[derive(Debug, Clone, Copy)]
pub struct Waits {
    pub int: Duration,
    pub term: Duration,
    pub poll: Duration,
}

impl Waits {
    pub const DEFAULT: Waits =
        Waits { int: Duration::from_secs(3), term: Duration::from_secs(5), poll: Duration::from_millis(100) };
}

/// Why it couldn't be stopped. Either way it's still running there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Another user's, or not a process we may signal.
    NotOurs,
    /// Asked twice and still running.
    StillRunning,
    /// No signals here (Windows).
    Unsupported,
}

impl Refused {
    pub fn why(self) -> &'static str {
        match self {
            Refused::NotOurs => "it isn't yours to stop",
            Refused::StillRunning => "it's still running",
            Refused::Unsupported => "it can't be stopped from here",
        }
    }
}

/// Stop the Claude Code process `pid`: SIGINT, then, if it's still there
/// after `w.int`, SIGTERM, and `w.term` more. `gone` says whether it has
/// let go of the conversation (its session file gone, or it isn't
/// running); it's asked before each signal, so a pid that has ended is
/// never signalled again. Blocks; run it off the runtime.
pub fn stop(pid: u32, gone: impl Fn() -> bool, w: Waits) -> Result<(), Refused> {
    if gone() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::{Pid, geteuid},
        };
        // Never a group (0, a negative pid), init, or this daemon.
        let Ok(raw) = i32::try_from(pid) else { return Err(Refused::NotOurs) };
        if raw <= 1 || pid == std::process::id() {
            return Err(Refused::NotOurs);
        }
        match crate::procinfo::uid(pid) {
            Some(uid) if uid == geteuid().as_raw() => {}
            _ if gone() => return Ok(()),
            _ => return Err(Refused::NotOurs),
        }
        for (signal, wait) in [(Signal::SIGINT, w.int), (Signal::SIGTERM, w.term)] {
            if gone() {
                return Ok(());
            }
            if let Err(e) = kill(Pid::from_raw(raw), signal) {
                return if e == nix::errno::Errno::ESRCH || gone() { Ok(()) } else { Err(Refused::NotOurs) };
            }
            let until = Instant::now() + wait;
            while Instant::now() < until {
                if gone() {
                    return Ok(());
                }
                std::thread::sleep(w.poll);
            }
        }
        if gone() { Ok(()) } else { Err(Refused::StillRunning) }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, w, Instant::now());
        Err(Refused::Unsupported)
    }
}

/// The terminal app `pid` runs in, by the names of its ancestors: the
/// first that's one ([`terminal_app`]). None when none is, or it's
/// through ssh (the terminal is on another machine).
pub fn terminal_of(pid: u32) -> Option<&'static str> {
    let mut p = pid;
    for _ in 0..32 {
        p = crate::procinfo::ppid(p).filter(|p| *p > 1)?;
        let comm = crate::procinfo::comm(p)?;
        if comm == "sshd" || comm.starts_with("sshd-") {
            return None;
        }
        if let Some(app) = terminal_app(&comm) {
            return Some(app);
        }
    }
    None
}

/// What people call the terminal app a process name (`comm`) is, if it's
/// one. Linux cuts names at 15 bytes (`gnome-terminal-`).
fn terminal_app(comm: &str) -> Option<&'static str> {
    let c = comm.to_ascii_lowercase();
    let c = c.strip_suffix(".exe").unwrap_or(&c);
    Some(match c {
        _ if c.starts_with("iterm") => "iTerm",
        "terminal" => "Terminal",
        "ghostty" => "Ghostty",
        "kitty" => "kitty",
        "alacritty" => "Alacritty",
        "warp" => "Warp",
        _ if c.starts_with("wezterm") => "WezTerm",
        _ if c.starts_with("gnome-terminal") => "GNOME Terminal",
        "konsole" => "Konsole",
        "xfce4-terminal" => "Xfce Terminal",
        "tilix" => "Tilix",
        "terminator" => "Terminator",
        "foot" => "foot",
        "xterm" => "xterm",
        "windowsterminal" => "Windows Terminal",
        _ if c.starts_with("tmux") => "tmux",
        "screen" => "screen",
        "zellij" => "Zellij",
        "code" | "code-insiders" => "VS Code",
        _ if c.starts_with("code helper") => "VS Code",
        "cursor" => "Cursor",
        _ if c.starts_with("cursor helper") => "Cursor",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_apps_by_name() {
        for (comm, app) in [
            ("iTerm2", "iTerm"),
            ("iTermServer-3.5.4", "iTerm"),
            ("Terminal", "Terminal"),
            ("gnome-terminal-", "GNOME Terminal"),
            ("tmux: server", "tmux"),
            ("Code Helper (Plugin)", "VS Code"),
            ("ghostty", "Ghostty"),
        ] {
            assert_eq!(terminal_app(comm), Some(app), "{comm}");
        }
        for comm in ["zsh", "login", "bash", "node", "launchd"] {
            assert_eq!(terminal_app(comm), None, "{comm}");
        }
    }

    #[cfg(unix)]
    mod unix {
        use std::{
            process::{Child, Command},
            sync::Mutex,
            time::Duration,
        };

        use super::super::*;

        const QUICK: Waits = Waits {
            int: Duration::from_millis(1500),
            term: Duration::from_millis(1500),
            poll: Duration::from_millis(20),
        };

        /// A stand-in for Claude Code: a shell ignoring `ignores`, which
        /// says it's ready once its traps are set.
        fn fake(ignores: &str) -> Child {
            let ready = std::env::temp_dir().join(format!("arugula-moving-{}-{}", std::process::id(), ignores.len()));
            let _ = std::fs::remove_file(&ready);
            let script = format!(r#"{} touch "$1"; while :; do sleep 0.05; done"#, trap(ignores));
            let child = Command::new("sh").args(["-c", &script, "fake-claude"]).arg(&ready).spawn().unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            while !ready.exists() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = std::fs::remove_file(&ready);
            child
        }

        fn trap(ignores: &str) -> String {
            if ignores.is_empty() { String::new() } else { format!("trap '' {ignores};") }
        }

        /// Stop it, watching it as Move here watches the session file.
        fn stopping(child: Child) -> (Result<(), Refused>, Duration, Child) {
            let pid = child.id();
            let child = Mutex::new(child);
            let gone = || child.lock().unwrap().try_wait().unwrap().is_some();
            let at = Instant::now();
            let out = stop(pid, gone, QUICK);
            (out, at.elapsed(), child.into_inner().unwrap())
        }

        /// The stop sequence against a fake Claude Code: SIGINT is enough
        /// for one that quits on it; SIGTERM follows, after the wait, for
        /// one that ignores it; and one that ignores both is left running
        /// (never SIGKILL), which Move here reports.
        #[test]
        fn stops_gently_and_never_kills() {
            let (out, took, _) = stopping(fake(""));
            assert_eq!(out, Ok(()));
            assert!(took < QUICK.int, "SIGINT stopped it: {took:?}");

            let (out, took, _) = stopping(fake("INT"));
            assert_eq!(out, Ok(()));
            assert!(took >= QUICK.int && took < QUICK.int + QUICK.term, "SIGTERM after the wait: {took:?}");

            let (out, took, mut child) = stopping(fake("INT TERM"));
            assert_eq!(out, Err(Refused::StillRunning));
            assert!(took >= QUICK.int + QUICK.term, "{took:?}");
            assert!(child.try_wait().unwrap().is_none(), "left running, not killed");
            child.kill().unwrap();
            child.wait().unwrap();
        }

        #[test]
        fn never_a_group_init_or_itself() {
            for pid in [0, 1, u32::MAX, std::process::id()] {
                assert_eq!(stop(pid, || false, QUICK), Err(Refused::NotOurs), "{pid}");
            }
            // Gone already: nothing to do, nothing signalled.
            assert_eq!(stop(0, || true, QUICK), Ok(()));
        }
    }
}
