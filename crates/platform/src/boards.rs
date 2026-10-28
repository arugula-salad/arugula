//! Boards over the platform's routes (live status board, step 1): a
//! project's boards ([`db::boards`]), each an ordered list of zones that
//! hold an ordered list of rows. A board is a project-scoped platform
//! object (`brd_`), not a daemon block: it is here, with every machine
//! asleep. It has a thread like an issue (`chat::objects`). The wire is
//! [`arugula_control_wire::boards`], which says how revs work.
//!
//! **Who may do what,** by membership of the board's project, as for
//! issues: a personal project's account does everything; in a team's
//! project owners and editors make boards and change them, viewers read;
//! anyone else gets 404.
//!
//! **A machine's agents** ([`agent`], routes under `/api/daemon`) act for
//! their owner: the owner's role in the project is the ceiling, and the
//! machine needs a grant the owner gave over the project too, asked again
//! on every call: `boards.read` to read, `boards.write` to change (which
//! reads too). With `boards.write` an agent may add rows to a zone that is
//! there, set any row's note and status (a PROOFS row's too), and move
//! rows. It may never make or delete a board or a zone, rename or move a
//! zone, repoint a row, or delete a row some other hand added: the rows its
//! own machine added, it may. These rules are here, in the functions both
//! kinds of caller use, and not in the routes that call them.
//!
//! **Gating:** the `boards` Labs flag ([`FLAG`], resolved as chat's is).
//! With it off for the account, every route here answers 404, as if boards
//! weren't there.
//!
//! **Pointers are stored, and their state is read:** a row's `fi_`, `iss_`,
//! `run_`, `hw_` id or agent block is kept as given, after a check of its
//! shape. A read adds each `fi_` and `iss_` row's state now as `live`
//! ([`resolve`]): one query for the mirror's items, one for their checks
//! and one for the issues, through the reader's own forge access. `status`
//! is the row's own and never changes with it.
//!
//! **A machine's blocks** (step 4): a daemon tells control the state of its
//! own agent blocks ([`agent::put_block_state`], stored by machine and
//! pane), and a row with a `Block` pointer shows it as `block` in a read
//! ([`resolve_blocks`]) when the block's machine holds `boards.write` over
//! the board's project, asked on that read. What the report holds is only
//! what aru-116 allows ([`wire::BlockState`]). A report pushes an object
//! change for the boards whose rows point at the block.
//!
//! **Work** ([`work`]): a project's issues, PRs and runs, for a row to point
//! at, and the open issues that no board of the project has a row for.
//!
//! **Live:** a change to a board, to an issue a row points at, or (a
//! webhook) to a mirrored item it points at pushes an object change
//! ([`changed`]), so clients following the board read it again.
//!
//! [`db::boards`]: crate::db::boards

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::{boards as wire, chat::ChatAgent};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    Platform,
    auth::{Machine, Person},
    chat::flag_on,
    db::{
        boards::{self as db, Put, Refused},
        forge::{CheckState, ForgeItem},
        issues::Actor,
        projects::{BOARDS_READ, BOARDS_WRITE},
        runs::{Run, State as RunState},
    },
    forge::{
        links::membership,
        viewer::{self, Viewer},
    },
    grants::{AgentScope, agent_may, agent_names},
    issues::PERSONAL,
};

pub mod agent;
pub mod work;

/// The Labs flag boards are behind.
pub const FLAG: &str = "boards";
/// The reason a refusal gives for a write made on a stale rev.
pub const STALE_REV: &str = "stale_rev";

type J<T> = Result<Json<T>, ApiError>;

fn nothing() -> ApiError {
    err(StatusCode::NOT_FOUND, "not found")
}

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

fn no_board() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such board")
}

fn bad(msg: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

/// 404 unless boards are on for `account`.
async fn gate(app: &Platform, account: &str) -> Result<(), ApiError> {
    if flag_on(app, account, FLAG).await? { Ok(()) } else { Err(nothing()) }
}

/// The project `asked` names (its id, or [`PERSONAL`]) as `account` sees
/// it: 404 for a stranger or when boards are off; with `write`, 403 for a
/// viewer, and the team's roster comes from the hub, not the cache. An
/// agent also needs its machine's grant over the project (`boards.read`, or
/// `boards.write` to change; a write reads too).
async fn place(app: &Platform, by: Actor<'_>, asked: &str, write: bool) -> Result<String, ApiError> {
    let account = by.account;
    gate(app, account).await?;
    let id = if asked == PERSONAL {
        crate::projects::personal_project(app, account).await?.ok_or_else(no_project)?
    } else {
        asked.to_owned()
    };
    let project = app.db.issue_project(&id)?.ok_or_else(no_project)?;
    let role = match (&project.account, &project.team) {
        (Some(a), _) if a == account => TeamRole::Owner,
        (None, Some(t)) => {
            let t = if write { app.cache.team_now(&*app.hub, t).await? } else { app.cache.team(&*app.hub, t).await? };
            t.ok_or_else(no_project)?.role(account).ok_or_else(no_project)?
        }
        _ => return Err(no_project()),
    };
    if write && role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer reads boards, and doesn't change them"));
    }
    if let Some(machine) = by.agent {
        let m = Machine { account: account.to_owned(), daemon: machine.to_owned() };
        let scope = AgentScope::Project(project.id.clone());
        if write {
            agent_may(app, &m, BOARDS_WRITE, &scope).await?;
        } else if let Err(e) = agent_may(app, &m, BOARDS_READ, &scope).await {
            // A grant to change a board reads it too.
            if agent_may(app, &m, BOARDS_WRITE, &scope).await.is_err() {
                return Err(e);
            }
        }
    }
    Ok(project.id)
}

/// A board by its id as `account` sees it: 404 unless they may read it
/// (or, with `write`, change it).
async fn board_place(app: &Platform, by: Actor<'_>, id: &str, write: bool) -> Result<wire::Board, ApiError> {
    gate(app, by.account).await?;
    let board = app.db.board(id)?.ok_or_else(no_board)?;
    match place(app, by, &board.project, write).await {
        Err(e) if e.0 == StatusCode::NOT_FOUND => Err(no_board()),
        r => r.map(|_| board),
    }
}

fn refused(r: Refused) -> ApiError {
    match r {
        Refused::Missing(what) => err(StatusCode::NOT_FOUND, what),
        Refused::Stale { rev } => {
            refusal(StatusCode::CONFLICT, STALE_REV, &format!("this entry is at rev {rev} now: read the board again"))
        }
        Refused::Bad(why) => err(StatusCode::BAD_REQUEST, &why),
    }
}

fn one_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

fn title_ok(t: &str) -> Result<&str, ApiError> {
    let t = t.trim();
    if t.is_empty() {
        return Err(bad("a title can't be empty"));
    }
    if !one_line(t) {
        return Err(bad("a title is one line, without control characters"));
    }
    if t.chars().count() > wire::TITLE_MAX {
        return Err(bad(&format!("a title is at most {} characters", wire::TITLE_MAX)));
    }
    Ok(t)
}

/// A goal or done note: trimmed, in lines, at most [`wire::GOAL_MAX`]
/// characters. `what` names it in the refusal.
fn text_ok<'a>(t: &'a str, what: &str) -> Result<&'a str, ApiError> {
    let t = t.trim();
    if t.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Err(bad(&format!("{what} is text, without control characters")));
    }
    if t.chars().count() > wire::GOAL_MAX {
        return Err(bad(&format!("{what} is at most {} characters", wire::GOAL_MAX)));
    }
    Ok(t)
}

fn note_ok(n: &str) -> Result<&str, ApiError> {
    let n = n.trim();
    if !one_line(n) {
        return Err(bad("a note is one line, without control characters"));
    }
    if n.chars().count() > wire::NOTE_MAX {
        return Err(bad(&format!("a note is at most {} characters", wire::NOTE_MAX)));
    }
    Ok(n)
}

/// `id` is `prefix_` and a few letters and digits: a platform id's shape.
fn id_ok(id: &str, prefix: &str) -> Result<(), ApiError> {
    let rest = id.strip_prefix(prefix).and_then(|r| r.strip_prefix('_'));
    match rest {
        Some(r) if !r.is_empty() && r.len() <= 64 && r.chars().all(|c| c.is_ascii_alphanumeric()) => Ok(()),
        _ => Err(bad(&format!("expected an id that starts {prefix}_"))),
    }
}

fn name_ok(s: &str, what: &str) -> Result<(), ApiError> {
    if s.is_empty() || s.chars().count() > 128 || !one_line(s) {
        return Err(bad(&format!("a {what} is 1 to 128 characters, on one line")));
    }
    Ok(())
}

fn pointer_ok(p: &wire::Pointer) -> Result<(), ApiError> {
    match p {
        wire::Pointer::None => Ok(()),
        wire::Pointer::ForgeItem { id } => id_ok(id, "fi"),
        wire::Pointer::Issue { id } => id_ok(id, "iss"),
        wire::Pointer::Run { id } => id_ok(id, "run"),
        wire::Pointer::Workspace { id } => id_ok(id, "hw"),
        wire::Pointer::Block { machine, pane } => {
            name_ok(machine, "machine")?;
            name_ok(pane, "pane")
        }
        wire::Pointer::Link { url } => link_ok(url),
    }
}

/// A link pointer's URL: `https://` and a host, at most
/// [`wire::LINK_MAX`] bytes, no userinfo, no whitespace or control
/// characters.
fn link_ok(url: &str) -> Result<(), ApiError> {
    let refuse = |why: &str| Err(bad(&format!("a link is an https:// URL {why}")));
    if url.len() > wire::LINK_MAX {
        return refuse(&format!("of at most {} bytes", wire::LINK_MAX));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return refuse("without spaces or control characters");
    }
    let Some(rest) = url.strip_prefix("https://") else { return refuse("(only https is taken)") };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return refuse("without a user name or password before the host");
    }
    if authority.is_empty() || authority.starts_with(':') {
        return refuse("with a host");
    }
    Ok(())
}

fn view(s: db::Stored, names: &std::collections::HashMap<String, String>) -> wire::BoardView {
    let mut zones: Vec<wire::Zone> = vec![];
    for e in s.entries {
        match e.kind {
            wire::EntryKind::Zone => {
                zones.push(wire::Zone { id: e.id, rev: e.rev, title: e.title, kind: e.zone_kind, rows: vec![] })
            }
            wire::EntryKind::Row => {
                if let Some(z) = zones.last_mut() {
                    z.rows.push(wire::Row {
                        id: e.id,
                        rev: e.rev,
                        pointer: e.pointer,
                        note: e.note,
                        status: e.status,
                        live: None,
                        block: None,
                        agent: e.agent.map(|m| ChatAgent {
                            machine_name: names.get(&m).cloned().unwrap_or_default(),
                            machine: m,
                        }),
                        created_at: e.created_at,
                        updated_at: e.updated_at,
                    });
                }
            }
        }
    }
    wire::BoardView { board: s.board, zones }
}

/// A forge item's state now, from the checks on its head.
fn live_item(i: &ForgeItem, repo: &str, checks: &[&CheckState]) -> wire::Live {
    use wire::LiveState::*;
    let state = match (i.kind.as_str(), i.state.as_str()) {
        (_, "merged") => Merged,
        (_, "closed") => Closed,
        ("pr", _) => {
            let red = |c: &&CheckState| {
                c.status == "completed"
                    && matches!(
                        c.conclusion.as_deref(),
                        Some("failure" | "error" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
                    )
            };
            if checks.iter().any(red) {
                ChecksFailed
            } else if checks.iter().any(|c| c.status != "completed") {
                ChecksRunning
            } else {
                Open
            }
        }
        _ => Open,
    };
    // A stale mirror row may hold no list, or not a list: no assignees.
    let assignees: Vec<String> = serde_json::from_str(&i.assignees).unwrap_or_default();
    wire::Live {
        state,
        kind: i.kind.clone(),
        repo: Some(repo.to_owned()),
        number: i.number,
        title: i.title.clone(),
        assignees,
    }
}

/// A run as a board shows it: running, succeeded, failed, or other.
fn live_run(run: &Run) -> wire::Live {
    let state = match run.state {
        RunState::Running => wire::LiveState::Running,
        RunState::Succeeded => wire::LiveState::Succeeded,
        RunState::Failed => wire::LiveState::RunFailed,
        _ => wire::LiveState::Other,
    };
    wire::Live {
        state,
        kind: "run".to_owned(),
        repo: None,
        number: 0,
        title: format!("{} run", run.trigger.as_str()),
        assignees: vec![],
    }
}

/// Add each `fi_`, `iss_` and `run_` row's live state to `v`, as `account` may see
/// it: items of the project's linked repos they may read, and its issues.
async fn resolve(app: &Platform, account: &str, v: &mut wire::BoardView) -> Result<(), ApiError> {
    let (items, issues) = app.db.board_targets(&v.board.id)?;
    let mut live: HashMap<String, wire::Live> = HashMap::new();
    if !items.is_empty() {
        let repos = app.db.linked_repos(&v.board.project)?;
        let checked = viewer::check(app, &Viewer::of(app, account).await?, &repos).await?;
        let ids: Vec<&str> = items.iter().map(String::as_str).collect();
        let found = app.db.forge_items_among(&checked.readable, &ids)?;
        let heads: Vec<&str> = found.iter().filter_map(|i| i.head_sha.as_deref()).collect();
        let checks = app.db.forge_checks_on_heads(&checked.readable, &heads)?;
        for i in &found {
            let repo = repos.iter().find(|r| r.repo_id == i.repo_id).map_or("", |r| r.full_name.as_str());
            let mine: Vec<_> = checks
                .iter()
                .filter(|c| c.repo_id == i.repo_id && Some(c.head_sha.as_str()) == i.head_sha.as_deref())
                .collect();
            live.insert(i.id.clone(), live_item(i, repo, &mine));
        }
    }
    if !issues.is_empty() {
        let ids: Vec<&str> = issues.iter().map(String::as_str).collect();
        let found = app.db.issue_states(&v.board.project, &ids)?;
        // One read of every account named, named as the issues page names
        // them: login, else name, else someone who left.
        let accounts: Vec<String> = found
            .iter()
            .flat_map(|f| f.assignees.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let people =
            if accounts.is_empty() { HashMap::new() } else { app.cache.accounts(&*app.hub, &accounts).await? };
        for db::IssueState { id, number, title, closed, assignees: who } in found {
            let state = if closed { wire::LiveState::Closed } else { wire::LiveState::Open };
            let assignees = who
                .into_iter()
                .map(|a| match people.get(&a) {
                    Some(p) if !p.login.is_empty() => p.login.clone(),
                    Some(p) if !p.name.is_empty() => p.name.clone(),
                    _ => "someone who left".to_owned(),
                })
                .collect();
            live.insert(id, wire::Live { state, kind: "native".to_owned(), repo: None, number, title, assignees });
        }
    }
    let runs = app.db.board_run_targets(&v.board.id)?;
    if !runs.is_empty() {
        // The runs routes' own check: a member of the run's project.
        let mut may: HashMap<String, bool> = HashMap::new();
        for id in &runs {
            let Some(run) = app.db.run(id)? else { continue };
            if !may.contains_key(&run.project) {
                let ok = membership(app, account, &run.project, false).await.is_ok();
                may.insert(run.project.clone(), ok);
            }
            if may[&run.project] {
                live.insert(id.clone(), live_run(&run));
            }
        }
    }
    for row in v.zones.iter_mut().flat_map(|z| z.rows.iter_mut()) {
        if let wire::Pointer::ForgeItem { id } | wire::Pointer::Issue { id } | wire::Pointer::Run { id } = &row.pointer
        {
            row.live = live.get(id).cloned();
        }
    }
    Ok(())
}

/// Add each agent block row's state as its machine reported it, when that
/// machine holds `boards.write` over the board's project now (the grant is
/// asked on every read, so a revoked one hides what it reported).
async fn resolve_blocks(app: &Platform, v: &mut wire::BoardView) -> Result<(), ApiError> {
    let mut by_machine: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for row in v.zones.iter().flat_map(|z| z.rows.iter()) {
        if let wire::Pointer::Block { machine, pane } = &row.pointer {
            by_machine.entry(machine).or_default().push(pane);
        }
    }
    let mut states: HashMap<(String, String), wire::BlockState> = HashMap::new();
    for (machine, panes) in by_machine {
        if !machine_writes(app, machine, &v.board.project).await {
            continue;
        }
        for (pane, state) in app.db.block_states(machine, &panes)? {
            states.insert((machine.to_owned(), pane), state);
        }
    }
    for row in v.zones.iter_mut().flat_map(|z| z.rows.iter_mut()) {
        if let wire::Pointer::Block { machine, pane } = &row.pointer {
            row.block = states.get(&(machine.clone(), pane.clone())).cloned();
        }
    }
    Ok(())
}

/// Whether `machine` holds a live `boards.write` grant over `project`, one
/// its owner may still give ([`agent_may`]).
async fn machine_writes(app: &Platform, machine: &str, project: &str) -> bool {
    let Ok(grants) = app.db.machine_write_projects(machine, BOARDS_WRITE) else { return false };
    for (scope, owner) in grants.into_iter().filter(|(p, _)| p == project) {
        let m = Machine { account: owner, daemon: machine.to_owned() };
        if agent_may(app, &m, BOARDS_WRITE, &AgentScope::Project(scope)).await.is_ok() {
            return true;
        }
    }
    false
}

/// Fill each board's progress: its issue rows, each issue once, done when
/// closed at its source. Mirrored issues count as `account` may read them,
/// as [`resolve`] shows them: a row they can't read counts toward neither.
/// One read of the project's rows, and one per kind of issue.
async fn progress(app: &Platform, account: &str, project: &str, boards: &mut [wire::Board]) -> Result<(), ApiError> {
    let targets = app.db.board_issue_targets(project)?;
    if targets.is_empty() {
        return Ok(());
    }
    let ids = |kind: &str| -> Vec<&str> {
        let mut v: Vec<&str> = targets.iter().filter(|t| t.1 == kind).map(|t| t.2.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    // Each issue the reader may read, closed or not.
    let mut closed: HashMap<String, bool> = HashMap::new();
    for i in app.db.issue_states(project, &ids("issue"))? {
        closed.insert(i.id, i.closed);
    }
    let mirrored = ids("forge_item");
    if !mirrored.is_empty() {
        let repos = app.db.linked_repos(project)?;
        let checked = viewer::check(app, &Viewer::of(app, account).await?, &repos).await?;
        for i in app.db.forge_items_among(&checked.readable, &mirrored)?.into_iter().filter(|i| i.kind == "issue") {
            closed.insert(i.id, i.state != "open");
        }
    }
    for b in boards {
        let mut seen: std::collections::HashSet<&str> = Default::default();
        for (_, _, id) in targets.iter().filter(|t| t.0 == b.id) {
            if let Some(done) = closed.get(id.as_str())
                && seen.insert(id)
            {
                b.progress.total += 1;
                b.progress.done += u32::from(*done);
            }
        }
    }
    Ok(())
}

async fn read(app: &Platform, account: &str, id: &str) -> Result<wire::BoardView, ApiError> {
    let stored = app.db.board_stored(id)?.ok_or_else(no_board)?;
    let mut machines: Vec<String> = stored.entries.iter().filter_map(|e| e.agent.clone()).collect();
    machines.sort();
    machines.dedup();
    let names = if machines.is_empty() {
        Default::default()
    } else {
        agent_names(app, &machines).await?.into_iter().map(|(id, d)| (id, d.name)).collect()
    };
    let mut v = view(stored, &names);
    let project = v.board.project.clone();
    progress(app, account, &project, std::slice::from_mut(&mut v.board)).await?;
    resolve(app, account, &mut v).await?;
    resolve_blocks(app, &mut v).await?;
    Ok(v)
}

/// The board `id`, in `project`, changed: pages showing it read it again
/// (as for an issue, through its thread's readers).
pub fn changed(app: &Platform, project: &str, id: &str) {
    app.chat.object_changed(&app.db, &format!("project:{project}"), id);
}

/// A native issue changed: the boards that point at it.
pub fn issue_changed(app: &Platform, issue: &str) {
    match app.db.boards_on_issue(issue) {
        Ok(boards) => boards.iter().for_each(|(b, p)| changed(app, p, b)),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on an issue"),
    }
}

/// Mirror rows of `repo` were written (a webhook): the boards that point
/// at the items they changed, or at items whose head a check landed on.
pub fn mirror_changed(app: &Platform, repo: u64, numbers: &[u64], shas: &[String]) {
    match app.db.boards_on_items(repo, numbers, shas) {
        Ok(boards) => boards.iter().for_each(|(b, p)| changed(app, p, b)),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on a mirrored item"),
    }
}

/// What an agent is refused of a zone.
fn no_zones() -> ApiError {
    err(
        StatusCode::FORBIDDEN,
        "an agent doesn't make, change, move or delete zones: it works in the rows of the ones there are",
    )
}

fn is_zone(app: &Platform, board: &str, entry: &str) -> Result<bool, ApiError> {
    Ok(app.db.board_entry(board, entry)?.is_some_and(|e| e.kind == wire::EntryKind::Zone))
}

/// What an agent's write to a board may be: a new row in a zone there is,
/// or a row's note or status. Not a zone, a zone's title, a row's pointer.
fn agent_put_allowed(app: &Platform, board: &str, p: &wire::PutEntry) -> Result<(), ApiError> {
    match (&p.id, p.entry) {
        (None, Some(wire::EntryKind::Row)) => {}
        (None, _) => return Err(no_zones()),
        (Some(id), _) => {
            if is_zone(app, board, id)? {
                return Err(no_zones());
            }
        }
    }
    if p.id.is_some() && p.pointer.is_some() {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent sets a row's note and status, and doesn't repoint it: add a row for something else",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- the seam

/// A project's boards as `account` may read them.
pub async fn list(app: &Platform, by: Actor<'_>, project: &str) -> Result<wire::BoardList, ApiError> {
    let id = place(app, by, project, false).await?;
    let mut boards = app.db.boards(&id)?;
    progress(app, by.account, &id, &mut boards).await?;
    Ok(wire::BoardList { boards, project: id })
}

/// Make a board in `project`.
pub async fn create(
    app: &Platform,
    account: &str,
    project: &str,
    new: &wire::NewBoard,
) -> Result<wire::BoardView, ApiError> {
    // People only: an agent never makes a board.
    let id = place(app, Actor::person(account), project, true).await?;
    let title = title_ok(&new.title)?;
    let board = app
        .db
        .create_board(&id, title, account, now_ms(), wire::BOARDS_MAX)?
        .ok_or_else(|| bad(&format!("a project has at most {} boards", wire::BOARDS_MAX)))?;
    read(app, account, &board.id).await
}

/// A board with its zones and rows in order, as `account` may read it.
pub async fn get(app: &Platform, by: Actor<'_>, id: &str) -> Result<wire::BoardView, ApiError> {
    board_place(app, by, id, false).await?;
    read(app, by.account, id).await
}

/// Set a board's title or goal. People only.
pub async fn edit(app: &Platform, account: &str, id: &str, e: &wire::EditBoard) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, Actor::person(account), id, true).await?;
    let title = e.title.as_deref().map(title_ok).transpose()?;
    let goal = e.goal.as_deref().map(|g| text_ok(g, "a goal")).transpose()?;
    if title.is_none() && goal.is_none() {
        return Err(bad("nothing to change: give a title or a goal"));
    }
    if !app.db.edit_board(id, title, goal, now_ms())? {
        return Err(no_board());
    }
    changed(app, &board.project, id);
    read(app, account, id).await
}

/// Close a board with its done note. People only. The issues and rows are
/// left as they are.
pub async fn close(app: &Platform, account: &str, id: &str, c: &wire::CloseBoard) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, Actor::person(account), id, true).await?;
    let note = text_ok(&c.done_note, "a done note")?;
    if note.is_empty() {
        return Err(bad("say what was done: a board closes with a note"));
    }
    if !app.db.close_board(id, note, account, now_ms())? {
        return Err(no_board());
    }
    changed(app, &board.project, id);
    read(app, account, id).await
}

/// Open a closed board again, keeping its done note. People only.
pub async fn reopen(app: &Platform, account: &str, id: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, Actor::person(account), id, true).await?;
    if !app.db.reopen_board(id, now_ms())? {
        return Err(no_board());
    }
    changed(app, &board.project, id);
    read(app, account, id).await
}

/// Make or change a zone or a row (see [`wire::PutEntry`]).
pub async fn put(app: &Platform, by: Actor<'_>, id: &str, p: &wire::PutEntry) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if by.agent.is_some() {
        agent_put_allowed(app, id, p)?;
    }
    if let Some(i) = &p.id
        && id_ok(i, "row").is_err()
        && id_ok(i, "zon").is_err()
    {
        return Err(bad("expected an id that starts zon_ or row_"));
    }
    if let Some(z) = &p.zone {
        id_ok(z, "zon")?;
    }
    let title = p.title.as_deref().map(title_ok).transpose()?;
    let note = p.note.as_deref().map(note_ok).transpose()?;
    if let Some(ptr) = &p.pointer {
        pointer_ok(ptr)?;
    }
    let put = Put {
        id: p.id.as_deref(),
        rev: p.rev,
        entry: p.entry,
        zone: p.zone.as_deref(),
        title,
        kind: p.kind,
        pointer: p.pointer.as_ref(),
        note,
        status: p.status,
        agent: by.agent,
    };
    app.db.put_entry(id, &put, now_ms(), wire::ZONES_MAX, wire::ROWS_MAX)?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Reorder a row or a zone.
pub async fn move_entry(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    entry: &str,
    m: &wire::MoveEntry,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if by.agent.is_some() && is_zone(app, id, entry)? {
        return Err(no_zones());
    }
    app.db
        .move_entry(
            id,
            entry,
            &db::Move { zone: m.zone.as_deref(), before: m.before.as_deref(), rev: m.rev },
            now_ms(),
            wire::ROWS_MAX,
        )?
        .map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Delete a row, or a zone and its rows.
pub async fn delete(app: &Platform, by: Actor<'_>, id: &str, entry: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if let (Some(machine), Some(e)) = (by.agent, app.db.board_entry(id, entry)?) {
        if e.kind == wire::EntryKind::Zone {
            return Err(no_zones());
        }
        if e.agent.as_deref() != Some(machine) {
            return Err(err(
                StatusCode::FORBIDDEN,
                "an agent removes only the rows its own machine added: this one is someone else's",
            ));
        }
    }
    app.db.delete_entry(id, entry, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

// ---------------------------------------------------------------- routes

/// `GET /api/projects/{project}/boards`: a project's boards, open ones
/// first, each oldest first.
pub async fn list_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
) -> J<wire::BoardList> {
    Ok(Json(list(&app, Actor::person(&s.account), &project).await?))
}

/// `POST /api/projects/{project}/boards`: make one (owners and editors).
pub async fn create_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(new): Json<wire::NewBoard>,
) -> J<wire::BoardView> {
    Ok(Json(create(&app, &s.account, &project, &new).await?))
}

/// `GET /api/boards/{id}`: the board with its zones and rows in order.
pub async fn get_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<wire::BoardView> {
    Ok(Json(get(&app, Actor::person(&s.account), &id).await?))
}

/// `PATCH /api/boards/{id}`: set the title or the goal (owners and editors).
pub async fn edit_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(e): Json<wire::EditBoard>,
) -> J<wire::BoardView> {
    Ok(Json(edit(&app, &s.account, &id, &e).await?))
}

/// `POST /api/boards/{id}/close`: close it with a done note (owners and
/// editors).
pub async fn close_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(c): Json<wire::CloseBoard>,
) -> J<wire::BoardView> {
    Ok(Json(close(&app, &s.account, &id, &c).await?))
}

/// `POST /api/boards/{id}/reopen`: open it again, keeping the note (owners
/// and editors).
pub async fn reopen_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<wire::BoardView> {
    Ok(Json(reopen(&app, &s.account, &id).await?))
}

/// `PUT /api/boards/{id}/entries`: make or change a zone or a row (owners
/// and editors); 409 `stale_rev` when the entry moved on.
pub async fn put_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(p): Json<wire::PutEntry>,
) -> J<wire::BoardView> {
    Ok(Json(put(&app, Actor::person(&s.account), &id, &p).await?))
}

/// `POST /api/boards/{id}/entries/{entry}/move`: reorder within or between
/// zones (owners and editors).
pub async fn move_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
    Json(m): Json<wire::MoveEntry>,
) -> J<wire::BoardView> {
    Ok(Json(move_entry(&app, Actor::person(&s.account), &id, &entry, &m).await?))
}

/// `DELETE /api/boards/{id}/entries/{entry}`: a row, or a zone and its rows
/// (owners and editors).
pub async fn delete_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
) -> J<wire::BoardView> {
    Ok(Json(delete(&app, Actor::person(&s.account), &id, &entry).await?))
}

#[cfg(test)]
#[path = "boards_tests.rs"]
mod tests;
