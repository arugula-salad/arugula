//! Runs for agents (M216, platform step 7): `start_run`, `read_run`, and
//! `wait` with `run`, over a project's runs in Arugula control. A run
//! executes on one of the project's hosted boxes (M212), never here.
//!
//! - **Through the owner's grants.** Each call is a machine-signed route on
//!   the platform (`/api/daemon/projects/{project}/runs`,
//!   `/api/daemon/runs/{run}`, `crates/platform/src/triggers/agent.rs`),
//!   which asks this machine's grants every time: `runs.start` to start a
//!   run spec, `runs.command` to start a command or an instruction of its
//!   own, `runs.read` to read, over one project each, given by its owner
//!   (`arugula grants add <machine> runs.start --project <name>`, or the
//!   platform page). A start is the owner's by hand, recorded with this
//!   machine, and runs under the project's standing grant like any
//!   trigger's. Nothing here approves anything: a deploy's approval is an
//!   owner device's (M218).
//! - **Waiting** asks the platform for the run, which answers once it
//!   moves (`since` and `wait`, at most 15 s a time), until it ends or the
//!   wait's timeout. Each ask is one read, counted apart from the others;
//!   past the platform's cap on waits open at once it backs off and asks
//!   again. Once it ends, it reads the log's tail.
//! - **The log** is the platform's durable copy (M213), paged by byte
//!   offset. Until the platform keeps one, a read says where the output is:
//!   the run's pane on its box, which the project's people watch end to
//!   end, and which `arugula runs logs` reads from a person's CLI. This
//!   machine's agents can't read that pane: a machine reaches another only
//!   for its agents (`/api/a2a/`).
//!
//! An agent a guest stands behind may not use them: they act as the owner.

use std::time::{Duration, Instant};

use arugula_control_wire::runs::{
    AGENT_RUN_REACH, AgentRunStart, RUN_WAIT_MAX, agent_project_runs, agent_run, agent_run_log,
};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    results::{RunsProject, RunsReach},
    tools::{Call, Out, PAGE, PAGE_MAX, done},
};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const START_TOOL: &str = "start_run";

/// How long a wait backs off when the platform has too many open.
const BACK_OFF: Duration = Duration::from_secs(5);
pub(crate) const READ_TOOL: &str = "read_run";

pub(crate) const START_DESCRIPTION: &str = "Start a run on one of a project's cloud workspaces in Arugula control: a run spec of the project by name, a command, or an instruction to an agent (Claude Code, headless). It runs there, not here, in a pane the project's people watch, under the project's standing grant (without one it waits for an owner to approve it). Your person's grants say where you may, over the project: runs.start for a run spec, runs.command for a command or an instruction of your own. Never more than their role: it's recorded as theirs, started by their agent on this machine. Returns the run (run_…) at once, pending; wait with run follows it, read_run reads its timeline and log.";

pub(crate) const READ_DESCRIPTION: &str = "Read a run in Arugula control (with this machine's runs.read grant over its project): its state, exit code, timeline, and a page of its log by byte offset (pass next_offset back as offset for more). Without run: a project's latest runs. wait with run waits for one to end.";

#[derive(Deserialize, JsonSchema)]
pub struct StartRunArgs {
    /// The project in Arugula control: its id (prj_…), or personal.
    pub project: String,
    /// A run spec of the project, by its name or id: what it runs and on
    /// which box.
    #[serde(default)]
    pub spec: Option<String>,
    /// Or a command: the program and its arguments, not through a shell
    /// (["sh", "-c", "..."] for one).
    #[serde(default)]
    pub command: Vec<String>,
    /// Or an instruction to an agent (Claude Code, headless).
    #[serde(default)]
    pub instruction: Option<String>,
    /// With instruction: the agent recipe to run it as.
    #[serde(default)]
    pub recipe: Option<String>,
    /// Where it runs: an absolute directory on the box (default its home).
    #[serde(default)]
    pub directory: Option<String>,
    /// Which of the project's cloud workspaces: its name or id (hw_…). Needed
    /// when it has more than one.
    #[serde(default, rename = "box")]
    pub on_box: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadRunArgs {
    /// The run (run_…). Without it: a project's latest runs.
    #[serde(default)]
    pub run: Option<String>,
    /// Without run: the project, its id (prj_…) or personal.
    #[serde(default)]
    pub project: Option<String>,
    /// Without run: only runs in this state (pending, running, succeeded,
    /// failed, canceled, ...).
    #[serde(default)]
    pub state: Option<String>,
    /// The log from this byte offset (a previous read's next_offset).
    #[serde(default)]
    pub offset: Option<u64>,
    /// At most this many characters of log (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where runs live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't let agents start or read runs yet".into()
        }
        e => e.to_string(),
    }
}

fn run_id(s: &str) -> Result<&str, String> {
    let s = s.trim();
    let ok = s.strip_prefix("run_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    if ok { Ok(s) } else { Err(format!("a run is run_ and 32 hex characters, not {s:?}")) }
}

fn project_of(p: &str) -> Result<String, String> {
    let p = p.trim();
    if p == "personal"
        || p.strip_prefix("prj_").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Ok(p.to_owned())
    } else {
        Err(format!("project: its id (prj_…) or personal, not {p:?}"))
    }
}

/// What a run runs, from its operation: the command, or the instruction.
fn what(r: &Value) -> String {
    let op = r["op"].as_str().unwrap_or_default();
    match arugula_e2e::run::Op::decode(op) {
        Some((_, o)) => match o.instruction {
            Some(i) => format!("agent: {}", one_line(&i, 60)),
            None => format!("`{}`", one_line(&o.argv.join(" "), 60)),
        },
        None => "?".into(),
    }
}

fn one_line(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max { s } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// `run_… [failed, exit 1] `just test` on hw_a`, and who started it.
fn line(r: &Value) -> String {
    let state = r["state"].as_str().unwrap_or("?");
    let exit = r["exit_code"].as_i64().map(|c| format!(", exit {c}")).unwrap_or_default();
    let by = match (r["started_by"].as_str().filter(|s| !s.is_empty()), r["started_by_agent"].as_str()) {
        (Some(a), Some(m)) => format!(", started by {a}'s agent on {m}"),
        (Some(a), None) => format!(", started by {a}"),
        _ => String::new(),
    };
    format!(
        "{} [{state}{exit}] {} on {} ({}{by})",
        r["id"].as_str().unwrap_or("?"),
        what(r),
        r["workspace"].as_str().unwrap_or("?"),
        r["trigger"].as_str().unwrap_or("?"),
    )
}

/// Its timeline, one line each.
fn timeline(r: &Value) -> String {
    r["events"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let d = &e["detail"];
            let mut s = format!("[{}] {}", e["seq"], e["to"].as_str().unwrap_or("?"));
            if let Some(p) = d["pane"].as_u64() {
                s += &format!(" in pane %{p}");
            }
            if let Some(x) = d["exit"].as_i64() {
                s += &format!(", exit {x}");
            }
            if let Some(w) = d["reason"].as_str() {
                s += &format!(": {w}");
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ended(r: &Value) -> bool {
    r["state"].as_str().and_then(arugula_e2e::run::State::parse).is_some_and(|s| s.terminal())
}

impl Call<'_> {
    /// They act as this machine's owner: only the owner's own agents may.
    async fn owners_runs(&self, what: &str) -> Result<(), String> {
        if let Some(me) = self.me()
            && self.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err(format!("{what} acts as this machine's owner: only their own agents may"));
        }
        Ok(())
    }

    async fn runs_call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.app().control.cloud(method, path, body.as_ref()).await.map_err(said)
    }

    /// `start_run`.
    pub(crate) async fn start_run(&self, a: StartRunArgs) -> Out {
        self.owners_runs(START_TOOL).await?;
        let project = project_of(&a.project)?;
        let given =
            usize::from(a.spec.is_some()) + usize::from(!a.command.is_empty()) + usize::from(a.instruction.is_some());
        if given != 1 {
            return Err("say what to run: a spec, a command, or an instruction (one of them)".into());
        }
        let body = AgentRunStart {
            spec: a.spec,
            workspace: a.on_box,
            argv: a.command,
            instruction: a.instruction,
            agent: a.recipe,
            directory: a.directory,
        };
        let body = serde_json::to_value(body).map_err(|e| e.to_string())?;
        let r = self.runs_call(Method::POST, &agent_project_runs(&project), Some(body)).await?;
        let waits = if r["grant"].is_null() {
            " The project has no standing grant, so it waits for an owner to approve it."
        } else {
            ""
        };
        let s = format!(
            "Started {}, now, as your person's agent on this machine.{waits} wait with run {} follows it; read_run reads its log.",
            line(&r),
            r["id"].as_str().unwrap_or("?")
        );
        done(s, json!({ "run": r }))
    }

    /// `read_run`.
    pub(crate) async fn read_run(&self, a: ReadRunArgs) -> Out {
        self.owners_runs(READ_TOOL).await?;
        let Some(run) = a.run.as_deref() else {
            let project = project_of(a.project.as_deref().ok_or("read_run needs a run, or a project to list")?)?;
            let mut path = agent_project_runs(&project);
            if let Some(s) = a.state.as_deref().filter(|s| !s.trim().is_empty()) {
                path += &format!(
                    "?state={}",
                    url::form_urlencoded::byte_serialize(s.trim().as_bytes()).collect::<String>()
                );
            }
            let v = self.runs_call(Method::GET, &path, None).await?;
            let runs = v["runs"].as_array().cloned().unwrap_or_default();
            let s = match runs.len() {
                0 => format!("No runs in project {project} match."),
                n => format!(
                    "{n} in project {project}, newest first:\n{}",
                    runs.iter().map(line).collect::<Vec<_>>().join("\n")
                ),
            };
            return done(s, v);
        };
        let run = run_id(run)?;
        if a.project.is_some() || a.state.is_some() {
            return Err("project and state list runs: leave them out with run".into());
        }
        let r = self.runs_call(Method::GET, &agent_run(run), None).await?;
        let max = a.max_chars.unwrap_or(PAGE).clamp(1, PAGE_MAX);
        let log = self.run_log(run, a.offset.unwrap_or(0), max).await?;
        done(format!("{}\nTimeline:\n{}\n{}", line(&r), timeline(&r), log_text(&log)), json!({ "run": r, "log": log }))
    }

    async fn run_log(&self, run: &str, offset: u64, max: usize) -> Result<Value, String> {
        self.runs_call(Method::GET, &format!("{}?offset={offset}&max={max}", agent_run_log(run)), None).await
    }

    /// `wait` with `run`: until it ends, or `limit`.
    pub(crate) async fn wait_run(&self, run: &str, limit: Duration) -> Out {
        self.owners_runs("wait with run").await?;
        let run = run_id(run)?;
        let started = Instant::now();
        let poll = async {
            let mut since = None::<u64>;
            loop {
                let path = match since {
                    None => agent_run(run),
                    Some(s) => format!("{}?since={s}&wait={RUN_WAIT_MAX}", agent_run(run)),
                };
                let r = match self.app().control.cloud(Method::GET, &path, None).await {
                    Ok(r) => r,
                    // Too many waits open: back off, then ask again.
                    Err(CloudRefused::Said { status: 429, .. }) => {
                        tokio::time::sleep(BACK_OFF).await;
                        continue;
                    }
                    Err(e) => return Err(said(e)),
                };
                if ended(&r) {
                    return Ok::<_, String>(r);
                }
                since = r["last_seq"].as_u64();
            }
        };
        let Some(r) = self.waiting(&format!("waiting for {run}"), limit, poll).await else {
            let secs = started.elapsed().as_secs();
            let r = self.runs_call(Method::GET, &agent_run(run), None).await?;
            return done(
                format!("{} hasn't ended after {secs}s: call wait with run again", line(&r)),
                json!({ "state": "still running", "run": r }),
            );
        };
        let r = r?;
        // The end of its log, as kept.
        let path = format!("{}?tail=true&max={PAGE}", agent_run_log(run));
        let log = self.runs_call(Method::GET, &path, None).await.unwrap_or_else(|e| json!({ "note": e }));
        done(format!("{} ended.\n{}", line(&r), log_text(&log)), json!({ "state": r["state"], "run": r, "log": log }))
    }

    /// Whether run tools are live here, for `capabilities`: joined, and a
    /// live run grant over a project its owner reads.
    pub(super) async fn runs_reach(&self) -> RunsReach {
        let asked = tokio::time::timeout(
            Duration::from_secs(3),
            self.app().control.cloud_now(Method::GET, AGENT_RUN_REACH, None),
        );
        let v = match asked.await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return RunsReach { live: false, projects: vec![], note: said(e) },
            Err(_) => {
                return RunsReach {
                    live: false,
                    projects: vec![],
                    note: "Arugula control didn't answer in time".into(),
                };
            }
        };
        let projects: Vec<RunsProject> = serde_json::from_value(v["projects"].clone()).unwrap_or_default();
        let live = !projects.is_empty();
        let note = if live {
            "start_run starts run specs where runs.start reaches, and commands or instructions of your own where runs.command does; read_run and wait with run read them where runs.read does. A start is labelled as your person's agent on this machine; it never approves anything.".into()
        } else {
            "This machine's owner hasn't given its agents a run grant: `arugula grants add <machine> runs.read runs.start --project <name>` (and runs.command for commands of your own), or Settings, Agents and runs, on the platform page.".into()
        };
        RunsReach { live, projects, note }
    }
}

/// A log page as text: what it holds, and where the rest is.
fn log_text(log: &Value) -> String {
    let next = log["next_offset"].as_u64().unwrap_or_default();
    if log["kept"] != true {
        return format!("Log: {}", log["note"].as_str().unwrap_or("none kept"));
    }
    let text = log["text"].as_str().unwrap_or_default();
    let more = if log["end"] == true { "the end of its log".to_owned() } else { format!("more from offset {next}") };
    format!("Log (from offset {}, {more}):\n{text}", log["offset"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_projects_are_named_by_id() {
        let id = format!("run_{}", "0a".repeat(16));
        assert_eq!(run_id(&format!(" {id} ")).unwrap(), id);
        for not in ["run_12", "12", "prj_00", ""] {
            assert!(run_id(not).is_err(), "{not}");
        }
        assert_eq!(project_of("personal").unwrap(), "personal");
        assert_eq!(project_of("prj_00ff").unwrap(), "prj_00ff");
        assert!(project_of("acme").unwrap_err().contains("prj_"));
    }

    #[test]
    fn a_run_reads_as_a_line_and_a_timeline() {
        let op =
            arugula_e2e::run::Op { argv: vec!["just".into(), "test".into()], ..Default::default() }.encode("dev_a");
        let r = json!({ "id": "run_1", "state": "failed", "exit_code": 1, "op": op, "workspace": "hw_a",
            "trigger": "manual", "started_by": "alice", "started_by_agent": "geek", "events": [
                { "seq": 1, "to": "pending", "detail": {} },
                { "seq": 2, "to": "running", "detail": { "pane": 3 } },
                { "seq": 3, "to": "failed", "detail": { "exit": 1 } }] });
        assert_eq!(line(&r), "run_1 [failed, exit 1] `just test` on hw_a (manual, started by alice's agent on geek)");
        assert_eq!(timeline(&r), "[1] pending\n[2] running in pane %3\n[3] failed, exit 1");
        assert!(ended(&r) && !ended(&json!({ "state": "running" })));
        let kept = json!({ "kept": true, "text": "ok\n", "offset": 0, "next_offset": 3, "end": true });
        assert_eq!(log_text(&kept), "Log (from offset 0, the end of its log):\nok\n");
        assert!(log_text(&json!({ "kept": false, "note": "in pane %3" })).contains("in pane %3"));
    }
}
