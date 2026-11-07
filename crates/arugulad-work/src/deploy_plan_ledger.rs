//! A chant-fly deploy's plan (#1985): chant's ledger, checked and pushed by
//! this daemon itself, never by the run user.
//!
//! The plan's step that runs the commit's code leaves chant's ledger as a
//! bundle of what it added (`before..tip`). Once that step and everything
//! it started are gone, this daemon, as its own user, in a directory of its
//! own (0700) and with git reading no configuration but its own:
//! - takes the bundle as data ([`PlanLedger::take_bundle`]): handed over by
//!   the run user, only a regular file with one name that it owns, not a
//!   link, at most [`MAX_BUNDLE`] bytes, streamed (`crate::run_files`);
//! - checks it ([`PlanLedger::check`]), git's fsck first: its commits are a
//!   line on top of `before`, origin's ledger as this daemon read it before
//!   the plan ran ([`PlanLedger::fetch_before`]), or new from nothing when
//!   origin had none; each commit only appends whole lines to the two files
//!   chant writes when an op stops at a gate, under this deploy's member's
//!   prefix and environment (`<prefix>_gates/<op>.jsonl` and
//!   `<prefix><env>/runs__<op>.jsonl`); and in all they add exactly this
//!   run's pending fact for the gate the plan reported, which the
//!   environment's `chant_gates` allows, and at most its run record, each
//!   with only the keys and values chant writes there. That layout is chant
//!   [`VERIFIED_CHANT`]'s, read in its source; a plan by any other chant is
//!   refused as not supported yet;
//! - only then is the token that writes asked for, and the ledger pushed
//!   ([`PlanLedger::push`]), with the token in git's environment only (this
//!   daemon's process, never a file or a command line), and every copy of
//!   it this daemon holds wiped after.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use arugula_e2e::deploy::chant::Gate;
use serde_json::{Map, Value};

use crate::RunAs;

/// The largest bundle of chant's ledger a plan may leave: 32 MiB.
pub const MAX_BUNDLE: u64 = 32 * 1024 * 1024;
/// The largest ledger file a check reads, before or after: 64 MiB.
pub const MAX_LEDGER_FILE: u64 = 64 * 1024 * 1024;
/// The most commits a plan may add to chant's ledger: chant makes two.
pub const MAX_COMMITS: usize = 8;
/// The chants whose gated stop writes exactly the layout [`PlanLedger::check`]
/// knows, from their source: 0.124.64 to 0.124.68, inclusive.
pub const VERIFIED_CHANT: (&str, &str) = ("0.124.64", "0.124.68");
/// The longest a pending fact may wait, from its time: an op's gate
/// `timeout` (chant's default 48h).
const MAX_EXPIRY_MS: u64 = 30 * 24 * 3600 * 1000;
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
    /// Where this deploy's chant member keeps its ledger: `""`, or
    /// `_members/<member>/` (what the plan's fetch read at the commit).
    pub prefix: &'a str,
    /// The environment's `chant_env`: a run record goes under it, or under
    /// `local` (an op with no `Env` label).
    pub chant_env: &'a str,
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

/// Whether chant `v` (`x.y.z`) is in [`VERIFIED_CHANT`].
pub fn verified_chant(v: &str) -> bool {
    let parse = |s: &str| -> Option<[u64; 3]> {
        let mut it = s.split('.').map(|p| p.parse::<u64>().ok());
        let out = [it.next()??, it.next()??, it.next()??];
        it.next().is_none().then_some(out)
    };
    match (parse(v), parse(VERIFIED_CHANT.0), parse(VERIFIED_CHANT.1)) {
        (Some(v), Some(lo), Some(hi)) => lo <= v && v <= hi,
        _ => false,
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
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&dir).map_err(|e| format!("its ledger's directory: {e}"))?;
        let l = Self { dir };
        let init = l.git(&["init", "-q", "--bare"], None, false)?;
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

    /// `git args` on its repository (`in_repo`; else the repository made),
    /// reading no configuration but its own, with `token` (when given) as
    /// GitHub's basic auth through its environment only. The header made
    /// of it is wiped once git ends.
    fn git(&self, args: &[&str], token: Option<&str>, in_repo: bool) -> Result<Output, String> {
        let mut c = Command::new("git");
        c.env_clear()
            .env("PATH", crate::RUN_PATH)
            .env("HOME", &self.dir)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&self.dir)
            .stdin(Stdio::null());
        if in_repo {
            c.env("GIT_DIR", self.repo());
        } else {
            c.arg("init").args(&args[1..]).arg(self.repo());
        }
        let mut header = None;
        if let Some(t) = token {
            use base64::Engine as _;
            let mut pair = format!("x-access-token:{t}").into_bytes();
            let basic = base64::engine::general_purpose::STANDARD.encode(&pair);
            pair.iter_mut().for_each(|b| *b = 0);
            let h = format!("AUTHORIZATION: basic {basic}");
            super::wipe_bytes(basic.into_bytes());
            c.env("GIT_CONFIG_COUNT", "1").env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader");
            c.env("GIT_CONFIG_VALUE_0", &h);
            header = Some(h);
        }
        if in_repo {
            c.args(args);
        }
        let out = c.output().map_err(|e| format!("git: {e}"));
        // The command's own copy of its environment goes with it.
        drop(c);
        if let Some(h) = header {
            super::wipe_bytes(h.into_bytes());
        }
        out
    }

    /// `git args`'s standard output, or why it failed.
    fn out(&self, args: &[&str]) -> Result<String, String> {
        let o = self.git(args, None, true)?;
        if !o.status.success() {
            return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// Origin's ledger, fetched here with `token` (one that reads): its
    /// tip, or `None` when origin has none. What the plan must add to.
    pub fn fetch_before(&self, origin: &str, token: Option<&str>) -> Result<Option<String>, String> {
        let o = self.git(&["ls-remote", "--", origin, LEDGER], token, true)?;
        if !o.status.success() {
            return Err(format!("reading origin's ledger: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        if String::from_utf8_lossy(&o.stdout).trim().is_empty() {
            return Ok(None);
        }
        let spec = format!("+{LEDGER}:{LEDGER}");
        let o = self.git(&["fetch", "-q", "--no-tags", "--", origin, &spec], token, true)?;
        if !o.status.success() {
            return Err(format!("fetching origin's ledger: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(Some(self.out(&["rev-parse", "--verify", LEDGER])?.trim().to_owned()))
    }

    /// The bundle the plan left at `from`, handed over by the run user
    /// (`crate::run_files`: a regular file with one name that it owns, not a
    /// link) and kept here, at most [`MAX_BUNDLE`] bytes, streamed.
    pub async fn take_bundle(&self, as_: Option<&RunAs>, from: &Path) -> Result<(), String> {
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut to = opts.open(self.bundle()).await.map_err(|e| format!("keeping the plan's bundle: {e}"))?;
        match crate::run_files::copy_own(as_, from, MAX_BUNDLE, &mut to).await {
            Ok(_) => Ok(()),
            Err(crate::run_files::NotRead::TooLarge) => Err(format!(
                "the plan's bundle of chant's ledger is larger than {} MiB: nothing is pushed",
                MAX_BUNDLE >> 20
            )),
            Err(why) => Err(format!("the plan's bundle of chant's ledger {why}: nothing is pushed")),
        }
    }

    /// The taken bundle, checked against `before` (origin's tip as fetched
    /// here, `None`: origin had none) and `want`: its tip, to push.
    pub fn check(&self, before: Option<&str>, want: &Want) -> Result<String, String> {
        let refused = |why: String| Err(format!("chant's ledger from the plan {why}: nothing is pushed"));
        let g = want.gate;
        if !verified_chant(&g.chant) {
            return Err(format!(
                "this chant isn't supported for planning yet: the plan ran chant {}, and Arugula checks the ledger \
                 chant {} to {} writes at a gate. Pin one of those in the repository's package.json",
                g.chant, VERIFIED_CHANT.0, VERIFIED_CHANT.1
            ));
        }
        if g.op != want.op {
            return refused(format!("is for op {}, not {}", g.op, want.op));
        }
        if let Some(allowed) = want.gates_allowed
            && !allowed.split(',').any(|a| a == g.gate)
        {
            return refused(format!("stops at gate {}, which the environment's chant_gates doesn't name", g.gate));
        }
        if g.checkpoint.is_some() {
            return Err(
                "a gate that resumes from a checkpoint (chant#3947) isn't supported for planning yet: nothing is pushed"
                    .into(),
            );
        }
        let bundle = self.bundle();
        let b = bundle.to_string_lossy();
        let spec = format!("+{LEDGER}:{PLANNED}");
        if let Err(why) = self.out(&["fetch", "-q", "--no-tags", &b, &spec]) {
            return refused(format!("doesn't read as a bundle of chant/lifecycle on top of origin's ({why})"));
        }
        // git's own check of every object, whatever its transfer checks did.
        if let Err(why) = self.out(&["fsck", "--strict", "--no-dangling", "--no-progress"]) {
            return refused(format!("doesn't pass git fsck ({why})"));
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
        // What each commit changes: only chant's two gate files, under this
        // member's prefix and environment, each only added to, by whole lines.
        let mut gates_file = None;
        let mut runs_file: Option<(String, String)> = None;
        for c in &commits {
            let raw =
                self.out(&["-c", "core.quotePath=false", "diff-tree", "-r", "--raw", "--no-renames", "--root", c[0]])?;
            for line in raw.lines().filter(|l| l.starts_with(':')) {
                let (meta, path) = line.split_once('\t').unwrap_or((line, ""));
                let f: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
                let [old_mode, new_mode, old_blob, new_blob, status] = f[..] else {
                    return refused(format!("changes {path} in a way it can't read"));
                };
                let Some(kind) = allowed(path, want) else {
                    return refused(format!(
                        "changes {path}, which chant doesn't write for this deploy when an op stops at a gate"
                    ));
                };
                let added = status == "A" && old_mode == "000000";
                let modified = status == "M" && old_mode == "100644";
                if !(added || modified) || new_mode != "100644" {
                    return refused(format!("changes {path} other than by adding to a file"));
                }
                let old = if modified { self.blob(old_blob)? } else { Vec::new() };
                if appended(&old, &self.blob(new_blob)?).is_none() {
                    return refused(format!("changes {path} other than by adding whole lines to its end"));
                }
                match kind {
                    Kind::Gates => {
                        if gates_file.get_or_insert_with(|| path.to_owned()) != path {
                            return refused(format!("writes two gate files ({path})"));
                        }
                    }
                    Kind::Runs(env) => {
                        if runs_file.get_or_insert_with(|| (path.to_owned(), env.clone())).0 != path {
                            return refused(format!("writes two run records' files ({path})"));
                        }
                    }
                }
            }
        }
        // What it adds in all: this run's pending fact, and its run record.
        let Some(gates) = gates_file else { return refused("has no pending fact of this run's".into()) };
        let added = self.added(before, &tip, &gates)?;
        let [pending] = added.as_slice() else {
            return refused(format!("adds {} lines to {gates}, not one pending fact", added.len()));
        };
        if let Err(why) = pending_ok(pending, want) {
            return refused(format!("adds to {gates} a pending fact that {why}"));
        }
        if let Some((runs, env)) = runs_file {
            let added = self.added(before, &tip, &runs)?;
            let ok = match added.as_slice() {
                [] => Ok(()),
                [line] => run_record_ok(line, want, &env),
                _ => Err(format!("is {} lines, not one record", added.len())),
            };
            if let Err(why) = ok {
                return refused(format!("adds to {runs} a run record that {why}"));
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
        let o = self.git(&["cat-file", "blob", id], None, true)?;
        if !o.status.success() {
            return Err(format!("reading chant's ledger: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(o.stdout)
    }

    /// The lines `tip` adds to `path` after `before`.
    fn added(&self, before: Option<&str>, tip: &str, path: &str) -> Result<Vec<String>, String> {
        let id = |rev: &str| -> Result<Option<String>, String> {
            let o = self.git(&["rev-parse", "-q", "--verify", &format!("{rev}:{path}")], None, true)?;
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
        appended(&old, &new).ok_or_else(|| format!("chant's ledger from the plan rewrites {path}: nothing is pushed"))
    }

    /// Push `tip` as origin's ledger, with `token` (one that writes), only
    /// while origin's is still `before` (none: still none).
    pub fn push(&self, origin: &str, tip: &str, before: Option<&str>, token: Option<&str>) -> Result<(), PushFailed> {
        let lease = format!("--force-with-lease={LEDGER}:{}", before.unwrap_or(""));
        let refspec = format!("{tip}:{LEDGER}");
        let o = self
            .git(&["push", "--porcelain", "--no-verify", &lease, "--", origin, &refspec], token, true)
            .map_err(PushFailed::Other)?;
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

/// The whole lines `new` adds to the end of `old`: `None` unless `old` is
/// empty or ends with a newline, `new` starts with it, and what follows is
/// lines that each end with one.
fn appended(old: &[u8], new: &[u8]) -> Option<Vec<String>> {
    if !(old.is_empty() || old.ends_with(b"\n")) {
        return None;
    }
    let rest = new.strip_prefix(old)?;
    if !(rest.is_empty() || rest.ends_with(b"\n")) {
        return None;
    }
    let text = std::str::from_utf8(rest).ok()?;
    Some(text.split_terminator('\n').map(str::to_owned).collect())
}

/// The keys of `o`, each checked by `check` against what it must be; any key
/// it doesn't know is refused.
fn exact(o: &Map<String, Value>, check: impl Fn(&str, &Value) -> Option<bool>) -> Result<(), String> {
    for (k, v) in o {
        match check(k, v) {
            Some(true) => {}
            Some(false) => return Err(format!("has {k} {v}, not this run's")),
            None => return Err(format!("carries {k}, which chant doesn't write at a gate on a box")),
        }
    }
    Ok(())
}

/// The pending fact, exactly as chant writes it when this run stops at the
/// gate: its keys, each one's value from the gate, run, commit and time,
/// and chant's own defaults for the rest.
fn pending_ok(line: &str, want: &Want) -> Result<(), String> {
    let g = want.gate;
    let p: Value = serde_json::from_str(line).map_err(|_| "doesn't read as JSON".to_owned())?;
    let o = p.as_object().ok_or("isn't an object")?;
    for k in ["version", "kind", "op", "gate", "runId", "gitSha", "timestamp", "expiresAt", "planDigest", "origin"] {
        if !o.contains_key(k) {
            return Err(format!("has no {k}"));
        }
    }
    if g.environment.is_some() && !o.contains_key("environment") {
        return Err("has no environment, which the gate names".into());
    }
    let at = arugula_e2e::deploy::chant::timestamp_ms(&g.recorded);
    exact(o, |k, v| {
        Some(match k {
            "version" => v == 1,
            "kind" => v == "pending",
            "op" => v == want.op,
            "gate" => v == g.gate.as_str(),
            "runId" => v == g.run.as_str(),
            "gitSha" => v == want.source_ref,
            "timestamp" => v == g.recorded.as_str(),
            "planDigest" => v == g.plan.as_str(),
            // chant's own origin on a box, with no MCP or ACP channel.
            "origin" => v == "cli",
            // From the op's gate `timeout`: after the fact, and not for long.
            "expiresAt" => {
                let end = v.as_str().and_then(arugula_e2e::deploy::chant::timestamp_ms);
                matches!((at, end), (Some(a), Some(e)) if e > a && e - a <= MAX_EXPIRY_MS)
            }
            // Only ever narrows who may approve.
            "neverOverMcp" => v == true,
            // The gate's own description, as the op declares it.
            "description" => v.as_str().is_some_and(|d| d.len() <= 1024 && !d.chars().any(char::is_control)),
            "environment" => g.environment.as_deref().is_some_and(|e| v == e),
            _ => return None,
        })
    })
}

/// The run record chant writes beside it, for this run gated at the gate,
/// under environment `env`.
fn run_record_ok(line: &str, want: &Want, env: &str) -> Result<(), String> {
    let g = want.gate;
    let r: Value = serde_json::from_str(line).map_err(|_| "doesn't read as JSON".to_owned())?;
    let o = r.as_object().ok_or("isn't an object")?;
    for k in ["version", "id", "op", "env", "started", "ended", "status", "labels", "outcomes", "phases", "gate"] {
        if !o.contains_key(k) {
            return Err(format!("has no {k}"));
        }
    }
    let time = |v: &Value| v.as_str().and_then(arugula_e2e::deploy::chant::timestamp_ms).is_some();
    exact(o, |k, v| {
        Some(match k {
            "version" => v == 1,
            "id" => v == g.run.as_str(),
            "op" => v == want.op,
            "env" => v == env,
            "status" => v == "gated",
            "gitSha" => v == want.source_ref,
            "started" | "ended" => time(v),
            "gate" => *v == serde_json::json!({ "name": g.gate, "since": g.recorded }),
            "labels" => {
                v.as_object().is_some_and(|l| l.values().all(Value::is_string) && l.get("Env").is_none_or(|e| e == env))
            }
            "outcomes" => v.is_object(),
            "phases" => v.is_array(),
            _ => return None,
        })
    })
}

/// Which of chant's gate files `path` is, for `want`'s deploy: under its
/// member's prefix exactly, the op's gate ledger, or its run records under
/// `chant_env` or `local`. No part of it starts with a dot.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Gates,
    Runs(String),
}

fn allowed(path: &str, want: &Want) -> Option<Kind> {
    if path.split('/').any(|p| p.is_empty() || p.starts_with('.')) {
        return None;
    }
    let rest = path.strip_prefix(want.prefix)?;
    if rest == format!("_gates/{}.jsonl", want.op) {
        return Some(Kind::Gates);
    }
    let (env, file) = rest.split_once('/')?;
    let envs = [want.chant_env, "local"];
    (file == format!("runs__{}.jsonl", want.op) && envs.contains(&env) && !env.is_empty())
        .then(|| Kind::Runs(env.to_owned()))
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

    /// chant 0.124.68's pending fact for `gate()`, as a gated stop on a box
    /// writes it (the keys of the one it wrote on a scratch repository).
    fn pending() -> Value {
        let g = gate();
        serde_json::json!({
            "description": "Approve deploying this commit", "expiresAt": "2026-11-07T23:27:30.435Z", "gate": g.gate,
            "gitSha": SHA, "kind": "pending", "neverOverMcp": true, "op": "release", "origin": "cli",
            "planDigest": g.plan, "runId": g.run, "timestamp": g.recorded, "version": 1,
        })
    }

    /// Its run record.
    fn run_record() -> Value {
        let g = gate();
        serde_json::json!({
            "ended": "2026-11-06T23:27:30.752Z", "env": "local", "gate": { "name": g.gate, "since": g.recorded },
            "gitSha": SHA, "id": g.run, "labels": {}, "op": "release", "outcomes": {},
            "phases": [{ "name": "Plan", "status": "ok", "steps": [] }], "started": "2026-11-06T23:27:30.197Z",
            "status": "gated", "version": 1,
        })
    }

    fn want(g: &Gate) -> Want<'_> {
        Want { op: "release", gate: g, gates_allowed: None, source_ref: SHA, prefix: "", chant_env: "github" }
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

    /// Append `line` to `path` as one commit.
    fn append(dir: &Path, path: &str, line: &Value) {
        let parent = Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let mk = if parent.is_empty() { String::new() } else { format!("mkdir -p {parent} && ") };
        commit(dir, &format!("{mk}printf '%s\\n' '{line}' >> {path}"));
    }

    /// The gated stop's two commits, as chant makes them.
    fn chants(dir: &Path) {
        append(dir, "_gates/release.jsonl", &pending());
        append(dir, "local/runs__release.jsonl", &run_record());
    }

    /// Bundle the plan's commits as the plan step does, take it, and check
    /// it against `want(g)` as `how` changes it: the tip, or why not.
    fn checked(
        dir: &Path,
        name: &str,
        g: &Gate,
        how: impl Fn(Want) -> Want,
    ) -> (PlanLedger, Option<String>, Result<String, String>) {
        let l = PlanLedger::new(dir.join(format!("daemon-{name}"))).unwrap();
        let origin = dir.join("origin.git");
        let before = l.fetch_before(origin.to_str().unwrap(), None).unwrap();
        let since = before.as_ref().map(|b| format!(" ^{b}")).unwrap_or_default();
        sh(&dir.join("work"), &format!("git bundle create -q ../{name}.bundle refs/heads/chant/lifecycle{since}"));
        let w = how(want(g));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let taken = rt.block_on(l.take_bundle(None, &dir.join(format!("{name}.bundle"))));
        let r = taken.and_then(|()| l.check(before.as_deref(), &w));
        (l, before, r)
    }

    #[test]
    fn chants_gate_files_only() {
        let g = gate();
        let flat = want(&g);
        assert_eq!(allowed("_gates/release.jsonl", &flat), Some(Kind::Gates));
        assert_eq!(allowed("local/runs__release.jsonl", &flat), Some(Kind::Runs("local".into())));
        assert_eq!(allowed("github/runs__release.jsonl", &flat), Some(Kind::Runs("github".into())));
        let member = Want { prefix: "_members/api/", ..want(&g) };
        assert_eq!(allowed("_members/api/_gates/release.jsonl", &member), Some(Kind::Gates));
        assert_eq!(allowed("_members/api/local/runs__release.jsonl", &member), Some(Kind::Runs("local".into())));
        for (no, w) in [
            ("_members/other/_gates/release.jsonl", &member),
            ("_gates/release.jsonl", &member),
            ("_members/api/_gates/release.jsonl", &flat),
            ("staging/runs__release.jsonl", &flat),
            ("_gates/other.jsonl", &flat),
            ("_gates/release/applied.jsonl", &flat),
            ("_gates/release/checkpoints/x.json", &flat),
            (".github/workflows/x.yml", &flat),
            ("github/releases.jsonl", &flat),
            ("./_gates/release.jsonl", &flat),
            (".local/runs__release.jsonl", &flat),
            ("_gates//release.jsonl", &flat),
            ("release.jsonl", &flat),
        ] {
            assert_eq!(allowed(no, w), None, "{no}");
        }
    }

    #[test]
    fn only_verified_chants() {
        assert!(verified_chant("0.124.64") && verified_chant("0.124.66") && verified_chant("0.124.68"));
        assert!(!verified_chant("0.124.63") && !verified_chant("0.124.69") && !verified_chant("0.125.0"));
        assert!(!verified_chant("0.124") && !verified_chant("0.124.68.1") && !verified_chant("x"));
    }

    /// What chant writes when an op stops at a gate goes through, on top of
    /// origin's ledger or as its first, and is pushed from here.
    #[test]
    fn chants_gated_stop_is_pushed() {
        for ledger in [true, false] {
            let dir = scratch(&format!("ok-{ledger}"), ledger);
            chants(&dir);
            let (l, before, r) =
                checked(&dir, "ok", &gate(), |w| Want { gates_allowed: Some("approve-release,ship"), ..w });
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
        fn with(k: &str, v: Value) -> Value {
            let mut p = pending();
            p[k] = v;
            p
        }
        let cases: Vec<(&str, bool, Make, Option<&str>, &str)> = vec![
            // Another op's fact rewritten, or this op's old lines.
            (
                "other-op",
                true,
                |d| {
                    chants(d);
                    append(d, "_gates/other.jsonl", &serde_json::json!({ "kind": "resolution", "op": "other" }));
                },
                None,
                "changes _gates/other.jsonl",
            ),
            (
                "rewrite",
                true,
                |d| commit(d, &format!("printf '%s\\n' '{}' > _gates/release.jsonl", pending())),
                None,
                "other than by adding whole lines",
            ),
            // A path chant doesn't write at a gate: a workflow, on top of
            // origin's ledger or when origin has none.
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
            // Another run's fact too.
            (
                "two-facts",
                true,
                |d| {
                    chants(d);
                    append(d, "_gates/release.jsonl", &with("runId", "local-9".into()));
                },
                None,
                "adds 2 lines",
            ),
            // An executable mode.
            (
                "mode",
                true,
                |d| {
                    chants(d);
                    commit(d, "chmod +x _gates/release.jsonl");
                },
                None,
                "other than by adding to a file",
            ),
            // Not whole lines: a line with no newline.
            (
                "no-newline",
                true,
                |d| commit(d, &format!("printf '%s' '{}' >> _gates/release.jsonl", pending())),
                None,
                "other than by adding whole lines",
            ),
            // A key chant doesn't write at a gate on a box, or one of
            // chant's own values the plan picked.
            (
                "extra-key",
                true,
                |d| append(d, "_gates/release.jsonl", &with("approval", serde_json::json!({ "quorum": 0 }))),
                None,
                "carries approval",
            ),
            (
                "origin",
                true,
                |d| append(d, "_gates/release.jsonl", &with("origin", "mcp".into())),
                None,
                "has origin \"mcp\"",
            ),
            (
                "never-over-mcp",
                true,
                |d| append(d, "_gates/release.jsonl", &with("neverOverMcp", false.into())),
                None,
                "has neverOverMcp false",
            ),
            (
                "expires",
                true,
                |d| append(d, "_gates/release.jsonl", &with("expiresAt", "2099-01-01T00:00:00.000Z".into())),
                None,
                "has expiresAt",
            ),
            (
                "plan",
                true,
                |d| append(d, "_gates/release.jsonl", &with("planDigest", "sha256:00".into())),
                None,
                "has planDigest",
            ),
            // Its run record: another status, or another environment than
            // the deploy's or `local`.
            (
                "run-record",
                true,
                |d| {
                    append(d, "_gates/release.jsonl", &pending());
                    let mut r = run_record();
                    r["status"] = "ok".into();
                    append(d, "local/runs__release.jsonl", &r);
                },
                None,
                "has status \"ok\"",
            ),
            (
                "other-env",
                true,
                |d| {
                    append(d, "_gates/release.jsonl", &pending());
                    append(d, "staging/runs__release.jsonl", &run_record());
                },
                None,
                "changes staging/runs__release.jsonl",
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
            let (_, _, r) = checked(&dir, name, &gate(), |w| Want { gates_allowed: gates, ..w });
            let why = r.expect_err(name);
            assert!(why.contains(says) && why.ends_with("nothing is pushed"), "{name}: {why}");
        }
    }

    /// A gate from a chant whose layout isn't verified, one that resumes, a
    /// member's ledger that isn't this deploy's, or a gate's environment the
    /// fact doesn't name: refused, saying which.
    #[test]
    fn the_deploys_own_chant_member_and_environment_only() {
        let dir = scratch("own", true);
        chants(&dir);
        let newer = Gate { chant: "0.124.69".into(), ..gate() };
        let why = checked(&dir, "newer", &newer, |w| w).2.unwrap_err();
        assert!(
            why.starts_with("this chant isn't supported for planning yet") && why.contains("0.124.64 to 0.124.68"),
            "{why}"
        );
        let resumes = Gate { checkpoint: Some(format!("jcs1-sha256:{}", "c".repeat(64))), ..gate() };
        assert!(checked(&dir, "resumes", &resumes, |w| w).2.unwrap_err().contains("resumes from a checkpoint"));
        let why = checked(&dir, "member", &gate(), |w| Want { prefix: "_members/api/", ..w }).2.unwrap_err();
        assert!(why.contains("changes _gates/release.jsonl"), "{why}");
        let named = Gate { environment: Some("prod".into()), ..gate() };
        assert!(checked(&dir, "env", &named, |w| w).2.unwrap_err().contains("has no environment"));
    }

    /// A bundle larger than [`MAX_BUNDLE`], or a link, isn't taken.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_oversized_bundle_or_a_link_isnt_taken() {
        let dir = scratch("big", true);
        let l = PlanLedger::new(dir.join("daemon")).unwrap();
        std::fs::File::create(dir.join("big.bundle")).unwrap().set_len(MAX_BUNDLE + 1).unwrap();
        let why = l.take_bundle(None, &dir.join("big.bundle")).await.unwrap_err();
        assert!(why.contains("larger than 32 MiB"), "{why}");
        let l = PlanLedger::new(dir.join("daemon")).unwrap();
        std::os::unix::fs::symlink(dir.join("origin.git/HEAD"), dir.join("link.bundle")).unwrap();
        let why = l.take_bundle(None, &dir.join("link.bundle")).await.unwrap_err();
        assert!(why.contains("is a link"), "{why}");
    }

    /// Origin's ledger moved since the plan read it: said as that, not as
    /// the token's fault.
    #[test]
    fn a_ledger_that_moved_says_so() {
        let dir = scratch("moved", true);
        chants(&dir);
        let (l, before, r) = checked(&dir, "moved", &gate(), |w| w);
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
}
