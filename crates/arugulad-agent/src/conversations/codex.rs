//! Codex's sessions on this machine, listed beside Claude Code's: its
//! rollout files, `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<when>-<id>.jsonl`
//! (`~/.codex` without it), which Codex compresses to `.jsonl.zst` once
//! they've been idle a while. Read only: nothing here writes under
//! `$CODEX_HOME`.
//!
//! - **A rollout** is one JSON line per item: `{timestamp, type, payload}`.
//!   `session_meta` comes first (the thread's id, `cwd`, `originator`,
//!   `cli_version`, `source`, `git`), then `turn_context` per turn (its
//!   `model`), `response_item`s (what the model saw) and `event_msg`s (what
//!   happened). What someone typed is an `item_completed` event whose item
//!   is a `UserMessage` (0.16x), a `user_message` event before that, and in
//!   the oldest rollouts, which have no `type` on the meta line, only the
//!   user's `message` items.
//! - **A name** given with `/rename` is in `session_index.jsonl`, appended
//!   to: the last line for an id wins.
//! - **Liveness** (Linux): the `codex` process that has a thread holds its
//!   rollout and `thread-writer-locks/<id>.lock` open, so `/proc/*/fd` says
//!   who. Elsewhere it isn't knowable without taking that lock, which we
//!   don't.
//!
//! Continue here runs `codex resume <id>` in a terminal pane in its folder
//! ([`arugula_mux::resume::command`], as a restart of a pane that ran it
//! does). Opening one as a transcript is for later.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde_json::Value;

use super::{Conversation, Live, Ours, Source, convert, short};

/// How much of a plain rollout's start is read: its meta line carries
/// Codex's base instructions (22 KiB in 0.163), so more than a Claude Code
/// transcript's.
const HEAD: u64 = 256 * 1024;
/// And of its end, for the last prompt and model.
const TAIL: u64 = 64 * 1024;

/// `$CODEX_HOME`, else `~/.codex`.
pub fn home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::home().join(".codex"))
}

/// Who started a thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Kind {
    /// The TUI or an editor: someone at its prompt.
    #[default]
    Interactive,
    /// `codex exec` or Codex as an MCP server: on the list with everything.
    Run,
    /// A thread an agent spawned: never listed, as a Claude Code subagent's
    /// transcript isn't.
    Sub,
}

/// What a rollout says.
#[derive(Debug, Clone, Default)]
pub(super) struct Rollout {
    id: String,
    cwd: Option<String>,
    branch: Option<String>,
    originator: Option<String>,
    version: Option<String>,
    created_ms: u64,
    model: Option<String>,
    forked_from: Option<String>,
    kind: Kind,
    /// From the events, which say what was typed.
    first_prompt: Option<String>,
    last_prompt: Option<String>,
    /// From the model's input, for rollouts whose events don't say.
    first_item: Option<String>,
    last_item: Option<String>,
    /// `<environment_context>`'s `<cwd>`, for one with no meta line.
    env_cwd: Option<String>,
}

impl Rollout {
    fn take(&mut self, v: &Value) {
        let p = &v["payload"];
        let s = |v: &Value, k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        match v["type"].as_str() {
            Some("session_meta") => self.meta(p),
            Some("turn_context") => {
                if let Some(m) = s(p, "model") {
                    self.model = Some(m);
                }
                if self.env_cwd.is_none() {
                    self.env_cwd = s(p, "cwd");
                }
            }
            Some("event_msg") => {
                let text = match p["type"].as_str() {
                    Some("user_message") => s(p, "message"),
                    Some("item_completed") if p["item"]["type"] == "UserMessage" => texts(&p["item"]["content"]),
                    _ => None,
                };
                if let Some(t) = text.as_deref().and_then(request) {
                    self.first_prompt.get_or_insert_with(|| t.clone());
                    self.last_prompt = Some(t);
                }
            }
            Some("response_item") => self.item(p),
            // The oldest rollouts: the meta line has no `type`, and the
            // items are bare.
            Some("message") => self.item(v),
            None if v["id"].is_string() && v["timestamp"].is_string() => self.meta(v),
            _ => {}
        }
    }

    fn meta(&mut self, m: &Value) {
        if !self.id.is_empty() {
            return;
        }
        let s = |k: &str| m[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        self.id = s("id").unwrap_or_default();
        self.cwd = s("cwd");
        self.branch = m["git"]["branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned);
        self.originator = s("originator");
        self.version = s("cli_version");
        self.created_ms = convert::at_ms(m["timestamp"].as_str().unwrap_or(""));
        self.forked_from = s("forked_from_id");
        self.kind = match &m["source"] {
            Value::String(src) if src == "exec" || src == "mcp" => Kind::Run,
            Value::Object(o) if o.contains_key("subagent") => Kind::Sub,
            _ if m["parent_thread_id"].is_string() || m["thread_source"] == "subagent" => Kind::Sub,
            _ => Kind::Interactive,
        };
    }

    /// A `message` the model was given: the user's words, or the context
    /// Codex puts in a user message (`<environment_context>`, AGENTS.md).
    fn item(&mut self, p: &Value) {
        if p["type"] != "message" || p["role"] != "user" {
            return;
        }
        let Some(t) = texts(&p["content"]) else { return };
        if self.env_cwd.is_none()
            && let Some(cwd) = between(&t, "<cwd>", "</cwd>")
        {
            self.env_cwd = Some(cwd.to_owned());
        }
        if let Some(t) = request(&t) {
            self.first_item.get_or_insert_with(|| t.clone());
            self.last_item = Some(t);
        }
    }

    fn prompts(&self) -> (Option<&str>, Option<&str>) {
        match self.first_prompt {
            Some(_) => (self.first_prompt.as_deref(), self.last_prompt.as_deref()),
            None => (self.first_item.as_deref(), self.last_item.as_deref()),
        }
    }
}

/// A content array's text parts (`input_text`, `text`), joined.
fn texts(content: &Value) -> Option<String> {
    let t = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|c| matches!(c["type"].as_str(), Some("input_text" | "text")))
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    (!t.trim().is_empty()).then_some(t)
}

/// What someone asked, from a user message: not the context Codex adds
/// (a tag, AGENTS.md's instructions), and from an editor only what follows
/// its context.
fn request(t: &str) -> Option<String> {
    let t = t.rsplit_once("## My request for Codex:").map_or(t, |(_, r)| r).trim();
    (!t.is_empty() && !t.starts_with('<') && !t.starts_with("# AGENTS.md instructions")).then(|| t.to_owned())
}

fn between<'a>(t: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = t.find(open)? + open.len();
    let end = t[start..].find(close)? + start;
    Some(t[start..end].trim()).filter(|s| !s.is_empty())
}

/// Read a rollout: a plain one's two ends, a compressed one whole (they're
/// idle, and read again only when they change).
pub(super) fn read(path: &Path) -> Option<Rollout> {
    let mut r = Rollout::default();
    if path.extension().is_some_and(|e| e == "zst") {
        let f = std::fs::File::open(path).ok()?;
        let lines = BufReader::new(zstd::stream::read::Decoder::new(f).ok()?).lines();
        for line in lines.map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                r.take(&v);
            }
        }
    } else {
        for v in super::edges(path, HEAD, TAIL)? {
            r.take(&v);
        }
    }
    if r.id.is_empty() {
        r.id = id_of(path)?;
    }
    Some(r)
}

/// The thread's id in a rollout's name: `rollout-<when>-<id>.jsonl`, or
/// `…-<id>_<rollout>.jsonl` for a reverted thread's newer rollout.
fn id_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let core = name.strip_suffix(".zst").unwrap_or(name).strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let ids = core.get(20..).filter(|_| core.get(19..20) == Some("-"))?;
    Some(ids.split_once('_').map_or(ids, |(t, _)| t).to_owned()).filter(|id| !id.is_empty())
}

/// Every rollout under `sessions/` (`YYYY/MM/DD`), with its size and mtime.
fn rollouts(dir: &Path, depth: u8, out: &mut Vec<(PathBuf, u64, u64)>) {
    for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = f.path();
        let Ok(md) = f.metadata() else { continue };
        if md.is_dir() {
            if depth < 3 {
                rollouts(&path, depth + 1, out);
            }
            continue;
        }
        let name = f.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("rollout-") || !(name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")) {
            continue;
        }
        let mtime =
            md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
        out.push((path, md.len(), mtime));
    }
}

/// The names given with `/rename`, by thread id: the last line for each.
fn names(home: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(home.join("session_index.jsonl")) else { return HashMap::new() };
    let mut out = HashMap::new();
    for v in text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if let (Some(id), Some(name)) = (v["id"].as_str(), v["thread_name"].as_str()) {
            out.insert(id.to_owned(), name.trim().to_owned());
        }
    }
    out.retain(|_, n| !n.is_empty());
    out
}

/// Codex's conversations, by the cache of rollouts read (by path: its size,
/// mtime and what it said), which this keeps to the files there now.
pub(super) fn scan(home: &Path, cache: &mut HashMap<PathBuf, (u64, u64, Rollout)>, ours: &Ours) -> Vec<Conversation> {
    let mut files = vec![];
    rollouts(&home.join("sessions"), 0, &mut files);
    cache.retain(|p, _| files.iter().any(|(f, ..)| f == p));
    let names = names(home);
    let live = live(home, ours);
    // A thread can have more than one rollout (a revert starts another):
    // its newest stands for it.
    let mut by_id: HashMap<String, Conversation> = HashMap::new();
    for (path, size, mtime) in files {
        if !matches!(cache.get(&path), Some((s, m, _)) if *s == size && *m == mtime) {
            let Some(r) = read(&path) else { continue };
            cache.insert(path.clone(), (size, mtime, r));
        }
        let (_, _, r) = &cache[&path];
        let (first, last) = r.prompts();
        if r.kind == Kind::Sub || (first.is_none() && last.is_none()) {
            continue;
        }
        if by_id.get(&r.id).is_some_and(|c| c.updated_ms >= mtime) {
            continue;
        }
        let cwd = r.cwd.clone().or(r.env_cwd.clone()).unwrap_or_default();
        let title = names
            .get(&r.id)
            .cloned()
            .or(first.map(|p| short(p, 80)))
            .or(last.map(|p| short(p, 80)))
            .unwrap_or_else(|| r.id.clone());
        let c = Conversation {
            id: r.id.clone(),
            agent: "codex",
            path: path.clone(),
            cwd_exists: !cwd.is_empty() && Path::new(&cwd).is_dir(),
            cwd,
            branch: r.branch.clone().filter(|b| b != "HEAD"),
            source: if r.kind == Kind::Run { Source::Other } else { Source::Codex },
            entrypoint: r.originator.clone().unwrap_or_default(),
            title,
            first_prompt: first.map(|p| short(p, 400)),
            last_prompt: last.map(|p| short(p, 400)),
            model: r.model.clone(),
            version: r.version.clone().unwrap_or_default(),
            created_ms: if r.created_ms > 0 { r.created_ms } else { mtime },
            updated_ms: mtime,
            size,
            forked_from: r.forked_from.clone(),
            continued_in: None,
            archived: false,
            live: live.get(&r.id).cloned(),
            estimate: None,
            context: None,
        };
        by_id.insert(r.id.clone(), c);
    }
    by_id.into_values().collect()
}

/// The running Codex processes, by thread: whoever holds its writer lock or
/// its rollout open under `home`.
#[cfg(target_os = "linux")]
fn live(home: &Path, ours: &Ours) -> HashMap<String, Live> {
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_owned());
    let mut out = HashMap::new();
    for p in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = p.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        // The native binary; npm's `codex` is node, which runs it.
        if !crate::procinfo::comm(pid).is_some_and(|c| c.starts_with("codex")) {
            continue;
        }
        for fd in std::fs::read_dir(p.path().join("fd")).into_iter().flatten().flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else { continue };
            if let Some(id) = held(&home, &target) {
                out.entry(id).or_insert_with(|| holder(pid, ours));
            }
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn live(_: &Path, _: &Ours) -> HashMap<String, Live> {
    HashMap::new()
}

/// The thread an open file under `home` belongs to: its writer lock, or
/// one of its rollouts.
fn held(home: &Path, file: &Path) -> Option<String> {
    let rest = file.strip_prefix(home).ok()?;
    let name = rest.file_name()?.to_str()?;
    match rest.parent()?.to_str()? {
        "thread-writer-locks" => {
            name.strip_suffix(".lock").filter(|id| !id.is_empty() && !id.starts_with('.')).map(str::to_owned)
        }
        d if d.starts_with("sessions") => id_of(rest),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn holder(pid: u32, ours: &Ours) -> Live {
    let (pane, block) = ours.holder(pid).unwrap_or_else(|| super::scope_of(pid));
    let mut l = Live {
        pid,
        kind: "interactive".into(),
        entrypoint: "codex".into(),
        status: String::new(),
        pane,
        block,
        place: String::new(),
        cwd: crate::procinfo::cwd(pid).map(|d| d.display().to_string()),
        started: 0,
    };
    l.place = l.place();
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const REAL: &str = "rollout-2026-11-23T05-08-16-01a206ce-0c41-7ce0-a62f-517a7a089b86.jsonl";
    const ID: &str = "01a206ce-0c41-7ce0-a62f-517a7a089b86";

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(format!("{}/tests/fixtures/conversations/codex/{name}", env!("CARGO_MANIFEST_DIR")))
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "arugula-codex-{name}-{}-{}",
            std::process::id(),
            crate::store::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A Codex home with the real rollout in it, its folder `work` and its
    /// source `source` (the fixture is `codex exec`'s).
    fn home_with(root: &Path, work: &Path, source: &str) -> PathBuf {
        let home = root.join("codex");
        let day = home.join("sessions/2026/11/23");
        std::fs::create_dir_all(&day).unwrap();
        let quoted = serde_json::to_string(work.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(fixture(REAL))
            .unwrap()
            .replace("/home/user/codex-demo", &quoted[1..quoted.len() - 1])
            .replace("\"source\":\"exec\"", &format!("\"source\":{source}"));
        std::fs::write(day.join(REAL), text).unwrap();
        home
    }

    #[test]
    fn reads_a_real_rollout() {
        // Codex 0.163's `codex exec`, then `codex exec resume` with a second
        // prompt; scrubbed.
        let r = read(&fixture(REAL)).unwrap();
        assert_eq!(r.id, ID);
        assert_eq!(r.cwd.as_deref(), Some("/home/user/codex-demo"));
        assert_eq!(r.branch.as_deref(), Some("fixture-branch"));
        assert_eq!((r.originator.as_deref(), r.version.as_deref()), (Some("codex_exec"), Some("0.163.0")));
        assert_eq!(r.kind, Kind::Run);
        assert_eq!(r.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(r.created_ms, convert::at_ms("2026-11-23T05:08:16.586Z"));
        assert_eq!(r.prompts(), (Some("Add a README that says what demo is"), Some("Now add a LICENSE too")));
        // The model's input says the same, past the context Codex adds.
        assert_eq!(r.first_item.as_deref(), Some("Add a README that says what demo is"));
    }

    #[test]
    fn reads_the_older_shapes() {
        let root = tmp("older");
        // Before `item_completed`: `user_message` events, and an editor's
        // context before the request.
        let mid = root.join("rollout-2025-09-01T10-00-00-0199aaaa-0000-7000-8000-000000000001.jsonl");
        let lines = [
            json!({"timestamp":"2025-09-01T10:00:00.000Z","type":"session_meta","payload":{"id":"0199aaaa-0000-7000-8000-000000000001","timestamp":"2025-09-01T10:00:00.000Z","cwd":"/w","originator":"codex_vscode","cli_version":"0.30.0","source":"vscode","git":{"branch":"main"}}}),
            json!({"timestamp":"2025-09-01T10:00:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"# Context from my IDE setup:\n## Open tabs: a.rs\n## My request for Codex:\nfix the build\n","kind":"plain"}}),
        ];
        std::fs::write(&mid, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        let r = read(&mid).unwrap();
        assert_eq!((r.kind, r.originator.as_deref()), (Kind::Interactive, Some("codex_vscode")));
        assert_eq!(r.prompts().0, Some("fix the build"));

        // The oldest: an untyped meta line, bare items, the folder only in
        // the environment's context.
        let old = root.join("rollout-2025-05-07T17-24-21-5973b6c0-94b8-487b-a530-2aeb6098ae0e.jsonl");
        let lines = [
            json!({"id":"5973b6c0-94b8-487b-a530-2aeb6098ae0e","timestamp":"2025-05-07T17:24:21.123Z","instructions":null,"git":{"branch":"old"}}),
            json!({"record_type":"state"}),
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n  <cwd>/old/repo</cwd>\n</environment_context>"}]}),
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"explain main.rs"}]}),
        ];
        std::fs::write(&old, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        let r = read(&old).unwrap();
        assert_eq!(r.id, "5973b6c0-94b8-487b-a530-2aeb6098ae0e");
        assert_eq!((r.cwd.as_deref(), r.env_cwd.as_deref()), (None, Some("/old/repo")));
        assert_eq!((r.kind, r.branch.as_deref()), (Kind::Interactive, Some("old")));
        assert_eq!(r.prompts(), (Some("explain main.rs"), Some("explain main.rs")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_compressed_rollout_reads_the_same() {
        let root = tmp("zst");
        let z = root.join(format!("{REAL}.zst"));
        let plain = std::fs::read(fixture(REAL)).unwrap();
        std::fs::write(&z, zstd::encode_all(&plain[..], 3).unwrap()).unwrap();
        let r = read(&z).unwrap();
        assert_eq!(r.id, ID);
        assert_eq!(r.prompts(), (Some("Add a README that says what demo is"), Some("Now add a LICENSE too")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ids_from_names_and_held_files() {
        let p = |s: &str| id_of(Path::new(s));
        assert_eq!(p(REAL).as_deref(), Some(ID));
        assert_eq!(p(&format!("{REAL}.zst")).as_deref(), Some(ID));
        assert_eq!(
            p(&format!("rollout-2026-11-23T05-08-16-{ID}_01a2ffff-0000-7000-8000-000000000000.jsonl")).as_deref(),
            Some(ID)
        );
        assert_eq!(p("rollout-short.jsonl"), None);
        let home = Path::new("/h/.codex");
        assert_eq!(held(home, &home.join(format!("thread-writer-locks/{ID}.lock"))).as_deref(), Some(ID));
        assert_eq!(held(home, &home.join("thread-writer-locks/.coordination.lock")), None);
        assert_eq!(held(home, &home.join(format!("sessions/2026/11/23/{REAL}"))).as_deref(), Some(ID));
        assert_eq!(held(home, &home.join("state_5.sqlite")), None);
        assert_eq!(held(home, Path::new("/elsewhere/sessions/x.jsonl")), None);
    }

    #[test]
    fn lists_with_folder_first_prompt_and_name() {
        let root = tmp("list");
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let home = home_with(&root, &work, "\"cli\"");
        // A subagent's thread isn't listed.
        let sub = json!({"timestamp":"2026-11-23T06:00:00Z","type":"session_meta","payload":{"id":"sub-1","timestamp":"2026-11-23T06:00:00Z","cwd":work,"originator":"codex_cli_rs","cli_version":"0.163.0","source":{"subagent":"review"}}});
        let typed = json!({"type":"event_msg","payload":{"type":"user_message","message":"review it"}});
        std::fs::write(
            home.join("sessions/2026/11/23/rollout-2026-11-23T06-00-00-sub-1.jsonl"),
            format!("{sub}\n{typed}\n"),
        )
        .unwrap();
        let mut cache = HashMap::new();
        let all = scan(&home, &mut cache, &Ours::default());
        assert_eq!(all.len(), 1, "{all:#?}");
        let c = &all[0];
        assert_eq!((c.id.as_str(), c.agent, c.source), (ID, "codex", Source::Codex));
        assert_eq!(c.cwd, work.display().to_string());
        assert!(c.cwd_exists && super::super::shown(c, false));
        assert_eq!(c.title, "Add a README that says what demo is");
        assert_eq!(c.last_prompt.as_deref(), Some("Now add a LICENSE too"));
        assert_eq!(c.branch.as_deref(), Some("fixture-branch"));
        assert_eq!(serde_json::to_value(c).unwrap()["source"], "codex");

        // A name from `/rename`, the newest line winning.
        let names = [
            json!({"id":ID,"thread_name":"Old","updated_at":"x"}),
            json!({"id":ID,"thread_name":"Docs for demo","updated_at":"y"}),
        ];
        std::fs::write(home.join("session_index.jsonl"), names.iter().map(|l| format!("{l}\n")).collect::<String>())
            .unwrap();
        assert_eq!(scan(&home, &mut cache, &Ours::default())[0].title, "Docs for demo");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exec_runs_are_listed_only_with_everything() {
        let root = tmp("exec");
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let home = home_with(&root, &work, "\"exec\"");
        let c = &scan(&home, &mut HashMap::new(), &Ours::default())[0];
        assert_eq!(c.source, Source::Other);
        assert!(!super::super::shown(c, false) && super::super::shown(c, true));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_holding_its_lock_is_live() {
        // A stand-in named codex, holding the thread's writer lock open as
        // Codex does.
        let root = tmp("live");
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let home = home_with(&root, &work, "\"cli\"");
        let locks = home.join("thread-writer-locks");
        std::fs::create_dir_all(&locks).unwrap();
        let lock = locks.join(format!("{ID}.lock"));
        std::fs::write(&lock, "").unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let bin = root.join("bin/codex");
        std::fs::copy("/bin/sleep", &bin).unwrap();
        let mut child = std::process::Command::new(&bin)
            .arg("30")
            .stdin(std::fs::File::open(&lock).unwrap())
            .current_dir(&work)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut ours = Ours::default();
        ours.panes.insert(pid, 7);
        let live = (0..50)
            .find_map(|_| {
                let c = scan(&home, &mut HashMap::new(), &ours).remove(0);
                c.live.or_else(|| {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    None
                })
            })
            .expect("live");
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!((live.pid, live.pane), (pid, Some(7)));
        assert_eq!(live.place, "open in pane %7");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
