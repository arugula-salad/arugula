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
//! `POST /api/mcp/call` (`mcp_server`'s `call_here`), which runs the tool there as the
//! owner would, with this machine named as who started what it starts.
//! A read-only caller here stays read only there.

use std::time::Duration;

use reqwest::Method;
use rmcp::model::CallToolResult;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{
    Scope,
    tools::{Call, Out, done},
};
use crate::peer;

/// The route a driving machine sends a tool call to.
pub const CALL: &str = "/api/mcp/call";

/// How long a waking workspace's machine is waited for.
const WAKE: Duration = Duration::from_secs(90);

/// A tab of a cloud workspace: an agent block on its machine, by pane id.
pub fn tab_pane(tab: &str) -> Result<Value, String> {
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
    pub async fn on_workspace(&self, tool: &str, which: &str, args: Value) -> Out {
        if self.scope() == Scope::Read && !super::read_only_tool(tool) {
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
    pub async fn cancel_block(&self, pane: u64) -> Result<(), String> {
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
    pub async fn hosted_wait(&self, which: &str, tab: &str, until: &str, timeout: Option<f64>) -> Out {
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
