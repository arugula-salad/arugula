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

use arugula_control_wire::boards::{AttachBlock, BlockAttached, CLAIMED, IssueRef, agent_project_attach_block};
use std::sync::Arc;

use arugula_proto::{PaneId, flags};
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boards: Vec<WorkBoard>,
}

/// A board with a row the block is attached to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkBoard {
    pub project: String,
    pub board: String,
    pub title: String,
    pub row: String,
}

/// What a config opening an agent block says of its issue: nothing, or the
/// issue as written. An issue that isn't one is the opener's mistake.
pub fn issue_of(config: &serde_json::Value) -> Result<Option<String>, String> {
    let Some(w) = config.get("work").filter(|w| !w.is_null()) else { return Ok(None) };
    let issue = w["issue"].as_str().unwrap_or_default().trim();
    match IssueRef::parse(issue) {
        Some(_) => Ok(Some(issue.to_owned())),
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
/// issue), or why it wasn't tried or was refused.
pub async fn keep(
    app: &App,
    home: Option<String>,
    issue: &str,
    pane: PaneId,
    take: bool,
) -> Result<Vec<WorkBoard>, NotKept> {
    let boards = attach(app, home, issue, pane, take).await?;
    if !boards.is_empty()
        && let Some(b) = app.mux.api(|r| Api::Block(pane, r)).await.flatten()
    {
        b.call("set_work", serde_json::json!({ "boards": boards })).await?;
    }
    Ok(boards)
}

/// [`keep`], in the background, for a block opened through the API
/// (`arugula agent --issue`): nobody waits for its answer.
pub fn keep_later(app: Arc<App>, issue: String, pane: PaneId) {
    crate::task::spawn(async move {
        match keep(&app, None, &issue, pane, false).await {
            Ok(boards) => debug!(pane, issue, boards = boards.len(), "a block is on its issue's rows"),
            Err(why) => debug!(pane, issue, "a block started on an issue isn't on a row: {why}"),
        }
    });
}

async fn attach(
    app: &App,
    home: Option<String>,
    issue: &str,
    pane: PaneId,
    take: bool,
) -> Result<Vec<WorkBoard>, NotKept> {
    if !crate::lab_flags::flags(app.control.state_dir()).has(flags::BOARDS) {
        return Err("boards are off on this machine (Labs: boards), so the row wasn't attached".into());
    }
    let Some(enrolled) = app.control.enrolled() else {
        return Err("this machine hasn't joined Arugula control, so the row wasn't attached".into());
    };
    let project = write_project(app, home).await?;
    let body = serde_json::to_value(AttachBlock {
        issue: issue.to_owned(),
        machine: enrolled.keys.id(),
        pane: pane.to_string(),
        take,
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
