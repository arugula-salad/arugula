//! Hosted workspaces from agents (M184, #1245). A hosted workspace is a
//! machine in a project ("A hosted machine is arugulad in a rented
//! sandbox"), so an agent here reaches it as it reaches any machine:
//!
//! - **`cloud_workspace` on the machine tools.** `run`, `start_agent`,
//!   `prompt_agent`, `agent_respond`, `send_input`, `read_output`,
//!   `read_file`, `wait`, `show`, `close` and `list` take `cloud_workspace`
//!   (a hosted workspace's `hw_…` id or its name): the call is made on its
//!   machine instead of here ([`Call::on_workspace`]), and its panes and
//!   blocks are that machine's (pass it again to reach them).
//! - **`cloud` and `read_cloud`** take a hosted workspace: its tabs are
//!   agent blocks on its machine, by pane id ([`Call::hosted_read`],
//!   [`Call::hosted_drive`]); `list kind cloud_workspaces` lists them,
//!   with `kind: hosted`.
//!
//! **How.** Control's `/api/workspaces` says which machine a workspace is,
//! for this machine's owner (a `cloud.drive` grant). This daemon opens a
//! channel to it through control's relay, as it reaches another machine's
//! agents (`peer.rs`), checking the hosted machine for its project as the
//! page does; control routes it only while the grant is live, and the
//! hosted machine lets in its project's owners' and editors' machines as
//! their person (`Enrolled::drivers`). The call goes to its
//! `POST /api/mcp/call` ([`call_here`]), which runs the tool there as the
//! owner would, with this machine named as who started what it starts.
//! A read-only caller here stays read only there.

use std::{sync::Arc, time::Duration};

use axum::{Extension, Json, Router, extract::State, http::StatusCode, routing::post};
use reqwest::Method;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{
    Caller, Scope,
    tools::{Call, Out, PAGE, PAGE_MAX, done},
};
use crate::{peer, server::App};

/// The machine tools that take `workspace`.
pub(crate) const TOOLS: &[&str] = &[
    "run",
    "start_agent",
    "prompt_agent",
    "agent_respond",
    "send_input",
    "read_output",
    "read_file",
    "wait",
    "show",
    "close",
    "list",
];

/// The route a driving machine sends a tool call to.
pub const CALL: &str = "/api/mcp/call";

/// How long a waking workspace's machine is waited for.
const WAKE: Duration = Duration::from_secs(90);

const WORKSPACE_ARG: &str = "A cloud workspace (its hw_… id or its name, from list kind cloud_workspaces): \
do this on its machine instead of here, through Arugula control. Its panes and blocks are that machine's: pass \
cloud_workspace again to reach them.";

/// The argument that names one.
pub(crate) const ARG: &str = "cloud_workspace";

/// `schema` with [`ARG`] in it, for the tools in [`TOOLS`] (`wait` has it
/// already, for a Cloud workspace's tab).
pub(crate) fn with_workspace(
    tool: &str,
    schema: Arc<serde_json::Map<String, Value>>,
) -> Arc<serde_json::Map<String, Value>> {
    if !TOOLS.contains(&tool) || tool == "wait" {
        return schema;
    }
    let mut s = (*schema).clone();
    if let Some(Value::Object(props)) = s.get_mut("properties") {
        match props.get_mut(ARG) {
            // A grouped tool's kind takes one already (list kind
            // cloud_tabs): said beside it.
            Some(Value::Object(p)) => {
                let was = p.get("description").and_then(Value::as_str).unwrap_or_default().to_owned();
                p.insert(
                    "description".into(),
                    json!(format!("{was} With the other kinds: {WORKSPACE_ARG}").trim().to_owned()),
                );
            }
            _ => {
                props.insert(ARG.into(), json!({ "type": "string", "description": WORKSPACE_ARG }));
            }
        }
    }
    Arc::new(s)
}

/// The cloud workspace a machine tool's call names, taken out of its
/// arguments. A `wait` on a Cloud workspace's tab (`tab`) is `wait`'s own.
pub(crate) fn take_workspace(tool: &str, args: &mut Value) -> Option<Result<String, String>> {
    if !TOOLS.contains(&tool) {
        return None;
    }
    let o = args.as_object_mut()?;
    if tool == "wait" && o.get("tab").is_some_and(|t| !t.is_null()) {
        return None;
    }
    // The kind that names a Cloud workspace itself.
    if o.get("kind").and_then(Value::as_str) == Some("cloud_tabs") {
        return None;
    }
    match o.remove(ARG)? {
        Value::Null => None,
        Value::String(s) if !s.trim().is_empty() => Some(Ok(s.trim().to_owned())),
        v => Some(Err(format!("{ARG} is a cloud workspace's id or name, not {v}"))),
    }
}

/// A tab of a cloud workspace: an agent block on its machine, by pane id.
fn tab_pane(tab: &str) -> Result<Value, String> {
    let t = tab.trim().trim_start_matches('%');
    t.parse::<u64>().map(|n| json!(n)).map_err(|_| {
        format!("{tab:?} isn't a cloud workspace's tab: they're its machine's agent blocks, by pane number (list kind cloud_tabs)")
    })
}

/// A cloud workspace as control lists it.
#[derive(Debug, Clone, Deserialize)]
struct Workspace {
    id: String,
    name: String,
    project: String,
    #[serde(default)]
    project_name: String,
    state: String,
    #[serde(default)]
    machine: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// What `POST /api/mcp/call` takes.
#[derive(Debug, Serialize, Deserialize)]
pub struct Forwarded {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
    /// Who asked, on which machine: "started by mcp:<client>".
    #[serde(default)]
    pub client: String,
    /// The caller may only read.
    #[serde(default)]
    pub read_only: bool,
}

/// A tool's result as structured content, or its error's text.
fn out_of(r: CallToolResult) -> Out {
    if r.is_error == Some(true) {
        let text = r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect::<Vec<_>>();
        return Err(text.join("\n"));
    }
    Ok(r.structured_content.unwrap_or_default())
}

impl Call<'_> {
    async fn hosted_workspaces(&self) -> Result<Vec<Workspace>, String> {
        let v = self.app().control.cloud(Method::GET, "/api/workspaces", None).await.map_err(|e| e.to_string())?;
        serde_json::from_value(v["workspaces"].clone()).map_err(|e| format!("control's list of cloud workspaces: {e}"))
    }

    /// The cloud workspace `which` names, by id or name.
    async fn hosted_workspace(&self, which: &str) -> Result<Workspace, String> {
        let all = self.hosted_workspaces().await?;
        let found: Vec<&Workspace> = all.iter().filter(|w| w.id == which || w.name == which).collect();
        match found.as_slice() {
            [w] => Ok((*w).clone()),
            [] => Err(format!("no cloud workspace {which} (list kind cloud_workspaces lists them)")),
            _ => Err(format!("more than one cloud workspace is called {which}: name it by id")),
        }
    }

    /// The workspace's machine, as this daemon reaches it: woken first if
    /// its box is paused, and waited for while it comes up.
    async fn workspace_machine(&self, w: &Workspace) -> Result<peer::Machine, String> {
        let control = &self.app().control;
        let started = Instant::now();
        let mut woke = false;
        let mut w = w.clone();
        loop {
            if let Some(id) = &w.machine {
                let machines = peer::machines(control).await.map_err(|e| e.to_string())?;
                match machines.into_iter().find(|m| &m.id == id) {
                    Some(m) if m.online => return Ok(m),
                    Some(_) => {}
                    None => {
                        return Err(format!(
                            "{}'s machine isn't one this machine accepts: its project's delegation must be signed by an owner, and its team known here (open Control in a browser of this account once)",
                            w.name
                        ));
                    }
                }
            }
            match w.state.as_str() {
                "failed" => {
                    return Err(format!(
                        "{} failed: {} (cloud kind rebuild makes it again)",
                        w.name,
                        w.error.as_deref().unwrap_or("no reason given")
                    ));
                }
                "deleting" | "deleted" => return Err(format!("{} is being deleted", w.name)),
                // Opening a paused one asks for its wake.
                "paused" if !woke => {
                    let path = format!("/api/projects/{}/workspaces/{}", w.project, w.id);
                    control.cloud(Method::GET, &path, None).await.map_err(|e| e.to_string())?;
                    woke = true;
                }
                _ => {}
            }
            if started.elapsed() > WAKE {
                return Err(format!(
                    "{} isn't up after {}s ({}): try again in a moment",
                    w.name,
                    WAKE.as_secs(),
                    w.state
                ));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            w = self.hosted_workspace(&w.id).await?;
        }
    }

    /// `tool` with `args`, on workspace `which`'s machine.
    pub(crate) async fn on_workspace(&self, tool: &str, which: &str, args: Value) -> Out {
        if self.scope() == Scope::Read && !super::tools::read_only_tool(tool) {
            return Err(format!("this token may only read; {tool} changes things (ask for a full token)"));
        }
        let w = self.hosted_workspace(which).await?;
        let limit = Self::limit(args["timeout"].as_f64());
        let what = format!("{tool} on {}", w.name);
        let fut = async {
            let m = self.workspace_machine(&w).await?;
            let body = Forwarded {
                tool: tool.to_owned(),
                arguments: args,
                client: format!("{} via {}", self.by().trim_start_matches("mcp:"), self.machine_name()),
                read_only: self.scope() == Scope::Read,
            };
            let body = serde_json::to_value(body).map_err(|e| e.to_string())?;
            let (status, got) = peer::post_waiting(&self.app().control, &m, CALL, &body, limit)
                .await
                .map_err(|e| format!("{}'s machine: {e:#}", w.name))?;
            if status != 200 {
                let v: Value = serde_json::from_slice(&got).unwrap_or_default();
                return Err(format!(
                    "{}'s machine answered {status}: {}",
                    w.name,
                    v["error"].as_str().unwrap_or("no reason given")
                ));
            }
            let r: CallToolResult = serde_json::from_slice(&got).map_err(|e| format!("{}'s answer: {e}", w.name))?;
            out_of(r)
        };
        // Its own waits count down there; this waits a little longer, saying
        // it's still going.
        let mut v = match self.waiting(&what, limit + Duration::from_secs(30), fut).await {
            Some(r) => r?,
            None => return Err(format!("{} didn't answer {tool} in time", w.name)),
        };
        if let Some(s) = v["summary"].as_str() {
            v["summary"] = json!(format!("On {} (cloud workspace {}): {s}", w.name, w.id));
        }
        v["workspace"] = json!(w.id);
        Ok(v)
    }

    /// The name this machine has in control, for "started by".
    fn machine_name(&self) -> String {
        self.app().control.enrolled().map(|e| e.saved.cert.name.clone()).unwrap_or_else(|| "another machine".into())
    }

    /// The cloud workspaces, for `list kind cloud_workspaces`.
    pub async fn hosted_rows(&self) -> Result<Vec<Value>, String> {
        let all = self.hosted_workspaces().await?;
        let online: Vec<String> = match peer::machines(&self.app().control).await {
            Ok(ms) => ms.into_iter().filter(|m| m.online).map(|m| m.id).collect(),
            Err(_) => Vec::new(),
        };
        Ok(all
            .into_iter()
            .map(|w| {
                let up = w.machine.as_ref().is_some_and(|m| online.contains(m));
                json!({
                    "kind": "hosted", "id": w.id, "name": w.name, "project": w.project,
                    "project_name": w.project_name, "status": w.state, "error": w.error,
                    "machine": w.machine, "online": up,
                })
            })
            .collect())
    }

    // ------------------------------------------------------------ cloud tools

    /// `list kind cloud_tabs` for a cloud workspace: its machine's agent
    /// blocks.
    pub async fn hosted_tabs(&self, which: &str) -> Out {
        let v = self.on_workspace("list", which, json!({ "kind": "panes" })).await?;
        let panes = v["panes"].as_array().cloned().unwrap_or_default();
        let tabs: Vec<Value> = panes.into_iter().filter(|p| p["type"] == "agent").collect();
        let summary = match tabs.len() {
            0 => format!(
                "{which} has no agent blocks: cloud {{kind: open_tab}} (or start_agent with workspace) opens one."
            ),
            n => format!("{n} agent block(s) on {which}'s machine: their pane ids are the tabs"),
        };
        done(summary, json!({ "cloud_tabs": tabs, "panes": v["panes"] }))
    }

    /// `read_cloud` for a cloud workspace.
    pub async fn hosted_read(&self, kind: &str, which: &str, args: Value) -> Out {
        let tab = || tab_pane(args["tab"].as_str().unwrap_or_default());
        match kind {
            "events" => {
                let mut a = json!({ "pane": tab()? });
                for k in ["offset", "max_chars"] {
                    if !args[k].is_null() {
                        a[k] = args[k].clone();
                    }
                }
                self.on_workspace("read_output", which, a).await
            }
            "requests" => {
                self.on_workspace("wait", which, json!({ "pane": tab()?, "until": "needs_input", "timeout": 0 })).await
            }
            "files" | "file" | "diff" | "git_status" => {
                let mut a = args.clone();
                if let Some(o) = a.as_object_mut() {
                    o.remove("cloud_workspace");
                    o.insert("kind".into(), json!(kind));
                }
                self.on_workspace(MACHINE_READ, which, a).await
            }
            k => Err(format!("read_cloud kind {k} isn't wired up")),
        }
    }

    /// `cloud` for a cloud workspace (all but `create`, which takes a
    /// project instead).
    pub async fn hosted_drive(&self, kind: &str, which: &str, args: Value) -> Out {
        let tab = || tab_pane(args["tab"].as_str().unwrap_or_default());
        match kind {
            "open_tab" => {
                let agent = match args["agent"].as_str() {
                    None | Some("claude" | "claude_code" | "curated_coding") => "claude",
                    Some("codex") => "codex",
                    Some(a) => return Err(format!("a cloud workspace's tab runs claude or codex, not {a:?}")),
                };
                let prompt = args["prompt"].as_str().ok_or("a cloud workspace's new tab needs its first prompt")?;
                let mut a = json!({ "agent": agent, "prompt": prompt });
                for k in ["model", "cwd"] {
                    if !args[k].is_null() {
                        a[k] = args[k].clone();
                    }
                }
                let mut v = self.on_workspace("start_agent", which, a).await?;
                v["tab"] = v["pane"].clone();
                Ok(v)
            }
            "prompt" => {
                let mut a = json!({ "pane": tab()?, "text": args["text"] });
                if !args["timeout"].is_null() {
                    a["timeout"] = args["timeout"].clone();
                }
                self.on_workspace("prompt_agent", which, a).await
            }
            "interrupt" => self.on_workspace(MACHINE_READ, which, json!({ "kind": "cancel", "pane": tab()? })).await,
            "answer" => {
                let mut a = json!({ "pane": tab()? });
                match (args["option"].as_str(), args["action"].as_str()) {
                    (Some(o), None) => {
                        a["action"] =
                            json!(if o.starts_with("reject") || o.starts_with("deny") { "deny" } else { "allow" });
                        a["option"] = json!(if o.contains("always") { "always" } else { "once" });
                    }
                    (None, Some("accept")) => {
                        a["action"] = json!("answer");
                        a["answers"] = args["content"].clone();
                    }
                    (None, Some(_)) => a["action"] = json!("skip"),
                    _ => return Err("give one of option (a permission request) or action (a question)".into()),
                }
                if let Some(r) = args["request"].as_str() {
                    a["id"] = json!(r);
                }
                self.on_workspace("agent_respond", which, a).await
            }
            "close_tab" => {
                let v = self.on_workspace("close", which, json!({ "pane": tab()? })).await?;
                Ok(v)
            }
            "rebuild" => {
                let w = self.hosted_workspace(which).await?;
                let path = format!("/api/projects/{}/workspaces/{}/rebuild", w.project, w.id);
                let got = self.app().control.cloud(Method::POST, &path, None).await.map_err(|e| e.to_string())?;
                done(
                    format!("{} is being made again from its spec: its disk starts over", w.name),
                    json!({ "cloud_workspace": got }),
                )
            }
            k => Err(format!("cloud kind {k} isn't wired up")),
        }
    }

    /// `cloud kind create` with a project: a cloud workspace in it.
    pub async fn hosted_create(&self, project: &str, args: &Value) -> Out {
        let repos: Vec<Value> =
            args["repositories"].as_array().into_iter().flatten().map(|r| r["url"].clone()).collect();
        let mut packages: Vec<Value> = args["apt"].as_array().cloned().unwrap_or_default();
        packages.extend(args["npm"].as_array().cloned().unwrap_or_default());
        let mut spec = json!({
            "repos": repos, "packages": packages,
            "setup": args["setup_script"].as_str().unwrap_or_default(),
            "env": args["secrets"].as_array().cloned().unwrap_or_default(),
        });
        // Small unless asked.
        match &args["size"] {
            Value::Null => {}
            Value::String(z) if z == "small" || z == "large" => spec["size"] = json!(z),
            z => return Err(format!("size is small or large, not {z}")),
        }
        let body = json!({ "name": args["name"], "spec": spec });
        let path = format!("/api/projects/{project}/workspaces");
        let w = self.app().control.cloud(Method::POST, &path, Some(&body)).await.map_err(|e| e.to_string())?;
        let id = w["id"].as_str().unwrap_or_default();
        done(
            format!(
                "Made cloud workspace {id} ({}): its box is being made; list kind cloud_workspaces says when it's ready",
                w["name"].as_str().unwrap_or("?")
            ),
            json!({ "cloud_workspace": w }),
        )
    }

    /// Stop an agent block's turn here.
    async fn cancel_block(&self, pane: u64) -> Result<(), String> {
        let id = arugula_proto::PaneId::try_from(pane).map_err(|_| format!("no pane %{pane}"))?;
        let b = self
            .app()
            .mux
            .api(|r| crate::mux::Api::Block(id, r))
            .await
            .flatten()
            .ok_or_else(|| format!("no block %{pane}"))?;
        if b.kind() != arugula_proto::BlockType::Agent {
            return Err(format!("%{pane} isn't an agent block"));
        }
        b.call("cancel", json!({})).await.map(|_| ())
    }

    /// `wait` on a cloud workspace's tab.
    pub(crate) async fn hosted_wait(&self, which: &str, tab: &str, until: &str, timeout: Option<f64>) -> Out {
        let mut a = json!({ "pane": tab_pane(tab)?, "until": until });
        if let Some(t) = timeout {
            a["timeout"] = json!(t);
        }
        self.on_workspace("wait", which, a).await
    }
}

// ---------------------------------------------------------------- this machine's side

/// What only a driving machine's call reaches (not an MCP tool here): a
/// cloud workspace's files, diff and git status for `read_cloud`, and
/// stopping an agent block's turn for `cloud kind interrupt`.
pub const MACHINE_READ: &str = "machine_read";

/// `POST /api/mcp/call`: a tool call another machine makes for an agent of
/// its, on this one (M184: a cloud workspace's machine, driven by one of
/// its project's people's machines). Only the owner here: the machine got
/// in as its person, an owner or an editor of the workspace.
pub fn routes() -> Router<Arc<App>> {
    Router::new().route(CALL, post(call_here))
}

async fn call_here(
    State(app): State<Arc<App>>,
    who: Option<Extension<crate::acl::Principal>>,
    Json(f): Json<Forwarded>,
) -> Result<Json<CallToolResult>, (StatusCode, Json<Value>)> {
    if who.is_some_and(|Extension(p)| !p.is_owner()) {
        return Err((StatusCode::FORBIDDEN, Json(json!({ "error": "only the owner can do that" }))));
    }
    let scope = if f.read_only { Scope::Read } else { Scope::Full };
    let client = if f.client.trim().is_empty() { "another machine".to_owned() } else { f.client.clone() };
    let caller = Caller { scope, token: Some(format!("machine:{client}")), pane: None };
    tracing::info!(tool = %f.tool, client, ?scope, "mcp call from another machine");
    let call = Call::detached(&app, caller, client);
    let r = match f.tool.as_str() {
        MACHINE_READ => match machine_read(&call, f.arguments).await {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        },
        tool => call.dispatch(tool, f.arguments).await,
    };
    Ok(Json(r))
}

#[derive(Deserialize)]
struct ReadArgs {
    kind: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    staged: bool,
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    max_chars: Option<usize>,
    #[serde(default)]
    pane: Option<u64>,
}

/// At most `max` bytes of `s`, cut on a char boundary, and whether it was.
fn cut(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_owned(), false);
    }
    let mut at = max;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    (s[..at].to_owned(), true)
}

/// This machine's home.
fn home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| "/".into())
}

/// The repository a git read looks in: `path`, else the home if it's one,
/// else the first repository under it (where setup cloned the spec's).
fn repository(path: Option<&str>) -> Result<std::path::PathBuf, String> {
    repository_in(&home(), path)
}

fn repository_in(home: &std::path::Path, path: Option<&str>) -> Result<std::path::PathBuf, String> {
    if let Some(p) = path.filter(|p| !p.trim().is_empty()) {
        return Ok(p.into());
    }
    if home.join(".git").exists() {
        return Ok(home.to_owned());
    }
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(home)
        .map_err(|e| format!("{}: {e}", home.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join(".git").exists())
        .collect();
    dirs.sort();
    dirs.into_iter().next().ok_or_else(|| format!("no git repository in {}: give path", home.display()))
}

/// git, with nothing a repository's own config could make it run.
async fn git(dir: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null", "--no-pager"])
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .await
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn machine_read(call: &Call<'_>, args: Value) -> Out {
    let a: ReadArgs = serde_json::from_value(args).map_err(|e| format!("bad arguments: {e}"))?;
    if a.kind == "cancel" {
        if call.scope() == Scope::Read {
            return Err("this token may only read".into());
        }
        let pane = a.pane.ok_or("cancel needs the agent block's pane")?;
        call.cancel_block(pane).await?;
        return done(format!("Interrupted %{pane}'s turn."), json!({ "pane": pane }));
    }
    read_here(a).await
}

/// A read of this machine's files and repositories.
async fn read_here(a: ReadArgs) -> Out {
    let max = a.max_chars.unwrap_or(PAGE).clamp(1, PAGE_MAX);
    match a.kind.as_str() {
        "diff" => {
            let dir = repository(a.path.as_deref())?;
            let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color", "-M"];
            if a.staged {
                args.push("--staged");
            }
            if let Some(r) = a.git_ref.as_deref() {
                if r.starts_with('-') {
                    return Err(format!("{r:?} isn't a commit, branch or tag"));
                }
                args.push(r);
            }
            args.push("--");
            let root = git(&dir, &["rev-parse", "--show-toplevel"]).await?.trim().to_owned();
            let (diff, truncated) = cut(&git(&dir, &args).await?, max);
            let summary = match diff.is_empty() {
                true => format!("No changes in {root}."),
                false => format!("The diff of {root}{}", if truncated { " (cut short)" } else { "" }),
            };
            done(summary, json!({ "repo_root": root, "diff": diff, "truncated": truncated }))
        }
        "git_status" => {
            let dir = repository(a.path.as_deref())?;
            let root = git(&dir, &["rev-parse", "--show-toplevel"]).await?.trim().to_owned();
            let out = git(&dir, &["status", "--porcelain=v1", "-b", "--untracked-files=all"]).await?;
            let mut lines = out.lines();
            let branch = lines.next().and_then(|l| l.strip_prefix("## ")).map(|b| b.split("...").next().unwrap_or(b));
            let entries: Vec<Value> = lines
                .take(1000)
                .filter(|l| l.len() > 3)
                .map(|l| json!({ "status": l[..2].trim(), "path": &l[3..] }))
                .collect();
            let summary = format!("{root} on {}: {} changed or untracked", branch.unwrap_or("?"), entries.len());
            done(summary, json!({ "repo_root": root, "branch": branch, "entries": entries }))
        }
        "files" => {
            let dir = a.path.filter(|p| !p.trim().is_empty()).map(std::path::PathBuf::from).unwrap_or_else(home);
            let mut entries: Vec<Value> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .filter_map(|e| e.ok())
                .take(1000)
                .map(|e| {
                    let m = e.metadata().ok();
                    let kind = match &m {
                        Some(m) if m.is_dir() => "dir",
                        Some(m) if m.is_file() => "file",
                        _ => "other",
                    };
                    json!({ "name": e.file_name().to_string_lossy(), "type": kind, "size": m.map(|m| m.len()) })
                })
                .collect();
            entries.sort_by(|x, y| x["name"].as_str().cmp(&y["name"].as_str()));
            let n = entries.len();
            done(
                format!("{}: {n} entr{}", dir.display(), if n == 1 { "y" } else { "ies" }),
                json!({ "path": dir, "entries": entries }),
            )
        }
        "file" => {
            let path = a.path.ok_or("file needs its path")?;
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            if bytes.iter().take(8000).any(|b| *b == 0) {
                return Err(format!("{path} isn't text"));
            }
            let size = bytes.len();
            let (content, truncated) = cut(&String::from_utf8_lossy(&bytes), max);
            let summary = format!("{path}: {size} bytes{}", if truncated { ", the start of it" } else { "" });
            done(summary, json!({ "path": path, "size": size, "content": content, "truncated": truncated }))
        }
        k => Err(format!("no machine read {k}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_goes_on_the_machine_tools_only() {
        let mut args = json!({ "command": "ls", "cloud_workspace": " dev " });
        assert_eq!(take_workspace("run", &mut args), Some(Ok("dev".into())));
        assert_eq!(args, json!({ "command": "ls" }), "taken out");
        assert_eq!(take_workspace("run", &mut json!({ "command": "ls" })), None);
        assert_eq!(take_workspace("run", &mut json!({ "cloud_workspace": null })), None);
        assert!(matches!(take_workspace("run", &mut json!({ "cloud_workspace": 3 })), Some(Err(_))));
        let mut other = json!({ "cloud_workspace": "dev" });
        assert_eq!(take_workspace("cloud", &mut other), None, "not a machine tool");
        assert_eq!(other["cloud_workspace"], "dev");
        // wait: a pane on a hosted one's machine, by id or name; a tab is
        // wait's own.
        let mut w = json!({ "cloud_workspace": "hw_ab", "pane": 3, "until": "idle" });
        assert_eq!(take_workspace("wait", &mut w), Some(Ok("hw_ab".into())));
        let mut w = json!({ "cloud_workspace": "dev", "pane": 3, "until": "idle" });
        assert_eq!(take_workspace("wait", &mut w), Some(Ok("dev".into())));
        assert_eq!(take_workspace("wait", &mut json!({ "cloud_workspace": "hw_ab", "tab": "3" })), None);
        // list kind cloud_tabs names one itself.
        assert_eq!(take_workspace("list", &mut json!({ "kind": "cloud_tabs", "cloud_workspace": "hw_ab" })), None);
        assert!(take_workspace("list", &mut json!({ "kind": "panes", "cloud_workspace": "hw_ab" })).is_some());

        let schema = Arc::new(json!({ "type": "object", "properties": { "pane": {} } }).as_object().unwrap().clone());
        assert!(with_workspace("read_output", schema.clone())["properties"]["cloud_workspace"].is_object());
        assert!(with_workspace("cloud", schema)["properties"].get("cloud_workspace").is_none());

        assert_eq!(tab_pane("%7").unwrap(), json!(7));
        assert!(tab_pane("cwc_ab").is_err());
    }

    /// The reads a driving machine makes for read_cloud: a repository's
    /// diff and status (the first one under home without a path), a
    /// directory, a file; what isn't text, isn't.
    // Hosted boxes are Linux: its paths and shell.
    #[cfg(unix)]
    #[tokio::test]
    async fn reads_here() {
        let dir = std::env::temp_dir().join(format!("arugula-hosted-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let sh = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "first"]);
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        std::fs::write(repo.join("new.txt"), "new\n").unwrap();
        std::fs::write(repo.join("bin"), b"\0\x01").unwrap();
        let read = |kind: &str, path: Option<&std::path::Path>| ReadArgs {
            kind: kind.into(),
            path: path.map(|p| p.display().to_string()),
            staged: false,
            git_ref: None,
            max_chars: None,
            pane: None,
        };
        let repo_root = repo.canonicalize().unwrap().display().to_string();
        let v = read_here(read("diff", Some(&repo))).await.unwrap();
        assert!(v["diff"].as_str().unwrap().contains("+two"), "{v}");
        assert_eq!(v["repo_root"], repo_root);
        let v = read_here(read("git_status", Some(&repo))).await.unwrap();
        assert_eq!(v["branch"], "main");
        let paths: Vec<&str> = v["entries"].as_array().unwrap().iter().filter_map(|e| e["path"].as_str()).collect();
        assert!(paths.contains(&"a.txt") && paths.contains(&"new.txt"), "{v}");
        let v = read_here(read("files", Some(&repo))).await.unwrap();
        assert!(v["entries"].as_array().unwrap().iter().any(|e| e["name"] == "a.txt" && e["type"] == "file"));
        let v = read_here(read("file", Some(&repo.join("a.txt")))).await.unwrap();
        assert_eq!(v["content"], "two\n");
        assert!(read_here(read("file", Some(&repo.join("bin")))).await.unwrap_err().contains("isn't text"));
        let mut bad = read("diff", Some(&repo));
        bad.git_ref = Some("--output=/tmp/x".into());
        assert!(read_here(bad).await.is_err(), "no option for a ref");
        // Without a path: the first repository under home.
        assert_eq!(repository_in(&dir, None).unwrap(), repo);
        assert!(repository_in(&repo.join("nothing"), None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
