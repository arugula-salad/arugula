//! A chant-fly deploy's plan (#1985): chant's ledger, checked and pushed by
//! this daemon itself, never by the run user.
//!
//! The plan's step that runs the commit's code leaves chant's ledger as a
//! bundle of what it added (`before..tip`). Once that step and everything
//! it started are gone, this daemon, as its own user, in a directory of its
//! own (0700) and with git reading no configuration but its own:
//! - takes the bundle as data ([`PlanLedger::take_bundle`]): opened without
//!   following a link, a regular file with one name that the run user owns,
//!   at most [`MAX_BUNDLE`] bytes, copied without reading it into memory;
//! - checks it ([`PlanLedger::check`]): its commits are a line on top of
//!   `before`, origin's ledger as this daemon read it before the plan ran
//!   ([`PlanLedger::fetch_before`]), or new from nothing when origin had
//!   none; each commit only adds lines to the two files chant 0.124.68
//!   writes when an op stops at a gate (`<prefix>_gates/<op>.jsonl` and
//!   `<prefix><env>/runs__<op>.jsonl`); and what they add is this run's
//!   pending fact for the gate the plan reported, which the environment's
//!   `chant_gates` allows, and at most its run record;
//! - only then is the token that writes asked for, and the ledger pushed
//!   ([`PlanLedger::push`]), with the token in git's environment only (this
//!   daemon's process, never a file or a command line).

use std::{
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use arugula_e2e::deploy::chant::Gate;
use serde_json::Value;

/// The largest bundle of chant's ledger a plan may leave: 32 MiB.
pub const MAX_BUNDLE: u64 = 32 * 1024 * 1024;
/// The largest ledger file a check reads, before or after: 64 MiB.
pub const MAX_LEDGER_FILE: u64 = 64 * 1024 * 1024;
/// The most commits a plan may add to chant's ledger: chant makes two.
pub const MAX_COMMITS: usize = 8;
/// chant's ledger branch.
pub const LEDGER: &str = "refs/heads/chant/lifecycle";
/// Where a fetched bundle's ledger is kept, beside origin's.
const PLANNED: &str = "refs/heads/arugula/planned";

/// The bundle the plan's step leaves in its home.
pub const BUNDLE: &str = "ledger.bundle";

/// What a plan's ledger must hold: this run's pending fact for `gate`.
pub struct Want<'a> {
    pub op: &'a str,
    pub gate: &'a Gate,
    /// The environment's `chant_gates`, when it names some.
    pub gates_allowed: Option<&'a str>,
    /// The commit the deploy plans, which chant's facts name.
    pub source_ref: &'a str,
}

/// Why a push didn't land.
#[derive(Debug, PartialEq, Eq)]
pub enum PushFailed {
    /// Origin's ledger isn't what it was when the plan read it.
    Moved,
    /// Anything else: the token, the network, origin.
    Other(String),
}

impl std::fmt::Display for PushFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PushFailed::Moved => {
                write!(f, "chant's ledger on origin moved since this deploy was planned: plan it again")
            }
            PushFailed::Other(why) => write!(f, "chant's ledger wasn't pushed: {why}"),
        }
    }
}

/// A plan's ledger, in this daemon's own directory.
#[derive(Clone)]
pub struct PlanLedger {
    dir: PathBuf,
}

impl PlanLedger {
    /// A new directory `dir` (what was there goes), 0700, with a bare
    /// repository in it.
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        let _ = std::fs::remove_dir_all(&dir);
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("its ledger's directory: {e}"))?;
        }
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&dir).map_err(|e| format!("its ledger's directory: {e}"))?;
        let l = Self { dir };
        let init = Command::new("git")
            .env_clear()
            .env("PATH", crate::RUN_PATH)
            .env("HOME", &l.dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(["init", "-q", "--bare"])
            .arg(l.repo())
            .current_dir(&l.dir)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !init.status.success() {
            return Err(format!("git init: {}", String::from_utf8_lossy(&init.stderr).trim()));
        }
        Ok(l)
    }

    fn repo(&self) -> PathBuf {
        self.dir.join("ledger.git")
    }

    /// Where a bundle is kept once taken.
    fn bundle(&self) -> PathBuf {
        self.dir.join("planned.bundle")
    }

    /// git on its repository, reading no configuration but its own, with
    /// `token` (when given) as GitHub's basic auth through its environment.
    fn git(&self, token: Option<&str>) -> Command {
        let mut c = Command::new("git");
        c.env_clear()
            .env("PATH", crate::RUN_PATH)
            .env("HOME", &self.dir)
            .env("LC_ALL", "C")
            .env("GIT_DIR", self.repo())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&self.dir)
            .stdin(Stdio::null());
        if let Some(t) = token {
            use base64::Engine as _;
            let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{t}"));
            c.env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader")
                .env("GIT_CONFIG_VALUE_0", format!("AUTHORIZATION: basic {basic}"));
        }
        c
    }

    /// `git args`'s standard output, or why it failed.
    fn out(&self, args: &[&str]) -> Result<String, String> {
        let o = self.git(None).args(args).output().map_err(|e| format!("git: {e}"))?;
        if !o.status.success() {
            return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// Origin's ledger, fetched here with `token` (one that reads): its
    /// tip, or `None` when origin has none. What the plan must add to.
    pub fn fetch_before(&self, origin: &str, token: Option<&str>) -> Result<Option<String>, String> {
        let o = self.git(token).args(["ls-remote", "--", origin, LEDGER]).output().map_err(|e| format!("git: {e}"))?;
        if !o.status.success() {
            return Err(format!("reading origin's ledger: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        if String::from_utf8_lossy(&o.stdout).trim().is_empty() {
            return Ok(None);
        }
        let o = self
            .git(token)
            .args(["-c", "transfer.fsckObjects=true", "fetch", "-q", "--no-tags", "--", origin])
            .arg(format!("+{LEDGER}:{LEDGER}"))
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !o.status.success() {
            return Err(format!("fetching origin's ledger: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(Some(self.out(&["rev-parse", "--verify", LEDGER])?.trim().to_owned()))
    }

    /// The bundle the plan left at `from`, copied here: opened without
    /// following a link, a regular file with one name that `uid` owns, at
    /// most [`MAX_BUNDLE`] bytes, streamed.
    #[cfg(unix)]
    pub fn take_bundle(&self, from: &Path, uid: u32) -> Result<(), String> {
        use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
        let refused = |why: &str| Err(format!("the plan's bundle of chant's ledger {why}: nothing is pushed"));
        let Ok(mut f) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(from)
        else {
            return refused("isn't there, or is a link");
        };
        let Ok(m) = f.metadata() else { return refused("doesn't read") };
        if !m.is_file() || m.uid() != uid || m.nlink() != 1 {
            return refused("isn't a file of the run's own");
        }
        if m.len() > MAX_BUNDLE {
            return refused(&format!("is larger than {} MiB", MAX_BUNDLE >> 20));
        }
        let mut to = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.bundle())
            .map_err(|e| format!("keeping the plan's bundle: {e}"))?;
        let copied = std::io::copy(&mut f.by_ref().take(MAX_BUNDLE + 1), &mut to)
            .map_err(|e| format!("keeping the plan's bundle: {e}"))?;
        if copied > MAX_BUNDLE {
            return refused(&format!("is larger than {} MiB", MAX_BUNDLE >> 20));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn take_bundle(&self, _: &Path, _: u32) -> Result<(), String> {
        Err("a deploy's plan runs on a Linux box only".into())
    }

    /// The taken bundle, checked against `before` (origin's tip as fetched
    /// here, `None`: origin had none) and `want`: its tip, to push.
    pub fn check(&self, before: Option<&str>, want: &Want) -> Result<String, String> {
        let refused = |why: String| Err(format!("chant's ledger from the plan {why}: nothing is pushed"));
        let g = want.gate;
        if g.op != want.op {
            return refused(format!("is for op {}, not {}", g.op, want.op));
        }
        if let Some(allowed) = want.gates_allowed
            && !allowed.split(',').any(|a| a == g.gate)
        {
            return refused(format!("stops at gate {}, which the environment's chant_gates doesn't name", g.gate));
        }
        let bundle = self.bundle();
        let b = bundle.to_string_lossy();
        if let Err(why) = self.out(&[
            "-c",
            "transfer.fsckObjects=true",
            "fetch",
            "-q",
            "--no-tags",
            &b,
            &format!("+{LEDGER}:{PLANNED}"),
        ]) {
            return refused(format!("doesn't read as a bundle of chant/lifecycle on top of origin's ({why})"));
        }
        let tip = self.out(&["rev-parse", "--verify", PLANNED])?.trim().to_owned();
        // Its commits, oldest first, each with its parents: one line on top
        // of `before`, or from nothing when origin had none.
        let range = match before {
            Some(b) => vec![tip.clone(), format!("^{b}")],
            None => vec![tip.clone()],
        };
        let mut args = vec!["rev-list", "--reverse", "--topo-order", "--parents"];
        args.extend(range.iter().map(String::as_str));
        let listed = self.out(&args)?;
        let commits: Vec<Vec<&str>> = listed.lines().map(|l| l.split(' ').collect()).collect();
        if commits.is_empty() {
            return refused("adds nothing to it".into());
        }
        if commits.len() > MAX_COMMITS {
            return refused(format!("adds {} commits, more than the {MAX_COMMITS} a plan makes", commits.len()));
        }
        let mut prev = before.map(str::to_owned);
        for c in &commits {
            let parents: Vec<&str> = c[1..].to_vec();
            let expected: Vec<&str> = prev.iter().map(String::as_str).collect();
            if parents != expected {
                return refused(match &prev {
                    Some(p) => format!("doesn't add to origin's ledger as it was ({p}) in one line of commits"),
                    None => "isn't one line of new commits, and origin had no ledger".to_owned(),
                });
            }
            prev = Some(c[0].to_owned());
        }
        // What each commit changes: only chant's two gate files, under one
        // prefix, each only added to.
        let mut prefix: Option<String> = None;
        let mut gates_file = None;
        let mut runs_file = None;
        for c in &commits {
            let raw =
                self.out(&["-c", "core.quotePath=false", "diff-tree", "-r", "--raw", "--no-renames", "--root", c[0]])?;
            for line in raw.lines().filter(|l| l.starts_with(':')) {
                let (meta, path) = line.split_once('\t').unwrap_or((line, ""));
                let f: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
                let [old_mode, new_mode, old_blob, new_blob, status] = f[..] else {
                    return refused(format!("changes {path} in a way it can't read"));
                };
                let Some((p, kind)) = allowed(path, want.op) else {
                    return refused(format!("changes {path}, which chant doesn't write when an op stops at a gate"));
                };
                if prefix.get_or_insert_with(|| p.clone()) != &p {
                    return refused("writes under two members' prefixes".into());
                }
                let added = status == "A" && old_mode == "000000";
                let modified = status == "M" && old_mode == "100644";
                if !(added || modified) || new_mode != "100644" {
                    return refused(format!("changes {path} other than by adding to a file"));
                }
                if modified && !self.blob(new_blob)?.starts_with(&self.blob(old_blob)?) {
                    return refused(format!("rewrites {path}, which chant only appends to"));
                }
                let slot = if kind == Kind::Gates { &mut gates_file } else { &mut runs_file };
                if slot.get_or_insert_with(|| path.to_owned()) != path {
                    return refused(format!("writes two run records' files ({path})"));
                }
            }
        }
        // What it adds in all: this run's pending fact, and its run record.
        let Some(gates) = gates_file else { return refused("has no pending fact of this run's".into()) };
        let added = self.added(before, &tip, &gates)?;
        let [pending] = added.as_slice() else {
            return refused(format!("adds {} lines to {gates}, not one pending fact", added.len()));
        };
        let p: Value = serde_json::from_str(pending).unwrap_or_default();
        let fact_ok = p["kind"] == "pending"
            && p["op"] == want.op
            && p["gate"] == g.gate.as_str()
            && p["runId"] == g.run.as_str()
            && p["planDigest"] == g.plan.as_str()
            && p["gitSha"] == want.source_ref
            && p["timestamp"] == g.recorded.as_str();
        if !fact_ok {
            return refused(format!(
                "adds a line to {gates} that isn't the pending fact of run {} for gate {} with plan {}",
                g.run, g.gate, g.plan
            ));
        }
        if let Some(runs) = runs_file {
            let added = self.added(before, &tip, &runs)?;
            let ok = match added.as_slice() {
                [] => true,
                [line] => {
                    let r: Value = serde_json::from_str(line).unwrap_or_default();
                    r["id"] == g.run.as_str()
                        && r["op"] == want.op
                        && r["status"] == "gated"
                        && r["gitSha"] == want.source_ref
                        && r["gate"]["name"] == g.gate.as_str()
                }
                _ => false,
            };
            if !ok {
                return refused(format!(
                    "adds to {runs} something other than run {}'s record, gated at {}",
                    g.run, g.gate
                ));
            }
        }
        Ok(tip)
    }

    /// A blob's bytes, at most [`MAX_LEDGER_FILE`].
    fn blob(&self, id: &str) -> Result<Vec<u8>, String> {
        let size: u64 = self.out(&["cat-file", "-s", id])?.trim().parse().unwrap_or(u64::MAX);
        if size > MAX_LEDGER_FILE {
            return Err(format!(
                "a file of chant's ledger is larger than {} MiB: nothing is pushed",
                MAX_LEDGER_FILE >> 20
            ));
        }
        let o = self.git(None).args(["cat-file", "blob", id]).output().map_err(|e| format!("git: {e}"))?;
        if !o.status.success() {
            return Err(format!("reading chant's ledger: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(o.stdout)
    }

    /// The lines `tip` adds to `path` after `before` (whose copy, if any,
    /// the check found it starts with).
    fn added(&self, before: Option<&str>, tip: &str, path: &str) -> Result<Vec<String>, String> {
        let id = |rev: &str| -> Result<Option<String>, String> {
            let o = self.git(None).args(["rev-parse", "-q", "--verify", &format!("{rev}:{path}")]).output();
            let o = o.map_err(|e| format!("git: {e}"))?;
            Ok(o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_owned()))
        };
        let old = match before.map(id).transpose()?.flatten() {
            Some(b) => self.blob(&b)?,
            None => Vec::new(),
        };
        let new = match id(tip)? {
            Some(b) => self.blob(&b)?,
            None => Vec::new(),
        };
        let Some(rest) = new.strip_prefix(old.as_slice()) else {
            return Err(format!("chant's ledger from the plan rewrites {path}: nothing is pushed"));
        };
        Ok(String::from_utf8_lossy(rest).lines().filter(|l| !l.is_empty()).map(str::to_owned).collect())
    }

    /// Push `tip` as origin's ledger, with `token` (one that writes), only
    /// while origin's is still `before` (none: still none).
    pub fn push(&self, origin: &str, tip: &str, before: Option<&str>, token: Option<&str>) -> Result<(), PushFailed> {
        let lease = format!("--force-with-lease={LEDGER}:{}", before.unwrap_or(""));
        let o = self
            .git(token)
            .args(["push", "--porcelain", "--no-verify", &lease, "--", origin, &format!("{tip}:{LEDGER}")])
            .output()
            .map_err(|e| PushFailed::Other(format!("git: {e}")))?;
        if o.status.success() {
            return Ok(());
        }
        let said = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        let moved = said.lines().any(|l| {
            l.starts_with('!')
                && (l.contains("non-fast-forward") || l.contains("fetch first") || l.contains("stale info"))
        });
        if moved {
            return Err(PushFailed::Moved);
        }
        Err(PushFailed::Other(said.trim().lines().last().unwrap_or("git push failed").to_owned()))
    }
}

/// Which of chant's gate files `path` is, for `op`: its member prefix
/// (`""` or `_members/<member>/` per level) and which.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Gates,
    Runs,
}

fn allowed(path: &str, op: &str) -> Option<(String, Kind)> {
    let word = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    let mut parts: Vec<&str> = path.split('/').collect();
    let file = parts.pop()?;
    let mut prefix = String::new();
    let mut i = 0;
    // `_members/<member>/` per level, with something after it.
    while i + 2 < parts.len() && parts[i] == "_members" {
        let m = parts[i + 1];
        if !word(m) {
            return None;
        }
        prefix.push_str(&format!("_members/{m}/"));
        i += 2;
    }
    let rest = &parts[i..];
    match rest {
        ["_gates"] if file == format!("{op}.jsonl") => Some((prefix, Kind::Gates)),
        [env] if word(env) && *env != "_gates" && *env != "_members" && file == format!("runs__{op}.jsonl") => {
            Some((prefix, Kind::Runs))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn sh(dir: &Path, script: &str) -> String {
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(out.status.success(), "{script}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn gate() -> Gate {
        Gate {
            chant: "0.124.68".into(),
            op: "release".into(),
            gate: "approve-release".into(),
            environment: None,
            plan: format!("sha256:{}", "ab".repeat(32)),
            checkpoint: None,
            run: "local-1794007650156".into(),
            recorded: "2026-11-06T23:27:30.435Z".into(),
        }
    }

    /// chant 0.124.68's lines for `gate()`, as a gated stop writes them.
    fn pending() -> String {
        let g = gate();
        serde_json::json!({
            "description": "Approve deploying this commit", "expiresAt": "2026-11-07T23:27:30.435Z", "gate": g.gate,
            "gitSha": SHA, "kind": "pending", "neverOverMcp": true, "op": "release", "origin": "cli",
            "planDigest": g.plan, "runId": g.run, "timestamp": g.recorded, "version": 1,
        })
        .to_string()
    }

    fn run_record() -> String {
        let g = gate();
        serde_json::json!({
            "env": "local", "gate": { "name": g.gate, "since": g.recorded }, "gitSha": SHA, "id": g.run,
            "op": "release", "status": "gated", "version": 1,
        })
        .to_string()
    }

    /// A scratch: an origin (`origin.git`, with a ledger of its own when
    /// `ledger`), a checkout of it (`work`) to make a plan's commits in.
    fn scratch(name: &str, ledger: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arugula-plan-ledger-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, "git init -q --bare origin.git && git init -q -b main work");
        if ledger {
            sh(
                &dir.join("work"),
                r#"git checkout -q --orphan chant/lifecycle && mkdir -p _gates local \
                && echo '{"kind":"resolution","op":"release","resolvedBy":"arugula:a"}' > _gates/release.jsonl \
                && echo '{"kind":"pending","op":"other"}' > _gates/other.jsonl \
                && git add -A && git commit -q -m before && git push -q ../origin.git chant/lifecycle"#,
            );
        } else {
            sh(&dir.join("work"), "git checkout -q --orphan chant/lifecycle");
        }
        dir
    }

    /// Commit `script`'s changes in `work` as one commit of the plan's.
    fn commit(dir: &Path, script: &str) {
        sh(&dir.join("work"), &format!("{script} && git add -A && git commit -q -m plan"));
    }

    /// The gated stop's two commits, as chant makes them.
    fn chants(dir: &Path) {
        commit(dir, &format!("mkdir -p _gates && echo '{}' >> _gates/release.jsonl", pending()));
        commit(dir, &format!("mkdir -p local && echo '{}' >> local/runs__release.jsonl", run_record()));
    }

    /// Bundle the plan's commits as the plan step does, take it, and check
    /// it: the tip, or why not.
    fn checked(dir: &Path, name: &str, gates: Option<&str>) -> (PlanLedger, Option<String>, Result<String, String>) {
        let l = PlanLedger::new(dir.join(format!("daemon-{name}"))).unwrap();
        let origin = dir.join("origin.git");
        let before = l.fetch_before(origin.to_str().unwrap(), None).unwrap();
        let since = before.as_ref().map(|b| format!(" ^{b}")).unwrap_or_default();
        sh(&dir.join("work"), &format!("git bundle create -q ../{name}.bundle refs/heads/chant/lifecycle{since}"));
        let uid = nix::unistd::getuid().as_raw();
        let g = gate();
        let want = Want { op: "release", gate: &g, gates_allowed: gates, source_ref: SHA };
        let r =
            l.take_bundle(&dir.join(format!("{name}.bundle")), uid).and_then(|()| l.check(before.as_deref(), &want));
        (l, before, r)
    }

    /// What chant writes when an op stops at a gate goes through, on top of
    /// origin's ledger or as its first, and is pushed from here.
    #[test]
    fn chants_gated_stop_is_pushed() {
        for ledger in [true, false] {
            let dir = scratch(&format!("ok-{ledger}"), ledger);
            chants(&dir);
            let (l, before, r) = checked(&dir, "ok", Some("approve-release,ship"));
            let tip = r.unwrap();
            assert_eq!(before.is_some(), ledger);
            let origin = dir.join("origin.git");
            l.push(origin.to_str().unwrap(), &tip, before.as_deref(), None).unwrap();
            assert_eq!(sh(&origin, "git rev-parse refs/heads/chant/lifecycle"), tip);
        }
    }

    /// Anything else is refused before any token is asked for.
    #[test]
    fn anything_else_is_refused() {
        type Make = fn(&Path);
        let cases: [(&str, bool, Make, Option<&str>, &str); 9] = [
            // Another op's fact rewritten, or this op's old lines.
            (
                "other-op",
                true,
                |d| {
                    chants(d);
                    commit(d, "echo '{\"kind\":\"resolution\",\"op\":\"other\"}' >> _gates/other.jsonl");
                },
                None,
                "changes _gates/other.jsonl",
            ),
            (
                "rewrite",
                true,
                |d| {
                    commit(d, &format!("echo '{}' > _gates/release.jsonl", pending()));
                },
                None,
                "rewrites _gates/release.jsonl",
            ),
            // A path chant doesn't write at a gate: a workflow.
            (
                "workflow",
                true,
                |d| {
                    chants(d);
                    commit(d, "mkdir -p .github/workflows && echo 'on: push' > .github/workflows/x.yml");
                },
                None,
                "changes .github/workflows/x.yml",
            ),
            // When origin has none, only chant's paths.
            (
                "orphan-workflow",
                false,
                |d| {
                    chants(d);
                    commit(d, "mkdir -p .github/workflows && echo 'on: push' > .github/workflows/x.yml");
                },
                None,
                "changes .github/workflows/x.yml",
            ),
            // Not on top of origin's ledger.
            (
                "not-descending",
                true,
                |d| {
                    sh(
                        &d.join("work"),
                        "git checkout -q --orphan elsewhere && git rm -rq --cached . && rm -rf _gates local",
                    );
                    chants(d);
                    sh(&d.join("work"), "git branch -f chant/lifecycle elsewhere && git checkout -q chant/lifecycle");
                },
                None,
                "doesn't add to origin's ledger",
            ),
            // A gate the environment doesn't allow.
            ("gate", true, chants, Some("ship"), "chant_gates doesn't name"),
            // Another run's fact, or two.
            (
                "two-facts",
                true,
                |d| {
                    chants(d);
                    commit(
                        d,
                        &format!("echo '{}' >> _gates/release.jsonl", pending().replace("local-1794", "local-9994")),
                    );
                },
                None,
                "adds 2 lines",
            ),
            // A file that's a link, or a mode chant never writes.
            (
                "link",
                true,
                |d| {
                    chants(d);
                    commit(
                        d,
                        "ln -s /etc/passwd local/runs__release.jsonl.tmp && rm local/runs__release.jsonl.tmp && chmod +x _gates/release.jsonl",
                    );
                },
                None,
                "other than by adding to a file",
            ),
            // A merge.
            (
                "merge",
                true,
                |d| {
                    chants(d);
                    sh(
                        &d.join("work"),
                        "git checkout -q -b side HEAD~1 && git commit -q --allow-empty -m side && git checkout -q chant/lifecycle && git merge -q --no-edit side",
                    );
                },
                None,
                "doesn't add to origin's ledger",
            ),
        ];
        for (name, ledger, make, gates, says) in cases {
            let dir = scratch(name, ledger);
            make(&dir);
            let (_, _, r) = checked(&dir, name, gates);
            let why = r.expect_err(name);
            assert!(why.contains(says) && why.ends_with("nothing is pushed"), "{name}: {why}");
        }
    }

    /// A bundle larger than [`MAX_BUNDLE`], or a link, isn't taken.
    #[test]
    fn an_oversized_bundle_or_a_link_isnt_taken() {
        let dir = scratch("big", true);
        let l = PlanLedger::new(dir.join("daemon")).unwrap();
        let uid = nix::unistd::getuid().as_raw();
        std::fs::File::create(dir.join("big.bundle")).unwrap().set_len(MAX_BUNDLE + 1).unwrap();
        let why = l.take_bundle(&dir.join("big.bundle"), uid).unwrap_err();
        assert!(why.contains("larger than 32 MiB"), "{why}");
        std::os::unix::fs::symlink(dir.join("origin.git/HEAD"), dir.join("link.bundle")).unwrap();
        let why = l.take_bundle(&dir.join("link.bundle"), uid).unwrap_err();
        assert!(why.contains("is a link"), "{why}");
    }

    /// Origin's ledger moved since the plan read it: said as that, not as
    /// the token's fault.
    #[test]
    fn a_ledger_that_moved_says_so() {
        let dir = scratch("moved", true);
        chants(&dir);
        let (l, before, r) = checked(&dir, "moved", None);
        let tip = r.unwrap();
        sh(
            &dir,
            "git clone -q -b chant/lifecycle origin.git other && cd other && echo x >> _gates/other.jsonl \
             && git commit -qam other && git push -q origin chant/lifecycle",
        );
        let origin = dir.join("origin.git");
        let e = l.push(origin.to_str().unwrap(), &tip, before.as_deref(), None).unwrap_err();
        assert_eq!(e, PushFailed::Moved);
        assert!(e.to_string().contains("moved since this deploy was planned: plan it again"), "{e}");
    }

    #[test]
    fn chants_gate_files_only() {
        assert_eq!(allowed("_gates/release.jsonl", "release"), Some((String::new(), Kind::Gates)));
        assert_eq!(allowed("local/runs__release.jsonl", "release"), Some((String::new(), Kind::Runs)));
        assert_eq!(
            allowed("_members/api/_members/db/_gates/release.jsonl", "release"),
            Some(("_members/api/_members/db/".into(), Kind::Gates))
        );
        assert_eq!(
            allowed("_members/api/prod/runs__release.jsonl", "release"),
            Some(("_members/api/".into(), Kind::Runs))
        );
        for no in [
            "_gates/other.jsonl",
            "_gates/release/applied.jsonl",
            "_gates/release/requests/x.json",
            ".github/workflows/x.yml",
            "github/releases.jsonl",
            "_members/_gates/release.jsonl",
            "_members/a b/_gates/release.jsonl",
            "_gates/runs__release.jsonl",
            "a/b/runs__release.jsonl",
            "release.jsonl",
        ] {
            assert_eq!(allowed(no, "release"), None, "{no}");
        }
    }
}
