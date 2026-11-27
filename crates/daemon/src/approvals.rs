//! A person's **Approve** on their agent's card (Arugula #1417): the chief
//! of staff on this machine asked them (MCP's `ask_approval`, a card in
//! their staff room through control), they approved it, and control asks
//! this machine `agents.approved` ([`AskApproved`]) to do it, as its owner.
//! The agent never does it itself, and nothing said in chat gets here.
//!
//! - **`start`:** one agent block on the issue, as `start_agent {issue,
//!   worktree}` makes one: Claude Code with the person's own settings, in a
//!   worktree of the repo the asking agent works in, on a branch named for
//!   the issue (no repo: in its directory), in a tab of its own, attached
//!   to the issue's row. A row another agent holds refuses it, and so does
//!   an asking agent that's gone (moved or closed). Only on this
//!   machine: a start on another of the person's machines is a ladder
//!   start's, with control's start ticket (aru-143, #1415).
//! - **`close`:** each agent block named, as its `close` does (a worktree
//!   is removed only if nothing would be lost). One that's gone is said
//!   so; a pane that isn't an agent block is refused.
//! - **`run_board`** (Arugula #1416): the board's lead, here. Control wrote
//!   the board's grant on Approve and sends this to the grant's lead
//!   machine, whichever asked; the lead starts as a ladder start does
//!   (`ladder.rs`: a start ticket under the grant, then the board's lead),
//!   in a worktree of this machine's clone of the project's repo (the
//!   board's own first, Arugula #1458), in mode `auto` with [`LEAD_ALLOW`]
//!   and the person's own settings. A machine with no clone of any refuses
//!   it, saying so; a project that links no repo starts it in home.
//!
//! - **`agents.checkouts`** (Arugula #1458): before the person approves a
//!   "Run this board" card, control asks each machine it offers which of
//!   the project's repos it has a clone of, and the card says where none is.
//!
//! Remote input off here refuses an approved card, as it refuses a ladder
//! start: they type into terminals on this machine through control.

use std::sync::Arc;

use arugula_control_wire::forge::{AskApproved, CheckoutsAsk, OpAnswer};
use arugula_proto::{BlockType, PaneId, api::OpenRequest};
use serde_json::json;
use tracing::{info, warn};

use crate::{board_attach::NotKept, mux::Api, server::App};

/// `agents.approved`: `approved` with what it did, or `failed` with why
/// not, in a line for the card.
pub async fn approved(app: &Arc<App>, a: &AskApproved) -> OpAnswer {
    if !arugula_proto::sharing::remote_input(app.control.state_dir()) {
        let why = "remote input is off on this machine, so nothing starts or stops here through Arugula cloud";
        return OpAnswer::Failed { error: why.into() };
    }
    let done = match a.action.as_str() {
        "start" => start(app, a).await,
        "close" => close(app, &a.blocks).await.map(|said| (said, None)),
        "run_board" => run_board(app, a).await,
        other => Err(format!("this machine doesn't do {other:?}: start, close or run_board")),
    };
    match done {
        Ok((said, block)) => {
            info!(ask = a.ask, action = a.action, block, "an approved card done");
            OpAnswer::Approved { said, block }
        }
        Err(error) => OpAnswer::Failed { error },
    }
}

/// `agents.checkouts` (Arugula #1458): the first of the project's repos
/// this machine has a clone of, as a "Run this board" card shows it before
/// the person approves; found as the lead's start finds it.
pub async fn checkouts(app: &Arc<App>, c: &CheckoutsAsk) -> OpAnswer {
    let repos: Vec<String> = c.repos.iter().map(|r| r.trim().to_owned()).filter(|r| !r.is_empty()).collect();
    OpAnswer::Checkout { repo: crate::ladder::clone_among(app, &repos).await.map(|(repo, _)| repo) }
}

/// The tab an approved start opens in: its issue's.
pub fn session_of(number: u64) -> String {
    format!("issue-{number}")
}

/// The branch an approved start works on: `feat/N-` and up to six words of
/// the issue's title, as `start_agent`'s worktree names one.
pub fn branch_of(number: u64, title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
        .map(str::to_ascii_lowercase)
        .collect();
    match words.is_empty() {
        true => format!("feat/{number}"),
        false => format!("feat/{number}-{}", words.join("-")),
    }
}

/// What a start's agent is told: the card's prompt, else to work on the
/// issue.
pub fn prompt_of(number: u64, title: &str, prompt: Option<&str>) -> String {
    match prompt.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => p.to_owned(),
        None => format!(
            "Work on #{number}, {title}. Read it with forge_read {{kind: issue, item: \"#{number}\"}}, and say what you did in its thread when you're done."
        ),
    }
}

async fn start(app: &Arc<App>, a: &AskApproved) -> Result<(String, Option<PaneId>), String> {
    let issue = a.issue.as_ref().ok_or("a start names its issue")?;
    let item = format!("#{}", issue.number);
    let session = session_of(issue.number);
    let home = Some(a.project.clone());
    if let Some(held) = crate::board_attach::held(app, home.clone(), &item, Some(session.clone())).await {
        return Err(format!("{held}; no agent was started"));
    }
    // Where the asking agent works: its repo, when it's in one. Gone (it
    // was moved to another session, Arugula #1340, or closed), there's no
    // telling where: the card fails, and the agent that leads now asks again.
    let Some(dir) = asker_dir(app, a).await else {
        return Err(format!(
            "{} isn't running on {} now (moved or closed), so there's no telling where: ask your chief of staff again",
            a.session,
            app.hosts.name()
        ));
    };
    let runner = crate::worktree::local(app).await?;
    let branch = branch_of(issue.number, &issue.title);
    let worktree = match crate::worktree::make(&runner, &dir, &branch, "", "", true).await {
        Ok(made) => Some(crate::worktree::Owned { repo: made.repo, path: made.path, branch, base: made.base }),
        Err(e) if is_repo(&dir).await => return Err(format!("no worktree for it in {dir}: {e}")),
        Err(_) => None,
    };
    let cwd = worktree.as_ref().map_or(dir, |w| w.path.clone());
    let mut config = json!({
        "agent": "claude",
        // Arugula #1440: told first that it leads the issue's thread.
        "prompt": crate::mcp::registry::issue_agent_prompt(&item, None, &prompt_of(issue.number, &issue.title, a.prompt.as_deref())),
        "cwd": cwd,
        "title": format!("#{} {}", issue.number, issue.title),
        "work": { "issue": item, "project": a.project },
        // As the person: their own Claude Code settings, as their own
        // start would have.
        "user_settings": true,
        // The person isn't at its terminal: mode `auto` with the lead's
        // tools, as a board's lead starts (Arugula #1543).
        "permission_mode": "auto",
        "allow": LEAD_ALLOW.iter().map(|t| json!({ "tool": t })).collect::<Vec<_>>(),
    });
    if let Some(w) = &worktree {
        config["worktree"] = json!(w);
    }
    // A tab of the issue's that is in the card's project already stays; one
    // elsewhere (the owner's default, say) is moved after the open.
    let placed = app.mux.api(Api::Sessions).await.is_some_and(|all| {
        all.iter().any(|s| s.name == session && s.project.as_ref().is_some_and(|p| p.id == a.project))
    });
    let req = OpenRequest {
        kind: BlockType::Agent,
        config,
        session: Some(session.clone()),
        local: true,
        ..Default::default()
    };
    let block = match app.mux.api(|r| Api::Open(req, None, r)).await {
        Some(Ok(b)) => b,
        failed => {
            if let Some(w) = &worktree {
                crate::worktree::settle(&runner, w).await;
            }
            return Err(match failed {
                Some(Err(why)) => why,
                _ => "the daemon is stopping".into(),
            });
        }
    };
    if !placed {
        put_in_project(app, &session, &a.project).await;
    }
    let on = match crate::board_attach::keep(app, home, &item, block, false, Some(session)).await {
        Ok(boards) if boards.is_empty() => "on no board's row".to_owned(),
        Ok(boards) => {
            let titles: Vec<&str> = boards.iter().map(|b| b.title.as_str()).collect();
            format!("on its row on {}", titles.join(", "))
        }
        Err(NotKept::Claimed(why)) => {
            // Taken meanwhile: this agent isn't started after all.
            let _ = app.mux.api(|r| Api::Close(block, r)).await;
            if let Some(w) = &worktree {
                crate::worktree::settle(&runner, w).await;
            }
            return Err(format!("{why}; no agent was started"));
        }
        Err(NotKept::Why(why)) => format!("on no row ({why})"),
    };
    let at = worktree.as_ref().map(|w| format!(" in {} ({})", w.path, w.branch)).unwrap_or_default();
    let said = format!("started agent block %{block} on #{} on {}{at}, {on}", issue.number, app.hosts.name());
    Ok((said, Some(block)))
}

/// Arugula #1518: the tab a start opened is in the card's project, as set by
/// hand, not where its clone or its owner's default would put it. A failure
/// is logged: the agent is started either way.
async fn put_in_project(app: &Arc<App>, session: &str, project: &str) {
    let Some(id) =
        app.mux.api(Api::Sessions).await.and_then(|all| all.iter().find(|s| s.name == session).map(|s| s.id))
    else {
        warn!(session, project, "the start's tab isn't there to put in its project");
        return;
    };
    let known = crate::mcp::issues::project_now(&app.control, project).await;
    let known = known.unwrap_or_else(|| arugula_core::ProjectRef {
        id: project.to_owned(),
        name: project.to_owned(),
        team: None,
    });
    match app.mux.api(|r| Api::SetSessionProject(id, known.clone(), r)).await {
        Some(Ok(())) => {
            app.mux.api(|r| Api::ProjectKnown(known, r)).await;
        }
        Some(Err(why)) => warn!(session, project, "the start's tab isn't in its project: {why}"),
        None => warn!(session, project, "the daemon is stopping"),
    }
}

/// What a board's lead is told when its card gave no prompt.
pub fn lead_prompt_of(board: &str) -> String {
    format!(
        "You lead the board {board}: your person approved a card to run it. Read it with board_read {{board: \"{board}\"}}, then run it: start issue agents on its ready rows within its grant, and say what you're doing in the board's thread."
    )
}

/// The tools a board's lead uses without asking. It starts in mode `auto`
/// with these and the person's own settings, as the board-lead recipe starts
/// its issue agents (Arugula #1440): in the default mode every command it
/// runs is a card for its person. A chief of staff starts with them too
/// (Arugula #1490). Mode `auto` already lets Arugula's own MCP tools through.
pub const LEAD_ALLOW: &[&str] = &["Bash", "Read", "Edit", "Write", "Glob", "Grep", "WebFetch"];

/// `run_board`: the board's lead here, as a ladder start under the grant
/// control just wrote.
async fn run_board(app: &Arc<App>, a: &AskApproved) -> Result<(String, Option<PaneId>), String> {
    let board = a.board.as_deref().map(str::trim).filter(|b| !b.is_empty()).ok_or("a run_board names its board")?;
    // Arugula #1458: in this machine's clone of the project's repo, the
    // board's own first. A project with no repo: its home.
    let repos = repos_of(a);
    let repo = match crate::ladder::clone_among(app, &repos).await {
        Some((repo, _)) => Some(repo),
        None if repos.is_empty() => None,
        None => return Err(no_clone(app.hosts.name(), &repos)),
    };
    let prompt = match a.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => p.to_owned(),
        None => lead_prompt_of(board),
    };
    let ask = crate::ladder::Ask {
        board: board.to_owned(),
        rung: Some(arugula_control_wire::ladder::Rung::Lead),
        repo,
        prompt,
        title: a.board_title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| format!("Lead · {t}")),
        by_session: Some(a.session.clone()).filter(|s| !s.is_empty()),
        permission_mode: Some("auto".into()),
        allow: LEAD_ALLOW.iter().map(|&t| t.to_owned()).collect(),
        user_settings: true,
        ..Default::default()
    };
    let started = crate::ladder::start(app, ask).await?;
    let said = format!(
        "started the board's lead %{} ({}) on {} in {}: {}",
        started.block, started.session, started.machine, started.cwd, started.said
    );
    Ok((said, Some(started.block)))
}

/// A `run_board`'s repos, in order: its `repos`, then its `repo` (an older
/// control's), each once.
fn repos_of(a: &AskApproved) -> Vec<String> {
    let mut repos: Vec<String> = vec![];
    for r in a.repos.iter().chain(&a.repo).map(|r| r.trim()).filter(|r| !r.is_empty()) {
        if !repos.iter().any(|x| x.eq_ignore_ascii_case(r)) {
            repos.push(r.to_owned());
        }
    }
    repos
}

/// Why a board's lead didn't start on `machine`: it has no clone of the
/// project's repos.
fn no_clone(machine: &str, repos: &[String]) -> String {
    let which = match repos {
        [one] => one.clone(),
        [first, rest @ ..] => format!("{first} (or the project's {} other repos)", rest.len()),
        [] => "the project's repo".into(),
    };
    format!(
        "{machine} has no clone of {which}, so the board's lead didn't start: clone it there, or run the board with its lead on a machine that has one"
    )
}

/// The asking agent's directory: its block's, if it's still the agent of
/// that name; else a live block's of that name; none when it's gone.
async fn asker_dir(app: &App, a: &AskApproved) -> Option<String> {
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    let mut agents = vec![];
    for p in panes.into_iter().filter(|p| p.info.kind == BlockType::Agent && p.info.host.is_none()) {
        if crate::agent_mentions::session_name(app, p.info.id).await.as_deref() == Some(a.session.as_str()) {
            agents.push(p.info.id);
        }
    }
    let id = agents.iter().find(|&&id| id == a.pane).or(agents.first()).copied()?;
    let b = app.mux.api(|r| Api::Block(id, r)).await.flatten()?;
    // An agent block's pane says no cwd: its agent's is in its state.
    let (state, config) = (b.state(), b.config());
    state["cwd"].as_str().or(config["cwd"].as_str()).map(str::to_owned)
}

async fn is_repo(dir: &str) -> bool {
    tokio::process::Command::new("git")
        .args(["-C", dir, "rev-parse", "--git-dir"])
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// Close each of `blocks`, an agent block here, as its `close` does.
async fn close(app: &Arc<App>, blocks: &[PaneId]) -> Result<String, String> {
    if blocks.is_empty() {
        return Err("a close names its agent blocks".into());
    }
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    let (mut live, mut gone) = (vec![], vec![]);
    for &id in blocks {
        match panes.iter().find(|p| p.info.id == id) {
            None => gone.push(format!("%{id}")),
            Some(p) if p.info.kind != BlockType::Agent || p.info.host.is_some() => {
                return Err(format!("%{id} isn't an agent block on this machine: nothing was closed"));
            }
            Some(_) => live.push(id),
        }
    }
    let mut closed = vec![];
    for id in live {
        let owned: Option<crate::worktree::Owned> = match app.mux.api(|r| Api::Block(id, r)).await.flatten() {
            Some(b) => b.config().get("worktree").cloned().and_then(|w| serde_json::from_value(w).ok()),
            None => None,
        };
        if app.mux.api(|r| Api::Close(id, r)).await != Some(true) {
            gone.push(format!("%{id}"));
            continue;
        }
        if let Some(w) = owned
            && let Ok(runner) = crate::worktree::local(app).await
        {
            crate::worktree::settle(&runner, &w).await;
        }
        closed.push(format!("%{id}"));
    }
    Ok(match (closed.is_empty(), gone.is_empty()) {
        (false, true) => format!("closed {} on {}", closed.join(", "), app.hosts.name()),
        (false, false) => {
            format!("closed {} on {} ({} had closed)", closed.join(", "), app.hosts.name(), gone.join(", "))
        }
        (true, _) => format!("{} had closed already", gone.join(", ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_approved_start_names_its_branch_tab_and_prompt() {
        assert_eq!(branch_of(12, "Fix the login: it's slow!"), "feat/12-fix-the-login-it-s-slow");
        assert_eq!(branch_of(7, "—"), "feat/7");
        assert_eq!(session_of(12), "issue-12");
        assert!(prompt_of(12, "Fix it", None).starts_with("Work on #12, Fix it. Read it with forge_read"));
        assert_eq!(prompt_of(12, "Fix it", Some("  only the tests ")), "only the tests");
        assert!(prompt_of(12, "Fix it", Some(" ")).starts_with("Work on #12"));
        assert!(lead_prompt_of("brd_1").contains("board_read {board: \"brd_1\"}"));
    }

    #[test]
    fn a_run_board_tries_its_repos_in_order_and_says_when_none_is_here() {
        let a = AskApproved {
            repos: vec!["acme/widgets".into(), " acme/site ".into(), "ACME/Widgets".into()],
            repo: Some("acme/old".into()),
            ..Default::default()
        };
        assert_eq!(repos_of(&a), ["acme/widgets", "acme/site", "acme/old"]);
        let old = AskApproved { repo: Some("acme/widgets".into()), ..Default::default() };
        assert_eq!(repos_of(&old), ["acme/widgets"]);
        assert!(repos_of(&AskApproved::default()).is_empty());
        let one = no_clone("geek", &["acme/widgets".into()]);
        assert!(one.starts_with("geek has no clone of acme/widgets, so the board's lead didn't start"), "{one}");
        let three = no_clone("geek", &repos_of(&a));
        assert!(three.starts_with("geek has no clone of acme/widgets (or the project's 2 other repos)"), "{three}");
    }
}
