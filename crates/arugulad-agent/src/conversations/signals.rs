//! Live session signals by conversation (#453): what the Claude Code mod
//! says from inside a session (#396, `arugula session-signal`), kept per
//! Claude Code session id, so a session with no pane of its own (VS Code's
//! Claude Code, the desktop app, the editor block's) still has its figures,
//! and the conversations list shows them.
//!
//! **Which pane shows one** ([`Place`]): the pane a session runs in, not the
//! one an app was started from. The CLI says the pane its environment names
//! (`ARUGULA_PANE`) and its own pid; a terminal pane takes the session only
//! when that pid is in the pane's foreground job
//! ([`arugula_mux::procinfo::in_foreground`]), the same foreground the mux
//! reads to tell that Claude Code runs there. An agent block takes its own
//! session (its `session_id`). A signal without a pid (an older `arugula`,
//! through `pane.session_signal`) goes to its pane as it always did.
//!
//! **Through `tmux`**: when the pane's foreground job is a `tmux` client,
//! a session counts for the pane when it's in the foreground of the `tmux`
//! pane that client shows now ([`tmux_pane`]); one in a window behind it
//! doesn't. The pane keeps its figures while that stays so (the mux asks
//! again, [`arugula_mux::procinfo::in_front`]).
//!
//! Kept in memory only (lost on a restart, as a pane's attention is), at
//! most [`KEEP`] sessions, the quietest forgotten first.

use std::collections::HashMap;

use arugula_proto::{PaneId, api::SessionSignal};
use serde_json::Value;

use super::context::Context;

/// How many sessions' figures are kept.
pub const KEEP: usize = 256;

/// Where a signal's figures show besides its conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The terminal pane it runs in.
    Pane(PaneId),
    /// The agent block that holds its session.
    Block(PaneId),
    /// Only its conversation.
    Nowhere,
}

impl Place {
    pub fn pane(self) -> Option<PaneId> {
        match self {
            Self::Pane(p) | Self::Block(p) => Some(p),
            Self::Nowhere => None,
        }
    }
}

/// What the daemon knows of the pane a signal names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// No pane, or one that isn't here.
    None,
    /// A terminal pane, and whether the sender runs in its foreground job
    /// (`None`: no pid to tell by, an older CLI).
    Terminal(PaneId, Option<bool>),
    /// An agent block, and the session it holds.
    Block(PaneId, Option<String>),
}

/// Where a signal for `session` shows: the pane it runs in, the block that
/// holds it, or only its conversation. `held`: the agent block holding
/// this session, if one does (a signal from no pane).
pub fn place(session: Option<&str>, hint: &Hint, held: Option<PaneId>) -> Place {
    match hint {
        Hint::Terminal(p, None | Some(true)) => Place::Pane(*p),
        // Started from the pane, running elsewhere: an editor, the desktop
        // app, a job in the background.
        Hint::Terminal(_, Some(false)) => Place::Nowhere,
        Hint::Block(b, own) if session.is_none() || own.as_deref() == session => Place::Block(*b),
        Hint::Block(..) | Hint::None => held.map_or(Place::Nowhere, Place::Block),
    }
}

/// Each session's context, by Claude Code's session id, or `%N` for a
/// signal that came without one (an old caller, through its pane).
#[derive(Default)]
pub struct Signals {
    by: HashMap<String, (Context, u64)>,
}

impl Signals {
    /// The key a signal is kept under: its session id, else its pane's.
    pub fn key(s: &SessionSignal, pane: Option<PaneId>) -> Option<String> {
        match s.session_id.as_deref().filter(|id| !id.is_empty()) {
            Some(id) => Some(id.to_owned()),
            None => pane.map(|p| format!("%{p}")),
        }
    }

    /// Read a signal into its session's context: its summary now.
    pub fn take(&mut self, key: &str, s: &SessionSignal, at: u64) -> Value {
        // A new session (`/clear`, a new `claude`), or one resumed, starts
        // over.
        if s.event == "start" {
            self.by.remove(key);
        }
        let (c, seen) = self.by.entry(key.to_owned()).or_default();
        c.signal(s, at);
        *seen = at;
        let summary = c.summary().unwrap_or(Value::Null);
        if self.by.len() > KEEP
            && let Some(old) = self.by.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone())
        {
            self.by.remove(&old);
        }
        summary
    }

    /// A session's figures, if it has said any.
    pub fn get(&self, session: &str) -> Option<Value> {
        self.by.get(session).and_then(|(c, _)| c.summary())
    }

    /// This daemon's.
    pub fn global() -> &'static std::sync::Mutex<Signals> {
        static G: std::sync::LazyLock<std::sync::Mutex<Signals>> = std::sync::LazyLock::new(Default::default);
        &G
    }
}

/// The shell of the `tmux` pane a `tmux` client shows now, when `client`
/// is one: asked of its server (`list-clients`), found by the socket the
/// client was started with (`-L` or `-S`, and its `TMUX_TMPDIR`), with the
/// client's own `tmux`. None for anything else, or a server that doesn't
/// answer within a moment.
pub fn tmux_pane(client: u32) -> Option<u32> {
    use crate::procinfo;
    if !procinfo::comm(client)?.starts_with("tmux") {
        return None;
    }
    let exe = procinfo::exe(client)?;
    let socket = tmux_socket(&procinfo::argv(client)?);
    let mut cmd = std::process::Command::new(exe);
    cmd.args(&socket).args(["list-clients", "-F", "#{client_pid} #{pane_pid}"]).env_remove("TMUX");
    match procinfo::env_var(client, "TMUX_TMPDIR") {
        Some(d) => cmd.env("TMUX_TMPDIR", d),
        None => cmd.env_remove("TMUX_TMPDIR"),
    };
    let out = run_briefly(cmd, std::time::Duration::from_millis(500))?;
    tmux_client_pane(&out, client)
}

/// The server options in a `tmux` client's command line: `-L NAME` or
/// `-S PATH` (joined or not), up to its command.
fn tmux_socket(argv: &[String]) -> Vec<String> {
    let mut out = vec![];
    let mut args = argv.iter().skip(1);
    while let Some(a) = args.next() {
        if a == "--" || !a.starts_with('-') || a.len() < 2 {
            break;
        }
        // Flags bundle (`-2u`); one that takes a value takes the rest, or
        // the next argument.
        for (i, c) in a.char_indices().skip(1) {
            if "cfLST".contains(c) {
                let rest = &a[i + c.len_utf8()..];
                let value = if rest.is_empty() { args.next().cloned() } else { Some(rest.to_owned()) };
                if let (Some(v), true) = (value, c == 'L' || c == 'S') {
                    out.extend([format!("-{c}"), v]);
                }
                break;
            }
        }
    }
    out
}

/// `client`'s pane in `list-clients -F '#{client_pid} #{pane_pid}'`.
fn tmux_client_pane(out: &str, client: u32) -> Option<u32> {
    out.lines().find_map(|l| {
        let (c, p) = l.trim().split_once(' ')?;
        (c.parse::<u32>().ok()? == client).then(|| p.trim().parse().ok()).flatten()
    })
}

/// A command's output, if it finishes within `limit`; else it's killed.
fn run_briefly(mut cmd: std::process::Command, limit: std::time::Duration) -> Option<String> {
    use std::{io::Read, process::Stdio};
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
    let mut s = String::new();
    child.stdout.take()?.read_to_string(&mut s).ok()?;
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tmux_clients_server_and_pane() {
        let argv = |s: &str| s.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(tmux_socket(&argv("tmux -L work attach -t main")), ["-L", "work"]);
        assert_eq!(tmux_socket(&argv("tmux -2u -S/tmp/s attach")), ["-S", "/tmp/s"]);
        assert_eq!(tmux_socket(&argv("tmux -f /x.conf -L w a")), ["-L", "w"]);
        assert!(tmux_socket(&argv("tmux attach -L nope")).is_empty());
        assert!(tmux_socket(&argv("tmux")).is_empty());
        let out = "4100 5100\n4200 5200\n";
        assert_eq!(tmux_client_pane(out, 4200), Some(5200));
        assert_eq!(tmux_client_pane(out, 4300), None);
    }

    fn usage(session: &str, tokens: u64) -> SessionSignal {
        SessionSignal {
            event: "usage".into(),
            session_id: Some(session.into()),
            tokens: Some(tokens),
            window: Some(1_000_000),
            ..Default::default()
        }
    }

    #[test]
    fn each_session_keeps_its_own_figures() {
        let mut s = Signals::default();
        let a = usage("aaaa", 50_000);
        let b = usage("bbbb", 250_000);
        let key = |x: &SessionSignal| Signals::key(x, Some(4)).unwrap();
        s.take(&key(&a), &a, 1);
        s.take(&key(&b), &b, 2);
        assert_eq!(s.get("aaaa").unwrap()["tokens"], 50_000);
        assert_eq!(s.get("bbbb").unwrap()["tokens"], 250_000);
        // One without a session id (an old caller) is its pane's.
        let old = SessionSignal { session_id: None, ..usage("", 1_000) };
        assert_eq!(Signals::key(&old, Some(4)).as_deref(), Some("%4"));
        assert_eq!(Signals::key(&old, None), None);
        // A new start forgets what that session said before.
        let start = SessionSignal { event: "start".into(), session_id: Some("aaaa".into()), ..Default::default() };
        s.take("aaaa", &start, 3);
        assert!(s.get("aaaa").is_none_or(|v| v["tokens"].is_null()));
        assert_eq!(s.get("bbbb").unwrap()["tokens"], 250_000);
    }

    #[test]
    fn the_quietest_are_forgotten() {
        let mut s = Signals::default();
        for i in 0..=KEEP as u64 {
            let sig = usage(&format!("s{i}"), 1_000 + i);
            s.take(&format!("s{i}"), &sig, i + 1);
        }
        assert!(s.get("s0").is_none());
        assert!(s.get("s1").is_some());
        assert!(s.get(&format!("s{KEEP}")).is_some());
    }

    /// #453: VS Code started from pane %4 runs two sessions; `claude` runs
    /// in the pane's foreground. Only the pane's own shows on it; each of
    /// the editor's shows on its conversation only.
    #[test]
    fn an_app_started_from_a_pane_keeps_its_sessions_off_it() {
        let mut s = Signals::default();
        let mut on_pane = None;
        for (session, in_front, tokens) in
            [("term", true, 90_000), ("code-1", false, 10_000), ("code-2", false, 20_000)]
        {
            let sig = usage(session, tokens);
            let summary = s.take(session, &sig, 1);
            if let Some(p) = place(Some(session), &Hint::Terminal(4, Some(in_front)), None).pane() {
                assert_eq!(p, 4);
                on_pane = Some(summary);
            }
        }
        assert_eq!(on_pane.unwrap()["tokens"], 90_000);
        assert_eq!(s.get("code-1").unwrap()["tokens"], 10_000);
        assert_eq!(s.get("code-2").unwrap()["tokens"], 20_000);
    }

    #[test]
    fn a_session_with_no_pane_shows_on_its_conversation() {
        let mut s = Signals::default();
        let sig = usage("desktop", 42_000);
        s.take("desktop", &sig, 1);
        assert_eq!(place(Some("desktop"), &Hint::None, None), Place::Nowhere);
        assert_eq!(s.get("desktop").unwrap()["tokens"], 42_000);
        assert_eq!(s.get("desktop").unwrap()["source"], "live");
        // Unless an agent block holds it.
        assert_eq!(place(Some("desktop"), &Hint::None, Some(9)), Place::Block(9));
    }

    #[test]
    fn where_a_signal_shows() {
        // An older CLI says no pid: its pane, as before.
        assert_eq!(place(Some("x"), &Hint::Terminal(3, None), None), Place::Pane(3));
        assert_eq!(place(None, &Hint::Terminal(3, None), None), Place::Pane(3));
        // An agent block shows its own session, not another's.
        assert_eq!(place(Some("x"), &Hint::Block(5, Some("x".into())), None), Place::Block(5));
        assert_eq!(place(Some("y"), &Hint::Block(5, Some("x".into())), None), Place::Nowhere);
        assert_eq!(place(Some("y"), &Hint::Block(5, Some("x".into())), Some(6)), Place::Block(6));
        assert_eq!(place(None, &Hint::Block(5, None), None), Place::Block(5));
    }
}
