//! Cloud workspaces from agents (#1094): `list` kinds `cloud_workspaces`,
//! `cloud_tabs` and `cloud_agents`, `read_cloud` (a tab's transcript and
//! requests, the machine's files, diff, git status and setup log), `cloud`
//! (create one, open, prompt, interrupt, answer and close tabs, rebuild or
//! retry) and `wait` on a tab.
//!
//! They live in control, not on this machine: each call is control's
//! `/api/cloud/...` route, signed with this machine's key, so control acts
//! as the machine's owner ([`Control::cloud`](crate::control::Control::cloud)),
//! which waits out a maintenance window (M141) for up to five minutes.
//! Whoever may run things here (a teammate with run rights, a guest's
//! agent) drives the owner's Cloud workspaces through it. That was decided
//! (Jake, 2026-10-19), and docs/control.md says so.

use std::time::Duration;

use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::Instant;

use arugula_proto::{BlockType, api::OpenRequest};

use super::tools::{Call, Kind, NoArgs, Out, done};

/// How often a wait on a tab asks control: control reads a busy tab from
/// Managoat every 2s, an idle one every 5s.
const POLL: Duration = Duration::from_secs(2);
/// The most events one read pages through to find a transcript's end.
const END_PAGES: usize = 50;

// ---------------------------------------------------------------- arguments

#[derive(Deserialize, JsonSchema)]
pub struct CloudWorkspaceArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudTabArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// The tab's id (cwc_…, from list kind cloud_tabs).
    pub tab: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudEventsArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// The tab's id (cwc_…, from list kind cloud_tabs).
    pub tab: String,
    /// Where to start (a previous result's next_offset, or prompt's offset).
    /// Without it: from the start.
    #[serde(default)]
    pub offset: Option<u64>,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudPathArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// A path on its machine. Default: for git_status, its first
    /// repository's mount; for files, its home (/home/sprite).
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudFileArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// The file, an absolute path on its machine.
    pub path: String,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudDiffArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// A path in a repository on its machine (default its first
    /// repository's mount).
    #[serde(default)]
    pub path: Option<String>,
    /// Only what's staged.
    #[serde(default)]
    pub staged: bool,
    /// Against this commit, branch or tag instead of HEAD.
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudRepository {
    /// Its HTTPS clone URL, with no credentials in it.
    pub url: String,
    /// A branch, tag or commit (default the default branch).
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    /// Where it's checked out (default /home/sprite/<its name>).
    #[serde(default)]
    pub mount: Option<String>,
    /// The name of one of the user's cloud secrets to clone it with (a
    /// private repository's token).
    #[serde(default)]
    pub secret: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudCreateArgs {
    /// Its name.
    pub name: String,
    /// The repositories to check out on its machine.
    #[serde(default)]
    pub repositories: Vec<CloudRepository>,
    /// apt packages to install.
    #[serde(default)]
    pub apt: Vec<String>,
    /// npm packages to install.
    #[serde(default)]
    pub npm: Vec<String>,
    /// A shell script its setup runs after the packages and repositories.
    #[serde(default)]
    pub setup_script: Option<String>,
    /// Environment variables its agents see. Not for secrets: name the
    /// user's cloud secrets in secrets instead.
    #[serde(default)]
    pub env_vars: std::collections::BTreeMap<String, String>,
    /// The names of the user's cloud secrets its agents get, besides the
    /// repositories' own.
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudOpenTabArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// The agent, by id (list kind cloud_agents; default curated_coding).
    #[serde(default)]
    pub agent: Option<String>,
    /// The model, as list kind cloud_agents gives it (default the first).
    #[serde(default)]
    pub model: Option<String>,
    /// Where it works: a repository's mount or a directory under one, or
    /// under /home/sprite (default the first repository's).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Its first prompt. One sent while the Cloud workspace sets up waits
    /// until setup ends.
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudPromptArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// The tab's id (cwc_…, from list kind cloud_tabs).
    pub tab: String,
    /// The prompt.
    pub text: String,
    /// Seconds to wait for the turn (default 100): then call wait with
    /// the tab, until idle, after this result's offset.
    #[serde(default)]
    pub timeout: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudAnswerArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// The tab's id (cwc_…, from list kind cloud_tabs).
    pub tab: String,
    /// The request's id (read_cloud kind requests).
    pub request: String,
    /// A permission request: the option to pick, by its optionId.
    #[serde(default)]
    pub option: Option<String>,
    /// A question (an elicitation): accept, decline or cancel.
    #[serde(default)]
    pub action: Option<String>,
    /// With accept: the question's fields, by name.
    #[serde(default)]
    pub content: Option<serde_json::Map<String, Value>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloudRebuildArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// End the tabs' running turns rather than be refused while one runs.
    #[serde(default)]
    pub interrupt: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ShowCloudArgs {
    /// The Cloud workspace's id (cw_…, from list kind cloud_workspaces).
    pub cloud_workspace: String,
    /// One of its tabs to open it on (cwc_…, from list kind cloud_tabs).
    #[serde(default)]
    pub tab: Option<String>,
    /// Beside this pane. Default: the caller's own pane (an agent block, or the terminal pane `arugula mcp` runs in).
    #[serde(default)]
    pub beside: Option<super::tools::PaneArg>,
}

// ---------------------------------------------------------------- kinds

/// `show`'s Cloud workspace block.
pub(crate) const SHOW: Kind = Kind {
    name: "cloud_workspace",
    description: "One of the user's Cloud workspaces (list kind cloud_workspaces) as a block: its status and tabs, a tab's transcript with what it asks, a prompt box, and the changes on its machine, kept up to date from Arugula control while the user looks. With tab: open on that tab. The user can prompt, answer and interrupt its tabs there. Returns its status and tabs.",
    schema: schema_for_type::<ShowCloudArgs>,
    flag: None,
};

/// `list`'s Cloud workspace kinds.
pub(crate) const LIST_WORKSPACES: Kind = Kind {
    name: "cloud_workspaces",
    description: "The user's Cloud workspaces in Arugula control (machines in the cloud with their repositories checked out, each with tabs of agents): each one's id, name, status (provisioning while it sets up, with its step; ready; failed), whether a rebuild would change it, and its link on control's page. They're the machine owner's: control acts as them for any agent here.",
    schema: schema_for_type::<NoArgs>,
    flag: None,
};

pub(crate) const LIST_TABS: Kind = Kind {
    name: "cloud_tabs",
    description: "A Cloud workspace's tabs, each an agent's conversation on its machine: id, agent, model, cwd and status (opening, running, idle, needs_input, failed, closing).",
    schema: schema_for_type::<CloudWorkspaceArgs>,
    flag: None,
};

pub(crate) const LIST_AGENTS: Kind = Kind {
    name: "cloud_agents",
    description: "What a Cloud workspace's tab can run: the agents (curated ones and the user's own, by id) and the models, the first the default.",
    schema: schema_for_type::<NoArgs>,
    flag: None,
};

/// `read_cloud`'s reads.
pub(crate) const READ: &[Kind] = &[
    Kind {
        name: "events",
        description: "A tab's transcript as text: prompts, the agent's messages, its tool calls and their results, and what it asked. Paged: pass next_offset back as offset.",
        schema: schema_for_type::<CloudEventsArgs>,
        flag: None,
    },
    Kind {
        name: "requests",
        description: "What a tab's agent waits on someone for: each permission request with its options, and each question with its fields. cloud {kind: answer} answers one.",
        schema: schema_for_type::<CloudTabArgs>,
        flag: None,
    },
    Kind {
        name: "files",
        description: "A directory on a Cloud workspace's machine: its entries, with type and size.",
        schema: schema_for_type::<CloudPathArgs>,
        flag: None,
    },
    Kind {
        name: "file",
        description: "A text file on a Cloud workspace's machine, with secrets redacted.",
        schema: schema_for_type::<CloudFileArgs>,
        flag: None,
    },
    Kind {
        name: "diff",
        description: "git diff of the repository holding path on a Cloud workspace's machine (staged, or against ref), with secrets redacted. Without path: its first repository (the summary names the others).",
        schema: schema_for_type::<CloudDiffArgs>,
        flag: None,
    },
    Kind {
        name: "git_status",
        description: "git status of the repository holding path on a Cloud workspace's machine: its branch and changed and untracked files. Without path: its first repository (the summary names the others).",
        schema: schema_for_type::<CloudPathArgs>,
        flag: None,
    },
    Kind {
        name: "setup_log",
        description: "What a Cloud workspace's last setup printed, step by step (packages, repositories, setup script): why one failed. Reads while the machine is failed or parked.",
        schema: schema_for_type::<CloudWorkspaceArgs>,
        flag: None,
    },
];

/// `cloud`'s jobs.
pub(crate) const DRIVE: &[Kind] = &[
    Kind {
        name: "create",
        description: "A new Cloud workspace: a machine with these repositories checked out, packages installed and setup script run. It sets up for a few minutes (list kind cloud_workspaces says its step; read_cloud kind setup_log what it printed); a tab opened meanwhile starts once it's ready. Returns its id.",
        schema: schema_for_type::<CloudCreateArgs>,
        flag: None,
    },
    Kind {
        name: "open_tab",
        description: "A new tab in a Cloud workspace: an agent's conversation on its machine, with a first prompt if given. Returns the tab's id; wait with the tab until idle or needs_input.",
        schema: schema_for_type::<CloudOpenTabArgs>,
        flag: None,
    },
    Kind {
        name: "prompt",
        description: "Give a tab's agent a prompt and wait for its turn: returns when it ends (done, with the reply), when it asks for someone (needs_input, with its requests: answer one), or still running after timeout, with the offset to wait after. A parked machine wakes for it; a tab whose conversation failed starts a new one with it.",
        schema: schema_for_type::<CloudPromptArgs>,
        flag: None,
    },
    Kind {
        name: "interrupt",
        description: "Stop the turn a tab's agent is in. The tab stays open.",
        schema: schema_for_type::<CloudTabArgs>,
        flag: None,
    },
    Kind {
        name: "answer",
        description: "Answer what a tab's agent asks (read_cloud kind requests): a permission request with option, a question with action (and content, its fields, to accept).",
        schema: schema_for_type::<CloudAnswerArgs>,
        flag: None,
    },
    Kind {
        name: "close_tab",
        description: "Close a tab: its conversation and agent end; its transcript is kept. The other tabs and the machine carry on.",
        schema: schema_for_type::<CloudTabArgs>,
        flag: None,
    },
    Kind {
        name: "rebuild",
        description: "Replace a Cloud workspace's machine with one set up from its settings as saved: its disk is wiped; its tabs keep their conversations. Refused while a tab's turn runs, unless interrupt.",
        schema: schema_for_type::<CloudRebuildArgs>,
        flag: None,
    },
    Kind {
        name: "retry",
        description: "Set up a Cloud workspace whose setup failed again (read_cloud kind setup_log says why it failed).",
        schema: schema_for_type::<CloudWorkspaceArgs>,
        flag: None,
    },
];

// ---------------------------------------------------------------- calls

use crate::cloud_workspace::id;

fn workspace_path(workspace: &str) -> Result<String, String> {
    Ok(format!("/api/cloud/workspaces/{}", id("cloud_workspace", "cw_", workspace)?))
}

fn tab_path(workspace: &str, tab: &str) -> Result<String, String> {
    Ok(format!("{}/tabs/{}", workspace_path(workspace)?, id("cloud_tab", "cwc_", tab)?))
}

fn query(pairs: &[(&str, Option<String>)]) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        if let Some(v) = v {
            q.append_pair(k, v);
        }
    }
    let q = q.finish();
    if q.is_empty() { q } else { format!("?{q}") }
}

fn key() -> String {
    format!("mcp-{}", hex::encode(arugula_e2e::random::<12>()))
}

fn max_chars(m: Option<usize>) -> usize {
    m.unwrap_or(super::tools::PAGE).clamp(1, super::tools::PAGE_MAX)
}

/// At most the first `max` bytes of `s`, cut on a char boundary, and
/// whether it was cut.
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

/// A tab's status, in a few words.
fn tab_said(t: &Value) -> String {
    let status = t["status"].as_str().unwrap_or("?");
    match t["error"].as_str() {
        Some(e) => format!("{status} ({e})"),
        None => status.to_owned(),
    }
}

/// Events as text, a line or a run of text each, from `events` until `max`
/// characters: the text, and the id of the last event in it.
pub(crate) fn transcript(events: &[Value], max: usize) -> (String, Option<u64>, bool) {
    let mut out = String::new();
    let mut last = None;
    let mut prev = "";
    for e in events {
        let mut piece = String::new();
        if e["state"] == "failed" {
            let data: Value = e["data"].as_str().and_then(|d| serde_json::from_str(d).ok()).unwrap_or_default();
            let why = data["reason"].as_str().or(data["error"].as_str()).unwrap_or("it failed");
            piece.push_str(&format!("\n[failed: {why}]\n"));
            prev = "";
        }
        for b in e["blocks"].as_array().into_iter().flatten() {
            let kind = b["kind"].as_str().unwrap_or("");
            let body = match &b["body"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                v => v.to_string(),
            };
            let short = |s: &str, n: usize| {
                let (c, more) = cut(s, n);
                if more { format!("{c}…") } else { c }
            };
            let line = match kind {
                "init" | "raw" | "thinking" | "thought" => continue,
                "text" if prev == "text" => {
                    piece.push_str(&body);
                    continue;
                }
                "text" => format!("\n{body}"),
                "prompt" => format!("\n\n> {}\n", body.trim()),
                "tool_use" => format!(
                    "\n[{}] {}\n",
                    b["name"].as_str().unwrap_or("tool"),
                    short(b["summary"].as_str().unwrap_or(&body), 300)
                ),
                "tool_result" => format!("[result] {}\n", short(b["summary"].as_str().unwrap_or(&body), 600)),
                "permission_request" | "elicitation_request" => format!(
                    "\n[asks, request {}] {}\n",
                    b["request_id"].as_str().unwrap_or("?"),
                    short(b["summary"].as_str().unwrap_or(&body), 600)
                ),
                other => format!("\n[{other}] {}\n", short(b["summary"].as_str().unwrap_or(&body), 300)),
            };
            prev = kind;
            piece.push_str(&line);
        }
        if !out.is_empty() && out.len() + piece.len() > max {
            return (out, last, true);
        }
        let (piece, cut_short) = cut(&piece, max);
        out.push_str(&piece);
        last = e["id"].as_u64().or(last);
        if cut_short {
            return (out, last, true);
        }
    }
    (out, last, false)
}

/// Where a Cloud workspace's repositories are on its machine, in order: as
/// it was built (a repository saved since isn't on its disk until a
/// rebuild), else as saved. One without a mount is at /home/sprite/<name>.
fn mounts(w: &Value) -> Vec<String> {
    let spec = if w["built_spec"].is_object() { &w["built_spec"] } else { &w["saved_spec"] };
    spec["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| match r["mount"].as_str().filter(|m| !m.is_empty()) {
            Some(m) => Some(m.to_owned()),
            None => {
                let url = r["url"].as_str()?.trim_end_matches('/');
                let name = url.rsplit('/').next()?.trim_end_matches(".git");
                (!name.is_empty()).then(|| format!("/home/sprite/{name}"))
            }
        })
        .collect()
}

/// What a summary adds when git_status or diff read the first repository
/// for want of a path, and there are others.
fn others_note(others: &[String]) -> String {
    match others {
        [] => String::new(),
        _ => format!(" (its first repository; also {}: pass path for those)", others.join(", ")),
    }
}

/// What a wait on a tab waits for.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TabUntil {
    /// Its turn ended (or it asks for someone: it can't go on alone).
    Idle,
    /// It asks for someone.
    NeedsInput,
}

impl Call<'_> {
    /// Control's Cloud workspace route, as this machine's owner.
    async fn cloud_call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.app().control.cloud(method, path, body.as_ref()).await.map_err(|e| e.to_string())
    }

    async fn cloud_get(&self, path: &str) -> Result<Value, String> {
        self.cloud_call(Method::GET, path, None).await
    }

    /// Control's page for a Cloud workspace (or one of its tabs).
    pub(super) fn cloud_link(&self, workspace: &str, tab: Option<&str>) -> Option<String> {
        crate::cloud_workspace::Remote::link(&*self.app().control, workspace, tab)
    }

    /// Whether agents here can drive the owner's Cloud workspaces, for
    /// `capabilities`: control is joined and has them, and says so in time.
    pub(super) async fn cloud_reach(&self) -> super::results::CloudReach {
        use super::results::CloudReach;
        let asked = tokio::time::timeout(
            Duration::from_secs(3),
            self.app().control.cloud_now(Method::GET, "/api/cloud/models", None),
        );
        match asked.await {
            Ok(Ok(_)) => CloudReach {
                reachable: true,
                note: "You can drive the user's Cloud workspaces as them: list kind cloud_workspaces, read_cloud, cloud, and wait with a tab".into(),
            },
            Ok(Err(e)) => CloudReach { reachable: false, note: e.to_string() },
            Err(_) => CloudReach { reachable: false, note: "Arugula control didn't answer in time".into() },
        }
    }

    // ------------------------------------------------------------ list

    pub(crate) async fn list_cloud_workspaces(&self) -> Out {
        let v = self.cloud_get("/api/cloud/workspaces").await?;
        let all = v["cloud_workspaces"].as_array().cloned().unwrap_or_default();
        let rows: Vec<Value> = all
            .iter()
            .map(|w| {
                let id = w["id"].as_str().unwrap_or_default();
                let repositories: Vec<Value> = w["saved_spec"]["repositories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|r| json!({ "url": r["url"], "mount": r["mount"] }))
                    .collect();
                json!({
                    "id": id, "name": w["name"], "status": w["status"], "setup_step": w["setup_step"],
                    "error": w["error"], "stale": w["stale"], "rebuild_queued": w["rebuild_queued"],
                    "repositories": repositories, "link": self.cloud_link(id, None),
                })
            })
            .collect();
        let summary = match rows.len() {
            0 => "No Cloud workspaces yet: cloud {kind: create} makes one.".to_owned(),
            n => format!(
                "{n} Cloud workspace(s): {}",
                rows.iter()
                    .map(|w| format!(
                        "{} {} ({})",
                        w["id"].as_str().unwrap_or("?"),
                        w["name"].as_str().unwrap_or("?"),
                        w["status"].as_str().unwrap_or("?")
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        done(summary, json!({ "cloud_workspaces": rows }))
    }

    pub(crate) async fn list_cloud_tabs(&self, a: CloudWorkspaceArgs) -> Out {
        let v = self.cloud_get(&format!("{}/tabs", workspace_path(&a.cloud_workspace)?)).await?;
        let tabs = v["cloud_tabs"].as_array().cloned().unwrap_or_default();
        let summary = match tabs.len() {
            0 => format!("{} has no tabs: cloud {{kind: open_tab}} opens one.", a.cloud_workspace),
            n => format!(
                "{n} tab(s) in {}: {}",
                a.cloud_workspace,
                tabs.iter()
                    .map(|t| format!(
                        "{} {} ({})",
                        t["id"].as_str().unwrap_or("?"),
                        t["agent_name"].as_str().unwrap_or("?"),
                        tab_said(t)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        done(summary, json!({ "cloud_tabs": tabs }))
    }

    pub(crate) async fn list_cloud_agents(&self) -> Out {
        let agents = self.cloud_get("/api/cloud/agents").await?;
        let models = self.cloud_get("/api/cloud/models").await?;
        let agents: Vec<Value> = agents["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                json!({
                    "id": a["id"], "name": a["definition"]["name"], "curated": a["curated"],
                    "secret_names": a["definition"]["secret_names"],
                })
            })
            .collect();
        let models = models["models"].as_array().cloned().unwrap_or_default();
        let summary = format!(
            "{} agent(s): {}; models: {}",
            agents.len(),
            agents.iter().map(|a| a["id"].as_str().unwrap_or("?")).collect::<Vec<_>>().join(", "),
            models.iter().map(|m| m["model"].as_str().unwrap_or("?")).collect::<Vec<_>>().join(", "),
        );
        done(summary, json!({ "agents": agents, "models": models }))
    }

    // ------------------------------------------------------------ show

    /// `show` kind `cloud_workspace`: the block, once control says the
    /// workspace (and tab) are there.
    pub(crate) async fn show_cloud_workspace(&self, a: ShowCloudArgs) -> Out {
        let workspace = id("cloud_workspace", "cw_", &a.cloud_workspace)?;
        let tab = a.tab.as_deref().map(|t| id("cloud_tab", "cwc_", t)).transpose()?;
        // Asked first, so a workspace that isn't there (or a control that
        // can't be reached) opens nothing.
        let w = self.cloud_get(&workspace_path(&workspace)?).await?;
        let tabs = self.cloud_get(&format!("{}/tabs", workspace_path(&workspace)?)).await?;
        let tabs = tabs["cloud_tabs"].as_array().cloned().unwrap_or_default();
        if let Some(t) = &tab
            && !tabs.iter().any(|x| x["id"] == t.as_str())
        {
            return Err(format!("{workspace} has no open tab {t} (list kind cloud_tabs lists them)"));
        }
        let (beside, _) = self.beside(a.beside.as_ref()).await?;
        let req = OpenRequest {
            kind: BlockType::CloudWorkspace,
            config: json!({ "cloud_workspace": workspace, "tab": tab }),
            split: beside,
            from_pane: beside,
            local: true,
            ..Default::default()
        };
        let block = self.open(req).await?;
        let name = w["name"].as_str().unwrap_or("?");
        let on = tab.as_deref().map(|t| format!(" on tab {t}")).unwrap_or_default();
        done(
            format!(
                "Cloud workspace {name} ({workspace}, {}) in block %{block}{on}: {} tab(s)",
                w["status"].as_str().unwrap_or("?"),
                tabs.len()
            ),
            json!({
                "block": block, "cloud_workspace": workspace, "name": name, "status": w["status"],
                "tab": tab, "tabs": tabs, "link": self.cloud_link(&workspace, tab.as_deref()),
            }),
        )
    }

    // ------------------------------------------------------------ read

    /// The path git_status and diff read: the one given, else the
    /// workspace's first repository, and the other repositories then. With
    /// no path, control looks in the machine's home, which is no repository.
    async fn repository_path(&self, workspace: &str, path: Option<String>) -> Result<(String, Vec<String>), String> {
        if let Some(p) = path.filter(|p| !p.trim().is_empty()) {
            return Ok((p, vec![]));
        }
        let w = self.cloud_get(&workspace_path(workspace)?).await?;
        let mut all = mounts(&w).into_iter();
        match all.next() {
            Some(first) => Ok((first, all.collect())),
            None => Err(format!(
                "{workspace} has no repositories: give path, a directory in a git repository on its machine"
            )),
        }
    }

    pub(crate) async fn read_cloud(&self, kind: &str, args: Value) -> Out {
        use super::tools::parse;
        match kind {
            "events" => {
                let a: CloudEventsArgs = parse(args)?;
                let after = a.offset.unwrap_or(0);
                let v =
                    self.cloud_get(&format!("{}/events?after={after}", tab_path(&a.cloud_workspace, &a.tab)?)).await?;
                let events = v["events"].as_array().cloned().unwrap_or_default();
                let (text, last, cut_short) = transcript(&events, max_chars(a.max_chars));
                let more = cut_short || events.len() >= 1000;
                let next = last.unwrap_or(after);
                let summary = match (text.is_empty(), more) {
                    (true, _) => format!("Nothing in {} after {after} yet.", a.tab),
                    (false, true) => {
                        format!("{} chars of {}'s transcript; more after offset {next}", text.len(), a.tab)
                    }
                    (false, false) => {
                        format!("{} chars of {}'s transcript, to its end (offset {next})", text.len(), a.tab)
                    }
                };
                done(summary, json!({ "text": text, "next_offset": next, "more": more }))
            }
            "requests" => {
                let a: CloudTabArgs = parse(args)?;
                let v = self.cloud_get(&format!("{}/requests", tab_path(&a.cloud_workspace, &a.tab)?)).await?;
                let requests = v["requests"].as_array().cloned().unwrap_or_default();
                let summary = match requests.len() {
                    0 => format!("{} asks nothing now.", a.tab),
                    n => format!("{} asks {n} thing(s): cloud {{kind: answer}} answers one.", a.tab),
                };
                done(summary, json!({ "requests": requests }))
            }
            "files" | "git_status" => {
                let a: CloudPathArgs = parse(args)?;
                let route = if kind == "files" { "files" } else { "git-status" };
                let (at, others) = match kind {
                    "files" => (a.path, vec![]),
                    _ => {
                        let (p, others) = self.repository_path(&a.cloud_workspace, a.path).await?;
                        (Some(p), others)
                    }
                };
                let path = format!("{}/{route}{}", workspace_path(&a.cloud_workspace)?, query(&[("path", at)]));
                let v = self.cloud_get(&path).await?;
                let n = v["entries"].as_array().map_or(0, Vec::len);
                let summary = match kind {
                    "files" => {
                        format!("{}: {n} entr{}", v["path"].as_str().unwrap_or("?"), if n == 1 { "y" } else { "ies" })
                    }
                    _ => format!(
                        "{} on {}: {n} changed or untracked{}",
                        v["repo_root"].as_str().unwrap_or("?"),
                        v["branch"].as_str().unwrap_or("a detached HEAD"),
                        others_note(&others)
                    ),
                };
                done(summary, v)
            }
            "file" => {
                let a: CloudFileArgs = parse(args)?;
                let max = max_chars(a.max_chars);
                let path = format!(
                    "{}/file{}",
                    workspace_path(&a.cloud_workspace)?,
                    query(&[("path", Some(a.path)), ("max_bytes", Some(max.to_string()))])
                );
                let mut v = self.cloud_get(&path).await?;
                let (text, cut_short) = cut(v["content"].as_str().unwrap_or_default(), max);
                let truncated = cut_short || v["truncated"] == true;
                v["content"] = json!(text);
                v["truncated"] = json!(truncated);
                let summary = format!(
                    "{}: {} bytes{}",
                    v["path"].as_str().unwrap_or("?"),
                    v["size"],
                    if truncated { ", the start of it" } else { "" }
                );
                done(summary, v)
            }
            "diff" => {
                let a: CloudDiffArgs = parse(args)?;
                let max = max_chars(a.max_chars);
                let (at, others) = self.repository_path(&a.cloud_workspace, a.path).await?;
                let path = format!(
                    "{}/diff{}",
                    workspace_path(&a.cloud_workspace)?,
                    query(&[
                        ("path", Some(at)),
                        ("staged", a.staged.then(|| "true".to_owned())),
                        ("ref", a.git_ref),
                        ("max_bytes", Some(max.to_string())),
                    ])
                );
                let mut v = self.cloud_get(&path).await?;
                let (text, cut_short) = cut(v["diff"].as_str().unwrap_or_default(), max);
                let truncated = cut_short || v["truncated"] == true;
                v["diff"] = json!(text);
                v["truncated"] = json!(truncated);
                let summary = match text.is_empty() {
                    true => format!(
                        "No changes in {}{}.",
                        v["repo_root"].as_str().unwrap_or("its repository"),
                        others_note(&others)
                    ),
                    false => format!(
                        "The diff of {}{}{}",
                        v["repo_root"].as_str().unwrap_or("its repository"),
                        if truncated { " (cut short)" } else { "" },
                        others_note(&others)
                    ),
                };
                done(summary, v)
            }
            "setup_log" => {
                let a: CloudWorkspaceArgs = parse(args)?;
                let v = self.cloud_get(&format!("{}/setup-log", workspace_path(&a.cloud_workspace)?)).await?;
                let log = &v["log"];
                let summary = match log["status"].as_str() {
                    None => format!("{} hasn't started a setup yet.", a.cloud_workspace),
                    Some(s) => format!(
                        "{}'s last setup: {s}{}",
                        a.cloud_workspace,
                        log["step"].as_str().map(|st| format!(" (at {st})")).unwrap_or_default()
                    ),
                };
                done(summary, v)
            }
            k => Err(format!("read_cloud kind {k} isn't wired up")),
        }
    }

    // ------------------------------------------------------------ drive

    pub(crate) async fn drive_cloud(&self, kind: &str, args: Value) -> Out {
        use super::tools::parse;
        match kind {
            "create" => {
                let a: CloudCreateArgs = parse(args)?;
                let repositories: Vec<Value> = a
                    .repositories
                    .into_iter()
                    .map(|r| {
                        let mut o = json!({ "url": r.url });
                        if let Some(v) = r.git_ref {
                            o["ref"] = json!(v);
                        }
                        if let Some(v) = r.mount {
                            o["mount"] = json!(v);
                        }
                        if let Some(v) = r.secret {
                            o["secret"] = json!(v);
                        }
                        o
                    })
                    .collect();
                let spec = json!({
                    "name": a.name, "repositories": repositories,
                    "packages": { "apt": a.apt, "npm": a.npm },
                    "setup_script": a.setup_script.unwrap_or_default(),
                    "env_vars": a.env_vars, "secrets": a.secrets,
                });
                let body = json!({ "request_key": key(), "spec": spec });
                let w = self.cloud_call(Method::POST, "/api/cloud/workspaces", Some(body)).await?;
                let id = w["id"].as_str().unwrap_or_default().to_owned();
                done(
                    format!(
                        "Made Cloud workspace {id} ({}); it's setting up: list kind cloud_workspaces says its step, read_cloud kind setup_log what it printed",
                        w["name"].as_str().unwrap_or("?")
                    ),
                    json!({ "cloud_workspace": w, "link": self.cloud_link(&id, None) }),
                )
            }
            "open_tab" => {
                let a: CloudOpenTabArgs = parse(args)?;
                let base = workspace_path(&a.cloud_workspace)?;
                let models = self.cloud_get("/api/cloud/models").await?;
                let models = models["models"].as_array().cloned().unwrap_or_default();
                let model = match &a.model {
                    None => models.first(),
                    Some(m) => models.iter().find(|x| x["model"] == m.as_str()),
                };
                let Some(model) = model else {
                    let offered: Vec<&str> = models.iter().filter_map(|m| m["model"].as_str()).collect();
                    return Err(format!(
                        "no model {}: the ones offered are {}",
                        a.model.as_deref().unwrap_or("at all"),
                        offered.join(", ")
                    ));
                };
                let mut body = json!({
                    "request_key": key(),
                    "agent_id": a.agent.as_deref().unwrap_or("curated_coding"),
                    "runtime": model["runtime"], "model": model["model"],
                });
                if let Some(cwd) = a.cwd {
                    body["cwd"] = json!(cwd);
                }
                if let Some(p) = a.prompt {
                    body["prompt"] = json!(p);
                }
                let t = self.cloud_call(Method::POST, &format!("{base}/tabs"), Some(body)).await?;
                let tab = t["id"].as_str().unwrap_or_default().to_owned();
                done(
                    format!(
                        "Opened tab {tab} in {} ({}, {}): wait with cloud_workspace and tab until idle or needs_input",
                        a.cloud_workspace,
                        t["agent_name"].as_str().unwrap_or("?"),
                        tab_said(&t)
                    ),
                    json!({ "cloud_tab": t, "link": self.cloud_link(&a.cloud_workspace, Some(&tab)) }),
                )
            }
            "prompt" => {
                let a: CloudPromptArgs = parse(args)?;
                let path = tab_path(&a.cloud_workspace, &a.tab)?;
                let offset = self.end_of_transcript(&path).await?;
                let sent =
                    self.cloud_call(Method::POST, &format!("{path}/prompt"), Some(json!({ "prompt": a.text }))).await?;
                let relaunched = sent["relaunched"] == true;
                let limit = Self::limit(a.timeout);
                let mut out = self.wait_tab(&a.cloud_workspace, &a.tab, TabUntil::Idle, Some(offset), limit).await?;
                out["offset"] = json!(offset);
                if relaunched {
                    out["relaunched"] = json!(true);
                    if let Some(s) = out["summary"].as_str() {
                        out["summary"] = json!(format!("Its conversation had failed, so it started a new one. {s}"));
                    }
                }
                Ok(out)
            }
            "interrupt" => {
                let a: CloudTabArgs = parse(args)?;
                self.cloud_call(Method::POST, &format!("{}/interrupt", tab_path(&a.cloud_workspace, &a.tab)?), None)
                    .await?;
                done(format!("Interrupted {}'s turn.", a.tab), json!({}))
            }
            "answer" => {
                let a: CloudAnswerArgs = parse(args)?;
                let body =
                    match (a.option, a.action) {
                        (Some(o), None) if a.content.is_none() => json!({ "option_id": o }),
                        (None, Some(action)) => json!({ "action": action, "content": a.content }),
                        _ => return Err(
                            "give one of option (a permission request) or action (a question, with content to accept)"
                                .into(),
                        ),
                    };
                let path = format!(
                    "{}/requests/{}",
                    tab_path(&a.cloud_workspace, &a.tab)?,
                    url::form_urlencoded::byte_serialize(a.request.as_bytes()).collect::<String>()
                );
                self.cloud_call(Method::POST, &path, Some(body)).await?;
                done(
                    format!(
                        "Answered {} in {}: wait with the tab until idle for the rest of its turn.",
                        a.request, a.tab
                    ),
                    json!({}),
                )
            }
            "close_tab" => {
                let a: CloudTabArgs = parse(args)?;
                let t = self.cloud_call(Method::DELETE, &tab_path(&a.cloud_workspace, &a.tab)?, None).await?;
                done(format!("Closing {}: its transcript is kept.", a.tab), json!({ "cloud_tab": t }))
            }
            "rebuild" | "retry" => {
                let (workspace, body) = if kind == "rebuild" {
                    let a: CloudRebuildArgs = parse(args)?;
                    (a.cloud_workspace, Some(json!({ "interrupt": a.interrupt })))
                } else {
                    let a: CloudWorkspaceArgs = parse(args)?;
                    (a.cloud_workspace, None)
                };
                let w = self.cloud_call(Method::POST, &format!("{}/{kind}", workspace_path(&workspace)?), body).await?;
                let summary = match w["rebuild_queued"] == true {
                    true => format!("Managoat has no machine free: the {kind} of {workspace} waits for one"),
                    false => format!("{workspace} is setting up again ({kind})"),
                };
                done(summary, json!({ "cloud_workspace": w }))
            }
            k => Err(format!("cloud kind {k} isn't wired up")),
        }
    }

    /// The id of a tab's last event now (0 with none), paging through.
    async fn end_of_transcript(&self, tab_path: &str) -> Result<u64, String> {
        let mut after = 0;
        for _ in 0..END_PAGES {
            let v = self.cloud_get(&format!("{tab_path}/events?after={after}")).await?;
            let events = v["events"].as_array().cloned().unwrap_or_default();
            let last = events.iter().filter_map(|e| e["id"].as_u64()).max();
            match last {
                Some(l) if l > after => after = l,
                _ => return Ok(after),
            }
            if events.len() < 1000 {
                return Ok(after);
            }
        }
        Ok(after)
    }

    /// Wait until a tab's turn ends or it asks for someone. With `after`
    /// (the transcript's end when it was prompted), an idle tab counts only
    /// once it was seen busy or its transcript went past `after`: control
    /// reads an idle tab every few seconds, so right after a prompt it may
    /// still say idle.
    pub(crate) async fn wait_tab(
        &self,
        workspace: &str,
        tab: &str,
        until: TabUntil,
        after: Option<u64>,
        limit: Duration,
    ) -> Out {
        let path = tab_path(workspace, tab)?;
        let what = match until {
            TabUntil::Idle => format!("waiting for {tab} to finish"),
            TabUntil::NeedsInput => format!("waiting for {tab} to ask"),
        };
        let poll = async {
            let mut busy = false;
            loop {
                let t = self.cloud_get(&path).await?;
                let status = t["status"].as_str().unwrap_or_default().to_owned();
                match status.as_str() {
                    "needs_input" => return Ok::<_, String>(t),
                    "failed" | "terminated" | "closing" => return Ok(t),
                    "idle" if until == TabUntil::Idle => {
                        let moved = match after {
                            None => true,
                            Some(a) => {
                                busy || {
                                    let v = self.cloud_get(&format!("{path}/events?after={a}")).await?;
                                    v["events"].as_array().is_some_and(|e| !e.is_empty())
                                }
                            }
                        };
                        if moved {
                            return Ok(t);
                        }
                    }
                    "idle" => {}
                    _ => busy = true,
                }
                tokio::time::sleep(POLL).await;
            }
        };
        let started = Instant::now();
        let Some(t) = self.waiting(&what, limit, poll).await else {
            let secs = started.elapsed().as_secs();
            let label = if until == TabUntil::Idle { "end of its turn" } else { "question" };
            return done(
                format!(
                    "{tab}: no {label} yet after {secs}s: call wait again with cloud_workspace and tab (and after, if you have it)"
                ),
                json!({ "state": "still running", "tab": tab, "after": after }),
            );
        };
        let t = t?;
        let status = t["status"].as_str().unwrap_or_default();
        let mut out = json!({ "state": status, "tab": t });
        // What it said, from where it was prompted (or its last page).
        let from = after.unwrap_or(0);
        let reply = self.cloud_get(&format!("{path}/events?after={from}")).await?;
        let events = reply["events"].as_array().cloned().unwrap_or_default();
        let (text, last, more) = transcript(&events, super::tools::PAGE);
        out["reply"] = json!(text);
        out["next_offset"] = json!(last.unwrap_or(from));
        if more {
            out["more"] = json!(true);
        }
        let summary = match status {
            "needs_input" => {
                let r = self.cloud_get(&format!("{path}/requests")).await?;
                out["requests"] = r["requests"].clone();
                format!("{tab} asks for someone: read requests, then cloud {{kind: answer}}")
            }
            "idle" => format!("{tab} finished its turn"),
            "closing" => format!("{tab} is closing"),
            _ => format!("{tab} ended: {}", tab_said(&t)),
        };
        done(summary, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_controls_and_nothing_else_reaches_its_path() {
        assert_eq!(tab_path("cw_ab12", " cwc_ff ").unwrap(), "/api/cloud/workspaces/cw_ab12/tabs/cwc_ff");
        for bad in ["cw_", "cw_../x", "cwc_ab", "cw_ab/tabs", "cw_zz", "ab"] {
            assert!(workspace_path(bad).is_err(), "{bad}");
        }
        assert!(tab_path("cw_ab", "cwc_ab?x=1").is_err());
        assert_eq!(query(&[("path", Some("/home/sprite/a b".into())), ("ref", None)]), "?path=%2Fhome%2Fsprite%2Fa+b");
        assert_eq!(query(&[("path", None)]), "");
    }

    /// #1117's live check: without a path, git_status and diff read the
    /// first repository, not the machine's home (no repository there).
    #[test]
    fn git_reads_default_to_the_first_repository() {
        let w = json!({
            "saved_spec": { "repositories": [{ "url": "https://github.com/a/new.git", "mount": "/home/sprite/new" }] },
            "built_spec": { "repositories": [
                { "url": "https://github.com/jhgaylor/jhgaylor.github.io", "mount": "/home/sprite/jhgaylor.github.io" },
                { "url": "https://github.com/a/tools.git/" },
            ] },
        });
        assert_eq!(mounts(&w), ["/home/sprite/jhgaylor.github.io", "/home/sprite/tools"]);
        // Not built yet: as saved.
        assert_eq!(mounts(&json!({ "saved_spec": w["saved_spec"], "built_spec": null })), ["/home/sprite/new"]);
        assert!(mounts(&json!({ "saved_spec": { "repositories": [] } })).is_empty());
        assert_eq!(others_note(&[]), "");
        assert!(others_note(&["/home/sprite/tools".into()]).contains("also /home/sprite/tools: pass path"));
    }

    #[test]
    fn a_transcript_reads_as_text_and_pages_by_event() {
        let events = vec![
            json!({"id": 1, "blocks": [{"kind": "init"}, {"kind": "prompt", "body": "fix the build"}]}),
            json!({"id": 2, "blocks": [{"kind": "text", "body": "Look"}, {"kind": "text", "body": "ing."}]}),
            json!({"id": 3, "blocks": [{"kind": "thinking", "body": "hmm"}, {"kind": "tool_use", "name": "Bash", "summary": "cargo build"}]}),
            json!({"id": 4, "blocks": [{"kind": "tool_result", "body": "ok"}, {"kind": "permission_request", "request_id": "r1", "summary": "Edit src/main.rs"}]}),
            json!({"id": 5, "state": "failed", "data": "{\"reason\":\"setup failed\"}"}),
        ];
        let (text, last, more) = transcript(&events, 10_000);
        assert_eq!((last, more), (Some(5), false));
        assert!(text.contains("> fix the build\n\nLooking."), "{text}");
        assert!(text.contains("[Bash] cargo build") && !text.contains("hmm"), "{text}");
        assert!(text.contains("[result] ok") && text.contains("[asks, request r1] Edit src/main.rs"), "{text}");
        assert!(text.contains("[failed: setup failed]"), "{text}");
        // A page ends on an event: the next starts after it.
        let (page, last, more) = transcript(&events, 30);
        assert!(more && page.len() <= 30, "{page:?}");
        let (rest, _, _) = transcript(&events[last.unwrap() as usize..], 10_000);
        assert!(!rest.contains("fix the build") && rest.contains("[result] ok"), "{rest}");
    }
}
