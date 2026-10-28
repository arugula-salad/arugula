//! A person's agent and agent sessions (Fleet 3, aru-121): `session_start`
//! and `session_reply`. With its owner's grants over a project, this
//! machine's agents start an agent session there as the owner
//! (`sessions.start`) and answer a session's question or prompt it
//! (`sessions.reply`), without a browser.
//!
//! Like the thread tools, they live on control's platform, never here: each
//! is a `/api/daemon/sessions…` route signed with this machine's key, and
//! the platform asks the grant every time
//! (`crates/platform/src/chat/agent_sessions.rs`). The ask or the reply is
//! a message in the session's thread, labelled "<owner>'s agent on
//! <machine>"; the owner is the session's asker, and its bill is theirs.
//! Neither approves anything: a session's permission waits on a person.
//!
//! An agent a guest stands behind may not use them: they act as the owner.
//!
//! **Following a wave** (Fleet 5): `list` kind `sessions` lists a project's
//! sessions (state, box, branch, last push, what waits on its person),
//! narrowed by an issue's label, a tracker issue or who started them;
//! `read_session` reads one with its last answer whole (the thread has only
//! its end); and `wait` with `session` holds until any of a wave's sessions
//! finishes its turn, with its branch and head, so a lead doesn't poll.
//! Each with `sessions.start`, `sessions.reply` or `threads.read` over the
//! project.

use std::time::{Duration, Instant};

use arugula_control_wire::sessions::{
    AGENT_SESSIONS, AGENT_SESSIONS_WAIT, AgentSessionStart, SESSION_WAIT_MAX, agent_project_sessions, agent_session,
    agent_session_reply,
};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    issues::Native,
    tools::{Call, Out, done},
};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const START_TOOL: &str = "session_start";
pub(crate) const REPLY_TOOL: &str = "session_reply";
pub(crate) const READ_TOOL: &str = "read_session";

/// `list`'s kind.
pub(crate) const LIST: super::tools::Kind = super::tools::Kind {
    name: "sessions",
    description: "A project's agent sessions in Arugula control, a wave of them, newest first: each one's id, state, issue, box, branch and HEAD, when it last pushed, who started it, and what waits on your person (its question, a permission, CI that failed again, its PR to send). Narrow it with label (its issue's), tracker (an issue's number: the issues its body names as #N, and those naming it), asker (an account, or me), mine (only those this machine's agents started) and going. Needs your person's sessions.start, sessions.reply or threads.read grant over the project. wait with session waits for them; read_session reads one's whole answer.",
    schema: rmcp::handler::server::common::schema_for_type::<ListSessionsArgs>,
    flag: None,
};

pub(crate) const START_DESCRIPTION: &str = "Start an agent session in Arugula control as your person: @arugula on a native issue (issue: #N in the current project, or iss_…) or in a project's channel (room, and thread: a message's seq to ask in its thread). It runs on the project's cloud workspace, as your person (the asker, who pays for it), and its card says it was started by their agent on this machine. Needs their sessions.start grant over the project. prompt is what to ask (it may name the repo, owner/name, and a model); on an issue it defaults to working on it. Returns the session (ses_…), or what the platform said instead: why not, or which repo it asks (ask again there with the repo, owner/name, in prompt). session_reply answers it or prompts it after.";

pub(crate) const READ_DESCRIPTION: &str = "Read an agent session in Arugula control (ses_…): its state, box, branch, HEAD and last push, what its latest turn left (commits, diffstat, its finish step), what it asks while it waits on a person, and its last answer whole, as its box sent it (the thread's copy keeps only its end). Needs your person's sessions.start, sessions.reply or threads.read grant over its project. wait with session waits for its turn to finish.";

pub(crate) const REPLY_DESCRIPTION: &str = "Reply to an agent session in Arugula control as your person (ses_…, from session_start or the thread's card), in its thread: when it waits on a question, your text answers it; otherwise it's the session's next prompt (queued while a turn runs). Never an approval: a permission waits on a person, and your text is queued for after it. Needs your person's sessions.reply grant over its project. Says which it did.";

#[derive(Deserialize, JsonSchema)]
pub struct SessionStartArgs {
    /// A native issue: "#N" (in the current project) or "iss_…".
    #[serde(default)]
    pub issue: Option<String>,
    /// Or a project's channel: its room id (list kind threads).
    #[serde(default)]
    pub room: Option<String>,
    /// In the room, the thread of this message (its seq); without it, the
    /// ask is a message of its own, starting its thread.
    #[serde(default)]
    pub thread: Option<i64>,
    /// What to ask @arugula: plain words, which may name the repo
    /// (owner/name) and a model. On an issue, by default, to work on it.
    #[serde(default)]
    pub prompt: Option<String>,
    /// "#N"'s project: its id (prj_…) or personal. Default: the one this
    /// clone's GitHub repo is linked to, else your person's personal one.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SessionReplyArgs {
    /// The session: its id (ses_…).
    pub session: String,
    /// The answer or the prompt, in markdown.
    pub text: String,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct ListSessionsArgs {
    /// The project: its id (prj_…) or personal. Default: the one this
    /// clone's GitHub repo is linked to, else your person's personal one.
    #[serde(default)]
    pub project: Option<String>,
    /// Only sessions on an issue with this label.
    #[serde(default)]
    pub label: Option<String>,
    /// Only sessions on a tracker's issues: its number (#N), the issues its
    /// body names, those whose body names it, and itself.
    #[serde(default)]
    pub tracker: Option<String>,
    /// Only sessions this account asked for (or me: your person).
    #[serde(default)]
    pub asker: Option<String>,
    /// Only sessions this machine's agents started.
    #[serde(default)]
    pub mine: bool,
    /// Only ones going (not failed, stopped or merged).
    #[serde(default)]
    pub going: bool,
    /// At most this many (default 50, at most 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadSessionArgs {
    /// The session: its id (ses_…).
    pub session: String,
}

/// The sessions a `wait` names: ids (`ses_…`) separated by spaces or
/// commas, each once.
pub(crate) fn session_ids(s: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for id in s.split(|c: char| c == ',' || c.is_whitespace()).filter(|x| !x.is_empty()) {
        if !id.starts_with("ses_") {
            return Err(format!("session: ids, ses_…, not {id:?}"));
        }
        if !out.iter().any(|x| x == id) {
            out.push(id.to_owned());
        }
    }
    if out.is_empty() {
        return Err("session: one or more ids, ses_…".into());
    }
    Ok(out)
}

/// A session's row ([`arugula_control_wire::sessions::SessionRow`]) in a
/// line.
pub(crate) fn row_line(r: &Value) -> String {
    let s = |k: &str| r[k].as_str().unwrap_or_default();
    let mut out = format!("{} {}", s("session"), s("state"));
    if let Some(i) = r["issue"].as_object() {
        out += &format!(" · #{} {}", i["number"], i["title"].as_str().unwrap_or_default());
    }
    let place = if s("workspace_name").is_empty() { s("workspace") } else { s("workspace_name") };
    out += &format!(" · on {place} · {}", s("branch"));
    if let Some(h) = r["head"].as_str() {
        out += &format!(" at {h}");
    }
    match r["pushed_at"].as_u64() {
        Some(at) => out += &format!(" · pushed {}", super::tools::ago(at)),
        None => out += " · not pushed",
    }
    if let Some(by) = r["started_by"].as_str() {
        out += &format!(" · started by {by}");
    } else if !s("asker_name").is_empty() {
        out += &format!(" · asked by {}", s("asker_name"));
    }
    if let Some(w) = r["waiting"].as_str() {
        out += &format!(" · waits on your person: {w}");
    }
    if let Some(q) = r["question"].as_str() {
        out += &format!(" ({q})");
    }
    if let Some(pr) = r["pr"].as_str() {
        out += &format!(" · PR {pr}");
    }
    if let Some(why) = r["reason"].as_str() {
        out += &format!(" · {why}");
    }
    out
}

/// A session's read ([`arugula_control_wire::sessions::AgentSessionRead`])
/// as text: its line, its turn, what it asks, and its answer whole.
pub(crate) fn read_text(v: &Value) -> String {
    let mut out = row_line(&v["session"]);
    let t = &v["turn"];
    if t.is_object() {
        let commits: Vec<&str> = t["commits"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        out += &format!("\nIts latest turn: {} commit(s)", commits.len());
        if t["pushed"] == true {
            out += ", pushed";
        }
        for c in commits {
            out += &format!("\n  {c}");
        }
        if let Some(d) = t["diffstat"].as_str() {
            out += &format!("\n{d}");
        }
        if let Some(f) = t["finish"].as_object() {
            let ok = if f["ok"] == true { "passed" } else { "failed" };
            out += &format!("\nFinish step {ok}: {}", f["command"].as_str().unwrap_or_default());
        }
    }
    if let Some(q) = v["open"]["question"].as_str() {
        out += &format!("\nIt asks: {q}");
    }
    match v["answer"].as_object() {
        Some(a) => {
            let cut = if a["cut"] == true { " (its box kept only its end)" } else { "" };
            out += &format!(
                "\n\nIts last answer, {}{cut}:\n{}",
                a["at"].as_u64().map(super::tools::ago).unwrap_or_default(),
                a["text"].as_str().unwrap_or_default()
            );
        }
        None => out += "\n\nNo answer yet.",
    }
    out
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where sessions live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control can't start sessions for agents yet".into()
        }
        e => e.to_string(),
    }
}

/// The body for [`AGENT_SESSIONS`], from the tool's arguments: `issue`'s
/// number is sent with its project, which the caller resolved.
pub(crate) fn start_body(a: &SessionStartArgs, project: Option<String>) -> Result<AgentSessionStart, String> {
    let room = a.room.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let prompt = a.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned);
    match (a.issue.as_deref().map(str::trim).filter(|i| !i.is_empty()), room) {
        (Some(_), Some(_)) => Err("say where: an issue or a room, not both".into()),
        (None, None) => Err("say where: issue (#N or iss_…), or room (a project's channel)".into()),
        (Some(i), None) => {
            if a.thread.is_some() {
                return Err("an issue's thread is the issue: leave thread out".into());
            }
            match Native::parse(i) {
                Some(Native::Id(id)) => Ok(AgentSessionStart { issue: Some(id), prompt, ..Default::default() }),
                Some(Native::Number(n)) => {
                    Ok(AgentSessionStart { number: Some(n), project, prompt, ..Default::default() })
                }
                None => Err(format!("issue: #N (in the current project) or iss_…, not {i:?}")),
            }
        }
        (None, Some(r)) => {
            if prompt.is_none() {
                return Err("say what to ask @arugula: prompt".into());
            }
            Ok(AgentSessionStart { room: Some(r.to_owned()), thread: a.thread, prompt, ..Default::default() })
        }
    }
}

impl Call<'_> {
    /// They act as this machine's owner: only the owner's own agents may.
    async fn owners_sessions(&self, tool: &str) -> Result<(), String> {
        if let Some(me) = self.me()
            && self.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err(format!("{tool} acts as this machine's owner: only their own agents may"));
        }
        Ok(())
    }

    async fn sessions_call(&self, path: &str, body: Value) -> Result<Value, String> {
        self.app().control.cloud(Method::POST, path, Some(&body)).await.map_err(said)
    }

    /// `session_start`.
    pub(crate) async fn session_start(&self, a: SessionStartArgs) -> Out {
        self.owners_sessions(START_TOOL).await?;
        let number = a.issue.as_deref().and_then(Native::parse).is_some_and(|n| matches!(n, Native::Number(_)));
        let project = match number {
            true => Some(self.current_project(a.project.as_deref()).await?),
            false => None,
        };
        let body = serde_json::to_value(start_body(&a, project)?).map_err(|e| e.to_string())?;
        let v = self.sessions_call(AGENT_SESSIONS, body).await?;
        let at = format!("seq {} in {}", v["seq"], v["room"].as_str().unwrap_or("?"));
        let s = match v["session"].as_str() {
            Some(ses) => format!(
                "Started session {ses} as your person, asked by their agent on this machine ({at}). Its answers come in that thread (read_thread); session_reply answers or prompts it."
            ),
            None => {
                let said: Vec<&str> = v["said"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                match said.as_slice() {
                    [] => format!("Asked @arugula ({at}), but no session started and the platform said nothing."),
                    lines => format!("Asked @arugula ({at}); no session yet. The platform said:\n{}", lines.join("\n")),
                }
            }
        };
        done(s, v)
    }

    async fn sessions_get(&self, path: &str) -> Result<Value, String> {
        self.app().control.cloud(Method::GET, path, None).await.map_err(said)
    }

    /// `list` kind `sessions`.
    pub(crate) async fn list_sessions(&self, a: ListSessionsArgs) -> Out {
        let project = self.current_project(a.project.as_deref()).await?;
        let mut q: Vec<(&str, String)> = vec![];
        if let Some(l) = a.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            q.push(("label", l.to_owned()));
        }
        if let Some(t) = a.tracker.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let n: u64 =
                t.trim_start_matches('#').parse().map_err(|_| format!("tracker: an issue's number, #N, not {t:?}"))?;
            q.push(("tracker", n.to_string()));
        }
        if let Some(who) = a.asker.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            q.push(("asker", who.to_owned()));
        }
        if a.mine {
            q.push(("agent", "me".into()));
        }
        if a.going {
            q.push(("going", "true".into()));
        }
        if let Some(n) = a.limit {
            q.push(("limit", n.to_string()));
        }
        let mut path = agent_project_sessions(&project);
        if !q.is_empty() {
            let mut ser = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in &q {
                ser.append_pair(k, v);
            }
            path += &format!("?{}", ser.finish());
        }
        let v = self.sessions_get(&path).await?;
        let rows = v["sessions"].as_array().cloned().unwrap_or_default();
        let s = match rows.len() {
            0 => format!("No sessions in project {project} match."),
            n => {
                let more = if v["more"] == true { " (more match: narrow it, or raise limit)" } else { "" };
                format!(
                    "{n} in project {project}, newest first{more}:\n{}",
                    rows.iter().map(row_line).collect::<Vec<_>>().join("\n")
                )
            }
        };
        done(s, v)
    }

    /// `read_session`.
    pub(crate) async fn read_session(&self, a: ReadSessionArgs) -> Out {
        let ses = a.session.trim();
        if !ses.starts_with("ses_") {
            return Err(format!("session: its id, ses_…, not {ses:?}"));
        }
        let v = self.sessions_get(&agent_session(ses)).await?;
        done(read_text(&v), v)
    }

    /// `wait` with `session`: until any of them settles (its turn
    /// finished, it waits on a person, or it ended), or `limit`. Each
    /// settled one comes back read whole.
    pub(crate) async fn wait_sessions(&self, ids: &str, limit: Duration) -> Out {
        let ids = session_ids(ids)?;
        let started = Instant::now();
        let body = |wait: u64| json!({ "sessions": ids, "wait": wait });
        let poll = async {
            loop {
                let v = match self
                    .app()
                    .control
                    .cloud(Method::POST, AGENT_SESSIONS_WAIT, Some(&body(SESSION_WAIT_MAX)))
                    .await
                {
                    Ok(v) => v,
                    // Too many waits open: back off, then ask again.
                    Err(CloudRefused::Said { status: 429, .. }) => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    Err(e) => return Err(said(e)),
                };
                if v["settled"].as_array().is_some_and(|s| !s.is_empty()) {
                    return Ok::<_, String>(v);
                }
            }
        };
        let what = format!("waiting for {}", ids.join(", "));
        let Some(v) = self.waiting(&what, limit, poll).await else {
            let secs = started.elapsed().as_secs();
            let v = self.sessions_call(AGENT_SESSIONS_WAIT, body(0)).await?;
            let rows = v["sessions"].as_array().cloned().unwrap_or_default();
            return done(
                format!(
                    "None finished after {secs}s: call wait with session again.\n{}",
                    rows.iter().map(row_line).collect::<Vec<_>>().join("\n")
                ),
                json!({ "state": "still running", "sessions": rows }),
            );
        };
        let v = v?;
        let settled: Vec<String> =
            v["settled"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        let mut reads = Vec::new();
        let mut text = Vec::new();
        for id in &settled {
            let r = self.sessions_get(&agent_session(id)).await?;
            text.push(read_text(&r));
            reads.push(r);
        }
        let going: Vec<&Value> =
            v["sessions"].as_array().into_iter().flatten().filter(|r| r["settled"] != true).collect();
        let mut s = format!("Finished its turn: {}.\n\n{}", settled.join(", "), text.join("\n\n---\n\n"));
        if !going.is_empty() {
            let ids: Vec<&str> = going.iter().filter_map(|r| r["session"].as_str()).collect();
            s += &format!(
                "\n\nStill going (wait with session \"{}\" for them):\n{}",
                ids.join(" "),
                going.iter().map(|r| row_line(r)).collect::<Vec<_>>().join("\n")
            );
        }
        done(s, json!({ "state": "settled", "settled": reads, "sessions": v["sessions"] }))
    }

    /// `session_reply`.
    pub(crate) async fn session_reply(&self, a: SessionReplyArgs) -> Out {
        self.owners_sessions(REPLY_TOOL).await?;
        let ses = a.session.trim();
        if !ses.starts_with("ses_") {
            return Err(format!("session: its id, ses_…, not {ses:?}"));
        }
        let v = self.sessions_call(&agent_session_reply(ses), json!({ "text": a.text })).await?;
        let s = match v["did"].as_str() {
            Some("answered") => format!("Answered session {ses}'s question, as your person's agent on this machine."),
            Some("queued") if v["permission"] == true => format!(
                "Session {ses} waits on a permission, which a person allows or denies: your reply is queued as its prompt for after."
            ),
            Some("queued") => {
                format!("Queued as session {ses}'s next prompt (it starts when the turn under way ends).")
            }
            Some("refused") => format!(
                "Posted in session {ses}'s thread, but it took no prompt: {}",
                v["why"].as_str().unwrap_or("it said no")
            ),
            _ => {
                format!("Posted in session {ses}'s thread, but your person only views its project: it took no prompt.")
            }
        };
        done(s, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> SessionStartArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_start_is_an_issue_or_a_room() {
        let id = format!("iss_{}", "a".repeat(32));
        let b = start_body(&args(json!({ "issue": id })), None).unwrap();
        assert_eq!((b.issue.as_deref(), b.number, b.prompt), (Some(id.as_str()), None, None));
        let b = start_body(&args(json!({ "issue": "#12", "prompt": " fix it " })), Some("prj_1".into())).unwrap();
        assert_eq!((b.number, b.project.as_deref(), b.prompt.as_deref()), (Some(12), Some("prj_1"), Some("fix it")));
        let b = start_body(&args(json!({ "room": "ch_1", "thread": 4, "prompt": "go" })), None).unwrap();
        assert_eq!((b.room.as_deref(), b.thread, b.issue), (Some("ch_1"), Some(4), None));
        assert!(start_body(&args(json!({})), None).unwrap_err().contains("say where"));
        assert!(start_body(&args(json!({ "issue": "#1", "room": "ch_1" })), None).unwrap_err().contains("not both"));
        assert!(start_body(&args(json!({ "room": "ch_1" })), None).unwrap_err().contains("prompt"));
        assert!(start_body(&args(json!({ "issue": "#1", "thread": 2 })), None).unwrap_err().contains("thread"));
        assert!(start_body(&args(json!({ "issue": "o/r#1" })), None).unwrap_err().contains("#N"));
    }

    #[test]
    fn a_wait_names_its_sessions() {
        assert_eq!(session_ids(" ses_a, ses_b ses_a\nses_c").unwrap(), ["ses_a", "ses_b", "ses_c"]);
        assert!(session_ids(" , ").unwrap_err().contains("one or more"));
        assert!(session_ids("ses_a run_b").unwrap_err().contains("run_b"));
    }

    /// The done signal's text: the line, then the answer whole.
    #[test]
    fn a_read_says_its_branch_head_and_whole_answer() {
        let long = format!("{}\nCaveat: one spec skipped.", "word ".repeat(1000));
        let v = json!({
            "session": { "session": "ses_1", "state": "done", "settled": true, "branch": "arugula/1-x", "head": "abc1234",
                "workspace": "hw_1", "workspace_name": "dev", "pushed_at": null, "asker_name": "Jake",
                "started_by": "Jake's agent on geek", "issue": { "number": 267, "title": "Fleet 5" },
                "waiting": "its PR to send" },
            "turn": { "pushed": true, "commits": ["abc1234 Fleet 5: a wave"], "diffstat": "3 files changed" },
            "answer": { "at": 0, "text": long, "cut": false },
        });
        let t = read_text(&v);
        assert!(t.starts_with("ses_1 done · #267 Fleet 5 · on dev · arugula/1-x at abc1234 · not pushed"), "{t}");
        assert!(t.contains("started by Jake's agent on geek · waits on your person: its PR to send"), "{t}");
        assert!(t.contains("1 commit(s), pushed\n  abc1234 Fleet 5: a wave\n3 files changed"), "{t}");
        assert!(t.ends_with(&long), "the answer whole");
        let none = read_text(&json!({ "session": { "session": "ses_2", "state": "working" } }));
        assert!(none.ends_with("No answer yet."), "{none}");
    }
}
