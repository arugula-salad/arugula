//! A block started on an issue is on that issue's row (#372): `start_agent`'s
//! `issue` is kept on the block ([`Work`], in its config, so it survives a
//! restart), and, when this machine holds `boards.write` over the issue's
//! project, control attaches the block to that issue's row on every board of
//! the project ([`attach`]). Without the grant the block keeps its issue and
//! `start_agent` says the row wasn't attached and which grant would do it.
//!
//! The boards the block is on are kept with it ([`WorkBoard`]: the board's
//! title, from control's answer, and the row), for the chip in its header.
//! Only with the `boards` flag on here, and only when this machine has
//! joined control.

use arugula_control_wire::boards::{
    AttachBlock, BlockAttached, CLAIMED, CheckClaim, ClaimChecked, IssueRef, agent_project_attach_block,
    agent_project_claim_check,
};
use arugula_control_wire::issues::{agent_issue, agent_project_issue};
use std::sync::Arc;

use arugula_proto::PaneId;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{control::CloudRefused, mux::Api, server::App};

/// The issue a block was started on, as its starter wrote it (`#N`,
/// `owner/repo#N` or `IDENT-N`), and the boards it is on. Kept in the
/// block's config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub issue: String,
    /// The project the issue is in (Arugula #771): the agent's current
    /// project, for its MCP calls (`mcp::issues`) and, as [`PROJECT_ENV`],
    /// the `arugula` it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boards: Vec<WorkBoard>,
}

/// The current project of `arugula issue` and `arugula open`, set in an
/// agent's environment from its [`Work`]'s project.
pub const PROJECT_ENV: &str = "ARUGULA_PROJECT";

/// A board with a row the block is attached to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkBoard {
    pub project: String,
    pub board: String,
    pub title: String,
    pub row: String,
}

/// What a config opening an agent block says of its issue: nothing, or the
/// issue as written and whether to `take` the row. An issue that isn't one is
/// the opener's mistake.
pub fn issue_of(config: &serde_json::Value) -> Result<Option<(String, bool)>, String> {
    let Some(w) = config.get("work").filter(|w| !w.is_null()) else { return Ok(None) };
    let issue = w["issue"].as_str().unwrap_or_default().trim();
    match IssueRef::parse(issue) {
        Some(_) => Ok(Some((issue.to_owned(), w["take"].as_bool().unwrap_or(false)))),
        None => Err(format!("work.issue {issue:?}: an issue is #N, owner/repo#N or IDENT-N")),
    }
}

/// Why a block wasn't attached to its issue's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotKept {
    /// Another machine's block holds the row (Arugula #404), and `take` wasn't
    /// set: what control said, naming the holder.
    Claimed(String),
    /// Anything else: boards off, not joined, no grant, control down.
    Why(String),
}

impl std::fmt::Display for NotKept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (NotKept::Claimed(s) | NotKept::Why(s)) = self;
        f.write_str(s)
    }
}

impl From<&str> for NotKept {
    fn from(s: &str) -> Self {
        NotKept::Why(s.to_owned())
    }
}

impl From<String> for NotKept {
    fn from(s: String) -> Self {
        NotKept::Why(s)
    }
}

/// Attach `pane`, which was started on `issue`, and keep the boards it is on
/// with the block (for its chip). `home`: the project of whoever started it,
/// when known. `take`: take a row another machine's block holds, folding that
/// block under `earlier`. The boards (none when no board has a row for the
/// issue), or why it wasn't tried or was refused. `session`: the starter's
/// name, which holds the row from now (Arugula #765).
pub async fn keep(
    app: &App,
    home: Option<String>,
    issue: &str,
    pane: PaneId,
    take: bool,
    session: Option<String>,
) -> Result<Vec<WorkBoard>, NotKept> {
    let boards = attach(app, home, issue, pane, take, session).await?;
    if !boards.is_empty()
        && let Some(b) = app.mux.api(|r| Api::Block(pane, r)).await.flatten()
    {
        b.call("set_work", serde_json::json!({ "boards": boards })).await?;
    }
    Ok(boards)
}

/// [`keep`] for an issue found from a branch's name: the block is on it only
/// if a board has a row for it, so a block with none keeps no issue, and one
/// refused (a row another machine's block holds, no grant) says nothing. The
/// boards, none when it isn't on one.
pub async fn keep_inferred(app: &App, home: Option<String>, issue: &str, pane: PaneId) -> Vec<WorkBoard> {
    let boards = match attach(app, home, issue, pane, false, None).await {
        Ok(b) => b,
        Err(why) => {
            debug!(pane, issue, "a branch's issue isn't attached: {why}");
            return vec![];
        }
    };
    if !boards.is_empty()
        && let Some(b) = app.mux.api(|r| Api::Block(pane, r)).await.flatten()
        && let Err(why) = b.call("set_work", serde_json::json!({ "issue": issue, "boards": boards })).await
    {
        debug!(pane, issue, "a branch's issue isn't kept: {why}");
        return vec![];
    }
    boards
}

/// The project that holds `issue` (#430), and the issue as `#N` in it. `#N`
/// is looked up in each project this machine's boards grants reach; if more
/// than one has it, the one linked to the agent's repo (`cwd`) wins, else it
/// is refused, naming them. `iss_…` names its project. `None` for the other
/// forms (`owner/repo#N`, `IDENT-N`) and when no project has it: the caller
/// falls back to the repo's project, then its own.
pub async fn issue_project(app: &App, issue: &str, cwd: Option<&str>) -> Result<Option<(String, String)>, String> {
    if issue.starts_with("iss_") {
        let v = app
            .control
            .cloud(Method::GET, &agent_issue(issue), None)
            .await
            .map_err(|e| format!("issue {issue}: {e}"))?;
        return match (v["project"].as_str(), v["number"].as_u64()) {
            (Some(p), Some(n)) => Ok(Some((p.to_owned(), format!("#{n}")))),
            _ => Err(format!("issue {issue}: Arugula control named no project or number")),
        };
    }
    let Some(IssueRef::Number(n)) = IssueRef::parse(issue) else { return Ok(None) };
    let reach = crate::mcp::registry::boards_reach(app).await;
    let mut holders = vec![];
    for p in &reach.projects {
        if app.control.cloud(Method::GET, &agent_project_issue(&p.id, n), None).await.is_ok() {
            holders.push(p);
        }
    }
    match holders.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some((only.id.clone(), format!("#{n}")))),
        many => match many.iter().find(|p| Some(p.id.as_str()) == cwd) {
            Some(p) => Ok(Some((p.id.clone(), format!("#{n}")))),
            None => {
                let names: Vec<_> = many.iter().map(|p| format!("{} ({})", p.name, p.id)).collect();
                Err(format!(
                    "#{n} is an issue of several projects ({}): pass its iss_ id, or start the agent in the repo of the one you mean",
                    names.join(", ")
                ))
            }
        },
    }
}

/// [`keep`], in the background, for a block opened through the API
/// (`arugula agent --issue`): nobody waits for its answer.
pub fn keep_later(app: Arc<App>, issue: String, take: bool, pane: PaneId) {
    crate::task::spawn(async move {
        match keep(&app, None, &issue, pane, take, None).await {
            Ok(boards) => debug!(pane, issue, boards = boards.len(), "a block is on its issue's rows"),
            Err(why) => debug!(pane, issue, "a block started on an issue isn't on a row: {why}"),
        }
    });
}

/// Before a pane is opened on `issue`: who holds its row, if another machine's
/// block does (Arugula #404) or another session does, on this machine or
/// another (Arugula #765; `session` is the asker's name, whose own hold is no
/// claim). A machine that can't ask (not
/// joined, no grant, control down) is told nothing: the attach says why the
/// row isn't taken, as it always did, and refuses a race with its 409.
pub async fn held(app: &App, home: Option<String>, issue: &str, session: Option<String>) -> Option<String> {
    let enrolled = app.control.enrolled()?;
    let project = write_project(app, home).await.ok()?;
    let body =
        serde_json::to_value(CheckClaim { issue: issue.to_owned(), machine: enrolled.keys.id(), session }).ok()?;
    let v = app.control.cloud(Method::POST, &agent_project_claim_check(&project.id), Some(&body)).await.ok()?;
    serde_json::from_value::<ClaimChecked>(v).ok()?.held
}

async fn attach(
    app: &App,
    home: Option<String>,
    issue: &str,
    pane: PaneId,
    take: bool,
    session: Option<String>,
) -> Result<Vec<WorkBoard>, NotKept> {
    let Some(enrolled) = app.control.enrolled() else {
        return Err("this machine hasn't joined Arugula control, so the row wasn't attached".into());
    };
    let project = write_project(app, home).await?;
    let body = serde_json::to_value(AttachBlock {
        issue: issue.to_owned(),
        machine: enrolled.keys.id(),
        pane: pane.to_string(),
        take,
        session,
    })
    .map_err(|e| e.to_string())?;
    let v =
        app.control.cloud(Method::POST, &agent_project_attach_block(&project.id), Some(&body)).await.map_err(|e| {
            match e {
                CloudRefused::Said { status: 403, .. } => NotKept::Why(no_grant(&project.name)),
                CloudRefused::Said { status: 409, said, reason } if reason.as_deref() == Some(CLAIMED) => {
                    NotKept::Claimed(said)
                }
                e => NotKept::Why(format!("the row wasn't attached: {e}")),
            }
        })?;
    let attached: BlockAttached =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent an answer I can't read: {e}"))?;
    Ok(attached
        .on
        .into_iter()
        .map(|o| WorkBoard { project: project.id.clone(), board: o.board, title: o.title, row: o.row })
        .collect())
}

fn no_grant(project: &str) -> String {
    format!(
        "the row wasn't attached: this machine has no boards.write grant over {project}; its owner gives one with `arugula grants add <machine> boards.write --project {project}`"
    )
}

struct Project {
    id: String,
    name: String,
}

/// The project whose boards this block is attached in: the starter's own (the
/// one its clone is linked to, else the personal one) when this machine may
/// write its boards, else the only project it may.
async fn write_project(app: &App, home: Option<String>) -> Result<Project, String> {
    let reach = crate::mcp::registry::boards_reach(app).await;
    let writable: Vec<_> = reach.projects.iter().filter(|p| p.powers.iter().any(|w| w == "boards.write")).collect();
    let named = |id: &str, name: &str| Project { id: id.to_owned(), name: name.to_owned() };
    match (home.ok_or(()), writable.as_slice()) {
        (Ok(id), w) => match w.iter().find(|p| p.id == id) {
            Some(p) => Ok(named(&p.id, &p.name)),
            None => {
                let name = reach.projects.iter().find(|p| p.id == id).map_or(id.clone(), |p| p.name.clone());
                Err(no_grant(&name))
            }
        },
        (Err(_), [only]) => Ok(named(&only.id, &only.name)),
        (Err(_), []) if reach.note.is_empty() => Err(no_grant("a project")),
        (Err(_), []) => Err(format!("the row wasn't attached: {}", reach.note)),
        (Err(_), _) => Err("the row wasn't attached: this machine writes the boards of several projects, and the caller's own isn't one of them".into()),
    }
}
