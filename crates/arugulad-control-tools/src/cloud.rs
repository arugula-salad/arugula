//! Cloud workspaces from agents (#1094): `list` kinds `cloud_workspaces`
//! and `cloud_tabs`, `read_cloud` (a tab's transcript and requests, the
//! machine's files, diff and git status), `cloud` (create one, open,
//! prompt, interrupt, answer and close tabs, rebuild) and `wait` on a tab.
//!
//! A Cloud workspace is a hosted workspace (`hw_…`, M184): a machine of a
//! project, `arugulad` in a rented sandbox. Each tool here goes to its
//! machine through control (`hosted.rs`): its tabs are the agent blocks
//! on it, by pane number. Control says which machine a workspace is, as
//! this machine's owner ([`Control::cloud`](crate::control::Control::cloud)).
//! Whoever may run things here (a teammate with run rights, a guest's
//! agent) drives the owner's Cloud workspaces through it. That was decided
//! (Jake, 2026-10-19), and docs/control.md says so. The Managoat-backed
//! ones (`cw_…`) were removed (M189, M190).

use std::time::Duration;

use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::tools::{Call, Kind, NoArgs, Out, done};

// ---------------------------------------------------------------- arguments
//
// Each is read from the call's arguments as they came (`hosted_read`,
// `hosted_drive`, `hosted_create`): but for the workspace, these types
// are their schemas.

#[derive(Deserialize, JsonSchema)]
pub struct CloudWorkspaceArgs {
    /// The Cloud workspace: its hw_… id or its name (list kind
    /// cloud_workspaces).
    pub cloud_workspace: String,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudTabArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// The tab: an agent block on its machine, by pane number (list kind
    /// cloud_tabs).
    pub tab: String,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudEventsArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// The tab: an agent block on its machine, by pane number.
    pub tab: String,
    /// Where to start (a previous result's next_offset). Without it: the
    /// latest.
    #[serde(default)]
    pub offset: Option<u64>,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudPathArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// A path on its machine. Default: for git_status, the first
    /// repository in its home; for files, its home.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudFileArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// The file, an absolute path on its machine.
    pub path: String,
    /// At most this many characters (default 16000, at most 40000).
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudDiffArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// A path in a repository on its machine (default the first
    /// repository in its home).
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
#[allow(dead_code)]
pub struct CloudRepository {
    /// Its HTTPS clone URL, with no credentials in it.
    pub url: String,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudCreateArgs {
    /// Its name.
    pub name: String,
    /// The project it's a machine of: its prj_… id, or personal.
    pub project: String,
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
    /// The names of the secrets its box takes as env vars (set on
    /// Control).
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Its machine's size: small (2 vCPU, 2 GiB, the default) or large (8
    /// vCPU, 8 GiB, for big builds such as Rust's).
    #[serde(default)]
    pub size: Option<CloudSize>,
}

/// A Cloud workspace's size (the platform's, M266).
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum CloudSize {
    Small,
    Large,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudOpenTabArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// The agent: claude (default) or codex.
    #[serde(default)]
    pub agent: Option<String>,
    /// Its model, if not the agent's default.
    #[serde(default)]
    pub model: Option<String>,
    /// Where it works, a directory on its machine (default its home).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Its first prompt.
    pub prompt: String,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudPromptArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// The tab: an agent block on its machine, by pane number.
    pub tab: String,
    /// The prompt.
    pub text: String,
    /// Seconds to wait for the turn (default 100): then call wait with
    /// the tab, until idle.
    #[serde(default)]
    pub timeout: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct CloudAnswerArgs {
    /// The Cloud workspace: its hw_… id or its name.
    pub cloud_workspace: String,
    /// The tab: an agent block on its machine, by pane number.
    pub tab: String,
    /// The request's id (read_cloud kind requests).
    #[serde(default)]
    pub request: Option<String>,
    /// A permission request: the option to pick (allow_once,
    /// allow_always, reject_once, …).
    #[serde(default)]
    pub option: Option<String>,
    /// A question: accept, decline or cancel.
    #[serde(default)]
    pub action: Option<String>,
    /// With accept: the question's answers.
    #[serde(default)]
    pub content: Option<Value>,
}

// ---------------------------------------------------------------- kinds

/// `list`'s Cloud workspace kinds.
pub(crate) const LIST_WORKSPACES: Kind = Kind {
    name: "cloud_workspaces",
    description: "The Cloud workspaces of the user's projects in Arugula control (machines in the cloud with their repositories checked out, each with tabs of agents): each one's id, name, project, state (starting, running, paused, failed) and machine. They're the machine owner's: control acts as them for any agent here.",
    schema: schema_for_type::<NoArgs>,
    flag: None,
};

pub(crate) const LIST_TABS: Kind = Kind {
    name: "cloud_tabs",
    description: "A Cloud workspace's tabs: the agent blocks on its machine, by pane number, with their status.",
    schema: schema_for_type::<CloudWorkspaceArgs>,
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
        description: "What a tab's agent waits on someone for: a permission request with its options, or a question with its fields. cloud {kind: answer} answers it.",
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
        description: "A text file on a Cloud workspace's machine.",
        schema: schema_for_type::<CloudFileArgs>,
        flag: None,
    },
    Kind {
        name: "diff",
        description: "git diff of the repository holding path on a Cloud workspace's machine (staged, or against ref). Without path: the first repository in its home.",
        schema: schema_for_type::<CloudDiffArgs>,
        flag: None,
    },
    Kind {
        name: "git_status",
        description: "git status of the repository holding path on a Cloud workspace's machine: its branch and changed and untracked files. Without path: the first repository in its home.",
        schema: schema_for_type::<CloudPathArgs>,
        flag: None,
    },
];

/// `cloud`'s jobs.
pub(crate) const DRIVE: &[Kind] = &[
    Kind {
        name: "create",
        description: "A new Cloud workspace in a project: a machine of a size (small, or large for big builds) with these repositories checked out, packages installed and setup script run, which the project's people see too. Its box is made and sets itself up in a pane (list kind cloud_workspaces says when it's running). Returns its id.",
        schema: schema_for_type::<CloudCreateArgs>,
        flag: None,
    },
    Kind {
        name: "open_tab",
        description: "A new tab in a Cloud workspace: an agent block on its machine (claude or codex) with its first prompt. Returns its pane number, the tab; wait with the tab until idle or needs_input.",
        schema: schema_for_type::<CloudOpenTabArgs>,
        flag: None,
    },
    Kind {
        name: "prompt",
        description: "Give a tab's agent a prompt and wait for its turn: returns when it ends (with the reply), when it asks for someone (answer it), or still running after timeout. A paused machine wakes for it.",
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
        description: "Answer what a tab's agent asks (read_cloud kind requests): a permission request with option, a question with action (and content, its answers, to accept).",
        schema: schema_for_type::<CloudAnswerArgs>,
        flag: None,
    },
    Kind {
        name: "close_tab",
        description: "Close a tab: its agent block ends. The other tabs and the machine carry on.",
        schema: schema_for_type::<CloudTabArgs>,
        flag: None,
    },
    Kind {
        name: "rebuild",
        description: "Make a Cloud workspace's machine again from its spec as saved: its disk starts over.",
        schema: schema_for_type::<CloudWorkspaceArgs>,
        flag: None,
    },
];

// ---------------------------------------------------------------- calls

/// Whether agents here can drive the owner's Cloud workspaces, for
/// `capabilities`: control is joined and has them, and says so in time.
pub(super) async fn cloud_reach(app: &crate::server::App) -> crate::mcp::results::CloudReach {
    use crate::mcp::results::CloudReach;
    let asked =
        tokio::time::timeout(Duration::from_secs(3), app.control.cloud_now(Method::GET, "/api/workspaces", None));
    match asked.await {
        Ok(Ok(_)) => CloudReach {
            reachable: true,
            note: "You can drive the user's Cloud workspaces as them: list kind cloud_workspaces, read_cloud, cloud, and wait with a tab".into(),
        },
        Ok(Err(e)) => CloudReach { reachable: false, note: e.to_string() },
        Err(_) => CloudReach { reachable: false, note: "Arugula control didn't answer in time".into() },
    }
}

pub(crate) async fn list_cloud_workspaces(c: &Call<'_>) -> Out {
    let rows = c.hosted_rows().await?;
    let summary = match rows.len() {
        0 => "No Cloud workspaces yet: cloud {kind: create} with a project makes one.".to_owned(),
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

pub(crate) async fn list_cloud_tabs(c: &Call<'_>, a: CloudWorkspaceArgs) -> Out {
    c.hosted_tabs(a.cloud_workspace.trim()).await
}

pub(crate) async fn read_cloud(c: &Call<'_>, kind: &str, args: Value) -> Out {
    let a: CloudWorkspaceArgs = crate::mcp::tools::parse(args.clone())?;
    c.hosted_read(kind, a.cloud_workspace.trim(), args).await
}

pub(crate) async fn drive_cloud(c: &Call<'_>, kind: &str, args: Value) -> Out {
    if kind == "create" {
        let project = args["project"].as_str().map(str::trim).filter(|p| !p.is_empty());
        let project =
            project.ok_or("a Cloud workspace is a machine of a project: give project (its prj_… id, or personal)")?;
        return c.hosted_create(project, &args).await;
    }
    let a: CloudWorkspaceArgs = crate::mcp::tools::parse(args.clone())?;
    c.hosted_drive(kind, a.cloud_workspace.trim(), args).await
}
