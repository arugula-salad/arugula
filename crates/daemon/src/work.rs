//! Runs on a hosted machine (M212, platform step 7): S37's execution
//! contract (`spikes/s37-team-permissions/README.md`, q4) in
//! `arugulad --hosted`. The platform's half is `arugula_platform::work`.
//!
//! - **It takes work.** Once it's enrolled, the machine asks control for
//!   its work every few seconds ([`PULL`]), saying it takes runs
//!   (`GET /api/hosted/work?takes=run`, signed as the machine), and at once
//!   when a start names a run ([`nudge`]). Control answers every run that's
//!   ready for it and hasn't started, every time, until it starts, with the
//!   project's revokes. One run goes at a time: the others wait, pulled.
//! - **Checked again before anything starts** ([`check`]): the operation's
//!   bytes hash to the hash the run names; it's for this machine, its
//!   project and its workspace; it's an operation this machine runs (`kind
//!   run`, or `kind agent` with Claude Code); its directory is one a run may
//!   copy ([`source_dir`]); and the project's standing grant covers it
//!   ([`grant`]). If a check fails the run fails, with the check's name,
//!   and nothing runs.
//! - **At most once** ([`Ledger`], [`claim`]). Before it starts anything,
//!   the machine makes its ledger entry, `<state>/work/ID.json`, which can
//!   only be made once (`create_new`), saying `running`, and fsyncs. An id
//!   with an entry, or one that can't be read, is never started again,
//!   whatever control sends: a second delivery, or the same one after a
//!   restart, only sends again the reports control hasn't taken. A run the
//!   machine finds `running` after a restart with no live pane for it
//!   fails, "interrupted by a restart; not run again". A crash can lose a
//!   run, never double one.
//! - **As another user** ([`RunAs`]). A run runs as `runner`, which has no
//!   sudo, can't read arugulad's state or write anything of `user`'s, and
//!   can't reach the daemon's or the provider's ports on loopback
//!   ([`firewall`]). It starts with an empty environment (`env -i`), in a
//!   directory of its own (`~runner/work/ID`, also its `HOME`), with its own
//!   copy of the directory the operation names, never that directory
//!   itself. When it ends, or is stopped, everything `runner` runs is killed
//!   ([`kill_all`]), so nothing carries into the next run.
//! - **Agent runs** are Claude Code, headless, as the run user. On a
//!   hosted box each is a turn the platform says may run (M185's gate),
//!   with its placeholder key, ended with the run.
//! - **In a pane.** A run is its command in a new pane in the `runs`
//!   session, which ends when the command does. Its live view is an
//!   ordinary end-to-end pane, which the project's people watch as they
//!   watch any (the phone included), and the pane stays with its output.
//!   Its first lines, and its tab's name, say which run it is, its
//!   operation's hash and exactly what it runs ([`printable`]).
//! - **Reported, signed.** Each state change (`running` with its pane and
//!   operation hash, then `succeeded` or `failed` with the exit code) is
//!   signed with the machine's key (`arugula run event v1`,
//!   `arugula_e2e::run::Event`) and kept in the ledger before it's sent,
//!   then sent until control takes it (`POST /api/hosted/work/report`). If
//!   control says the run moved on without it (canceled before it started),
//!   it's stopped.
//! - **Revokes stop runs.** A revoke that says `running stop`, relayed or
//!   with a pull, stops the runs going under the grants it covers
//!   ([`stop_revoked`]).
//! - **Its log and artifacts** go to the run-log bucket as it runs and
//!   when it ends, before its end is reported (M213, [`crate::run_logs`]).
//! - **Busy while it runs** ([`running`]): the machine says it's working
//!   (M180's activity), so the platform holds its box (M181) and it doesn't
//!   pause mid-run.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use arugula_control_wire::{
    run_logs::ARTIFACTS_ENV,
    work::{Report, Reported, TAKES_DEPLOY, TAKES_RUN, WORK_REPORT, Work, WorkItem, WorkQuery},
};
use arugula_e2e::{
    now_ms,
    project_grant::ProjectGrant,
    run::{Event, Op, State},
};
use arugula_proto::{Policy, StartedBy, api::RunRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tracing::{info, warn};

use crate::{mux::Api, server::App};

#[path = "work_deploy.rs"]
mod deploy;

#[path = "work_apps_platform.rs"]
pub mod apps_platform;

#[path = "work_arugula_release.rs"]
pub mod arugula_release;

/// The ledger's directory, in the state directory.
pub const DIR: &str = "work";
/// How often the machine asks for work.
pub const PULL: Duration = Duration::from_secs(5);
/// The session a run's pane opens in.
pub const SESSION: &str = "runs";
/// What a run that a restart interrupted fails with.
pub const INTERRUPTED: &str = "interrupted by a restart; not run again";
/// What a run stopped by a revoke fails with.
pub const REVOKED: &str = "stopped: the project's standing grant was revoked";
/// What a run control canceled while it started fails with.
pub const CANCELED: &str = "stopped: control says it moved on without this machine";
/// A run's environment's `PATH`.
const RUN_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
/// The box's provider's agent on loopback (E2B's envd and its sibling),
/// which a run must never reach.
const PROVIDER_PORTS: &str = "49982:49983";

/// Runs this daemon is watching now: while there's one, it's busy.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Wakes the pull loop: a start named a run (`runs.start`).
static NUDGE: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Runs being stopped, and why.
static STOPPING: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// One writer at a time for the ledger.
static LEDGER: Mutex<()> = Mutex::new(());

/// Ask for work now, not at the next pull.
pub fn nudge() {
    NUDGE.notify_one();
}

/// Whether a run is going on this machine ([`crate::hosted::activity`]).
pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst) > 0
}

fn pull_every() -> Duration {
    std::env::var("ARUGULA_WORK_PULL_MS").ok().and_then(|v| v.parse().ok()).map_or(PULL, Duration::from_millis)
}

/// A state change, as signed and sent, and whether control has answered
/// it (taken it, or refused it for good).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub report: Report,
    #[serde(default)]
    pub answered: bool,
    /// Why control refused it, when it did (M219: a deploy's start waits
    /// for control to take it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// A run this machine took, in `<state>/work/ID.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub run: String,
    pub op_hash: String,
    /// What control said it was in when it was taken: `pending` or
    /// `approved`.
    pub from: String,
    /// Its latest event then: this machine's are the ones after.
    pub seq: u64,
    /// `running`, `succeeded` or `failed`.
    pub state: String,
    #[serde(default)]
    pub pane: Option<u32>,
    #[serde(default)]
    pub epoch: Option<u64>,
    pub taken_at: u64,
    /// The standing grant it ran under (M211).
    #[serde(default)]
    pub grant: Option<ProjectGrant>,
    /// An agent run's turn on a hosted box (M185), ended with the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    /// The deploy a deploy's run applies (M219): never followed after a
    /// restart, as its driver went with the daemon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy: Option<String>,
    #[serde(default)]
    pub events: Vec<Kept>,
    /// What's been sent of its log and artifacts (M213).
    #[serde(default)]
    pub logs: crate::run_logs::Sent,
}

impl Ledger {
    /// The state the run is in by this machine's own reports.
    fn last(&self) -> String {
        self.events.last().map_or_else(|| self.from.clone(), |k| k.report.to.clone())
    }

    /// Sign the run's next state change and keep it, to be sent.
    pub(crate) fn push(&mut self, keys: &arugula_e2e::DeviceKeys, me: &str, to: State, detail: serde_json::Value) {
        let from = self.last();
        let seq = self.seq + self.events.len() as u64 + 1;
        let detail = detail.to_string();
        let at = now_ms();
        let ev = Event { run: &self.run, seq, from: State::parse(&from), to, at, detail: &detail, daemon: me };
        let sig = ev.sign(keys);
        let report = Report { run: self.run.clone(), seq, from, to: to.as_str().into(), at, detail, sig };
        self.events.push(Kept { report, answered: false, refused: None });
        if to.terminal() || to == State::Running {
            self.state = to.as_str().into();
        }
    }
}

/// Where run `run`'s ledger is, if it's a run's id (`run_` and 32 hex):
/// control's word goes in a file name only once it's that.
pub fn ledger_path(dir: &Path, run: &str) -> Option<PathBuf> {
    good_id(run).then(|| dir.join(DIR).join(format!("{run}.json")))
}

fn good_id(run: &str) -> bool {
    run.strip_prefix("run_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Run `run`'s entry: `Ok(None)` only when there's none. One that's there
/// but can't be read is an error, and counts as taken.
pub fn read(dir: &Path, run: &str) -> std::io::Result<Option<Ledger>> {
    let Some(path) = ledger_path(dir, run) else { return Err(std::io::Error::other("not a run's id")) };
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map(Some).map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether a run's entry is there (or can't be read): taken either way.
fn taken(dir: &Path, run: &str) -> bool {
    !matches!(read(dir, run), Ok(None))
}

/// Make run `l.run`'s entry, once: `true` only for the call that made it
/// (written and fsynced, the file and the directory). Any error but none
/// being there counts as taken.
pub fn claim(dir: &Path, l: &Ledger) -> bool {
    let Some(path) = ledger_path(dir, &l.run) else { return false };
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let made = (|| -> std::io::Result<()> {
        crate::store::private_dir(&dir.join(DIR))?;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&path)?;
        f.write_all(&serde_json::to_vec_pretty(l).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
        #[cfg(unix)]
        std::fs::File::open(dir.join(DIR))?.sync_all()?;
        Ok(())
    })();
    match made {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            warn!(run = l.run, error = %e, "hosted: can't make a run's ledger entry; it doesn't start");
            false
        }
    }
}

/// Write an entry that's there already, fsynced.
pub fn write(dir: &Path, l: &Ledger) -> anyhow::Result<()> {
    let path = ledger_path(dir, &l.run).ok_or_else(|| anyhow::anyhow!("not a run's id: {:?}", l.run))?;
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    crate::store::write_atomic(&path, &serde_json::to_vec_pretty(l)?)?;
    Ok(())
}

/// Change run `run`'s entry with `f`, written and fsynced, if it's there:
/// one writer at a time, so a change made meanwhile (control's answers)
/// isn't lost.
pub fn update(dir: &Path, run: &str, f: impl FnOnce(&mut Ledger)) -> anyhow::Result<Option<Ledger>> {
    let path = ledger_path(dir, run).ok_or_else(|| anyhow::anyhow!("not a run's id: {run:?}"))?;
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut l) = read(dir, run)? else { return Ok(None) };
    f(&mut l);
    crate::store::write_atomic(&path, &serde_json::to_vec_pretty(&l)?)?;
    Ok(Some(l))
}

/// Every run in the ledger that reads. One that doesn't is said, and is
/// still taken ([`taken`]): it's never started again.
fn all(dir: &Path) -> Vec<Ledger> {
    let Ok(entries) = std::fs::read_dir(dir.join(DIR)) else { return vec![] };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(run) = name.strip_suffix(".json") else { continue };
        match read(dir, run) {
            Ok(Some(l)) => out.push(l),
            Ok(None) => {}
            Err(err) => {
                warn!(file = name, error = %err, "hosted: a ledger entry doesn't read; its run counts as taken")
            }
        }
    }
    out.sort_by_key(|l| l.taken_at);
    out
}

/// Why a run can't start: a check's name, and what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub &'static str, pub String);

/// Read an S37 operation (`arugula op v1`, as `arugula_e2e::run::Op`
/// writes it) for daemon `me`: a command (`kind run`), or an instruction
/// for Claude Code (`kind agent`, the default recipe or `claude`).
pub fn parse_op(op: &str, me: &str) -> Result<Op, Refused> {
    let Some((daemon, op)) = Op::decode(op) else {
        return Err(Refused("op_form", "the operation isn't one this machine reads".into()));
    };
    if daemon != me {
        return Err(Refused("op_daemon", "the operation is for another machine".into()));
    }
    if op.agent.as_deref().is_some_and(|a| a != "claude") {
        return Err(Refused("op_agent", "this machine runs only Claude Code as a run's agent".into()));
    }
    Ok(op)
}

/// The command line a shell runs: each argument quoted, so it's the
/// arguments, not a script.
pub fn command_line(argv: &[String]) -> String {
    argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

/// One word for a shell, single-quoted.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `s` with nothing that doesn't print as itself: control characters
/// (escapes included) and the invisible ones that reorder or hide text
/// (bidi controls, zero-width characters), each written as `\u{…}`. What
/// a run shows of itself is what it runs.
pub fn printable(s: &str) -> String {
    let hidden = |c: char| {
        c.is_control()
            || matches!(c as u32, 0x061C | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB)
    };
    s.chars().map(|c| if hidden(c) { format!("\\u{{{:x}}}", c as u32) } else { c.to_string() }).collect()
}

/// Who runs run as: a user with no sudo and no way into the daemon's
/// state, and their home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAs {
    pub user: String,
    pub home: PathBuf,
}

impl RunAs {
    /// Run `run`'s own directory, its `HOME` too.
    pub fn work(&self, run: &str) -> PathBuf {
        self.home.join("work").join(run)
    }
}

/// The user runs run as (`ARUGULA_HOSTED_RUN_USER`, `runner` by default;
/// empty: this daemon's own, for tests only). The template makes it (M177's
/// Dockerfile). A box without it runs nothing (fail closed).
pub fn run_as() -> Result<Option<RunAs>, Refused> {
    let user = std::env::var("ARUGULA_HOSTED_RUN_USER").unwrap_or_else(|_| "runner".into());
    if user.is_empty() {
        return Ok(None);
    }
    let missing = || {
        Refused(
            "run_user",
            format!("this box has no user `{user}` to run runs as: rebuild the workspace from a newer template"),
        )
    };
    if user.starts_with('-') || !user.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)) {
        return Err(missing());
    }
    let out = std::process::Command::new("getent").args(["passwd", &user]).output().map_err(|_| missing())?;
    let line = String::from_utf8_lossy(&out.stdout);
    let home = line.trim_end().split(':').nth(5).filter(|h| h.starts_with('/') && *h != "/").ok_or_else(missing)?;
    Ok(Some(RunAs { user, home: PathBuf::from(home) }))
}

/// The directory a run's copy is made from: an absolute path under this
/// daemon's home (`/home/user`), each part a plain name that isn't hidden
/// (no `..`, no dot-directory like `.config` or `.local`, where its state
/// and secrets are). A run never works in it: it gets its own copy.
pub fn source_dir(dir: &str, home: &Path) -> Result<PathBuf, Refused> {
    let refuse = || {
        Refused("op_directory", format!("a run copies a directory under {}, not {}", home.display(), printable(dir)))
    };
    let Some(rest) = dir.strip_prefix('/') else { return Err(refuse()) };
    // Each part as written: no empty one, no `.` or `..`, nothing hidden.
    if rest.split('/').any(|part| part.is_empty() || part.starts_with('.')) {
        return Err(refuse());
    }
    let p = Path::new(dir);
    if !p.starts_with(home) || p == home {
        return Err(refuse());
    }
    Ok(p.to_path_buf())
}

/// Claude Code's own CLI for an agent run: `claude` on the daemon's PATH,
/// else the one the daemon's Claude adapter brings.
pub fn claude_bin() -> Option<PathBuf> {
    if let Some(p) =
        std::env::var_os("PATH").iter().flat_map(std::env::split_paths).map(|d| d.join("claude")).find(|p| p.is_file())
    {
        return Some(p);
    }
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let p = crate::agent::defs::agents_dir(&arugula_mux::sys::home())
        .join(format!("claude/node_modules/@anthropic-ai/claude-agent-sdk-linux-{arch}/claude"));
    p.is_file().then_some(p)
}

/// The login an agent run is given (its name, and where the daemon has
/// it): the daemon's own, from its environment or its key files, as an
/// agent block in a VM gets it.
fn agent_login() -> Option<(&'static str, String)> {
    for k in ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN"] {
        if let Some(v) = std::env::var(k).ok().filter(|v| !v.trim().is_empty()) {
            return Some((k, v.trim().to_owned()));
        }
    }
    let config = arugula_proto::dirs::config_dir().unwrap_or_else(|| arugula_mux::sys::home().join(".config/arugula"));
    let file = |env: &str, name: &str| std::env::var_os(env).map(PathBuf::from).unwrap_or_else(|| config.join(name));
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    if let Some(k) = read(file("ARUGULA_ANTHROPIC_KEY_FILE", "anthropic-key")) {
        return Some(("ANTHROPIC_API_KEY", k));
    }
    read(file("ARUGULA_CLAUDE_TOKEN_FILE", "claude-oauth-token")).map(|t| ("CLAUDE_CODE_OAUTH_TOKEN", t))
}

/// What a run runs, in the shell's words, and its banner's second line.
fn what(op: &Op, claude: Option<&Path>) -> Result<(String, String), Refused> {
    match &op.instruction {
        None => {
            let cmd = command_line(&op.argv);
            Ok((cmd.clone(), format!("$ {cmd}")))
        }
        Some(i) => {
            let claude = claude.ok_or_else(|| Refused("op_agent", "Claude Code isn't on this box".into()))?;
            let cmd = format!(
                "{} -p {} --output-format text --dangerously-skip-permissions",
                quote(&claude.to_string_lossy()),
                quote(i)
            );
            Ok((cmd, format!("claude -p {}", quote(i))))
        }
    }
}

/// What an agent run is given: Claude Code's CLI; the daemon's login,
/// from a file the pane's shell reads and removes (away from a hosted box);
/// and its environment besides (M185's placeholder key, on one).
#[derive(Debug, Clone, Copy, Default)]
pub struct Agent<'a> {
    pub claude: Option<&'a Path>,
    pub login: Option<(&'a str, &'a Path)>,
    pub env: &'a [(String, String)],
}

/// What a run's pane runs (with `$SHELL -l -c`, as this daemon's user, in
/// `/`). First, two lines saying which run and exactly what (its
/// operation's hash, as signed in its events, and the command), escaped.
/// Then, as `as_` (`sudo -n -u`, an empty environment, no profile): a
/// fresh `~runner/work/ID` as its `HOME` and working directory, with its
/// own copy of `src` (a clone of a git repository, which its own git
/// config trusts though `user` owns it, else a copy), and the
/// command. An agent's login ([`Agent::login`]) is a file of this
/// daemon's the outer shell reads and removes, which reaches the run on its
/// standard input, never on a command line. `$ARUGULA_ARTIFACTS` names the
/// directory it leaves its artifacts in (M213): `artifacts`, made empty.
pub fn pane_command(
    run: &str,
    op_hash: &str,
    op: &Op,
    src: Option<&Path>,
    as_: Option<&RunAs>,
    agent: &Agent<'_>,
    artifacts: &Path,
) -> Result<String, Refused> {
    let Agent { claude, login, env: env_extra } = *agent;
    let (cmd, shown) = what(op, claude)?;
    let banner = format!(
        r"printf '%s\n' {} {}",
        quote(&printable(&format!("arugula run {run} · operation sha256:{op_hash}"))),
        quote(&printable(&shown))
    );
    let a = quote(&artifacts.to_string_lossy());
    let Some(as_) = as_ else {
        // Tests only: as this daemon's own user, where it is, with the
        // agent's environment besides its own.
        let env: String = env_extra
            .iter()
            .filter(|(k, _)| k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .map(|(k, v)| format!(" {k}={}", quote(v)))
            .collect();
        let env = if env.is_empty() { String::new() } else { format!("env{env} ") };
        return Ok(format!(
            "{banner}; rm -rf -- {a}; mkdir -p -m 700 -- {a}; export {ARTIFACTS_ENV}={a}; exec {env}{cmd}"
        ));
    };
    let work = as_.work(run);
    let mut inner = format!(
        "set -e; w={}; rm -rf -- \"$w\"; mkdir -p -m 700 -- \"$w\"; cd -- \"$w\"; mkdir -p -m 700 -- {a}; ",
        quote(&work.to_string_lossy())
    );
    if let Some(src) = src {
        let s = quote(&src.to_string_lossy());
        inner.push_str(&format!(
            "if [ -e {s}/.git ]; then git config --global --add safe.directory {s}; \
             git config --global --add safe.directory {s}/.git; git clone -q --no-hardlinks -- {s} repo; \
             else cp -R -- {s} repo; fi; cd repo; "
        ));
    }
    if let Some((name, _)) = login {
        inner.push_str(&format!("IFS= read -r k || true; export {name}=\"$k\"; unset k; "));
    }
    inner.push_str(&format!("set +e; exec {cmd}"));
    let mut env = format!(
        "env -i HOME={h} USER={u} LOGNAME={u} SHELL=/bin/bash PATH={RUN_PATH} TERM=xterm-256color LANG=C.UTF-8 \
         {ARTIFACTS_ENV}={a}",
        h = quote(&work.to_string_lossy()),
        u = as_.user
    );
    for (k, v) in env_extra.iter().filter(|(k, _)| k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
        env.push_str(&format!(" {k}={}", quote(v)));
    }
    let sudo = format!("exec sudo -n -u {} -- {env} /bin/bash --noprofile --norc -c {}", as_.user, quote(&inner));
    Ok(match login {
        Some((_, file)) => {
            let f = quote(&file.to_string_lossy());
            format!("{banner}; {{ cat -- {f}; rm -f -- {f}; }} | {sudo}")
        }
        None => format!("{banner}; {sudo} < /dev/null"),
    })
}

/// Whether the project's standing grant lets the platform start `item` on
/// this machine (M211): checked against the machine's own enrollment, with
/// the revokes it carries kept first ([`crate::hosted::runs::allows`]). An
/// approved run (one with no grant) waits for M218's check of its
/// approval: until then it doesn't start here (fail closed).
pub fn grant(app: &App, item: &WorkItem) -> Result<(), Refused> {
    if item.state != State::Pending.as_str() {
        return Err(Refused("approval_unchecked", "this machine can't check a run's approval yet".into()));
    }
    let Some(g) = item.grant.clone() else {
        return Err(Refused("no_grant", "a pending run starts only under the project's standing grant".into()));
    };
    let start = arugula_control_wire::runs::RunStart {
        project: item.project.clone(),
        workspace: item.workspace.clone(),
        run: Some(item.run.clone()),
        cap: "runs.start".into(),
        grant: g,
        revokes: item.revokes.clone(),
    };
    crate::hosted::runs::allows(app, &start).map_err(|r| Refused(r.code, r.says.into()))
}

/// Who this machine is: its keys, device id, project and workspace, while
/// it's a hosted machine in control.
fn me(app: &App) -> Option<(Arc<arugula_e2e::DeviceKeys>, String, String, String)> {
    let e = app.control.enrolled()?;
    let pin = e.saved.hosted.as_ref()?;
    Some((e.keys.clone(), pin.cert.device.clone(), pin.cert.project.clone(), pin.cert.workspace.clone()))
}

/// The checks before a run starts, in S37's order (the grant, then the
/// operation): the operation, and the directory its copy is made from.
pub fn check(
    app: &App,
    item: &WorkItem,
    me: &str,
    project: &str,
    workspace: &str,
) -> Result<(Op, Option<PathBuf>), Refused> {
    if item.project != project || item.workspace != workspace {
        return Err(Refused("op_workspace", "the run is another workspace's".into()));
    }
    if hex::encode(Sha256::digest(item.op.as_bytes())) != item.op_hash {
        return Err(Refused("op_hash", "the operation's bytes don't hash to the run's operation hash".into()));
    }
    // A deploy's box (M219) takes its one deploy's run, nothing else.
    if deploy::deploy_box(app) && item.deploy.is_none() {
        return Err(Refused("deploy_box", "this box was made for one deploy, and takes nothing else".into()));
    }
    // A deploy's run (M219) goes by its owner's approval of the deploy,
    // checked here against what this machine pinned; any other run by the
    // project's standing grant.
    match &item.deploy {
        Some(d) => deploy::check(app, item, d)?,
        None => grant(app, item)?,
    }
    let op = parse_op(&item.op, me)?;
    deploy::matches(&op, item.deploy.as_ref())?;
    let src = op.directory.as_deref().map(|d| source_dir(d, &arugula_mux::sys::home())).transpose()?;
    Ok((op, src))
}

/// Keep loopback from `as_`'s processes to the daemon's ports, the box
/// provider's and the privileged ones, through this daemon's sudo: a run
/// can't drive the daemon or start processes through the provider's agent.
/// nftables (`meta skuid`) first: E2B's kernel has nf_tables but not
/// iptables' owner match, so `-m owner` fails there; else `iptables` and
/// `ip6tables`. Once a daemon; until the rules are in, nothing runs (fail
/// closed).
pub fn firewall(state: &Path, as_: &RunAs) -> Result<(), Refused> {
    static DONE: Mutex<bool> = Mutex::new(false);
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    if *done {
        return Ok(());
    }
    if firewall_nft(state, &as_.user) {
        info!(user = as_.user, "hosted: runs can't reach the daemon's or the provider's ports (nftables)");
        *done = true;
        return Ok(());
    }
    for (bin, rule) in firewall_rules(state, &as_.user) {
        let run = |verb: &str| {
            std::process::Command::new("sudo")
                .args(["-n", bin, "-w", verb, "OUTPUT"])
                .args(if verb == "-I" { vec!["1"] } else { vec![] })
                .args(&rule)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        if !run("-C") && !run("-I") {
            return Err(Refused("firewall", format!("can't keep runs off the daemon's ports ({bin}): nothing runs")));
        }
    }
    info!(user = as_.user, "hosted: runs can't reach the daemon's or the provider's ports");
    *done = true;
    Ok(())
}

/// [`firewall`] through nftables: [`firewall_script`] for `user`'s uid,
/// loaded whole (`nft -f -`, which replaces the table each time). Whether it
/// took.
#[cfg(unix)]
fn firewall_nft(state: &Path, user: &str) -> bool {
    use std::io::Write as _;
    let Some(uid) = nix::unistd::User::from_name(user).ok().flatten().map(|u| u.uid.as_raw()) else { return false };
    let script = firewall_script(state, uid);
    let child = std::process::Command::new("sudo")
        .args(["-n", "nft", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    let wrote = child.stdin.take().is_some_and(|mut i| i.write_all(script.as_bytes()).is_ok());
    child.wait().is_ok_and(|s| s.success()) && wrote
}

#[cfg(not(unix))]
fn firewall_nft(_: &Path, _: &str) -> bool {
    false
}

/// The nftables table [`firewall_nft`] loads: TCP on loopback from `uid`
/// to the ports [`firewall_ports`] lists is reset. Declaring the table and
/// deleting it first makes loading it again replace it.
pub fn firewall_script(state: &Path, uid: u32) -> String {
    let ports = firewall_ports(state).iter().map(|p| p.replace(':', "-")).collect::<Vec<_>>().join(", ");
    format!(
        "table inet arugula_runs\ndelete table inet arugula_runs\ntable inet arugula_runs {{\n  chain out {{\n    \
         type filter hook output priority 0; policy accept;\n    \
         oif \"lo\" meta skuid {uid} meta l4proto tcp tcp dport {{ {ports} }} reject with tcp reset\n  }}\n}}\n"
    )
}

/// The loopback ports runs are kept off: the privileged ones, the box
/// provider's, and this daemon's (as its state directory says), each once
/// (an nftables set refuses one inside another).
fn firewall_ports(state: &Path) -> Vec<String> {
    let mut ports = vec!["1:1023".to_owned(), PROVIDER_PORTS.to_owned()];
    let inside = |p: u16, range: &str| {
        let (a, b) = range.split_once(':').unwrap_or((range, range));
        a.parse::<u16>().is_ok_and(|a| a <= p) && b.parse::<u16>().is_ok_and(|b| p <= b)
    };
    for f in ["listen", "block-listen"] {
        if let Some(p) = std::fs::read_to_string(state.join(f))
            .ok()
            .and_then(|a| a.trim().parse::<std::net::SocketAddr>().ok())
            .map(|a| a.port())
            .filter(|p| *p > 0)
            && !ports.iter().any(|r| inside(p, r))
        {
            ports.push(p.to_string());
        }
    }
    ports
}

/// The rules [`firewall`] keeps where nftables doesn't: each a REJECT of
/// TCP on loopback from `user` to some ports.
pub fn firewall_rules(state: &Path, user: &str) -> Vec<(&'static str, Vec<String>)> {
    let ports = firewall_ports(state);
    let mut out = Vec::new();
    for bin in ["iptables", "ip6tables"] {
        for p in &ports {
            let rule = ["-o", "lo", "-p", "tcp", "-m", "owner", "--uid-owner", user, "--dport", p, "-j", "REJECT"];
            out.push((bin, rule.iter().map(|s| (*s).to_owned()).collect()));
        }
    }
    out
}

/// Kill everything `as_` runs (`kill -KILL -1` as it): when a run ends or
/// is stopped, and before one starts, so nothing a run left carries into
/// the next.
pub async fn kill_all(as_: &RunAs) {
    let r = tokio::process::Command::new("sudo")
        .args(["-n", "-u", &as_.user, "--", "kill", "-KILL", "-1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    if let Err(e) = r {
        warn!(user = as_.user, error = %e, "hosted: can't kill what a run left");
    }
}

/// Stop run `l`: kill everything the run user runs (or, with no run user,
/// close its pane), and have it fail saying `why`.
async fn stop(app: &App, l: &Ledger, why: &str) {
    let first = STOPPING.lock().unwrap_or_else(|e| e.into_inner()).insert(l.run.clone(), why.into()).is_none();
    if !first {
        return;
    }
    info!(run = l.run, why, "hosted: stopping a run");
    match run_as() {
        Ok(Some(as_)) => kill_all(&as_).await,
        _ => {
            if let Some(p) = l.pane {
                app.mux.api(|r| Api::Close(p, r)).await;
            }
        }
    }
}

/// Stop the runs going under a standing grant that a revoke kept here
/// takes back, saying `running stop` (M211's check says which). Called as
/// a revoke arrives, and after each pull, with the project's revokes.
pub fn stop_revoked(app: &App) {
    let doomed: Vec<Ledger> = all(app.control.state_dir())
        .into_iter()
        .filter(|l| l.state == State::Running.as_str())
        .filter(|l| l.grant.as_ref().is_some_and(|g| crate::hosted::runs::stops(app, g)))
        .collect();
    if doomed.is_empty() {
        return;
    }
    let first = STOPPING.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect::<BTreeSet<_>>();
    let mux = app.mux.clone();
    let run_as = run_as();
    for l in doomed.into_iter().filter(|l| !first.contains(&l.run)) {
        STOPPING.lock().unwrap_or_else(|e| e.into_inner()).insert(l.run.clone(), REVOKED.into());
        info!(run = l.run, "hosted: stopping a run: its standing grant was revoked");
        match &run_as {
            Ok(Some(as_)) => {
                let as_ = as_.clone();
                tokio::spawn(async move { kill_all(&as_).await });
            }
            _ => {
                if let Some(p) = l.pane {
                    let (tx, _) = tokio::sync::oneshot::channel();
                    mux.send(crate::mux::Cmd::Api(Api::Close(p, tx)));
                }
            }
        }
    }
}

/// The pane's tab's name: which run, its operation's hash and what it
/// runs, outside the terminal, escaped and cut short.
fn tab_name(run: &str, op_hash: &str, shown: &str) -> String {
    let mut s = printable(&format!("{run} · {} · {shown}", &op_hash[..op_hash.len().min(12)]));
    if s.chars().count() > 96 {
        s = s.chars().take(95).collect::<String>() + "…";
    }
    s
}

/// Take `item`, once: a run in the ledger already (or one whose entry
/// can't be read) is only reported. One run goes at a time.
async fn take(app: &Arc<App>, item: WorkItem) {
    let dir = app.control.state_dir().to_owned();
    let Some((keys, me, project, workspace)) = me(app) else { return };
    if !good_id(&item.run) {
        warn!(run = item.run, "hosted: control sent work that isn't a run's id");
        return;
    }
    if taken(&dir, &item.run) {
        return;
    }
    let mut l = Ledger {
        run: item.run.clone(),
        op_hash: item.op_hash.clone(),
        from: item.state.clone(),
        seq: item.last_seq,
        state: State::Running.as_str().into(),
        pane: None,
        epoch: None,
        taken_at: now_ms(),
        grant: item.grant.clone(),
        turn: None,
        deploy: item.deploy.as_ref().map(|d| d.deploy.clone()),
        events: vec![],
        logs: Default::default(),
    };
    let checked = check(app, &item, &me, &project, &workspace).and_then(|(op, src)| {
        let as_ = run_as()?;
        // A deploy's box (M219) runs its deploy only as the run user, after
        // this daemon's user gave up root; checked, or nothing runs.
        if deploy::deploy_box(app) {
            deploy::deploy_box_user(as_.as_ref(), deploy::tests_unsafe())?;
        }
        if let Some(as_) = &as_ {
            firewall(&dir, as_)?;
            if deploy::deploy_box(app) {
                deploy::harden_sudo(as_)?;
            }
        }
        // On a hosted box an agent holds M185's placeholder key, which the
        // provider's proxy fills in; elsewhere, the daemon's own login.
        let brokered = crate::agent::hosted::gate().is_some();
        let login = (op.instruction.is_some() && !brokered).then(agent_login).flatten();
        if op.instruction.is_some() && !brokered && login.is_none() {
            return Err(Refused("op_agent", "this box has no login for Claude Code".into()));
        }
        Ok((op, src, as_, login))
    });
    // An agent run's turn, asked of the platform as any hosted agent's is
    // (M185): whose key pays, or why it can't run.
    let checked = match checked {
        Ok((op, src, as_, login)) if op.instruction.is_some() => match crate::agent::hosted::gate() {
            Some(g) => match g.start("claude").await {
                Ok(turn) => {
                    l.turn = Some(turn);
                    let env = crate::agent::hosted::env_here(crate::agent::defs::Kind::Claude);
                    Ok((op, src, as_, login, env))
                }
                Err(why) => Err(Refused("inference", why)),
            },
            None => Ok((op, src, as_, login, vec![])),
        },
        Ok((op, src, as_, login)) => Ok((op, src, as_, login, vec![])),
        Err(r) => Err(r),
    };
    let (op, src, as_, login, env_extra) = match checked {
        Ok(c) => c,
        Err(Refused(code, why)) => {
            info!(run = item.run, code, "hosted: a run's check at the start failed; it doesn't run");
            let detail = serde_json::json!({ "reason": format!("{code}: {why}"), "op": item.op_hash });
            l.push(&keys, &me, State::Failed, detail);
            claim(&dir, &l);
            return;
        }
    };
    // `running`, on disk, once, before anything starts.
    if !claim(&dir, &l) {
        return;
    }
    // Nothing a run left before (or a restart left) goes on into this one.
    if let Some(as_) = &as_ {
        kill_all(as_).await;
    }
    RUNNING.fetch_add(1, Ordering::SeqCst);
    // A deploy's apply: its driver, outside any pane, with the
    // environment's secrets (M219).
    if op.deploy.is_some() {
        tokio::spawn(deploy::apply(app.clone(), l, item, as_));
        return;
    }
    let fail = |mut l: Ledger, why: String| {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
        l.push(&keys, &me, State::Failed, serde_json::json!({ "reason": format!("start: {why}"), "op": l.op_hash }));
        if let Err(e) = write(&dir, &l) {
            warn!(run = l.run, error = %format!("{e:#}"), "hosted: can't write the ledger");
        }
    };
    // The agent's login, for the pane's shell to hand on and remove.
    let login_file = dir.join(DIR).join(format!("{}.login", item.run));
    let login = match &login {
        Some((name, value)) => {
            if let Err(e) = crate::store::write_atomic(&login_file, value.as_bytes()) {
                return fail(l, format!("the agent's login: {e}"));
            }
            Some((*name, login_file.as_path()))
        }
        None => None,
    };
    let claude = claude_bin();
    let artifacts = crate::run_logs::artifacts_dir(&dir, &item.run, as_.as_ref());
    let command = match pane_command(
        &item.run,
        &item.op_hash,
        &op,
        src.as_deref(),
        as_.as_ref(),
        &Agent { claude: claude.as_deref(), login, env: &env_extra },
        &artifacts,
    ) {
        Ok(c) => c,
        Err(Refused(code, why)) => return fail(l, format!("{code}: {why}")),
    };
    let shown = what(&op, claude.as_deref()).map(|w| w.1).unwrap_or_default();
    let events = app.mux.events();
    let req = RunRequest {
        command: Some(command),
        session: Some(SESSION.into()),
        // As the run user, the pane's shell starts in `/`, and the run in
        // its own directory: never in one the operation names.
        cwd: Some(if as_.is_some() { "/".into() } else { op.directory.clone().unwrap_or_else(|| "/".into()) }),
        policy: Some(Policy::None),
        ..Default::default()
    };
    let pane = match app.mux.api(|r| Api::Run(req, r)).await {
        Some(Ok(p)) => p,
        failed => {
            let _ = std::fs::remove_file(&login_file);
            let why = match failed {
                Some(Err(e)) => e,
                _ => "the daemon is stopping".into(),
            };
            return fail(l, why);
        }
    };
    // Tagged with its run, outside the terminal too.
    let _ = app.mux.api(|r| Api::OwnTab(pane, Some(tab_name(&item.run, &item.op_hash, &shown)), r)).await;
    app.mux.send(crate::mux::Cmd::Api(Api::StartedBy(
        pane,
        StartedBy { by: format!("run {} (operation sha256:{})", item.run, item.op_hash), block: None },
    )));
    let epoch = app.mux.api(|r| Api::Pane(pane, r)).await.flatten().map(|h| h.epoch);
    l.pane = Some(pane);
    l.epoch = epoch;
    // The operation's hash, signed: under a standing grant nothing else
    // signed binds what ran, so the machine says what it ran.
    l.push(&keys, &me, State::Running, serde_json::json!({ "pane": pane, "epoch": epoch, "op": l.op_hash }));
    if let Err(e) = write(&dir, &l) {
        warn!(run = l.run, error = %format!("{e:#}"), "hosted: can't write the ledger");
    }
    info!(run = l.run, pane, kind = op.kind(), "hosted: a run started");
    tokio::spawn(watch(app.clone(), l, Some(events)));
}

/// How a run's pane ended: its exit code, or why it has none.
async fn ended(
    app: &App,
    pane: u32,
    epoch: Option<u64>,
    mut events: Option<tokio::sync::broadcast::Receiver<arugula_proto::Event>>,
) -> Result<i32, String> {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        match app.mux.api(|r| Api::Pane(pane, r)).await.flatten() {
            None => return Err("its pane was closed".into()),
            Some(h) if epoch.is_some_and(|e| e != h.epoch) => return Err(INTERRUPTED.into()),
            Some(h) => match h.status().exited {
                Some(Some(code)) => return Ok(code),
                Some(None) => return Err("stopped".into()),
                None => {}
            },
        }
        let wait = tokio::time::sleep(Duration::from_secs(1));
        match events.as_mut() {
            Some(rx) => tokio::select! {
                e = rx.recv() => match e {
                    Ok(e) if e.pane == Some(pane) => {
                        if let arugula_proto::EventKind::Exit { code, .. } = e.kind {
                            return code.ok_or_else(|| "stopped".into());
                        }
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => events = None,
                },
                _ = wait => {}
            },
            None => wait.await,
        }
    }
}

/// Follow a running run to its end, kill what it left, and keep how it
/// ended.
async fn watch(app: Arc<App>, l: Ledger, events: Option<tokio::sync::broadcast::Receiver<arugula_proto::Event>>) {
    let Some(pane) = l.pane else { return };
    // Its log goes to the run-log bucket as it runs (M213).
    let (done, follow) = tokio::sync::watch::channel(false);
    let follower = tokio::spawn(crate::run_logs::follow(app.clone(), l.run.clone(), pane, l.epoch, follow));
    let mut end = ended(&app, pane, l.epoch, events).await;
    let as_ = run_as().ok().flatten();
    if let Some(as_) = &as_ {
        kill_all(as_).await;
    }
    let dir = app.control.state_dir().to_owned();
    let _ = std::fs::remove_file(dir.join(DIR).join(format!("{}.login", l.run)));
    if let (Some(turn), Some(g)) = (l.turn.clone(), crate::agent::hosted::gate()) {
        g.end(turn).await;
    }
    // The rest of its log and its artifacts, before its end is reported,
    // while the box is still busy with it.
    let _ = done.send(true);
    let _ = follower.await;
    crate::run_logs::finish(&app, &l.run, pane, l.epoch, as_.as_ref()).await;
    RUNNING.fetch_sub(1, Ordering::SeqCst);
    if let Some(why) = STOPPING.lock().unwrap_or_else(|e| e.into_inner()).remove(&l.run) {
        end = Err(why);
    }
    let Some((keys, me, ..)) = me(&app) else { return };
    // Onto the entry as it is now: what was written meanwhile (control's
    // answers) stands.
    let ended = update(&dir, &l.run, |now| match end {
        Ok(0) => now.push(&keys, &me, State::Succeeded, serde_json::json!({ "exit": 0 })),
        Ok(code) => now.push(&keys, &me, State::Failed, serde_json::json!({ "exit": code })),
        Err(why) => now.push(&keys, &me, State::Failed, serde_json::json!({ "reason": why })),
    });
    match ended {
        Ok(Some(now)) => info!(run = now.run, state = now.state, "hosted: a run ended"),
        Ok(None) => warn!(run = l.run, "hosted: a run's ledger entry is gone"),
        Err(e) => warn!(run = l.run, error = %format!("{e:#}"), "hosted: can't write the ledger"),
    }
    send(&app, &l.run).await;
}

/// After a restart: carry on following a run whose pane is still there
/// (the pane the ledger names, tagged with the run's id: a restart gives
/// it a new epoch); fail the others, which are never run again; close every pane in the
/// `runs` session the ledger doesn't account for (one a run started just
/// before its entry said so), and kill what they left.
async fn resume(app: &Arc<App>) {
    let dir = app.control.state_dir().to_owned();
    let Some((keys, me, ..)) = me(app) else { return };
    let ledger = all(&dir);
    let mut followed = false;
    for mut l in ledger.iter().filter(|l| l.state == State::Running.as_str()).cloned() {
        // Its pane, if it survived: the pane the entry names, still the
        // run's (tagged with its id when it started). Panes come back from
        // their shims as the daemon starts, each with a new epoch.
        let live = match l.pane {
            Some(p) => {
                let mut h = None;
                for _ in 0..20 {
                    h = app.mux.api(|r| Api::Pane(p, r)).await.flatten();
                    if h.is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                // A restored pane replays its history before it adopts what
                // its shim kept: until then it isn't running, nor ended.
                if let Some(h) = h.clone() {
                    let _ = tokio::task::spawn_blocking(move || h.begun(Duration::from_secs(30))).await;
                }
                let panes = match h {
                    Some(_) => app.mux.api(Api::Panes).await.unwrap_or_default(),
                    None => vec![],
                };
                let its = panes.iter().any(|s| s.info.id == p && s.session_name == SESSION && runs_pane(s, &l.run));
                // Its program kept by its shim: running, or ended while the
                // daemon was down. A pane only restored from the layout
                // (its program died with the box) waits for Enter: it isn't.
                h.filter(|h| its && (h.running() || h.status().exited.is_some())).map(|h| h.epoch)
            }
            None => None,
        };
        if let Some(epoch) = live
            && !followed
            && l.last() == State::Running.as_str()
            && l.deploy.is_none()
        {
            info!(run = l.run, "hosted: following a run that was running before the restart");
            followed = true;
            // Its pane's output is a new epoch now: the one to follow.
            l.epoch = Some(epoch);
            if let Err(e) = write(&dir, &l) {
                warn!(run = l.run, error = %format!("{e:#}"), "hosted: can't write the ledger");
            }
            RUNNING.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(watch(app.clone(), l, None));
            continue;
        }
        info!(run = l.run, "hosted: a run was interrupted by the restart; it isn't run again");
        l.push(&keys, &me, State::Failed, serde_json::json!({ "reason": INTERRUPTED }));
        if let Err(e) = write(&dir, &l) {
            warn!(run = l.run, error = %format!("{e:#}"), "hosted: can't write the ledger");
        }
    }
    let accounted: BTreeSet<u32> = all(&dir).iter().filter_map(|l| l.pane).collect();
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    for p in panes.iter().filter(|p| p.session_name == SESSION && !accounted.contains(&p.info.id)) {
        info!(pane = p.info.id, tab = ?p.tab_name, "hosted: closing a run's pane the ledger doesn't account for");
        app.mux.api(|r| Api::Close(p.info.id, r)).await;
    }
    if !followed && let Ok(Some(as_)) = run_as() {
        kill_all(&as_).await;
    }
}

/// Whether `pane` is run `run`'s: its tab names the run, or it says the
/// run started it, as [`take`] tagged it.
fn runs_pane(pane: &arugula_proto::api::PaneSummary, run: &str) -> bool {
    let tab = pane.tab_name.as_deref().is_some_and(|t| t.starts_with(&format!("{run} ")));
    let by = pane.info.started_by.as_ref().is_some_and(|s| s.by.starts_with(&format!("run {run} ")));
    tab || by
}

/// Send run `run`'s reports control hasn't answered, in order, until one
/// doesn't get through.
async fn send(app: &App, run: &str) {
    // One sender at a time: a run's end (`watch`) and the pull loop both
    // send, and each would otherwise send the same unanswered report.
    static SENDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = SENDING.lock().await;
    let dir = app.control.state_dir().to_owned();
    loop {
        let Ok(Some(l)) = read(&dir, run) else { return };
        let Some(i) = l.events.iter().position(|k| !k.answered) else { return };
        let r = &l.events[i].report;
        let got: anyhow::Result<Reported> = app.control.post_signed(WORK_REPORT, r).await;
        let mut refused = None;
        let canceled = match got {
            Ok(Reported::Taken | Reported::Already) => false,
            Ok(Reported::Refused { why, now, .. }) => {
                warn!(run, to = r.to, %why, "hosted: control refused a run's report");
                refused = Some(why);
                // It moved on without this machine (canceled before it
                // started): stop what it started.
                r.to == State::Running.as_str() && now.as_deref().and_then(State::parse).is_some_and(State::terminal)
            }
            Err(e) => {
                info!(run, error = %format!("{e:#}"), "hosted: a run's report didn't get through; trying again");
                return;
            }
        };
        let now = match update(&dir, run, |now| {
            if let Some(k) = now.events.get_mut(i) {
                k.answered = true;
                k.refused = refused.clone();
            }
        }) {
            Ok(Some(now)) => now,
            Ok(None) => return,
            Err(e) => {
                warn!(run, error = %format!("{e:#}"), "hosted: can't write the ledger");
                return;
            }
        };
        if canceled && now.state == State::Running.as_str() {
            stop(app, &now, CANCELED).await;
        }
    }
}

/// Send every report control hasn't answered.
async fn send_all(app: &App, open: &mut BTreeSet<String>) {
    let dir = app.control.state_dir().to_owned();
    for run in open.clone() {
        send(app, &run).await;
        let done = match read(&dir, &run) {
            Ok(Some(l)) => l.state != State::Running.as_str() && l.events.iter().all(|k| k.answered),
            _ => true,
        };
        if done {
            open.remove(&run);
        }
    }
}

/// A hosted machine's work: once it's in control, follow up what a restart
/// left, then pull, take what's ready (one run at a time), stop what a
/// revoke covers, and send what's unsent.
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        let dir = app.control.state_dir().to_owned();
        let every = pull_every();
        while me(&app).is_none() {
            tokio::time::sleep(every.min(Duration::from_secs(1))).await;
        }
        resume(&app).await;
        let mut open: BTreeSet<String> = all(&dir)
            .into_iter()
            .filter(|l| l.state == State::Running.as_str() || l.events.iter().any(|k| !k.answered))
            .map(|l| l.run)
            .collect();
        loop {
            send_all(&app, &mut open).await;
            if me(&app).is_some() {
                // Only a deploy's own box (M219) takes deploys.
                let takes: &[&str] = if deploy::deploy_box(&app) { &[TAKES_RUN, TAKES_DEPLOY] } else { &[TAKES_RUN] };
                match app.control.get_signed::<Work>(&WorkQuery::path(takes)).await {
                    Ok(w) => {
                        // The project's revokes come with every pull: kept,
                        // then checked against the runs going.
                        crate::hosted::runs::keep_revokes(&app, &w.revokes);
                        for item in w.items {
                            if running() {
                                break;
                            }
                            let run = item.run.clone();
                            take(&app, item).await;
                            if !matches!(read(&dir, &run), Ok(None)) {
                                open.insert(run);
                            }
                        }
                    }
                    Err(e) => info!(error = %format!("{e:#}"), "hosted: can't ask control for work yet"),
                }
                stop_revoked(&app);
                send_all(&app, &mut open).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                _ = NUDGE.notified() => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operation_is_read_as_written() {
        let run = Op { argv: vec!["echo".into(), "it's".into()], ..Op::default() };
        let got = parse_op(&run.encode("d1"), "d1").unwrap();
        assert_eq!(got, run);
        assert_eq!(command_line(&got.argv), r"'echo' 'it'\''s'");
        assert_eq!(parse_op(&run.encode("d1"), "d2").unwrap_err().0, "op_daemon");
        let agent = Op { instruction: Some("go".into()), ..Op::default() };
        assert_eq!(parse_op(&agent.encode("d1"), "d1").unwrap(), agent);
        let codex = Op { instruction: Some("go".into()), agent: Some("codex".into()), ..Op::default() };
        assert_eq!(parse_op(&codex.encode("d1"), "d1").unwrap_err().0, "op_agent");
        let extra = format!("{}env HOME\n", run.encode("d1"));
        assert_eq!(parse_op(&extra, "d1").unwrap_err().0, "op_form");
    }

    /// C1: a run never works in a directory the platform names: only in
    /// its own copy of one under the daemon's home, and never of a hidden
    /// one (state, secrets).
    #[test]
    fn a_runs_directory_is_only_ever_copied_from_the_home() {
        let home = Path::new("/home/user");
        assert_eq!(source_dir("/home/user/app", home).unwrap(), Path::new("/home/user/app"));
        assert_eq!(source_dir("/home/user/src/app", home).unwrap(), Path::new("/home/user/src/app"));
        for bad in [
            "/home/user",
            "/home/user/",
            "/home/user/.local/state/arugula",
            "/home/user/.config/arugula",
            "/home/user/app/.git",
            "/home/user/../user/app",
            "/home/user/./app",
            "/etc",
            "/home/userx/app",
            "app",
            "",
        ] {
            assert_eq!(source_dir(bad, home).map_err(|r| r.0), Err("op_directory"), "{bad}");
        }
    }

    #[cfg(unix)]
    fn sh(script: &str) -> String {
        let out = std::process::Command::new("sh").args(["-c", script]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    // Hosted boxes are Linux: its paths and shell.
    #[cfg(unix)]
    #[test]
    fn a_pane_says_what_it_runs_and_runs_it_as_the_run_user() {
        let op = Op { argv: vec!["echo".into(), "it's\x1b[2J\u{202e}".into()], ..Op::default() };
        let dir = crate::hosted::scratch("pane-command");
        let mine = pane_command("run_1", "abc", &op, None, None, &Agent::default(), &dir.join("a")).unwrap();
        assert_eq!(
            sh(&mine),
            "arugula run run_1 · operation sha256:abc\n$ 'echo' 'it'\\''s\\u{1b}[2J\\u{202e}'\nit's\x1b[2J\u{202e}\n"
        );
        let as_ = RunAs { user: "runner".into(), home: "/home/runner".into() };
        let src = Path::new("/home/user/app");
        let op = Op { argv: vec!["echo".into(), "it's".into()], ..Op::default() };
        let artifacts = as_.work("run_1").join(".arugula/artifacts");
        let theirs = pane_command("run_1", "abc", &op, Some(src), Some(&as_), &Agent::default(), &artifacts).unwrap();
        assert!(
            theirs.contains("; exec sudo -n -u runner -- env -i HOME='/home/runner/work/run_1' USER=runner "),
            "{theirs}"
        );
        assert!(theirs.contains("/bin/bash --noprofile --norc -c "), "{theirs}");
        assert!(theirs.contains(" ARUGULA_ARTIFACTS='/home/runner/work/run_1/.arugula/artifacts' "), "{theirs}");
        assert!(theirs.ends_with(" < /dev/null"), "{theirs}");
        // The inner script, once the outer shell unquotes it: a fresh
        // directory of its own, its own copy, then the command.
        let inner = theirs.split_once("--norc -c ").unwrap().1.trim_end_matches(" < /dev/null");
        let inner = sh(&format!("printf %s {inner}"));
        assert!(
            inner.starts_with(
                "set -e; w='/home/runner/work/run_1'; rm -rf -- \"$w\"; mkdir -p -m 700 -- \"$w\"; cd -- \"$w\"; "
            ),
            "{inner}"
        );
        assert!(
            inner.contains("git config --global --add safe.directory '/home/user/app'; ")
                && inner.contains("git clone -q --no-hardlinks -- '/home/user/app' repo"),
            "{inner}"
        );
        assert!(inner.ends_with(r"set +e; exec 'echo' 'it'\''s'"), "{inner}");
    }

    /// The agent's login reaches the run on its standard input, from a file
    /// the outer shell removes: never on a command line.
    #[test]
    fn an_agent_runs_claude_with_its_login_on_stdin() {
        let op = Op { instruction: Some("fix the build".into()), ..Op::default() };
        let as_ = RunAs { user: "runner".into(), home: "/home/runner".into() };
        let file = Path::new("/state/work/run_1.login");
        let claude = Path::new("/usr/local/bin/claude");
        let (cmd, shown) = what(&op, Some(claude)).unwrap();
        assert_eq!(
            cmd,
            "'/usr/local/bin/claude' -p 'fix the build' --output-format text --dangerously-skip-permissions"
        );
        assert_eq!(shown, "claude -p 'fix the build'");
        assert_eq!(what(&op, None).unwrap_err().0, "op_agent");
        let c = pane_command(
            "run_1",
            "abc",
            &op,
            None,
            Some(&as_),
            &Agent { claude: Some(claude), login: Some(("ANTHROPIC_API_KEY", file)), env: &[] },
            Path::new("/home/runner/work/run_1/.arugula/artifacts"),
        )
        .unwrap();
        assert!(
            c.contains("{ cat -- '/state/work/run_1.login'; rm -f -- '/state/work/run_1.login'; } | exec sudo"),
            "{c}"
        );
        assert!(c.contains(r#"IFS= read -r k || true; export ANTHROPIC_API_KEY="$k"; unset k; "#), "{c}");
        assert!(!c.contains("< /dev/null"), "{c}");
    }

    #[test]
    fn what_a_run_shows_of_itself_prints_as_itself() {
        assert_eq!(printable("make test"), "make test");
        assert_eq!(printable("a\nb\rc\x1b[31m"), r"a\u{a}b\u{d}c\u{1b}[31m");
        assert_eq!(printable("rm\u{202e}fdp.exe\u{200b}"), r"rm\u{202e}fdp.exe\u{200b}");
        let t = tab_name(&format!("run_{}", "a".repeat(32)), &"b".repeat(64), &"x".repeat(200));
        assert!(t.chars().count() <= 96 && t.contains("bbbbbbbbbbbb ·"), "{t}");
    }

    #[test]
    fn the_firewall_keeps_runs_off_the_daemons_ports() {
        let dir = crate::hosted::scratch("work-firewall");
        std::fs::write(dir.join("listen"), "127.0.0.1:7681").unwrap();
        std::fs::write(dir.join("block-listen"), "127.0.0.1:7690").unwrap();
        let rules = firewall_rules(&dir, "runner");
        let ports: Vec<String> = rules.iter().filter(|(b, _)| *b == "iptables").map(|(_, r)| r[9].clone()).collect();
        assert_eq!(ports, ["1:1023", "49982:49983", "7681", "7690"]);
        assert_eq!(rules.len(), 8, "and the same for IPv6");
        assert_eq!(rules[0].1.join(" "), "-o lo -p tcp -m owner --uid-owner runner --dport 1:1023 -j REJECT");
        // nftables, as E2B takes it (checked live on arugula-workspace:v0.60.3).
        let script = firewall_script(&dir, 1002);
        assert!(script.starts_with("table inet arugula_runs\ndelete table inet arugula_runs\n"), "{script}");
        assert!(
            script.contains(
                "oif \"lo\" meta skuid 1002 meta l4proto tcp tcp dport { 1-1023, 49982-49983, 7681, 7690 } reject with tcp reset"
            ),
            "{script}"
        );
        // A daemon port inside a range already kept isn't listed twice.
        std::fs::write(dir.join("listen"), "127.0.0.1:443").unwrap();
        std::fs::write(dir.join("block-listen"), "127.0.0.1:49983").unwrap();
        assert_eq!(firewall_ports(&dir), ["1:1023", "49982:49983"]);
    }

    #[test]
    fn a_ledger_is_only_ever_a_runs() {
        let dir = std::path::Path::new("/state");
        let id = format!("run_{}", "a".repeat(32));
        assert_eq!(ledger_path(dir, &id).unwrap(), dir.join("work").join(format!("{id}.json")));
        for bad in ["run_../../etc/passwd", "run_", "x", &format!("run_{}", "A".repeat(32))] {
            assert!(ledger_path(dir, bad).is_none(), "{bad}");
        }
    }

    fn ledger(run: &str) -> Ledger {
        Ledger {
            run: run.into(),
            op_hash: "h".into(),
            from: "pending".into(),
            seq: 1,
            state: "running".into(),
            pane: None,
            epoch: None,
            taken_at: 1,
            grant: None,
            turn: None,
            deploy: None,
            events: vec![],
            logs: Default::default(),
        }
    }

    /// C5: an entry is made once, and one that's there but doesn't read
    /// counts as taken, never as free.
    #[test]
    fn a_run_is_claimed_once_and_an_unreadable_entry_is_taken() {
        let dir = crate::hosted::scratch("work-claim");
        let a = format!("run_{}", "a".repeat(32));
        assert!(!taken(&dir, &a));
        assert!(claim(&dir, &ledger(&a)));
        assert!(!claim(&dir, &ledger(&a)), "once");
        assert!(taken(&dir, &a));
        let b = format!("run_{}", "b".repeat(32));
        std::fs::write(dir.join(DIR).join(format!("{b}.json")), "not json").unwrap();
        assert!(taken(&dir, &b), "unreadable is taken");
        assert!(!claim(&dir, &ledger(&b)));
        assert_eq!(all(&dir).iter().map(|l| l.run.clone()).collect::<Vec<_>>(), std::slice::from_ref(&a));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let c = format!("run_{}", "c".repeat(32));
            let f = dir.join(DIR).join(format!("{c}.json"));
            std::fs::write(&f, serde_json::to_vec(&ledger(&c)).unwrap()).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
            // Root reads it anyway; anyone else can't, and it's taken.
            if std::fs::read(&f).is_err() {
                assert!(taken(&dir, &c));
            }
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn each_report_is_the_runs_next_event_signed() {
        let keys = arugula_e2e::DeviceKeys::generate();
        let me = keys.id();
        let mut l = ledger(&format!("run_{}", "b".repeat(32)));
        l.push(&keys, &me, State::Running, serde_json::json!({ "pane": 3 }));
        l.push(&keys, &me, State::Succeeded, serde_json::json!({ "exit": 0 }));
        assert_eq!(l.state, "succeeded");
        let key = hex::encode(keys.sign_public());
        for (k, (seq, from, to)) in l.events.iter().zip([(2, "pending", "running"), (3, "running", "succeeded")]) {
            let r = &k.report;
            assert_eq!((r.seq, r.from.as_str(), r.to.as_str()), (seq, from, to));
            let ev = Event {
                run: &r.run,
                seq: r.seq,
                from: State::parse(&r.from),
                to: State::parse(&r.to).unwrap(),
                at: r.at,
                detail: &r.detail,
                daemon: &me,
            };
            assert!(ev.verify(&key, &r.sig));
        }
        let dir = crate::hosted::scratch("work-ledger");
        assert!(claim(&dir, &l));
        assert_eq!(read(&dir, &l.run).unwrap(), Some(l.clone()));
        assert_eq!(all(&dir), [l]);
    }
}
