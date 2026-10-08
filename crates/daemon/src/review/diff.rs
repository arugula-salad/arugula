//! Diff blocks (M11): what changed in a git repository on the block's
//! host, read-only.
//!
//! Config `{repo, rev_a?, rev_b?, run_as?}`. No revisions: the working
//! tree (staged, unstaged and untracked) against `HEAD`; one: that revision
//! against the working tree (untracked too); two: the range. A revision may
//! be a tree (git's empty tree: everything in the repository). `repo` is any
//! directory in the repository.
//!
//! `run_as: "fountain"` (M45b, from the Fountain runner view's *Changes*):
//! git runs as the runner's user, through `sudo -n -u fountain /bin/bash
//! -c` (the one form M45a's sudoers rule allows), for a sandbox arugulad
//! can't read itself. Only that user, only on this host, and `repo` must be
//! an absolute path; only the owner opens blocks other than agents, so only
//! the owner can ask for it.
//!
//! One `sh -c` on the host does the lot (one exec on a VM): find the
//! repository's top, check the revisions, `git diff -M`, then each
//! untracked file against `/dev/null`, cut at [`OUT_MAX`]. No external
//! diff, textconv or fsmonitor runs, so a repository's own config can't
//! make a poll run its commands. The state is a
//! file list with +/− and, for the files someone opened, their hunks; a
//! file whose diff is over [`FILE_MAX`] shows as too big (open the file),
//! a binary one as binary. `capture --text` is the unified diff.
//!
//! #619: `chant: {root, chant?}` (from a workspace member's *Changes*):
//! each open file's hunks also say which decision and run made them, read
//! from that workspace's chant when the file opens and again when its diff
//! or `HEAD` moves, one read at a time (`labs/workspace/why.rs`, through
//! [`crate::labs::file_why`]; a build without Labs says it can't). With `base: true`
//! and no revisions, the working tree is compared with where `HEAD` left
//! the default branch (the merge base with `origin/HEAD`, `main` or
//! `master`), so a member's committed work shows beside what isn't
//! committed yet; `chant.member` keeps it to the member's directory
//! (`repo`). When `HEAD` is on the default branch the merge base is `HEAD`,
//! and the state says so (`on_default`); `base {rev}` picks another base
//! (`HEAD~3`), `base {}` goes back to the merge base.
//!
//! Methods (editor role): `refresh`, and `file {path, open?}` to open or
//! close a file's hunks. "Open file" at a line is an ordinary file block
//! opened beside this one (`POST /api/blocks`, `from_pane` this block).

use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use arugula_proto::BlockType;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Live, Runner};
use crate::labs::FileWhy;
use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    store::now_ms,
};

/// A file's diff bigger than this shows as "too big, open the file".
pub const FILE_MAX: usize = 256 * 1024;
/// The most of a diff read in one go.
const OUT_MAX: usize = 4 * 1024 * 1024;
/// Files listed, and untracked files looked at.
const FILES_MAX: usize = 2000;
const UNTRACKED_MAX: usize = 200;
/// Files open at once (opening another closes the first opened).
const OPEN_MAX: usize = 12;
/// How often it looks again while drawn, here (a machine: every 3s).
const POLL: Duration = Duration::from_secs(2);
/// How often a `run_as` diff looks again while drawn.
const SUDO_POLL: Duration = Duration::from_secs(15);
/// git's empty tree: what a repository with no commits is compared with.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Everything in one `sh -c`: `$1` the directory, `$2` 1 to add untracked
/// files, `$3` the cap, `$4` 1 to compare with the default branch's merge
/// base when no revision is given, `$5` 1 to keep to `$1` (a member's
/// directory) rather than the whole repository, then the revisions. The
/// first line is `ok HEAD BASE TOP` (BASE `<branch>:<merge base>`, or
/// `=<branch>` when `HEAD` is on it; `-` for no `HEAD`, or no base) or
/// `err WHY`; then the diff, `arugula-untracked`, untracked files' diffs, `arugula-big
/// PATH` for one too big to read and `arugula-more` past the cap on how
/// many.
const SCRIPT: &str = r#"r=$1; u=$2; cap=$3; b=$4; p=$5; shift 5
case $r in "~") r=$HOME ;; "~/"*) r=$HOME/${r#"~/"} ;; esac
g="-c core.quotePath=false -c core.fsmonitor=false"
cd -- "$r" 2>/dev/null || { printf 'err no such directory: %s\n' "$r"; exit 0; }
command -v git >/dev/null 2>&1 || { echo "err git isn't installed here"; exit 0; }
top=$(git rev-parse --show-toplevel 2>&1) || { printf 'err %s\n' "$(printf '%s\n' "$top" | head -n 1)"; exit 0; }
ps=.
if [ "$p" = 1 ]; then ps=$(git rev-parse --show-prefix 2>/dev/null); ps=${ps:-.}; fi
cd -- "$top" || exit 0
for x; do git rev-parse -q --verify "$x^{tree}" >/dev/null 2>&1 || { printf 'err no such revision: %s\n' "$x"; exit 0; }; done
h=$(git rev-parse -q --verify HEAD 2>/dev/null) || h=-
base=-
if [ $# -eq 0 ]; then
  if [ "$h" != - ]; then set -- HEAD; else set -- EMPTY; fi
  if [ "$b" = 1 ] && [ "$h" != - ]; then
    for x in origin/HEAD main master; do
      m=$(git merge-base HEAD "$x" 2>/dev/null) || continue
      if [ "$m" = "$h" ]; then base==$x; else set -- "$m"; base=$x:$m; fi
      break
    done
  fi
fi
printf 'ok %s %s %s\n' "$h" "$base" "$top"
{
  git $g diff --no-color --no-ext-diff --no-textconv -M "$@" -- "$ps" 2>/dev/null
  if [ "$u" = 1 ]; then
    echo arugula-untracked
    git $g ls-files --others --exclude-standard -- "$ps" 2>/dev/null | {
      n=0
      while IFS= read -r f; do
        n=$((n+1))
        if [ $n -gt UNTRACKED ]; then echo arugula-more; break; fi
        if [ -f "$f" ] && [ ! -L "$f" ] && [ "$(wc -c < "$f")" -gt FILEMAX ]; then printf 'arugula-big %s\n' "$f"; continue; fi
        git $g diff --no-color --no-ext-diff --no-textconv --no-index -- /dev/null "$f" 2>/dev/null
      done
    }
  fi
} | head -c "$cap"
"#;

fn script() -> String {
    SCRIPT
        .replace("EMPTY", EMPTY_TREE)
        .replace("UNTRACKED", &UNTRACKED_MAX.to_string())
        .replace("FILEMAX", &FILE_MAX.to_string())
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Config {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    rev_a: Option<String>,
    #[serde(default)]
    rev_b: Option<String>,
    /// Files open when it was saved.
    #[serde(default)]
    open: Vec<String>,
    /// Run git as this user (only `fountain`, M45b).
    #[serde(default)]
    run_as: Option<String>,
    /// #619: the chant workspace it was opened from, to say what made each
    /// hunk.
    #[serde(default)]
    chant: Option<ChantConfig>,
    /// #619: with no revisions, against the default branch's merge base.
    #[serde(default)]
    base: bool,
    /// #619: the revision picked in its place.
    #[serde(default)]
    picked: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChantConfig {
    /// The workspace root, where chant is asked.
    root: String,
    /// The member it was opened for: the diff keeps to `repo`, its
    /// directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    member: Option<String>,
    /// The workspace's chant, as the workspace block found it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chant: Option<String>,
}

/// How long a file's why waits after its last read before the next, at
/// first; while its added lines keep moving it waits twice as long each
/// time, up to [`WHY_GAP_MAX`].
const WHY_GAP: Duration = Duration::from_secs(3);
const WHY_GAP_MAX: Duration = Duration::from_secs(30);

/// One open file's why: the added lines it was read for, and what chant
/// said. The last answer stays drawn until the next lands.
#[derive(Default)]
struct WhyRead {
    /// [`why_key`] of the added lines and `HEAD` read for.
    key: u64,
    read: Option<Result<FileWhy, String>>,
    reading: bool,
    last: Option<Instant>,
    gap: Option<Duration>,
}

/// What a `base` diff is compared with, in words.
fn against_base(base: &str) -> String {
    if let Some(branch) = base.strip_prefix('=') {
        return format!("the working tree against HEAD, on {branch}");
    }
    match base.split_once(':') {
        Some((branch, sha)) => {
            format!("the working tree against where it left {branch} ({})", &sha[..sha.len().min(8)])
        }
        None => "the working tree against HEAD".to_owned(),
    }
}

/// What a file's why is read for: which of its lines were added, and
/// `HEAD` (a commit moves lines from uncommitted to committed without
/// changing a diff against a fixed base). An edit within lines already
/// added changes neither, so it reads nothing.
fn why_key(text: &str, head: &str) -> u64 {
    let added: Vec<u32> = hunks(text).iter().flat_map(|h| h.lines.iter()).filter(|l| l.0 == '+').map(|l| l.2).collect();
    let mut h = DefaultHasher::new();
    (added, head).hash(&mut h);
    h.finish()
}

/// One file in the diff.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FileDiff {
    pub path: String,
    /// Where a renamed file was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    /// `modified`, `added`, `deleted`, `renamed`, `untracked`, `mode`.
    pub status: &'static str,
    pub add: u32,
    pub del: u32,
    #[serde(skip_serializing_if = "is_false")]
    pub binary: bool,
    /// Its diff is over [`FILE_MAX`]: open the file instead.
    #[serde(skip_serializing_if = "is_false")]
    pub big: bool,
    /// Its section of the diff (empty when big).
    #[serde(skip)]
    pub text: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A hunk as drawn: its header, then `[kind, old line, new line, text]`
/// per line. Kind is ` `, `+`, `-` or `\` ("no newline at end of file");
/// both numbers are always set: the other side's is where the line falls
/// there, so "open file" on a removed line lands where it was.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hunk {
    pub at: String,
    pub lines: Vec<(char, u32, u32, String)>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct State {
    /// The repository's top directory, once found.
    repo: Option<String>,
    name: Option<String>,
    rev_a: Option<String>,
    rev_b: Option<String>,
    /// What it's compared with, in words.
    against: String,
    /// #619: compared with the default branch's merge base (a member's
    /// *Changes*), so another base can be picked.
    #[serde(skip_serializing_if = "is_false")]
    base: bool,
    /// The default branch `HEAD` is on, when the merge base is `HEAD`
    /// itself: committed work on it isn't shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    on_default: Option<String>,
    /// The revision picked in place of the merge base.
    #[serde(skip_serializing_if = "Option::is_none")]
    picked: Option<String>,
    /// Whose git it runs (M45b: `fountain`), when not yours.
    #[serde(skip_serializing_if = "Option::is_none")]
    run_as: Option<String>,
    files: Vec<Value>,
    add: u32,
    del: u32,
    /// More files than listed, or a diff cut short.
    truncated: bool,
    loading: bool,
    error: Option<String>,
    /// Polling its host now (someone draws it).
    watching: bool,
    /// When it last read the diff.
    updated_ms: u64,
}

/// What the last read found.
#[derive(Default)]
struct Read {
    /// Of the script's output, to skip rebuilding the same thing.
    hash: u64,
    files: Vec<FileDiff>,
    truncated: bool,
    /// `HEAD` when it was read (`-`: none).
    head: String,
}

pub struct Diff {
    ctx: BlockCtx,
    me: Weak<Diff>,
    runner: Result<Runner, String>,
    config: Config,
    open: Mutex<Vec<String>>,
    read: Mutex<Read>,
    state: Mutex<State>,
    live: Live,
    /// One read at a time.
    reading: tokio::sync::Mutex<()>,
    /// #619: each open file's why, by path.
    whys: Mutex<HashMap<String, WhyRead>>,
    /// One chant read at a time.
    asking: tokio::sync::Mutex<()>,
    /// #619: the revision picked in place of the merge base.
    picked: Mutex<Option<String>>,
}

impl Diff {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("diff config: {e}"))?;
        for r in [&config.rev_a, &config.rev_b].into_iter().flatten() {
            if r.is_empty() || r.starts_with('-') || r.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!("not a revision: {r:?}"));
            }
        }
        if config.rev_b.is_some() && config.rev_a.is_none() {
            return Err("rev_b needs rev_a".into());
        }
        let repo = config.repo.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "~".into());
        let runner = Runner::of(&ctx);
        if let Some(user) = &config.run_as {
            crate::labs::check_run_as(user, &repo, ctx.sprite.is_some())?;
        } else if let Ok(Runner::Local { .. }) = &runner
            && !ctx.restoring
        {
            // Here, the same places are refused as for `/api/fs`.
            ctx.fs.resolve(&repo).map_err(|e| e.to_string())?;
        }
        let empty = |r: &str| r == EMPTY_TREE;
        let against = match (&config.rev_a, &config.rev_b) {
            (Some(a), None) if empty(a) => "everything in it (against an empty tree)".to_owned(),
            (None, _) => "the working tree against HEAD".to_owned(),
            (Some(a), None) => format!("the working tree against {a}"),
            (Some(a), Some(b)) => format!("{a}..{b}"),
        };
        let state = State {
            base: config.base && config.rev_a.is_none(),
            rev_a: config.rev_a.clone(),
            rev_b: config.rev_b.clone(),
            against,
            run_as: config.run_as.clone(),
            loading: !ctx.restoring,
            ..Default::default()
        };
        super::log(&ctx, &json!({ "e": "diff", "repo": repo, "rev_a": config.rev_a, "rev_b": config.rev_b }));
        let open = config.open.iter().take(OPEN_MAX).cloned().collect();
        let picked = config.picked.clone().filter(|r| !r.starts_with('-') && !r.chars().any(char::is_whitespace));
        let config = Config { repo: Some(repo), ..config };
        let restoring = ctx.restoring;
        let d = Arc::new_cyclic(|me| Self {
            ctx,
            me: me.clone(),
            runner,
            config,
            open: Mutex::new(open),
            read: Mutex::new(Read::default()),
            state: Mutex::new(state),
            live: Live::default(),
            reading: tokio::sync::Mutex::new(()),
            whys: Mutex::new(HashMap::new()),
            asking: tokio::sync::Mutex::new(()),
            picked: Mutex::new(picked),
        });
        // Read once now, so `capture` and `describe` have it; brought back
        // after a restart, only once someone looks.
        if !restoring {
            let me = d.clone();
            d.ctx.rt.spawn(async move { me.load().await });
        }
        Ok(d)
    }

    /// Read the diff again; the state changes only if it did.
    async fn load(&self) {
        let _one = self.reading.lock().await;
        if self.live.closed() {
            return;
        }
        let runner = match &self.runner {
            Ok(r) => r.clone(),
            Err(e) => return self.failed(e.clone()),
        };
        let untracked = self.config.rev_b.is_none();
        let mut args = vec![
            self.config.repo.clone().unwrap_or_default(),
            if untracked { "1" } else { "0" }.to_owned(),
            OUT_MAX.to_string(),
            if self.config.base { "1" } else { "0" }.to_owned(),
            if self.config.chant.as_ref().is_some_and(|c| c.member.is_some()) { "1" } else { "0" }.to_owned(),
        ];
        // #619: a revision picked in place of the merge base.
        let picked = self.picked.lock().unwrap().clone().filter(|_| self.config.rev_a.is_none());
        args.extend(self.config.rev_a.iter().chain(picked.iter()).chain(self.config.rev_b.iter()).cloned());
        let run = match &self.config.run_as {
            // Through sudo, as the runner's user.
            Some(_) => crate::labs::git_as_runner(&script(), &args).await,
            None => runner.sh(&script(), &args).await,
        };
        let (out, _) = match run {
            Ok(o) => o,
            Err(e) => return self.failed(e),
        };
        let mut h = DefaultHasher::new();
        out.hash(&mut h);
        let hash = h.finish();
        let text = String::from_utf8_lossy(&out);
        let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
        let (head, base, top) = match first.split_once(' ') {
            Some(("ok", rest)) => {
                let mut w = rest.splitn(3, ' ');
                match (w.next(), w.next(), w.next()) {
                    (Some(h), Some(b), Some(top)) => (h.to_owned(), b.to_owned(), top.to_owned()),
                    _ => return self.failed("couldn't read the diff".into()),
                }
            }
            Some(("err", why)) => return self.failed(why.to_owned()),
            _ => return self.failed("couldn't read the diff".into()),
        };
        let same = {
            let mut read = self.read.lock().unwrap();
            let same = read.hash == hash && read.hash != 0;
            if !same {
                let (files, truncated) = parse(rest, out.len() >= OUT_MAX);
                *read = Read { hash, files, truncated, head };
            }
            same
        };
        {
            let mut st = self.state.lock().unwrap();
            st.loading = false;
            if same && st.error.is_none() {
                return;
            }
            st.error = None;
            if st.base {
                st.on_default = base.strip_prefix('=').filter(|_| picked.is_none()).map(str::to_owned);
                st.against = match &picked {
                    Some(p) => format!("the working tree against {p}"),
                    None => against_base(&base),
                };
                st.picked = picked;
            }
            st.name = std::path::Path::new(&top).file_name().map(|n| n.to_string_lossy().into_owned());
            st.repo = Some(top);
            st.updated_ms = now_ms();
        }
        self.redraw();
    }

    fn failed(&self, why: String) {
        {
            let mut st = self.state.lock().unwrap();
            if st.error.as_deref() == Some(why.as_str()) && !st.loading {
                return;
            }
            st.loading = false;
            st.error = Some(why);
            st.files.clear();
            (st.add, st.del) = (0, 0);
            st.updated_ms = now_ms();
        }
        *self.read.lock().unwrap() = Read::default();
        self.ctx.changed();
    }

    /// The state from what was read and which files are open.
    fn redraw(&self) {
        let open = self.open.lock().unwrap().clone();
        let (files, add, del, truncated) = {
            let read = self.read.lock().unwrap();
            let whys = self.whys.lock().unwrap();
            let files: Vec<Value> = read
                .files
                .iter()
                .map(|f| {
                    let mut v = serde_json::to_value(f).unwrap_or_default();
                    if open.contains(&f.path) {
                        v["open"] = true.into();
                        let hs = hunks(&f.text);
                        v["hunks"] = serde_json::to_value(&hs).unwrap_or_default();
                        if self.config.chant.is_some() && !hs.is_empty() {
                            // The last answer, until the next lands: a hunk takes
                            // the blamed lines it falls in.
                            match whys.get(&f.path).and_then(|w| w.read.as_ref()) {
                                Some(Ok(why)) => {
                                    for (i, h) in hs.iter().enumerate() {
                                        let added: Vec<u32> =
                                            h.lines.iter().filter(|l| l.0 == '+').map(|l| l.2).collect();
                                        if let Some(hw) = why.hunk_json(&added) {
                                            v["hunks"][i]["why"] = hw;
                                        }
                                    }
                                }
                                Some(Err(e)) => v["why"] = json!({ "error": e }),
                                None => v["why"] = json!({ "reading": true }),
                            }
                        }
                    }
                    v
                })
                .collect();
            let add = read.files.iter().map(|f| f.add).sum();
            let del = read.files.iter().map(|f| f.del).sum();
            (files, add, del, read.truncated)
        };
        {
            let mut st = self.state.lock().unwrap();
            (st.files, st.add, st.del, st.truncated) = (files, add, del, truncated);
        }
        self.ctx.changed();
        self.ask_why();
    }

    /// #619: read the why of each open file whose added lines or `HEAD`
    /// moved since its last read, one chant read at a time, each file at
    /// most every [`WHY_GAP`], backing off while its lines keep moving.
    /// Nothing once the block is closed.
    fn ask_why(&self) {
        if self.config.chant.is_none() || self.live.closed() {
            return;
        }
        let want: Vec<String> = {
            let open = self.open.lock().unwrap();
            let read = self.read.lock().unwrap();
            let mut whys = self.whys.lock().unwrap();
            whys.retain(|p, _| open.contains(p));
            read.files
                .iter()
                .filter(|f| open.contains(&f.path) && !f.big && !f.binary && !f.text.is_empty())
                .filter(|f| {
                    let w = whys.entry(f.path.clone()).or_default();
                    let go = !w.reading && (w.read.is_none() || w.key != why_key(&f.text, &read.head));
                    w.reading |= go;
                    go
                })
                .map(|f| f.path.clone())
                .collect()
        };
        for path in want {
            let Some(me) = self.me.upgrade() else { return };
            self.ctx.rt.spawn(async move {
                let _one = me.asking.lock().await;
                let (last, gap) = me.whys.lock().unwrap().get(&path).map(|w| (w.last, w.gap)).unwrap_or_default();
                let gap = gap.unwrap_or(WHY_GAP);
                if let Some(wait) = last.and_then(|l| gap.checked_sub(l.elapsed())) {
                    tokio::time::sleep(wait).await;
                }
                if me.live.closed() {
                    return;
                }
                // Read for the lines as they are now.
                let key = |me: &Diff| {
                    let read = me.read.lock().unwrap();
                    read.files.iter().find(|f| f.path == path).map(|f| why_key(&f.text, &read.head))
                };
                let Some(asked) = key(&me) else {
                    if let Some(w) = me.whys.lock().unwrap().get_mut(&path) {
                        w.reading = false;
                    }
                    return;
                };
                let got = me.why(&path).await;
                let moved = key(&me) != Some(asked);
                if let Some(w) = me.whys.lock().unwrap().get_mut(&path) {
                    (w.reading, w.last, w.key, w.read) = (false, Some(Instant::now()), asked, Some(got));
                    w.gap = Some(if moved { (gap * 2).min(WHY_GAP_MAX) } else { WHY_GAP });
                }
                me.redraw();
            });
        }
    }

    /// One file's why from the workspace's chant (a Labs feature).
    async fn why(&self, path: &str) -> Result<FileWhy, String> {
        let c = self.config.chant.clone().ok_or("not from a workspace")?;
        let top = self.state.lock().unwrap().repo.clone().ok_or("no repository yet")?;
        let file = format!("{}/{path}", top.trim_end_matches('/'));
        crate::labs::file_why(&self.ctx, &c.root, c.chant.as_deref(), &file, self.config.rev_b.as_deref()).await
    }

    /// Open or close a file's hunks.
    fn file(&self, path: &str, open: Option<bool>) -> Result<bool, String> {
        if !self.read.lock().unwrap().files.iter().any(|f| f.path == path) {
            return Err(format!("{path} isn't in this diff"));
        }
        let now = {
            let mut o = self.open.lock().unwrap();
            let was = o.iter().any(|p| p == path);
            let now = open.unwrap_or(!was);
            o.retain(|p| p != path);
            if now {
                o.push(path.to_owned());
                while o.len() > OPEN_MAX {
                    o.remove(0);
                }
            }
            now
        };
        self.redraw();
        Ok(now)
    }

    /// Poll while drawn.
    fn watch(&self, round: u64) {
        let Some(me) = self.me.upgrade() else { return };
        // As `fountain` (M45b) every read is a sudo, which sudo logs: less often.
        let every = match self.config.run_as {
            Some(_) => SUDO_POLL,
            None => super::every(self.runner.as_ref().is_ok_and(Runner::local), POLL),
        };
        self.ctx.rt.spawn(async move {
            while me.live.on(round) {
                me.load().await;
                tokio::time::sleep(every).await;
            }
        });
    }
}

impl Block for Diff {
    fn kind(&self) -> BlockType {
        BlockType::Diff
    }

    fn config(&self) -> Value {
        let mut v = json!({
            "repo": self.config.repo,
            "rev_a": self.config.rev_a,
            "rev_b": self.config.rev_b,
            "open": *self.open.lock().unwrap(),
        });
        if let Some(u) = &self.config.run_as {
            v["run_as"] = json!(u);
        }
        if let Some(c) = &self.config.chant {
            v["chant"] = json!(c);
        }
        if let Some(p) = &*self.picked.lock().unwrap() {
            v["picked"] = json!(p);
        }
        if self.config.base {
            v["base"] = json!(true);
        }
        v
    }

    fn state(&self) -> Value {
        serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default()
    }

    fn text(&self) -> String {
        let read = self.read.lock().unwrap();
        let mut out = String::new();
        for f in &read.files {
            if f.big {
                out.push_str(&format!("diff --git a/{p} b/{p}\n(too big to show here: open the file)\n", p = f.path));
            } else {
                out.push_str(&f.text);
            }
        }
        if read.truncated {
            out.push_str("(the diff was cut short)\n");
        }
        out
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let me = self.me.upgrade();
        match method {
            "refresh" => Box::pin(async move {
                let me = me.ok_or("closed")?;
                me.load().await;
                let st = me.state.lock().unwrap();
                match &st.error {
                    Some(e) => Err(e.clone()),
                    None => Ok(json!({ "files": st.files.len(), "add": st.add, "del": st.del })),
                }
            }),
            "file" => {
                let r = match args["path"].as_str() {
                    Some(p) => self.file(p, args["open"].as_bool()).map(|open| json!({ "open": open })),
                    None => Err("file needs {\"path\": …}".into()),
                };
                Box::pin(async move { r })
            }
            // #619: another base than the merge base (`rev`), or back to
            // it (none).
            "base" => {
                let rev = args["rev"].as_str().map(str::trim).filter(|r| !r.is_empty()).map(str::to_owned);
                let ok = match (&rev, self.state.lock().unwrap().base) {
                    (_, false) => Err("this diff wasn't opened against a merge base".to_owned()),
                    (Some(r), _) if r.starts_with('-') || r.chars().any(|c| c.is_whitespace() || c.is_control()) => {
                        Err(format!("not a revision: {r:?}"))
                    }
                    _ => Ok(()),
                };
                if ok.is_ok() {
                    *self.picked.lock().unwrap() = rev.clone();
                    self.read.lock().unwrap().hash = 0;
                    self.whys.lock().unwrap().clear();
                }
                Box::pin(async move {
                    ok?;
                    let me = me.ok_or("closed")?;
                    me.load().await;
                    let st = me.state.lock().unwrap();
                    match &st.error {
                        Some(e) => Err(e.clone()),
                        None => Ok(json!({ "against": st.against })),
                    }
                })
            }
            "state" => {
                let s = self.state();
                Box::pin(async move { Ok(s) })
            }
            m => {
                let e = no_method(BlockType::Diff, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn drawn(&self, on: bool) {
        if let Some(round) = self.live.set(on) {
            self.watch(round);
        }
        let mut st = self.state.lock().unwrap();
        if st.watching != self.live.drawn() {
            st.watching = self.live.drawn();
            drop(st);
            self.ctx.changed();
        }
    }

    fn close(&self) {
        self.live.close();
    }

    fn summary(&self) -> Summary {
        let st = self.state.lock().unwrap();
        let local = self.runner.as_ref().is_ok_and(Runner::local);
        // A sandbox's directory isn't one of yours to start things in.
        let cwd = match self.config.run_as {
            Some(_) => None,
            None => st.repo.clone().or_else(|| self.config.repo.clone()),
        };
        let who = self.config.run_as.as_deref().map(|u| format!(" (as {u})")).unwrap_or_default();
        Summary {
            project: cwd.as_deref().and_then(|c| super::project(c, local && self.config.run_as.is_none())),
            title: Some(format!("Changes in {}{who}", st.name.as_deref().unwrap_or("…"))),
            cwd,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------- parsing

/// The script's output after its first line: each file, in order, with its
/// section of the diff. `cut`: the output hit its cap.
pub fn parse(out: &str, cut: bool) -> (Vec<FileDiff>, bool) {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut truncated = cut;
    let mut untracked = false;
    let mut cur: Option<FileDiff> = None;
    let mut in_hunk = false;
    let finish = |f: Option<FileDiff>, files: &mut Vec<FileDiff>| {
        if let Some(mut f) = f {
            if f.text.len() > FILE_MAX {
                f.big = true;
                f.text.clear();
            }
            files.push(f);
        }
    };
    for line in out.split_inclusive('\n') {
        let l = line.trim_end_matches('\n');
        if l == "arugula-untracked" {
            finish(cur.take(), &mut files);
            untracked = true;
            continue;
        }
        if l == "arugula-more" {
            finish(cur.take(), &mut files);
            truncated = true;
            continue;
        }
        if let Some(p) = l.strip_prefix("arugula-big ") {
            finish(cur.take(), &mut files);
            files.push(FileDiff { path: p.to_owned(), status: "untracked", big: true, ..Default::default() });
            continue;
        }
        if let Some(rest) = l.strip_prefix("diff --git ") {
            finish(cur.take(), &mut files);
            in_hunk = false;
            let path = git_line_path(rest).unwrap_or_default();
            cur = Some(FileDiff {
                path,
                status: if untracked { "untracked" } else { "modified" },
                text: line.to_owned(),
                ..Default::default()
            });
            continue;
        }
        let Some(f) = cur.as_mut() else { continue };
        if f.text.len() <= FILE_MAX {
            f.text.push_str(line);
        }
        if in_hunk {
            match l.as_bytes().first() {
                Some(b'+') => f.add += 1,
                Some(b'-') => f.del += 1,
                Some(b' ' | b'\\') => {}
                Some(b'@') if l.starts_with("@@") => {}
                _ => in_hunk = false,
            }
            if in_hunk {
                continue;
            }
        }
        if l.starts_with("@@") {
            in_hunk = true;
        } else if let Some(p) = l.strip_prefix("rename from ") {
            f.old = Some(unquote(p));
            f.status = "renamed";
        } else if let Some(p) = l.strip_prefix("rename to ") {
            f.path = unquote(p);
            f.status = "renamed";
        } else if l.starts_with("new file mode") && !untracked {
            f.status = "added";
        } else if l.starts_with("deleted file mode") {
            f.status = "deleted";
        } else if l.starts_with("old mode") && f.status == "modified" {
            f.status = "mode";
        } else if let Some(p) = l.strip_prefix("+++ ") {
            if p != "/dev/null" {
                f.path = unquote(p).strip_prefix("b/").map(str::to_owned).unwrap_or_else(|| unquote(p));
            }
        } else if let Some(p) = l.strip_prefix("--- ") {
            if p != "/dev/null" && f.status == "deleted" {
                f.path = unquote(p).strip_prefix("a/").map(str::to_owned).unwrap_or_else(|| unquote(p));
            }
        } else if l.starts_with("Binary files ") || l == "GIT binary patch" {
            f.binary = true;
        }
    }
    finish(cur.take(), &mut files);
    // A mode change that came with content changes is a modification.
    for f in &mut files {
        if f.status == "mode" && (f.add > 0 || f.del > 0) {
            f.status = "modified";
        }
    }
    if files.len() > FILES_MAX {
        files.truncate(FILES_MAX);
        truncated = true;
    }
    (files, truncated)
}

/// A file's hunks, from its section of the diff.
pub fn hunks(text: &str) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    for l in text.lines() {
        if l.starts_with("@@") {
            let (o, n) = hunk_starts(l).unwrap_or((1, 1));
            (old, new) = (o, n);
            out.push(Hunk { at: l.to_owned(), lines: Vec::new() });
            continue;
        }
        let Some(h) = out.last_mut() else { continue };
        let Some(kind) = l.chars().next() else { continue };
        let body = l[kind.len_utf8()..].to_owned();
        match kind {
            ' ' => {
                h.lines.push((' ', old, new, body));
                old += 1;
                new += 1;
            }
            '+' => {
                h.lines.push(('+', old, new, body));
                new += 1;
            }
            '-' => {
                h.lines.push(('-', old, new, body));
                old += 1;
            }
            '\\' => h.lines.push(('\\', old, new, body)),
            _ => {}
        }
    }
    out
}

/// `@@ -12,7 +12,8 @@`: where the hunk starts on each side (a side with
/// no lines starts at the line before; it's shown at the next one).
fn hunk_starts(l: &str) -> Option<(u32, u32)> {
    let mut parts = l.split_whitespace().skip(1);
    let side = |s: &str| -> Option<u32> {
        let (start, len) = s[1..].split_once(',').map_or((&s[1..], "1"), |(a, b)| (a, b));
        let start: u32 = start.parse().ok()?;
        Some(if len == "0" { start + 1 } else { start })
    };
    Some((side(parts.next()?)?, side(parts.next()?)?))
}

/// The path in `a/P b/P` (both the same unless renamed, and then the
/// rename lines say), quoted or not.
fn git_line_path(rest: &str) -> Option<String> {
    if rest.starts_with('"') {
        // "a/x" "b/x": the second quoted name.
        let second = rest.rfind(" \"")?;
        let p = unquote(&rest[second + 1..]);
        return Some(p.strip_prefix("b/").unwrap_or(&p).to_owned());
    }
    // Unquoted: `a/` + P + ` b/` + P.
    let n = rest.len();
    if n >= 5 && (n - 1).is_multiple_of(2) {
        let half = (n - 1) / 2;
        if rest.is_char_boundary(half) && rest.is_char_boundary(half + 1) {
            let (a, b) = (&rest[..half], &rest[half + 1..]);
            if let (Some(a), Some(b)) = (a.strip_prefix("a/"), b.strip_prefix("b/"))
                && a == b
            {
                return Some(b.to_owned());
            }
        }
    }
    rest.rsplit_once(" b/").map(|(_, p)| p.to_owned())
}

/// A path as git wrote it: C-quoted if it has odd characters.
fn unquote(p: &str) -> String {
    let p = p.trim_end_matches('\t');
    let Some(inner) = p.strip_prefix('"').and_then(|x| x.strip_suffix('"')) else { return p.to_owned() };
    let mut bytes = Vec::new();
    let mut it = inner.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match it.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(b'"') => bytes.push(b'"'),
            Some(b'\\') => bytes.push(b'\\'),
            Some(d @ b'0'..=b'7') => {
                let mut v = (d - b'0') as u32;
                for _ in 0..2 {
                    if let Some(&n @ b'0'..=b'7') = it.peek() {
                        v = v * 8 + (n - b'0') as u32;
                        it.next();
                    }
                }
                bytes.push(v as u8);
            }
            Some(o) => bytes.push(o),
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "diff --git a/a.txt b/a.txt
index de98044..a7bc997 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,4 @@
 a
-b
+B
 c
+d
diff --git a/bin.dat b/bin.dat
index 8352675..a903574 100644
Binary files a/bin.dat and b/bin.dat differ
diff --git a/del.txt b/del.txt
deleted file mode 100644
index c1b0730..0000000
--- a/del.txt
+++ /dev/null
@@ -1 +0,0 @@
-x
\\ No newline at end of file
diff --git a/mv.txt b/moved.txt
similarity index 97%
rename from mv.txt
rename to moved.txt
index 96cc558..1c5a36f 100644
--- a/mv.txt
+++ b/moved.txt
@@ -48,3 +48,4 @@
 48
 49
 50
+51
diff --git a/sp ace.txt b/sp ace.txt
old mode 100644
new mode 100755
diff --git a/staged.txt b/staged.txt
new file mode 100644
index 0000000..3e75765
--- /dev/null
+++ b/staged.txt
@@ -0,0 +1 @@
+new
arugula-untracked
diff --git a/untracked.txt b/untracked.txt
new file mode 100644
index 0000000..2f0fda4
--- /dev/null
+++ b/untracked.txt
@@ -0,0 +1,2 @@
+u1
+u2
\\ No newline at end of file
arugula-big huge.log
";

    #[test]
    fn reads_what_git_says() {
        let (files, cut) = parse(OUT, false);
        assert!(!cut);
        let got: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.status, f.add, f.del, f.binary, f.big)).collect();
        assert_eq!(
            got,
            [
                ("a.txt", "modified", 2, 1, false, false),
                ("bin.dat", "modified", 0, 0, true, false),
                ("del.txt", "deleted", 0, 1, false, false),
                ("moved.txt", "renamed", 1, 0, false, false),
                ("sp ace.txt", "mode", 0, 0, false, false),
                ("staged.txt", "added", 1, 0, false, false),
                ("untracked.txt", "untracked", 2, 0, false, false),
                ("huge.log", "untracked", 0, 0, false, true),
            ]
        );
        assert_eq!(files[3].old.as_deref(), Some("mv.txt"));
        assert!(files[0].text.starts_with("diff --git a/a.txt") && files[0].text.ends_with("+d\n"));
    }

    #[test]
    fn hunks_number_both_sides() {
        let (files, _) = parse(OUT, false);
        let h = hunks(&files[0].text);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].at, "@@ -1,3 +1,4 @@");
        let l: Vec<_> = h[0].lines.iter().map(|(k, o, n, t)| (*k, *o, *n, t.as_str())).collect();
        assert_eq!(l, [(' ', 1, 1, "a"), ('-', 2, 2, "b"), ('+', 3, 2, "B"), (' ', 3, 3, "c"), ('+', 4, 4, "d")]);
        // A new file starts at line 1; a deleted one's lines land at 1.
        assert_eq!(hunks(&files[5].text)[0].lines[0], ('+', 1, 1, "new".into()));
        assert_eq!(hunks(&files[2].text)[0].lines[0], ('-', 1, 1, "x".into()));
        assert_eq!(hunks(&files[3].text)[0].lines[3], ('+', 51, 51, "51".into()));
    }

    #[test]
    fn caps_and_odd_names() {
        let big =
            format!("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1,{n} @@\n{}", "+y\n".repeat(100_000), n = 100_000);
        let (files, _) = parse(&big, false);
        assert!(files[0].big && files[0].text.is_empty());
        assert_eq!(files[0].add, 100_000);
        let (_, cut) = parse("arugula-more\n", false);
        assert!(cut);
        assert_eq!(unquote(r#""a\"b\303\274.txt""#), "a\"bü.txt");
        assert_eq!(git_line_path(r#""a/x\"y" "b/x\"y""#).as_deref(), Some("x\"y"));
        assert_eq!(git_line_path("a/a b/c b/a b/c").as_deref(), Some("a b/c"));
        assert_eq!(hunk_starts("@@ -0,0 +1 @@"), Some((1, 1)));
        assert_eq!(hunk_starts("@@ -5,2 +4,0 @@ fn"), Some((5, 5)));
    }
}
