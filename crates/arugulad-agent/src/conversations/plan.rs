//! A Claude Code session's own task list (#445): the steps it tracks with
//! its todo and task tools, read from its transcript, for the swarm's plan
//! board.
//!
//! Two tools write one. `TodoWrite` sends the whole list each time
//! (`todos: [{content, status, activeForm}]`), so the last one that
//! succeeded is the list. The task tools build it a step at a time:
//! `TaskCreate {subject, activeForm}` (its id is in the result:
//! `toolUseResult.task.id`, or "Task #N created"), then `TaskUpdate {taskId,
//! status, subject, activeForm, owner, addBlockedBy, addBlocks}`, where
//! `deleted` takes it off. A session uses one or the other; if both, the
//! one written last is the list.
//!
//! A call counts once its result says it worked: a refused or failed call
//! (`is_error`) changes nothing. Subagents' lines (`isSidechain`) are their
//! own work, not the session's list.
//!
//! [`Plans::line`] is fed parsed lines, as [`super::context::Context`] is;
//! [`PlanFile`] follows a transcript on its own for a terminal's Claude
//! Code, parsing only the lines that can matter, so a 50 MB transcript isn't
//! parsed whole.

use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use arugula_proto::{Plan, PlanStep, StepStatus};
use serde_json::Value;

/// At most this many steps go out, the most recent kept.
pub const MAX_STEPS: usize = 60;
/// A step's text is cut to this many characters.
const MAX_TEXT: usize = 200;

#[derive(Clone, Debug, Default)]
struct Step {
    id: String,
    text: String,
    active: Option<String>,
    status: StepStatus,
    blocked_by: Vec<String>,
    owner: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Plans {
    /// TodoWrite's last list, and when.
    todos: Vec<Step>,
    todos_at: Option<u64>,
    /// The task tools' steps, in the order they were made, and when one
    /// last changed.
    tasks: Vec<Step>,
    tasks_at: Option<u64>,
    /// Calls waiting for their result: tool_use id to (tool, input).
    calls: HashMap<String, (String, Value)>,
    model: Option<String>,
}

/// The tools that write a task list.
const TOOLS: [&str; 3] = ["TodoWrite", "TaskCreate", "TaskUpdate"];

impl Plans {
    pub fn line(&mut self, v: &Value) {
        if v["isSidechain"] == true {
            return;
        }
        let content = v["message"]["content"].as_array();
        match v["type"].as_str() {
            Some("assistant") => {
                if let Some(m) = v["message"]["model"].as_str().filter(|s| !s.is_empty() && !s.starts_with('<')) {
                    self.model = Some(m.to_owned());
                }
                for b in content.into_iter().flatten() {
                    if b["type"] == "tool_use"
                        && let (Some(id), Some(name)) = (b["id"].as_str(), b["name"].as_str())
                        && TOOLS.contains(&name)
                    {
                        self.calls.insert(id.to_owned(), (name.to_owned(), b["input"].clone()));
                    }
                }
            }
            Some("user") => {
                let at = super::convert::at_ms(v["timestamp"].as_str().unwrap_or(""));
                let results: Vec<&Value> =
                    content.into_iter().flatten().filter(|b| b["type"] == "tool_result").collect();
                // The line's `toolUseResult` is its one result's.
                let extra = if results.len() == 1 { &v["toolUseResult"] } else { &Value::Null };
                for r in results {
                    let Some((name, input)) = r["tool_use_id"].as_str().and_then(|id| self.calls.remove(id)) else {
                        continue;
                    };
                    if r["is_error"] == true {
                        continue;
                    }
                    self.apply(&name, &input, r, extra, at);
                }
            }
            _ => {}
        }
    }

    fn apply(&mut self, name: &str, input: &Value, result: &Value, extra: &Value, at: u64) {
        match name {
            "TodoWrite" => {
                let list = input["todos"].as_array().or(extra["newTodos"].as_array());
                self.todos = list
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        Some(Step {
                            id: (i + 1).to_string(),
                            text: text(t["content"].as_str()?)?,
                            active: t["activeForm"].as_str().and_then(text),
                            status: status(t["status"].as_str()?)?,
                            ..Default::default()
                        })
                    })
                    .collect();
                self.todos_at = Some(at);
            }
            "TaskCreate" => {
                let id = extra["task"]["id"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| extra["task"]["id"].as_u64().map(|n| n.to_string()))
                    .or_else(|| created_id(&result_text(result)));
                let (Some(id), Some(subject)) = (id, input["subject"].as_str().and_then(text)) else { return };
                self.tasks.retain(|t| t.id != id);
                self.tasks.push(Step {
                    id,
                    text: subject,
                    active: input["activeForm"].as_str().and_then(text),
                    status: StepStatus::Pending,
                    ..Default::default()
                });
                self.tasks_at = Some(at);
            }
            "TaskUpdate" => {
                let Some(id) =
                    input["taskId"].as_str().map(str::to_owned).or(input["taskId"].as_u64().map(|n| n.to_string()))
                else {
                    return;
                };
                if input["status"] == "deleted" {
                    self.tasks.retain(|t| t.id != id);
                    for t in &mut self.tasks {
                        t.blocked_by.retain(|b| *b != id);
                    }
                    self.tasks_at = Some(at);
                    return;
                }
                let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) else { return };
                if let Some(s) = input["status"].as_str().and_then(status) {
                    t.status = s;
                }
                if let Some(s) = input["subject"].as_str().and_then(text) {
                    t.text = s;
                }
                if let Some(s) = input["activeForm"].as_str().and_then(text) {
                    t.active = Some(s);
                }
                if let Some(s) = input["owner"].as_str() {
                    t.owner = text(s);
                }
                for b in ids(&input["addBlockedBy"]) {
                    if !t.blocked_by.contains(&b) && b != id {
                        t.blocked_by.push(b);
                    }
                }
                for b in ids(&input["addBlocks"]) {
                    if let Some(other) = self.tasks.iter_mut().find(|o| o.id == b)
                        && !other.blocked_by.contains(&id)
                    {
                        other.blocked_by.push(id.clone());
                    }
                }
                self.tasks_at = Some(at);
            }
            _ => {}
        }
    }

    /// A raw line that could change the list, to parse.
    pub fn wants(&self, line: &[u8]) -> bool {
        wanted(line, !self.calls.is_empty())
    }

    /// The list now, or nothing when the session never wrote one (or
    /// emptied it).
    pub fn summary(&self) -> Option<Plan> {
        let (steps, at) = match (self.todos_at, self.tasks_at) {
            (Some(a), Some(b)) if b >= a => (&self.tasks, b),
            (Some(a), _) => (&self.todos, a),
            (None, Some(b)) => (&self.tasks, b),
            (None, None) => return None,
        };
        if steps.is_empty() {
            return None;
        }
        let done = |id: &str| steps.iter().find(|s| s.id == id).is_none_or(|s| s.status == StepStatus::Completed);
        let skip = steps.len().saturating_sub(MAX_STEPS);
        Some(Plan {
            steps: steps
                .iter()
                .skip(skip)
                .map(|s| PlanStep {
                    id: s.id.clone(),
                    text: s.text.clone(),
                    active: s.active.clone(),
                    status: s.status,
                    // Only what it still waits on: a finished blocker
                    // doesn't hold it.
                    blocked_by: s.blocked_by.iter().filter(|b| !done(b)).cloned().collect(),
                    owner: s.owner.clone(),
                })
                .collect(),
            model: self.model.as_deref().map(super::context::short_model),
            at_ms: at,
        })
    }
}

fn status(s: &str) -> Option<StepStatus> {
    Some(match s {
        "pending" => StepStatus::Pending,
        "in_progress" => StepStatus::InProgress,
        "completed" => StepStatus::Completed,
        _ => return None,
    })
}

/// A step's words, on one line and cut short; nothing if empty.
fn text(s: &str) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return None;
    }
    if s.chars().count() <= MAX_TEXT {
        return Some(s);
    }
    let mut out: String = s.chars().take(MAX_TEXT - 1).collect();
    out.push('…');
    Some(out)
}

/// Task ids in an `addBlockedBy`/`addBlocks` list: strings or numbers.
fn ids(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_owned).or(x.as_u64().map(|n| n.to_string())))
        .collect()
}

fn result_text(r: &Value) -> String {
    match &r["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

/// "Task #3 created successfully: …" says 3.
fn created_id(s: &str) -> Option<String> {
    let rest = s.split_once("Task #")?.1;
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    (!id.is_empty()).then_some(id)
}

/// A line that can change the list: one naming a task tool, or a result
/// while a call waits for one, or one that names a model. Others aren't
/// parsed.
fn wanted(line: &[u8], waiting: bool) -> bool {
    let has = |n: &[u8]| line.windows(n.len()).any(|w| w == n);
    TOOLS.iter().any(|t| has(t.as_bytes())) || (waiting && has(b"tool_result"))
}

/// The last `"model":"…"` on an assistant line, without parsing it.
fn model_in(line: &[u8]) -> Option<&str> {
    const KEY: &[u8] = b"\"model\":";
    let at = line.windows(KEY.len()).position(|w| w == KEY)? + KEY.len();
    let rest = &line[at..];
    let start = rest.iter().position(|c| *c == b'"')? + 1;
    let len = rest[start..].iter().position(|c| *c == b'"')?;
    std::str::from_utf8(&rest[start..start + len]).ok()
}

/// A transcript followed for its task list alone (a terminal's Claude
/// Code): what's new is read on each [`PlanFile::read`]; a file that
/// shrank or was replaced is read again from the start.
#[derive(Debug, Default)]
pub struct PlanFile {
    path: PathBuf,
    read: u64,
    /// A line not yet ended.
    rest: Vec<u8>,
    stamp: Option<(u64, std::time::SystemTime)>,
    plans: Plans,
}

impl PlanFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path, ..Default::default() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read what's new. True if it read anything.
    pub fn read(&mut self) -> std::io::Result<bool> {
        let mut f = std::fs::File::open(&self.path)?;
        let md = f.metadata()?;
        let stamp = Some((md.len(), md.modified()?));
        if stamp == self.stamp {
            return Ok(false);
        }
        self.stamp = stamp;
        if md.len() < self.read {
            *self = Self::new(std::mem::take(&mut self.path));
            self.stamp = stamp;
        }
        f.seek(SeekFrom::Start(self.read))?;
        let mut new = vec![];
        f.read_to_end(&mut new)?;
        self.read += new.len() as u64;
        let mut buf = std::mem::take(&mut self.rest);
        buf.extend_from_slice(&new);
        let mut lines: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
        let last = lines.pop().unwrap_or_default();
        for line in lines {
            self.line(line);
        }
        self.rest = last.to_vec();
        Ok(true)
    }

    fn line(&mut self, line: &[u8]) {
        if line.is_empty() {
            return;
        }
        if wanted(line, !self.plans.calls.is_empty()) {
            if let Ok(v) = serde_json::from_slice::<Value>(line) {
                self.plans.line(&v);
            }
        } else if [&b"\"role\":\"assistant\""[..], b"\"role\": \"assistant\""]
            .iter()
            .any(|k| line.windows(k.len()).any(|w| w == *k))
        {
            // Only the model, the cheap way.
            if let Some(m) = model_in(line).filter(|m| !m.is_empty() && !m.starts_with('<')) {
                self.plans.model = Some(m.to_owned());
            }
        }
    }

    pub fn summary(&self) -> Option<Plan> {
        self.plans.summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> Vec<u8> {
        let p = format!("{}/tests/fixtures/conversations/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    fn read(jsonl: &[u8]) -> Plans {
        let mut p = Plans::default();
        for l in jsonl.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
            p.line(&serde_json::from_slice(l).unwrap());
        }
        p
    }

    fn statuses(p: &Plan) -> Vec<(&str, StepStatus)> {
        p.steps.iter().map(|s| (s.id.as_str(), s.status)).collect()
    }

    /// A tool call and its result, as Claude Code writes them.
    fn call(id: &str, name: &str, input: Value) -> Value {
        json!({"type": "assistant", "isSidechain": false, "timestamp": "2026-10-06T09:00:00.000Z",
            "message": {"model": "claude-sonnet-5-5", "role": "assistant",
                "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
    }
    fn result(id: &str, text: &str, extra: Value, error: bool) -> Value {
        json!({"type": "user", "isSidechain": false, "timestamp": "2026-10-06T09:00:01.000Z",
            "message": {"role": "user", "content": [{"tool_use_id": id, "type": "tool_result", "content": text, "is_error": error}]},
            "toolUseResult": extra})
    }
    fn lines(vs: &[Value]) -> Plans {
        let mut p = Plans::default();
        for v in vs {
            p.line(v);
        }
        p
    }

    #[test]
    fn the_task_tools_build_the_list_from_a_real_transcript() {
        // Five made, two worked through, one deleted, one done without
        // starting (a real session's calls, its words replaced).
        let p = read(&fixture("shapes/tasks.jsonl")).summary().expect("a plan");
        assert_eq!(
            statuses(&p),
            [
                ("1", StepStatus::Completed),
                ("2", StepStatus::Completed),
                ("4", StepStatus::Completed),
                ("5", StepStatus::Completed)
            ]
        );
        assert_eq!(p.steps[0].text, "Turn on line tables in CI");
        assert_eq!(p.steps[0].active.as_deref(), Some("Turning on line tables"));
        assert_eq!(p.model.as_deref(), Some("Opus 5.5"));
        assert!(p.at_ms > 0);
    }

    #[test]
    fn partway_through_a_step_is_in_progress() {
        // Cut the real transcript after the first TaskUpdate's result.
        let all = fixture("shapes/tasks.jsonl");
        let lines: Vec<&[u8]> = all.split(|b| *b == b'\n').collect();
        let upto = lines.iter().position(|l| String::from_utf8_lossy(l).contains("statusChange")).unwrap();
        let p = read(&lines[..=upto].join(&b'\n')).summary().unwrap();
        assert_eq!(p.steps.len(), 5);
        assert_eq!(p.steps[1].status, StepStatus::InProgress);
        assert!(p.steps.iter().filter(|s| s.id != "2").all(|s| s.status == StepStatus::Pending));
    }

    #[test]
    fn the_last_todo_write_is_the_list() {
        let p = read(&fixture("shapes/todo-write.jsonl")).summary().expect("a plan");
        assert_eq!(p.steps.len(), 3);
        assert_eq!(
            statuses(&p),
            [("1", StepStatus::Completed), ("2", StepStatus::InProgress), ("3", StepStatus::Pending)]
        );
        assert_eq!(p.steps[1].text, "Add the retry test");
        assert_eq!(p.steps[1].active.as_deref(), Some("Adding the retry test"));
    }

    #[test]
    fn a_failed_call_changes_nothing() {
        let p = lines(&[
            call("a", "TaskCreate", json!({"subject": "Write it", "activeForm": "Writing it"})),
            result(
                "a",
                "Task #1 created successfully: Write it",
                json!({"task": {"id": "1", "subject": "Write it"}}),
                false,
            ),
            call("b", "TaskUpdate", json!({"taskId": "1", "status": "in_progress"})),
            result("b", "<tool_use_error>refused</tool_use_error>", Value::Null, true),
        ]);
        assert_eq!(p.summary().unwrap().steps[0].status, StepStatus::Pending);
        // Nor does a call with no result yet.
        let p = lines(&[call(
            "c",
            "TodoWrite",
            json!({"todos": [{"content": "x", "status": "pending", "activeForm": "x"}]}),
        )]);
        assert!(p.summary().is_none());
    }

    #[test]
    fn a_step_waiting_on_an_open_one_is_blocked_until_it_finishes() {
        let mk = |id: &str, n: &str, s: &str| {
            [
                call(id, "TaskCreate", json!({"subject": s})),
                result(id, &format!("Task #{n} created successfully: {s}"), Value::Null, false),
            ]
        };
        let mut vs: Vec<Value> =
            [mk("a", "1", "Schema"), mk("b", "2", "Backfill"), mk("c", "3", "Switch reads")].concat();
        vs.push(call("d", "TaskUpdate", json!({"taskId": "3", "addBlockedBy": ["2"]})));
        vs.push(result("d", "Updated task #3 blockedBy", Value::Null, false));
        // addBlocks says the same from the other side.
        vs.push(call("e", "TaskUpdate", json!({"taskId": "1", "addBlocks": ["2"], "owner": "migrator"})));
        vs.push(result("e", "Updated task #1 blocks", Value::Null, false));
        let p = lines(&vs).summary().unwrap();
        assert_eq!(p.steps[2].blocked_by, ["2"]);
        assert_eq!(p.steps[1].blocked_by, ["1"]);
        assert_eq!(p.steps[0].owner.as_deref(), Some("migrator"));
        // 1 done: 2 isn't held, 3 still is.
        vs.push(call("f", "TaskUpdate", json!({"taskId": "1", "status": "completed"})));
        vs.push(result("f", "Updated task #1 status", Value::Null, false));
        let p = lines(&vs).summary().unwrap();
        assert!(p.steps[1].blocked_by.is_empty());
        assert_eq!(p.steps[2].blocked_by, ["2"]);
        // A deleted blocker holds nothing.
        vs.push(call("g", "TaskUpdate", json!({"taskId": "2", "status": "deleted"})));
        vs.push(result("g", "Updated task #2 deleted", Value::Null, false));
        let p = lines(&vs).summary().unwrap();
        assert_eq!(p.steps.len(), 2);
        assert!(p.steps[1].blocked_by.is_empty());
    }

    #[test]
    fn subagents_lists_are_theirs_and_an_empty_list_is_none() {
        let mut side = call("a", "TodoWrite", json!({"todos": [{"content": "x", "status": "pending"}]}));
        side["isSidechain"] = json!(true);
        let mut r = result("a", "ok", Value::Null, false);
        r["isSidechain"] = json!(true);
        assert!(lines(&[side, r]).summary().is_none());
        let p = lines(&[call("b", "TodoWrite", json!({"todos": []})), result("b", "ok", Value::Null, false)]);
        assert!(p.summary().is_none());
    }

    #[test]
    fn the_list_written_last_wins() {
        let p = lines(&[
            call(
                "a",
                "TodoWrite",
                json!({"todos": [{"content": "Old", "status": "in_progress", "activeForm": "Old"}]}),
            ),
            result("a", "ok", Value::Null, false),
            call("b", "TaskCreate", json!({"subject": "New"})),
            result("b", "Task #1 created successfully: New", Value::Null, false),
        ]);
        assert_eq!(p.summary().unwrap().steps[0].text, "New");
    }

    #[test]
    fn a_followed_file_reads_only_whats_new_and_agrees() {
        let dir = std::env::temp_dir().join(format!("arugula-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let all = fixture("shapes/tasks.jsonl");
        let half = all.len() / 2;
        std::fs::write(&path, &all[..half]).unwrap();
        let mut f = PlanFile::new(path.clone());
        assert!(f.read().unwrap());
        assert!(!f.read().unwrap(), "nothing new");
        {
            use std::io::Write;
            let mut w = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            w.write_all(&all[half..]).unwrap();
        }
        assert!(f.read().unwrap());
        let whole = read(&all).summary();
        assert_eq!(f.summary(), whole);
        assert_eq!(f.summary().unwrap().model.as_deref(), Some("Opus 5.5"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_steps_are_cut_and_one_lined() {
        assert_eq!(text("  a\n  b ").as_deref(), Some("a b"));
        assert_eq!(text("x".repeat(500).as_str()).unwrap().chars().count(), MAX_TEXT);
        assert_eq!(created_id("Task #12 created successfully: x").as_deref(), Some("12"));
    }
}
