//! A hosted workspace's snapshot (M263, Arugula #140), the box's side.
//! The platform's side is `arugula_platform::hosted::snapshot`, which says
//! why the snapshot is filesystem-only.
//!
//! - **A builder** (control enrolled it with purpose `snapshot`) runs its
//!   workspace's setup like any box, then wipes itself ([`wipe`], as root:
//!   `arugulad _seal`, its list at [`wipe`]), says it's set up at step
//!   `sealed`, and stops its daemon, so nothing writes to the disk before
//!   the platform snapshots it. The wipe writes the builder's sandbox id to
//!   `/etc/arugula/snapshot-of`, and what setup did beside it
//!   ([`Record`]).
//! - **A box from the snapshot** boots from that disk, its daemon started
//!   by `arugulad.service`. Its sandbox id isn't the one in `snapshot-of`,
//!   so this is a box of its own: it clears whatever of a daemon's state
//!   the disk has ([`first_boot`]), takes the id at once (the cold boot:
//!   `super::enroll::new_box` would otherwise wait for an id other than the
//!   one it starts with, which is already this box's), makes its own key and
//!   enrolls as a new machine. Its setup says `ready` from the record,
//!   without running ([`from_snapshot`]).

use std::{
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

use arugula_control_wire::hosted::SetupPlan;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::{resume, setup::Cloned};

/// Where the snapshot's record goes: root's, read by everyone.
pub const DIR: &str = "/etc/arugula";
/// The builder's sandbox id, in [`DIR`].
pub const SNAPSHOT_OF: &str = "snapshot-of";
/// What the builder's setup did ([`Record`]), in [`DIR`].
pub const RECORD: &str = "snapshot-setup.json";
/// The shortest secret value the wipe looks for: a shorter one would match
/// files that never held it, so the wipe refuses ([`Refused::Short`]).
pub const MIN_SECRET: usize = 8;

/// Shell histories, in each home.
const HISTORIES: &[&str] = &[
    ".bash_history",
    ".zsh_history",
    ".python_history",
    ".node_repl_history",
    ".psql_history",
    ".mysql_history",
    ".sqlite_history",
    ".lesshst",
    ".viminfo",
    ".wget-hsts",
];

/// [`DIR`], or another for tests (`ARUGULA_SNAPSHOT_DIR`).
pub fn dir() -> PathBuf {
    std::env::var_os("ARUGULA_SNAPSHOT_DIR").filter(|v| !v.is_empty()).map_or(DIR.into(), PathBuf::from)
}

/// The sandbox id of the builder this box's disk is a snapshot of, if it
/// is one.
pub fn snapshot_of(dir: &Path) -> Option<String> {
    let id = std::fs::read_to_string(dir.join(SNAPSHOT_OF)).ok()?;
    Some(id.trim().to_owned()).filter(|id| !id.is_empty())
}

/// What the builder's setup did: the spec it set up, and its repos.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub of: String,
    pub spec_hash: String,
    #[serde(default)]
    pub repos: Vec<Cloned>,
}

/// This box's first start from a workspace's snapshot: the disk names a
/// builder that isn't this box, and the state directory isn't this box's
/// yet. Then everything in the state directory goes, whatever the builder's
/// daemon left after its wipe: nothing of another machine is kept. Whether
/// it was. Waits a while for the sandbox id, which envd writes at boot.
pub fn first_boot(state_dir: &Path, id_file: &Path, dir: &Path) -> bool {
    let Some(of) = snapshot_of(dir) else { return false };
    let mut id = None;
    for _ in 0..600 {
        id = resume::stamp(id_file).map(|(id, _)| id).filter(|id| !id.is_empty());
        if id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let Some(id) = id else { return false };
    if id == of || super::noted_box(state_dir).as_deref() == Some(id.as_str()) {
        return false;
    }
    if let Ok(entries) = std::fs::read_dir(state_dir) {
        for e in entries.flatten() {
            let p = e.path();
            let gone =
                if p.is_dir() && !p.is_symlink() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            if let Err(err) = gone {
                warn!(path = %p.display(), error = %err, "hosted: can't clear the snapshot's state");
            }
        }
    }
    info!("hosted: a new box from a workspace's snapshot; its state directory starts empty");
    true
}

/// The record of the snapshot this box was made from, when it's a box
/// made from one (not the builder itself).
pub fn from_snapshot(dir: &Path, id_file: &Path) -> Option<Record> {
    let of = snapshot_of(dir)?;
    let (id, _) = resume::stamp(id_file)?;
    if id.is_empty() || id == of {
        return None;
    }
    let r: Record = serde_json::from_slice(&std::fs::read(dir.join(RECORD)).ok()?).ok()?;
    (r.of == of).then_some(r)
}

/// Why a box isn't sealed: a fixed code for control (`SETUP_REASONS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// A secret's value is under [`MIN_SECRET`] bytes.
    Short,
    /// A secret is still somewhere after the wipe: where (never what).
    Left(Vec<String>),
    /// The wipe itself failed.
    Failed(String),
}

impl Refused {
    pub fn reason(&self) -> &'static str {
        match self {
            Refused::Short => "seal_short",
            Refused::Left(_) => "seal_left",
            Refused::Failed(_) => "seal",
        }
    }
}

/// Every secret the setup was given, as the wipe looks for them: each
/// clone's password and the `Authorization` value it went into git as,
/// and each env value.
pub fn needles(plan: &SetupPlan) -> Result<Vec<Vec<u8>>, Refused> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    for r in &plan.repos {
        let Some(p) = r.password.as_ref().filter(|p| !p.is_empty()) else { continue };
        out.push(p.clone().into_bytes());
        if let Some(u) = &r.username {
            out.push(base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}")).into_bytes());
        }
    }
    out.extend(plan.env.values().filter(|v| !v.is_empty()).map(|v| v.clone().into_bytes()));
    out.sort();
    out.dedup();
    if out.iter().any(|n| n.len() < MIN_SECRET) {
        return Err(Refused::Short);
    }
    Ok(out)
}

/// Where the wipe works: the real box's ([`Places::real`]), or a test's.
#[derive(Debug, Clone)]
pub struct Places {
    pub state_dir: PathBuf,
    /// Looked through for files holding a secret, but [`Places::skip`].
    pub roots: Vec<PathBuf>,
    pub skip: Vec<PathBuf>,
    /// Whose shell histories go: `/root` and each in `/home`.
    pub homes: Vec<PathBuf>,
    /// Where journald keeps its files.
    pub journal: Vec<PathBuf>,
    /// `/proc`.
    pub procs: PathBuf,
    /// [`DIR`].
    pub record: PathBuf,
}

impl Places {
    pub fn real(state_dir: &Path) -> Places {
        let mut homes = vec![PathBuf::from("/root")];
        if let Ok(e) = std::fs::read_dir("/home") {
            homes.extend(e.flatten().map(|e| e.path()));
        }
        Places {
            state_dir: state_dir.to_owned(),
            roots: vec!["/".into()],
            skip: ["/proc", "/sys", "/dev"].map(PathBuf::from).to_vec(),
            homes,
            journal: ["/var/log/journal", "/run/log/journal"].map(PathBuf::from).to_vec(),
            procs: "/proc".into(),
            record: dir(),
        }
    }
}

/// journald, as the wipe drives it.
pub trait Journal {
    /// `journalctl --rotate && journalctl --vacuum-files=1`: what was
    /// logged goes into archived files, and they're dropped.
    fn rotate(&mut self) -> anyhow::Result<()>;
    /// Stop journald and its sockets, so its files can go; start it again.
    fn stop(&mut self) -> anyhow::Result<()>;
    fn start(&mut self) -> anyhow::Result<()>;
}

/// The box's journald, through `journalctl` and `systemctl`. A box without
/// journald (loam, Docker) has nothing to rotate.
pub struct Journald;

fn run(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let s = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()?;
    anyhow::ensure!(s.success(), "{cmd} {} exited {s}", args.join(" "));
    Ok(())
}

fn have(cmd: &str) -> bool {
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"].iter().any(|d| Path::new(d).join(cmd).exists())
}

const JOURNALD: &[&str] = &[
    "systemd-journald.socket",
    "systemd-journald-dev-log.socket",
    "systemd-journald-audit.socket",
    "systemd-journald.service",
];

impl Journal for Journald {
    fn rotate(&mut self) -> anyhow::Result<()> {
        if !have("journalctl") {
            return Ok(());
        }
        run("journalctl", &["--rotate"])?;
        run("journalctl", &["--vacuum-files=1"])
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !have("systemctl") {
            return Ok(());
        }
        let mut args = vec!["stop"];
        args.extend(JOURNALD);
        // A unit the box doesn't have is no reason to keep its files.
        let _ = run("systemctl", &args);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if !have("systemctl") {
            return Ok(());
        }
        run("systemctl", &["start", "systemd-journald.service"])
    }
}

/// What the wipe did.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wiped {
    pub killed: usize,
    pub removed: Vec<String>,
    pub histories: usize,
    /// `rotated`, or `reset` when a secret was left after the rotation.
    pub journal: String,
}

/// Whether `bytes` hold any of `needles`.
fn holds(finders: &[memchr::memmem::Finder<'_>], bytes: &[u8]) -> bool {
    finders.iter().any(|f| f.find(bytes).is_some())
}

/// Whether the file at `path` holds any of `needles`, read in chunks that
/// overlap by the longest needle.
fn file_holds(finders: &[memchr::memmem::Finder<'_>], longest: usize, path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = vec![0u8; (1 << 20) + longest];
    let mut kept = 0;
    loop {
        let n = match f.read(&mut buf[kept..]) {
            Ok(0) => return false,
            Ok(n) => n,
            Err(_) => return false,
        };
        let end = kept + n;
        if holds(finders, &buf[..end]) {
            return true;
        }
        kept = longest.saturating_sub(1).min(end);
        buf.copy_within(end - kept..end, 0);
    }
}

/// Every regular file under `roots` (never through a symlink, never under
/// `skip`) that holds a needle.
fn files_holding(
    finders: &[memchr::memmem::Finder<'_>],
    longest: usize,
    roots: &[PathBuf],
    skip: &[PathBuf],
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo: Vec<PathBuf> = roots.to_vec();
    while let Some(d) = todo.pop() {
        if skip.contains(&d) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&d) else { continue };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&d) {
                todo.extend(entries.flatten().map(|e| e.path()));
            }
        } else if meta.is_file() && file_holds(finders, longest, &d) {
            found.push(d);
        }
    }
    found.sort();
    found
}

/// Every process (by its pid under `procs`) whose environment holds a
/// needle, but this one and its parents' (sudo, the daemon), and init.
fn procs_holding(finders: &[memchr::memmem::Finder<'_>], procs: &Path) -> Vec<i32> {
    let me = std::process::id() as i32;
    #[cfg(unix)]
    let parent = nix::unistd::getppid().as_raw();
    #[cfg(not(unix))]
    let parent = 0;
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(procs) else { return out };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        if pid == me || pid == parent || pid == 1 {
            continue;
        }
        if std::fs::read(e.path().join("environ")).is_ok_and(|env| holds(finders, &env)) {
            out.push(pid);
        }
    }
    out.sort();
    out
}

/// Wipe this box for a snapshot (S42's list, q3), in order:
/// 1. kill every process holding a secret in its environment;
/// 2. remove the daemon's whole state directory;
/// 3. remove every file holding a secret;
/// 4. remove shell histories;
/// 5. rotate and vacuum the journal, and if a secret is left in it, stop
///    journald, remove its files and start it again;
/// 6. write the builder's sandbox id and `record`;
/// 7. look again for every secret, in files, environments and the
///    journal: any is a refusal.
///
/// `kill` kills a process (a test's records it).
pub fn wipe(
    at: &Places,
    needles: &[Vec<u8>],
    record: &Record,
    journal: &mut dyn Journal,
    kill: &mut dyn FnMut(i32),
) -> Result<Wiped, Refused> {
    let finders: Vec<_> = needles.iter().map(|n| memchr::memmem::Finder::new(n.as_slice())).collect();
    let longest = needles.iter().map(Vec::len).max().unwrap_or(1);
    let failed = |what: &str, e: &dyn std::fmt::Display| Refused::Failed(format!("{what}: {e}"));
    let mut done = Wiped::default();
    // 1. The processes, first, so none writes a secret down after.
    for pid in procs_holding(&finders, &at.procs) {
        kill(pid);
        done.killed += 1;
    }
    // 2. The daemon's whole state directory: its keys, its enrollment, the
    //    revokes it heard, its runs, its setup.
    match std::fs::remove_dir_all(&at.state_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed("removing the state directory", &e)),
    }
    // 3. Every file holding a secret (the journal's are journald's).
    let mut skip = at.skip.clone();
    skip.extend(at.journal.iter().cloned());
    for f in files_holding(&finders, longest, &at.roots, &skip) {
        std::fs::remove_file(&f).map_err(|e| failed("removing a file holding a secret", &e))?;
        done.removed.push(f.display().to_string());
    }
    // 4. Shell histories.
    for home in &at.homes {
        for h in HISTORIES {
            if std::fs::remove_file(home.join(h)).is_ok() {
                done.histories += 1;
            }
        }
    }
    // 5. The journal: sudo logs its command lines there, and journald keeps
    //    its file open (removing it isn't enough).
    journal.rotate().map_err(|e| failed("rotating the journal", &e))?;
    done.journal = "rotated".into();
    if !files_holding(&finders, longest, &at.journal, &[]).is_empty() {
        journal.stop().map_err(|e| failed("stopping journald", &e))?;
        for d in &at.journal {
            let Ok(entries) = std::fs::read_dir(d) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
        }
        journal.start().map_err(|e| failed("starting journald", &e))?;
        done.journal = "reset".into();
    }
    // 6. What a box from the snapshot reads.
    std::fs::create_dir_all(&at.record).map_err(|e| failed("making the record's directory", &e))?;
    let json = serde_json::to_vec_pretty(record).map_err(|e| failed("the record", &e))?;
    crate::store::write_atomic(&at.record.join(RECORD), &json).map_err(|e| failed("writing the record", &e))?;
    crate::store::write_atomic(&at.record.join(SNAPSHOT_OF), record.of.as_bytes())
        .map_err(|e| failed("writing the builder's id", &e))?;
    // 7. Again, everywhere.
    let mut left: Vec<String> = files_holding(&finders, longest, &at.roots, &at.skip)
        .into_iter()
        .chain(files_holding(&finders, longest, &at.journal, &[]))
        .map(|p| p.display().to_string())
        .collect();
    left.extend(procs_holding(&finders, &at.procs).into_iter().map(|p| format!("process {p}")));
    if at.state_dir.exists() {
        left.push(at.state_dir.display().to_string());
    }
    left.sort();
    left.dedup();
    if !left.is_empty() {
        return Err(Refused::Left(left));
    }
    Ok(done)
}

/// What the daemon sends `arugulad _seal` on its stdin: never on its
/// command line, which sudo logs and other users read.
#[derive(Serialize, Deserialize)]
struct SealAsk {
    state_dir: PathBuf,
    record: Record,
    /// The needles, base64.
    needles: Vec<String>,
}

/// What `arugulad _seal` prints.
#[derive(Debug, Serialize, Deserialize)]
struct SealAnswer {
    #[serde(default)]
    wiped: Option<Wiped>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    left: Vec<String>,
    #[serde(default)]
    error: String,
}

/// `arugulad _seal`, as root: read a [`SealAsk`] on stdin, wipe the box,
/// print a [`SealAnswer`].
pub fn cli() -> ! {
    let mut input = Vec::new();
    let answer = match std::io::stdin().read_to_end(&mut input).map_err(anyhow::Error::from).and_then(|_| {
        let ask: SealAsk = serde_json::from_slice(&input)?;
        let needles = ask
            .needles
            .iter()
            .map(|n| base64::engine::general_purpose::STANDARD.decode(n))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((ask, needles))
    }) {
        Err(e) => SealAnswer { wiped: None, reason: "seal".into(), left: vec![], error: format!("{e:#}") },
        Ok((ask, needles)) => {
            let at = Places::real(&ask.state_dir);
            let mut kill = |pid: i32| {
                #[cfg(unix)]
                let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGKILL);
                #[cfg(not(unix))]
                let _ = pid;
            };
            match wipe(&at, &needles, &ask.record, &mut Journald, &mut kill) {
                Ok(w) => SealAnswer { wiped: Some(w), reason: String::new(), left: vec![], error: String::new() },
                Err(r) => SealAnswer {
                    wiped: None,
                    reason: r.reason().into(),
                    left: match &r {
                        Refused::Left(l) => l.clone(),
                        _ => vec![],
                    },
                    error: match &r {
                        Refused::Failed(e) => e.clone(),
                        _ => String::new(),
                    },
                },
            }
        }
    };
    println!("{}", serde_json::to_string(&answer).unwrap_or_default());
    std::process::exit(if answer.wiped.is_some() { 0 } else { 1 })
}

/// Seal this builder after its setup: the wipe as root (`sudo -n arugulad
/// _seal`, the secrets on its stdin). What went wrong, as [`Refused`].
pub async fn seal(state_dir: &Path, sandbox: &str, plan: &SetupPlan, repos: &[Cloned]) -> Result<Wiped, Refused> {
    use tokio::io::AsyncWriteExt as _;
    let needles = needles(plan)?;
    let ask = SealAsk {
        state_dir: state_dir.to_owned(),
        record: Record { of: sandbox.to_owned(), spec_hash: plan.spec_hash.clone(), repos: repos.to_vec() },
        needles: needles.iter().map(|n| base64::engine::general_purpose::STANDARD.encode(n)).collect(),
    };
    let input = serde_json::to_vec(&ask).map_err(|e| Refused::Failed(e.to_string()))?;
    let exe = std::env::current_exe().map_err(|e| Refused::Failed(e.to_string()))?;
    let mut child = tokio::process::Command::new("sudo")
        .arg("-n")
        .arg(exe)
        .arg("_seal")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Refused::Failed(format!("can't run the wipe: {e}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Refused::Failed("no stdin".into()))?;
    stdin.write_all(&input).await.map_err(|e| Refused::Failed(e.to_string()))?;
    drop(stdin);
    // It reads the whole disk: a while, but not forever.
    let out = tokio::time::timeout(Duration::from_secs(30 * 60), child.wait_with_output())
        .await
        .map_err(|_| Refused::Failed("the wipe took over 30 minutes".into()))?
        .map_err(|e| Refused::Failed(e.to_string()))?;
    let answer: SealAnswer = serde_json::from_slice(&out.stdout)
        .map_err(|_| Refused::Failed(format!("the wipe exited {} without an answer", out.status)))?;
    match answer.wiped {
        Some(w) => Ok(w),
        None => Err(match answer.reason.as_str() {
            "seal_short" => Refused::Short,
            "seal_left" => Refused::Left(answer.left),
            _ => Refused::Failed(answer.error),
        }),
    }
}

/// The builder's last step once it said it's sealed: stop its daemon (it's
/// `arugulad.service`, whose `Restart=always` a stop doesn't fight), so
/// nothing writes to the disk before the snapshot. Without systemd, there's
/// nothing to stop it with, and the platform snapshots it anyway.
pub fn stop_self() {
    if Path::new("/run/systemd/system").is_dir() {
        let ok = std::process::Command::new("sudo")
            .args(["-n", "systemctl", "stop", "--no-block", "arugulad.service"])
            .stdin(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            info!("hosted: sealed for the snapshot; stopping");
            return;
        }
    }
    warn!("hosted: sealed for the snapshot, but can't stop arugulad.service");
}

#[cfg(test)]
mod tests {
    use arugula_control_wire::hosted::SetupRepo;

    use super::*;

    const SECRET: &str = "s42-secret-0123456789abcdef";

    fn plan(env: &[(&str, &str)]) -> SetupPlan {
        SetupPlan {
            workspace: "hw_1".into(),
            spec_hash: "h1".into(),
            repos: vec![SetupRepo {
                url: "https://github.com/acme/app".into(),
                via: "secret".into(),
                username: Some("x-access-token".into()),
                password: Some(format!("{SECRET}-git")),
            }],
            packages: vec![],
            setup: String::new(),
            env: env.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
            unset: vec![],
        }
    }

    /// A journald whose rotation leaves its open file where it is, as the
    /// real one did in S42's first run.
    #[derive(Default)]
    struct Stubborn {
        calls: Vec<&'static str>,
    }

    impl Journal for Stubborn {
        fn rotate(&mut self) -> anyhow::Result<()> {
            self.calls.push("rotate");
            Ok(())
        }
        fn stop(&mut self) -> anyhow::Result<()> {
            self.calls.push("stop");
            Ok(())
        }
        fn start(&mut self) -> anyhow::Result<()> {
            self.calls.push("start");
            Ok(())
        }
    }

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    /// A set-up builder, as S42's test made one: an enrolled daemon's
    /// state, secrets setup wrote (`~/.npmrc`, `~/.git-credentials`, one in
    /// `/etc`), the secret in a process's environment and in the journal,
    /// a shell history, and setup's real work.
    fn builder(root: &Path) -> Places {
        let state = root.join("home/user/.local/state/arugula");
        for f in [
            "daemon.key",
            "daemon-id",
            "local-token",
            "control.json",
            "run-grant-revokes.json",
            "run-grant-revokes.kept",
            "hosted-setup.json",
            "hosted-box",
            "work/run_1.json",
            "mcp/key",
        ] {
            write(&state.join(f), "old box");
        }
        write(&root.join("home/user/.npmrc"), &format!("//registry.npmjs.org/:_authToken={SECRET}\n"));
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{SECRET}-git"));
        write(&root.join("home/user/.git-credentials"), &format!("https://x-access-token:{SECRET}-git@github.com\n"));
        write(&root.join("home/user/.cache/git-trace"), &format!("Authorization: Basic {basic}\n"));
        write(&root.join("etc/app.conf"), &format!("token={SECRET}\n"));
        write(&root.join("home/user/.bash_history"), "make\n");
        write(&root.join("root/.bash_history"), "sudo make\n");
        write(&root.join("home/user/repo/built.txt"), "built\n");
        // Past a read's chunk, split across two of them.
        let mut big = vec![b'x'; (1 << 20) - 5];
        big.extend_from_slice(SECRET.as_bytes());
        std::fs::write(root.join("home/user/repo/big.bin"), big).unwrap();
        write(&root.join("var/log/journal/m1/system.journal"), &format!("COMMAND=/usr/bin/sh -c 'printf {SECRET}'"));
        write(&root.join("proc/77/environ"), &format!("PATH=/bin\0NPM_TOKEN={SECRET}\0"));
        write(&root.join("proc/78/environ"), "PATH=/bin\0");
        Places {
            state_dir: state,
            roots: vec![root.to_owned()],
            skip: vec![root.join("proc")],
            homes: vec![root.join("root"), root.join("home/user")],
            journal: vec![root.join("var/log/journal")],
            procs: root.join("proc"),
            record: root.join("etc/arugula"),
        }
    }

    /// The wipe's list, on a builder: no old machine key, revokes or state
    /// left, no secret on disk, in a process or in the journal, no history;
    /// setup's work kept, and the builder's id and setup's record written.
    #[test]
    fn the_wipe_leaves_no_key_revokes_or_secrets() {
        let root = super::super::scratch("wipe");
        let at = builder(&root);
        let needles = needles(&plan(&[("NPM_TOKEN", SECRET)])).unwrap();
        assert_eq!(needles.len(), 3, "the env value, the clone's password, and its Authorization value");
        let record = Record {
            of: "builder1".into(),
            spec_hash: "h1".into(),
            repos: vec![Cloned { url: "https://github.com/acme/app".into(), dir: root.join("home/user/repo") }],
        };
        let mut journal = Stubborn::default();
        let mut killed = vec![];
        let w = wipe(&at, &needles, &record, &mut journal, &mut |p| {
            killed.push(p);
            // As a killed process's: gone from /proc.
            std::fs::remove_dir_all(at.procs.join(p.to_string())).unwrap();
        })
        .unwrap();
        assert_eq!(killed, [77]);
        assert!(!at.state_dir.exists(), "the whole state directory goes");
        for gone in ["home/user/.npmrc", "home/user/.git-credentials", "home/user/.cache/git-trace", "etc/app.conf"] {
            assert!(!root.join(gone).exists(), "{gone}");
        }
        assert!(!root.join("home/user/repo/big.bin").exists(), "found across a chunk's edge");
        assert_eq!(w.histories, 2);
        assert!(!root.join("home/user/.bash_history").exists() && !root.join("root/.bash_history").exists());
        assert_eq!(journal.calls, ["rotate", "stop", "start"], "the rotation left it: journald's files go");
        assert_eq!(w.journal, "reset");
        assert!(!root.join("var/log/journal/m1").exists());
        assert_eq!(std::fs::read_to_string(root.join("home/user/repo/built.txt")).unwrap(), "built\n");
        assert_eq!(snapshot_of(&at.record).as_deref(), Some("builder1"));
        let kept: Record = serde_json::from_slice(&std::fs::read(at.record.join(RECORD)).unwrap()).unwrap();
        assert_eq!(kept, record);
        assert!(!std::fs::read(at.record.join(RECORD)).unwrap().windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
    }

    /// A secret still there after the wipe is a refusal that says where;
    /// one too short to look for is refused before anything.
    #[test]
    fn a_secret_left_or_too_short_is_refused() {
        let root = super::super::scratch("wipe-left");
        let at = builder(&root);
        let needles = needles(&plan(&[("NPM_TOKEN", SECRET)])).unwrap();
        let record = Record { of: "builder1".into(), ..Record::default() };
        // A process the kill doesn't take.
        let r = wipe(&at, &needles, &record, &mut Stubborn::default(), &mut |_| {});
        assert_eq!(r, Err(Refused::Left(vec!["process 77".into()])));
        assert_eq!(Refused::Left(vec![]).reason(), "seal_left");
        assert_eq!(needles_of_short(), Err(Refused::Short));
        assert_eq!(Refused::Short.reason(), "seal_short");
        // An empty value isn't a secret to look for.
        assert_eq!(super::needles(&plan(&[("EMPTY", "")])).unwrap().len(), 2);
    }

    fn needles_of_short() -> Result<Vec<Vec<u8>>, Refused> {
        needles(&plan(&[("CI", "1")]))
    }

    /// The cold boot: a box from a filesystem-only snapshot starts with
    /// envd's id for itself already written. Any id but the builder's in
    /// `snapshot-of` is its own: its state directory is cleared, and it
    /// takes the id at once instead of waiting for another. The builder
    /// itself, a box already noted, and a template's box are left alone.
    #[tokio::test]
    async fn a_box_from_a_snapshot_takes_its_own_id_at_once() {
        let state = super::super::scratch("cold-state");
        let run = super::super::scratch("cold-run");
        let dir = super::super::scratch("cold-etc");
        let id_file = run.join(".E2B_SANDBOX_ID");
        std::fs::write(&id_file, "child1\n").unwrap();
        write(&state.join("control.json"), "the builder's");
        write(&state.join("daemon.key"), "the builder's");
        assert!(!first_boot(&state, &id_file, &dir), "not a snapshot's: the template's own path");
        assert!(state.join("control.json").exists());

        std::fs::write(dir.join(SNAPSHOT_OF), "builder1").unwrap();
        let record = Record { of: "builder1".into(), spec_hash: "h1".into(), repos: vec![] };
        std::fs::write(dir.join(RECORD), serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(first_boot(&state, &id_file, &dir));
        assert_eq!(std::fs::read_dir(&state).unwrap().count(), 0, "nothing of the builder's");
        assert_eq!(from_snapshot(&dir, &id_file), Some(record), "set up, from the record");
        // It takes its id at once.
        let id = tokio::time::timeout(
            Duration::from_secs(2),
            super::super::enroll::new_box(&id_file, snapshot_of(&dir).as_deref()),
        )
        .await
        .expect("a box from a snapshot doesn't wait for another id");
        assert_eq!(id, "child1");
        // Once noted, a restart keeps its state.
        std::fs::write(state.join("hosted-box"), "child1").unwrap();
        write(&state.join("daemon.key"), "its own");
        assert!(!first_boot(&state, &id_file, &dir));
        assert!(state.join("daemon.key").exists());

        // The builder itself, after its wipe: its own id is the snapshot's,
        // so it waits, sets nothing up from the record, and keeps its state.
        std::fs::write(&id_file, "builder1").unwrap();
        std::fs::remove_file(state.join("hosted-box")).unwrap();
        assert!(!first_boot(&state, &id_file, &dir));
        assert_eq!(from_snapshot(&dir, &id_file), None);
        let waiting = tokio::spawn({
            let (f, of) = (id_file.clone(), snapshot_of(&dir));
            async move { super::super::enroll::new_box(&f, of.as_deref()).await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiting.is_finished(), "the builder's id isn't a new box");
        waiting.abort();
    }
}
