//! Boards over the platform's routes (boards, step 1): a
//! project's boards ([`db::boards`]), each an ordered list of rows, in
//! groups if its author made any. A board is a project-scoped platform
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
//! reads too). With `boards.write` an agent may do what a person with the
//! same role does, labelled as the agent (`created_by_agent`,
//! `closed_by_agent`, a row's `agent`): make a board, with a goal, and set
//! its title and goal; close it with a done note and open it again; add
//! rows, set any row's note, and attach work and proof to any row. It may
//! never destroy or rewrite another hand's work: it doesn't delete a row
//! another hand added or repoint one, or take off an attachment another
//! hand made. The rows its own machine added, it may delete and repoint.
//! These rules are here, in the functions both kinds of caller use, and not
//! in the routes that call them. The person's role is still the ceiling on
//! each: a viewer's machine makes nothing.
//!
//! **Gating:** the `boards` Labs flag ([`FLAG`], resolved as chat's is).
//! With it off for the account, every route here answers 404, as if boards
//! weren't there.
//!
//! **Lanes** (aru-125): a board is rows, a row is one piece of work, and
//! nobody sets its lane. A read answers the board as four synthetic zones
//! (`lane_next`, `lane_doing`, `lane_review`, `lane_done`), each row in the
//! lane [`lanes::lane`] works out from the live state of its items, never
//! stored. What a board's author sets is its **groups** (phases, waves,
//! areas): named, ordered sections, made, renamed, moved and deleted by
//! people and agents alike, with a row's group and its place in it changed
//! by a move. A read answers them as `groups`, with each row's `group`, and
//! a row's `marks` say what its lane doesn't: `merged_not_closed`, `after`
//! another open PR it is stacked on, and `earlier` agents that were
//! replaced. A row's primary item is an issue or a PR (its pointer); a PR, an
//! agent block or a run goes with it as a `work` attachment, a run or a link
//! as `proof` (`board_attachments`). A row with no issue or PR is a note and
//! stays in Next. Adding a PR that references the issue of a row attaches it
//! to that row; adding an issue that PR rows reference takes those rows in
//! as its work ([`refs`]).
//!
//! **Pointers are stored, and their state is read:** a row's `fi_`, `iss_`,
//! `run_`, `hw_` id or agent block is kept as given, after a check of its
//! shape. A read adds each item's state now as `live` ([`chips`]): one query
//! for the mirror's items, one for their checks and one for the issues,
//! through the reader's own forge access, for rows and attachments alike. A
//! lane and a proof are worked out from facts, which are the item's state
//! whoever may read it ([`Facts`]), so a reader who can't read a PR still
//! sees the row in Review, and its chip says nothing.
//!
//! **A machine's blocks** (step 4): a daemon tells control the state of its
//! own agent blocks ([`agent::put_block_state`], stored by machine and
//! pane), and a block pointer shows it as `block` in a read
//! ([`block_states`]) when the block's machine holds `boards.write` over
//! the board's project, asked on that read. What the report holds is only
//! what aru-116 allows ([`wire::BlockState`]). A report pushes an object
//! change for the boards whose rows or attachments point at the block.
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
        trackers::TrackerItem,
    },
    forge::{
        links::membership,
        viewer::{self, ReadableConnection, Viewer},
    },
    grants::{AgentScope, agent_may, agent_names},
    issues::PERSONAL,
    trackers,
};
use lanes::{Fact, Pr, Primary};
use refs::{IssueKey, Refs, Source, references};

pub mod agent;
pub mod auto;
pub mod lanes;
pub mod refs;
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

/// 404 unless boards are on for `account`. Plane connections (Arugula
/// #356) are behind the same flag: issues on boards are Labs.
pub(crate) async fn gate(app: &Platform, account: &str) -> Result<(), ApiError> {
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
        Refused::Forbidden(why) => err(StatusCode::FORBIDDEN, &why),
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

/// Name the machine on each board that an agent made or closed.
async fn name_boards(app: &Platform, boards: &mut [wire::Board]) -> Result<(), ApiError> {
    let mut machines: Vec<String> = boards
        .iter()
        .flat_map(|b| [&b.created_by_agent, &b.closed_by_agent])
        .flatten()
        .map(|a| a.machine.clone())
        .collect();
    machines.sort();
    machines.dedup();
    if machines.is_empty() {
        return Ok(());
    }
    let names = agent_names(app, &machines).await?;
    for a in boards.iter_mut().flat_map(|b| [&mut b.created_by_agent, &mut b.closed_by_agent]).flatten() {
        a.machine_name = names.get(&a.machine).map(|d| d.name.clone()).unwrap_or_default();
    }
    Ok(())
}

/// The ids a read looks up, by what they point at.
#[derive(Default)]
struct Ids {
    /// `fi_` ids: mirrored items (GitHub's and a tracker's).
    items: Vec<String>,
    /// `iss_` ids: native issues.
    issues: Vec<String>,
    runs: Vec<String>,
    /// (machine, pane) of agent blocks.
    blocks: Vec<(String, String)>,
}

impl Ids {
    fn of<'a>(pointers: impl Iterator<Item = &'a wire::Pointer>) -> Self {
        let mut ids = Ids::default();
        for p in pointers {
            match p {
                wire::Pointer::ForgeItem { id } => ids.items.push(id.clone()),
                wire::Pointer::Issue { id } => ids.issues.push(id.clone()),
                wire::Pointer::Run { id } => ids.runs.push(id.clone()),
                wire::Pointer::Block { machine, pane } => ids.blocks.push((machine.clone(), pane.clone())),
                _ => {}
            }
        }
        for v in [&mut ids.items, &mut ids.issues, &mut ids.runs] {
            v.sort_unstable();
            v.dedup();
        }
        ids.blocks.sort_unstable();
        ids.blocks.dedup();
        ids
    }
}

/// Whether a check on a head is red.
fn red(c: &CheckState) -> bool {
    c.status == "completed"
        && matches!(
            c.conclusion.as_deref(),
            Some("failure" | "error" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
        )
}

/// A forge item's state now, from the checks on its head.
fn live_item(i: &ForgeItem, repo: &str, checks: &[&CheckState]) -> wire::Live {
    use wire::LiveState::*;
    let state = match (i.kind.as_str(), i.state.as_str()) {
        (_, "merged") => Merged,
        (_, "closed") => Closed,
        ("pr", _) => {
            if checks.iter().any(|c| red(c)) {
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
        provider: None,
        identifier: None,
        state_name: None,
        url: None,
    }
}

/// A Plane item as a board shows it: open or closed, with Plane's own name
/// for the state, and its link.
fn live_plane(i: &TrackerItem, c: &ReadableConnection) -> wire::Live {
    wire::Live {
        state: if i.state == "closed" { wire::LiveState::Closed } else { wire::LiveState::Open },
        kind: "issue".to_owned(),
        repo: Some(c.name.clone()),
        number: i.number,
        title: i.title.clone(),
        assignees: vec![],
        provider: Some("plane".to_owned()),
        identifier: Some(c.identifier.clone()),
        state_name: Some(i.state_name.clone()),
        url: Some(trackers::item_url(&c.workspace_slug, &c.identifier, i.number)),
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
        provider: None,
        identifier: None,
        state_name: None,
        url: None,
    }
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

/// Each of `ids` the reader may see, as a chip says it: items of the
/// project's linked repos they may read (the viewer check), its tracker
/// items, its issues, and runs of a project they are a member of. What they
/// may not read is not in the answer, and says nothing.
async fn chips(
    app: &Platform,
    account: &str,
    project: &str,
    ids: &Ids,
) -> Result<HashMap<String, wire::Live>, ApiError> {
    let mut live: HashMap<String, wire::Live> = HashMap::new();
    if !ids.items.is_empty() {
        let repos = app.db.linked_repos(project)?;
        let viewer = Viewer::of(app, account).await?;
        let checked = viewer::check(app, &viewer, &repos).await?;
        let found: Vec<&str> = ids.items.iter().map(String::as_str).collect();
        // A Plane item is read through the connections of the project it's on a board of.
        let conns = viewer::check_connections(app, &viewer, &app.db.tracker_connections(project)?).await?;
        for i in app.db.tracker_items_among(&conns, &found)? {
            if let Some(c) = conns.get(i.mirror_id) {
                live.insert(i.id.clone(), live_plane(&i, c));
            }
        }
        let found_items = app.db.forge_items_among(&checked.readable, &found)?;
        let heads: Vec<&str> = found_items.iter().filter_map(|i| i.head_sha.as_deref()).collect();
        let checks = app.db.forge_checks_on_heads(&checked.readable, &heads)?;
        for i in &found_items {
            let repo = repos.iter().find(|r| r.repo_id == i.repo_id).map_or("", |r| r.full_name.as_str());
            let mine: Vec<_> = checks
                .iter()
                .filter(|c| c.repo_id == i.repo_id && Some(c.head_sha.as_str()) == i.head_sha.as_deref())
                .collect();
            live.insert(i.id.clone(), live_item(i, repo, &mine));
        }
    }
    if !ids.issues.is_empty() {
        let issue_ids: Vec<&str> = ids.issues.iter().map(String::as_str).collect();
        let found = app.db.issue_states(project, &issue_ids)?;
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
            live.insert(
                id,
                wire::Live {
                    state,
                    kind: "native".to_owned(),
                    repo: None,
                    number,
                    title,
                    assignees,
                    provider: None,
                    identifier: None,
                    state_name: None,
                    url: None,
                },
            );
        }
    }
    // The runs routes' own check: a member of the run's project.
    let mut may: HashMap<String, bool> = HashMap::new();
    for id in &ids.runs {
        let Some(run) = app.db.run(id)? else { continue };
        if !may.contains_key(&run.project) {
            let ok = membership(app, account, &run.project, false).await.is_ok();
            may.insert(run.project.clone(), ok);
        }
        if may[&run.project] {
            live.insert(id.clone(), live_run(&run));
        }
    }
    Ok(live)
}

/// What an item, issue, run or block is now, whoever may read it: what a
/// lane is worked out from ([`lanes`]). It holds no title, so a reader who
/// can't see an item learns only what moved the row.
#[derive(Default)]
struct Facts {
    /// By `fi_`, `iss_` and `run_` id.
    by_id: HashMap<String, Fact>,
    /// By (machine, pane): as reported, when the machine's grant stands.
    blocks: HashMap<(String, String), Fact>,
    /// By `fi_` id of an open PR: the number of the open PR of its repo that
    /// it is stacked on (it targets that PR's branch).
    after: HashMap<String, u64>,
}

/// The facts for `ids`, in `project`; `blocks` is what [`block_states`] found.
fn facts(
    app: &Platform,
    project: &str,
    ids: &Ids,
    blocks: &HashMap<(String, String), wire::BlockState>,
) -> Result<Facts, ApiError> {
    let mut f = Facts::default();
    let found: Vec<&str> = ids.items.iter().map(String::as_str).collect();
    let (items, checks) = app.db.item_facts(project, &found)?;
    for i in items {
        let fact = match i.kind.as_str() {
            "pr" => Fact::Pr(match i.state.as_str() {
                "merged" => {
                    let mine: Vec<&CheckState> = checks
                        .iter()
                        .filter(|c| c.repo_id as i64 == i.repo_id && Some(c.head_sha.as_str()) == i.head_sha.as_deref())
                        .collect();
                    Pr::Merged { green: !mine.is_empty() && mine.iter().all(|c| c.status == "completed" && !red(c)) }
                }
                "closed" => Pr::Closed,
                _ => Pr::Open,
            }),
            _ => Fact::Issue { closed: i.state != "open" },
        };
        f.by_id.insert(i.id, fact);
    }
    f.after = app.db.stacked_on(project, &found)?.into_iter().collect();
    let issues: Vec<&str> = ids.issues.iter().map(String::as_str).collect();
    for i in app.db.issue_states(project, &issues)? {
        f.by_id.insert(i.id, Fact::Issue { closed: i.closed });
    }
    for id in &ids.runs {
        if let Some(run) = app.db.run(id)? {
            f.by_id.insert(id.clone(), Fact::Run { running: run.state == RunState::Running });
        }
    }
    for (k, b) in blocks {
        f.blocks.insert(k.clone(), Fact::Block { closed: b.state == wire::BlockWord::Closed });
    }
    Ok(f)
}

impl Facts {
    fn of(&self, p: &wire::Pointer) -> Option<Fact> {
        match p {
            wire::Pointer::ForgeItem { id } | wire::Pointer::Issue { id } | wire::Pointer::Run { id } => {
                self.by_id.get(id).copied()
            }
            wire::Pointer::Block { machine, pane } => self.blocks.get(&(machine.clone(), pane.clone())).copied(),
            _ => None,
        }
    }
}

/// A row as its facts read it.
struct Assessed {
    lane: wire::Lane,
    /// On a Done row.
    proof: Option<wire::Proof>,
    merged_not_closed: bool,
}

/// A row's lane and, on Done, its proof: from its primary pointer and what
/// is attached to it as (role, pointer).
fn assess(pointer: &wire::Pointer, attached: &[(wire::AttachRole, &wire::Pointer)], facts: &Facts) -> Assessed {
    let counted = matches!(pointer, wire::Pointer::Issue { .. } | wire::Pointer::ForgeItem { .. });
    let item = counted.then(|| facts.of(pointer)).flatten();
    let primary = if counted { Primary::Item(item.as_ref()) } else { Primary::Note };
    let work: Vec<(&wire::Pointer, Fact)> = attached
        .iter()
        .filter(|(r, _)| *r == wire::AttachRole::Work)
        .filter_map(|(_, p)| facts.of(p).map(|f| (*p, f)))
        .collect();
    let seen: Vec<Fact> = work.iter().map(|(_, f)| *f).collect();
    let lane = lanes::lane(primary, &seen);
    let merged_not_closed = lanes::merged_not_closed(primary, &seen);
    if lane != wire::Lane::Done || !counted {
        return Assessed { lane, proof: None, merged_not_closed };
    }
    let evidence = attached.iter().find(|(r, _)| *r == wire::AttachRole::Proof).map(|(_, p)| *p);
    Assessed { lane, proof: lanes::proof((pointer, item.as_ref()), &work, evidence), merged_not_closed }
}

/// A row's marks (see [`wire::Mark`]): `merged_not_closed`; `after` the
/// first open PR of the row (its own, then those attached) that is stacked
/// on another, with the repo as `repo_of` says it; `earlier` for each
/// closed block attached.
fn marks_of(
    merged_not_closed: bool,
    pointer: &wire::Pointer,
    attachments: &[&db::Attached],
    facts: &Facts,
    repo_of: &dyn Fn(&wire::Pointer) -> Option<String>,
) -> Vec<wire::Mark> {
    let mut marks = vec![];
    if merged_not_closed {
        marks.push(wire::Mark::MergedNotClosed);
    }
    let work = attachments.iter().filter(|a| a.role == wire::AttachRole::Work).map(|a| &a.pointer);
    let stacked = std::iter::once(pointer).chain(work).find_map(|p| match p {
        wire::Pointer::ForgeItem { id } => facts.after.get(id).map(|n| (*n, repo_of(p))),
        _ => None,
    });
    if let Some((number, repo)) = stacked {
        marks.push(wire::Mark::After { number, repo });
    }
    marks.extend(
        attachments.iter().filter(|a| is_earlier(a, facts)).map(|a| wire::Mark::Earlier { attachment: a.id.clone() }),
    );
    marks
}

/// Whether an attachment is an agent block that was closed.
fn is_earlier(a: &db::Attached, facts: &Facts) -> bool {
    a.role == wire::AttachRole::Work
        && matches!(a.pointer, wire::Pointer::Block { .. })
        && matches!(facts.of(&a.pointer), Some(Fact::Block { closed: true }))
}

/// What each agent block reported, as a board shows it: only when its
/// machine holds `boards.write` over the project now (the grant is asked on
/// every read, so a revoked one hides what it reported).
async fn block_states(
    app: &Platform,
    project: &str,
    blocks: &[(String, String)],
) -> Result<HashMap<(String, String), wire::BlockState>, ApiError> {
    let mut by_machine: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for (machine, pane) in blocks {
        by_machine.entry(machine).or_default().push(pane);
    }
    let mut states: HashMap<(String, String), wire::BlockState> = HashMap::new();
    for (machine, panes) in by_machine {
        if !machine_writes(app, machine, project).await {
            continue;
        }
        for (pane, state) in app.db.block_states(machine, &panes)? {
            states.insert((machine.to_owned(), pane), state);
        }
    }
    Ok(states)
}

/// Fill each board's progress: its rows with an issue or a PR as their item,
/// the ones in the Done lane, and the ones of those that are proven. A lane
/// is worked out from the facts whoever reads, so a row the reader can't
/// read counts as it does for everyone. One read of the project's rows and
/// attachments, and one per kind of item.
async fn progress(app: &Platform, project: &str, boards: &mut [wire::Board]) -> Result<(), ApiError> {
    let things = app.db.project_things(project)?;
    if things.is_empty() {
        return Ok(());
    }
    let ids = Ids::of(things.iter().map(|t| &t.pointer));
    let blocks = block_states(app, project, &ids.blocks).await?;
    let facts = facts(app, project, &ids, &blocks)?;
    for b in boards {
        for t in things.iter().filter(|t| t.board == b.id && t.role.is_none()) {
            if !matches!(t.pointer, wire::Pointer::Issue { .. } | wire::Pointer::ForgeItem { .. }) {
                continue;
            }
            let attached: Vec<(wire::AttachRole, &wire::Pointer)> = things
                .iter()
                .filter(|a| a.board == b.id && a.row == t.row)
                .filter_map(|a| a.role.map(|r| (r, &a.pointer)))
                .collect();
            let a = assess(&t.pointer, &attached, &facts);
            b.progress.total += 1;
            b.progress.done += u32::from(a.lane == wire::Lane::Done);
            b.progress.proven += u32::from(a.proof.is_some());
        }
    }
    Ok(())
}

/// The board `id` as `account` may read it: its four lanes with their rows,
/// each row's lane and proof from the facts, and its chips (live state,
/// block state) only for what the reader may see.
async fn read(app: &Platform, account: &str, id: &str) -> Result<wire::BoardView, ApiError> {
    let stored = app.db.board_stored(id)?.ok_or_else(no_board)?;
    let mut machines: Vec<String> = stored.rows.iter().filter_map(|e| e.agent.clone()).collect();
    machines.sort();
    machines.dedup();
    let names: HashMap<String, String> = if machines.is_empty() {
        Default::default()
    } else {
        agent_names(app, &machines).await?.into_iter().map(|(id, d)| (id, d.name)).collect()
    };
    let project = stored.board.project.clone();
    let mut board = stored.board.clone();
    name_boards(app, std::slice::from_mut(&mut board)).await?;
    progress(app, &project, std::slice::from_mut(&mut board)).await?;
    let ids = Ids::of(stored.rows.iter().map(|r| &r.pointer).chain(stored.attachments.iter().map(|a| &a.pointer)));
    let live = chips(app, account, &project, &ids).await?;
    let blocks = block_states(app, &project, &ids.blocks).await?;
    let facts = facts(app, &project, &ids, &blocks)?;
    let chip = |p: &wire::Pointer| -> (Option<wire::Live>, Option<wire::BlockState>) {
        match p {
            wire::Pointer::ForgeItem { id } | wire::Pointer::Issue { id } | wire::Pointer::Run { id } => {
                (live.get(id).cloned(), None)
            }
            wire::Pointer::Block { machine, pane } => (None, blocks.get(&(machine.clone(), pane.clone())).cloned()),
            _ => (None, None),
        }
    };
    let mut zones: Vec<wire::Zone> = wire::Lane::ALL
        .iter()
        .map(|l| wire::Zone {
            id: l.id().to_owned(),
            rev: 0,
            title: l.title().to_owned(),
            kind: Some(wire::ZoneKind::Lane),
            rows: vec![],
        })
        .collect();
    let mut groups: Vec<wire::Group> = stored
        .groups
        .iter()
        .map(|g| wire::Group { id: g.id.clone(), rev: g.rev, title: g.title.clone(), progress: Default::default() })
        .collect();
    for e in &stored.rows {
        let mine: Vec<&db::Attached> = stored.attachments.iter().filter(|a| a.row == e.id).collect();
        let attached: Vec<(wire::AttachRole, &wire::Pointer)> = mine.iter().map(|a| (a.role, &a.pointer)).collect();
        let a = assess(&e.pointer, &attached, &facts);
        let (row_live, row_block) = chip(&e.pointer);
        // The attachments of a replaced agent come last.
        let mut ordered = mine.clone();
        ordered.sort_by_key(|a| is_earlier(a, &facts));
        let marks = marks_of(a.merged_not_closed, &e.pointer, &mine, &facts, &|p| chip(p).0.and_then(|l| l.repo));
        let attachments = ordered
            .iter()
            .map(|a| {
                let (live, block) = chip(&a.pointer);
                wire::Attachment { id: a.id.clone(), role: a.role, pointer: a.pointer.clone(), live, block }
            })
            .collect();
        if let Some(g) = groups.iter_mut().find(|g| Some(&g.id) == e.zone.as_ref())
            && matches!(e.pointer, wire::Pointer::Issue { .. } | wire::Pointer::ForgeItem { .. })
        {
            g.progress.total += 1;
            g.progress.done += u32::from(a.lane == wire::Lane::Done);
            g.progress.proven += u32::from(a.proof.is_some());
        }
        let at = wire::Lane::ALL.iter().position(|l| *l == a.lane).unwrap_or(0);
        zones[at].rows.push(wire::Row {
            id: e.id.clone(),
            rev: e.rev,
            pointer: e.pointer.clone(),
            note: e.note.clone(),
            status: e.status,
            lane: a.lane,
            attachments,
            proof: a.proof,
            group: e.zone.clone(),
            marks,
            live: row_live,
            block: row_block,
            agent: e
                .agent
                .clone()
                .map(|m| ChatAgent { machine_name: names.get(&m).cloned().unwrap_or_default(), machine: m }),
            created_at: e.created_at,
            updated_at: e.updated_at,
        });
    }
    Ok(wire::BoardView { board, zones, groups, said: None })
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

/// Tracker items (`fi_` ids) were written or removed: the boards that
/// point at them.
pub fn items_changed(app: &Platform, ids: &[String]) {
    match app.db.boards_on_item_ids(ids) {
        Ok(boards) => boards_changed(app, &boards),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on a tracker's item"),
    }
}

/// Each of these boards (id, project) changed.
pub fn boards_changed(app: &Platform, boards: &[(String, String)]) {
    boards.iter().for_each(|(b, p)| changed(app, p, b));
}

/// What an agent's write to a board may be: anything a person's may, but
/// to repoint a row only one its own machine added.
fn agent_put_allowed(app: &Platform, board: &str, machine: &str, p: &wire::PutEntry) -> Result<(), ApiError> {
    if let (Some(id), Some(_)) = (&p.id, &p.pointer)
        && let Some(e) = app.db.board_entry(board, id)?
        && e.agent.as_deref() != Some(machine)
    {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent repoints only the rows its own machine added: this one is someone else's",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- the seam

/// A project's boards as `account` may read them.
pub async fn list(app: &Platform, by: Actor<'_>, project: &str) -> Result<wire::BoardList, ApiError> {
    let id = place(app, by, project, false).await?;
    let mut boards = app.db.boards(&id)?;
    name_boards(app, &mut boards).await?;
    progress(app, &id, &mut boards).await?;
    Ok(wire::BoardList { boards, project: id })
}

/// Make a board in `project`, with its goal if one is given.
pub async fn create(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    new: &wire::NewBoard,
) -> Result<wire::BoardView, ApiError> {
    let id = place(app, by, project, true).await?;
    let title = title_ok(&new.title)?;
    let goal = new.goal.as_deref().map(|g| text_ok(g, "a goal")).transpose()?.unwrap_or("");
    let board = app
        .db
        .create_board(&id, title, goal, by, now_ms(), wire::BOARDS_MAX)?
        .ok_or_else(|| bad(&format!("a project has at most {} boards", wire::BOARDS_MAX)))?;
    read(app, by.account, &board.id).await
}

/// A board with its zones and rows in order, as `account` may read it.
pub async fn get(app: &Platform, by: Actor<'_>, id: &str) -> Result<wire::BoardView, ApiError> {
    board_place(app, by, id, false).await?;
    read(app, by.account, id).await
}

/// Set a board's title or goal.
pub async fn edit(app: &Platform, by: Actor<'_>, id: &str, e: &wire::EditBoard) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    let title = e.title.as_deref().map(title_ok).transpose()?;
    let goal = e.goal.as_deref().map(|g| text_ok(g, "a goal")).transpose()?;
    if title.is_none() && goal.is_none() {
        return Err(bad("nothing to change: give a title or a goal"));
    }
    if !app.db.edit_board(id, title, goal, now_ms())? {
        return Err(no_board());
    }
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Close a board with its done note. The issues and rows are left as they
/// are.
pub async fn close(app: &Platform, by: Actor<'_>, id: &str, c: &wire::CloseBoard) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    let note = text_ok(&c.done_note, "a done note")?;
    if note.is_empty() {
        return Err(bad("say what was done: a board closes with a note"));
    }
    if !app.db.close_board(id, note, by, now_ms())? {
        return Err(no_board());
    }
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Open a closed board again, keeping its done note.
pub async fn reopen(app: &Platform, by: Actor<'_>, id: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if !app.db.reopen_board(id, now_ms())? {
        return Err(no_board());
    }
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// A write refused because lanes follow the work: `why` is the rest of the
/// sentence (aru-125).
fn follows(why: &str) -> ApiError {
    bad(&format!("{}: {why}", wire::LANES_FOLLOW))
}

/// What a lane id in a group's place is told.
fn not_a_lane() -> ApiError {
    follows(
        "a lane is never set, and is no group. A row goes in a group (zon_…) or in none; its lane changes as its \
         issue closes, its PR opens or merges, or its agent block or run works",
    )
}

/// `id` as a group's: a lane is refused with why, anything else is a `zon_` id.
fn group_id_ok(id: &str) -> Result<(), ApiError> {
    if id.starts_with("lane_") {
        return Err(not_a_lane());
    }
    id_ok(id, "zon").map_err(|_| bad("expected a group id that starts zon_"))
}

/// A row's item is an issue or a PR, or nothing, or (a note) a link; a run
/// or an agent block is attached, and a workspace is neither.
fn primary_ok(p: &wire::Pointer) -> Result<(), ApiError> {
    match p {
        wire::Pointer::Run { .. } | wire::Pointer::Block { .. } | wire::Pointer::Workspace { .. } => Err(bad(
            "a row's item is an issue or a PR (a note row may carry a link): attach a run or an agent block to the row \
             that it is for, with attach",
        )),
        p => pointer_ok(p),
    }
}

/// Refuse what sets a lane: a lane for a group, a kind, or a status other
/// than `needs_input`.
fn refuse_lane_writes(p: &wire::PutEntry) -> Result<(), ApiError> {
    if let Some(z) = &p.zone {
        group_id_ok(z)?;
    }
    if p.kind.is_some() {
        return Err(bad("a group has a title and no kind"));
    }
    if p.status.is_some_and(|s| !matches!(s, wire::Status::NeedsInput | wire::Status::None)) {
        return Err(follows(
            "a row's lane comes from the live state of its issue, PR, agent block and run, so there is no status to \
             set: attach the work to the row instead",
        ));
    }
    Ok(())
}

/// What a row's item is, for finding the issue a PR is for: a PR with what it
/// references, or an issue as a reference names it.
struct Known {
    pr: bool,
    repo: Option<u64>,
    key: Option<IssueKey>,
    refs: Refs,
}

/// The `fi_` and `iss_` ids among `items` and `issues` as the reader may
/// see them: a PR with the references in its title, branch and (when the
/// content cache has it) body, an issue with its key. An item they may not
/// read isn't there.
async fn known(
    app: &Platform,
    account: &str,
    project: &str,
    items: &[&str],
    issues: &[&str],
) -> Result<HashMap<String, Known>, ApiError> {
    let mut out: HashMap<String, Known> = HashMap::new();
    for i in app.db.issue_states(project, issues)? {
        out.insert(
            i.id,
            Known {
                pr: false,
                repo: None,
                key: Some(IssueKey { source: Source::Native, number: i.number }),
                refs: Refs::default(),
            },
        );
    }
    if items.is_empty() {
        return Ok(out);
    }
    let viewer = Viewer::of(app, account).await?;
    let checked = viewer::check(app, &viewer, &app.db.linked_repos(project)?).await?;
    let conns = viewer::check_connections(app, &viewer, &app.db.tracker_connections(project)?).await?;
    for i in app.db.tracker_items_among(&conns, items)? {
        if let Some(c) = conns.get(i.mirror_id) {
            let key = IssueKey { source: Source::Plane(c.identifier.clone()), number: i.number };
            out.insert(i.id, Known { pr: false, repo: None, key: Some(key), refs: Refs::default() });
        }
    }
    for i in app.db.forge_items_among(&checked.readable, items)? {
        if i.kind == "pr" {
            let body = app
                .db
                .forge_content(&checked.readable, i.repo_id, i.number)?
                .and_then(|rows| rows.into_iter().find(|r| r.kind == "body"))
                .map(|r| r.body)
                .unwrap_or_default();
            let refs = references(&i.title, i.head_ref.as_deref().unwrap_or(""), &body, Some(i.number));
            out.insert(i.id, Known { pr: true, repo: Some(i.repo_id), key: None, refs });
        } else {
            let key = IssueKey { source: Source::Repo(i.repo_id), number: i.number };
            out.insert(i.id, Known { pr: false, repo: Some(i.repo_id), key: Some(key), refs: Refs::default() });
        }
    }
    Ok(out)
}

/// What adding an item to a board does (aru-125: one row is one piece of
/// work).
enum Plan {
    /// A row of its own; `absorb` are the PR rows that become its work.
    Add { absorb: Vec<String>, said: Option<String> },
    /// A PR for an issue that is a row: attached to it as work.
    Attach { row: String, said: String },
    /// There already: nothing changes, and this says where.
    Already(String),
}

/// Where `pointer` is on the board already, if it is.
fn already(stored: &db::Stored, pointer: &wire::Pointer) -> Option<String> {
    if let Some(r) = stored.rows.iter().find(|r| &r.pointer == pointer) {
        return Some(format!("that is row {} on this board already: it was not added again", r.id));
    }
    stored
        .attachments
        .iter()
        .find(|a| &a.pointer == pointer)
        .map(|a| format!("that is attached to row {} as {} already: it was not attached again", a.row, a.role.as_str()))
}

/// The id a pointer to an issue or a mirrored item holds.
fn item_id(p: &wire::Pointer) -> Option<&str> {
    match p {
        wire::Pointer::ForgeItem { id } | wire::Pointer::Issue { id } => Some(id),
        _ => None,
    }
}

async fn plan_add(
    app: &Platform,
    account: &str,
    stored: &db::Stored,
    pointer: &wire::Pointer,
) -> Result<Plan, ApiError> {
    if let Some(said) = already(stored, pointer) {
        return Ok(Plan::Already(said));
    }
    let Some(mine) = item_id(pointer) else { return Ok(Plan::Add { absorb: vec![], said: None }) };
    let rows: Vec<(&str, &str)> =
        stored.rows.iter().filter_map(|r| item_id(&r.pointer).map(|id| (r.id.as_str(), id))).collect();
    let (mut items, mut issues): (Vec<&str>, Vec<&str>) = (vec![], vec![]);
    for (_, id) in &rows {
        if id.starts_with("iss_") { issues.push(id) } else { items.push(id) }
    }
    if mine.starts_with("iss_") {
        issues.push(mine)
    } else {
        items.push(mine)
    }
    let found = known(app, account, &stored.board.project, &items, &issues).await?;
    let Some(me) = found.get(mine) else { return Ok(Plan::Add { absorb: vec![], said: None }) };
    if me.pr {
        // A PR for an issue that is a row.
        for (row, id) in &rows {
            if let Some(key) = found.get(*id).and_then(|k| k.key.as_ref())
                && me.refs.name(me.repo, key)
            {
                let said = format!(
                    "that PR references an issue that is row {row} on this board, so it is attached to that row as \
                     work instead of added as a row of its own"
                );
                return Ok(Plan::Attach { row: (*row).to_owned(), said });
            }
        }
    } else if let Some(key) = &me.key {
        // An issue that PR rows reference: they become its work.
        let absorb: Vec<String> = rows
            .iter()
            .filter(|(_, id)| found.get(*id).is_some_and(|k| k.pr && k.refs.name(k.repo, key)))
            .map(|(row, _)| (*row).to_owned())
            .collect();
        if !absorb.is_empty() {
            let said = format!(
                "{} PR row(s) on this board reference that issue, so they are attached to its row as work instead of \
                 rows of their own",
                absorb.len()
            );
            return Ok(Plan::Add { absorb, said: Some(said) });
        }
    }
    Ok(Plan::Add { absorb: vec![], said: None })
}

/// Add a row or a group, or change one (see [`wire::PutEntry`]). Adding an issue or a
/// PR that is on the board already, or that goes with a row there, changes
/// no rows: see [`Plan`], and the answer's `said`.
pub async fn put(app: &Platform, by: Actor<'_>, id: &str, p: &wire::PutEntry) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    refuse_lane_writes(p)?;
    if let Some(machine) = by.agent {
        agent_put_allowed(app, id, machine, p)?;
    }
    if let Some(i) = &p.id {
        if i.starts_with("lane_") {
            return Err(not_a_lane());
        }
        if id_ok(i, "zon").is_err() {
            id_ok(i, "row").map_err(|_| bad("expected an id that starts row_ or zon_"))?;
        }
    }
    let title = p.title.as_deref().map(title_ok).transpose()?;
    let note = p.note.as_deref().map(note_ok).transpose()?;
    let pointer = p.pointer.as_ref().filter(|ptr| **ptr != wire::Pointer::None);
    if let Some(ptr) = &p.pointer {
        primary_ok(ptr)?;
    }
    let mut said = None;
    let mut change = true;
    let put = Put {
        id: p.id.as_deref(),
        rev: p.rev,
        entry: p.entry,
        zone: p.zone.as_deref(),
        title,
        pointer: p.pointer.as_ref(),
        note,
        status: p.status,
        agent: by.agent,
    };
    if p.id.is_some() || p.entry == Some(wire::EntryKind::Zone) {
        app.db.put_entry(id, &put, &[], now_ms(), wire::ROWS_MAX, wire::GROUPS_MAX)?.map_err(refused)?;
    } else {
        let stored = app.db.board_stored(id)?.ok_or_else(no_board)?;
        let plan = match pointer {
            Some(ptr) => plan_add(app, by.account, &stored, ptr).await?,
            None => Plan::Add { absorb: vec![], said: None },
        };
        match plan {
            Plan::Already(s) => {
                change = false;
                said = Some(s);
            }
            Plan::Attach { row, said: s } => {
                let ptr = pointer.ok_or_else(|| bad("an attachment points at something"))?;
                app.db
                    .attach(id, &row, wire::AttachRole::Work, ptr, by.agent, now_ms(), wire::ATTACHMENTS_MAX)?
                    .map_err(refused)?;
                said = Some(s);
            }
            Plan::Add { absorb, said: s } => {
                app.db.put_entry(id, &put, &absorb, now_ms(), wire::ROWS_MAX, wire::GROUPS_MAX)?.map_err(refused)?;
                said = s;
            }
        }
    }
    if change {
        changed(app, &board.project, id);
    }
    let mut v = read(app, by.account, id).await?;
    v.said = said;
    Ok(v)
}

/// Attach work or proof to a row: a PR, an agent block or a run as `work`
/// (it moves the row's lane), a run or a link as `proof` (it marks a Done
/// row proven). An item that is on the board already isn't attached again.
pub async fn attach(app: &Platform, by: Actor<'_>, id: &str, a: &wire::AttachRow) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    id_ok(&a.row, "row").map_err(|_| bad("expected a row id that starts row_"))?;
    if !a.role.takes(&a.pointer) {
        return Err(bad(match a.role {
            wire::AttachRole::Work => "work attached to a row is a PR, an agent block or a run",
            wire::AttachRole::Proof => "proof attached to a row is a run or a link",
        }));
    }
    pointer_ok(&a.pointer)?;
    let stored = app.db.board_stored(id)?.ok_or_else(no_board)?;
    if !stored.rows.iter().any(|r| r.id == a.row) {
        return Err(refused(Refused::Missing("no such row")));
    }
    if let Some(said) = already(&stored, &a.pointer) {
        let mut v = read(app, by.account, id).await?;
        v.said = Some(said);
        return Ok(v);
    }
    if let (wire::AttachRole::Work, wire::Pointer::ForgeItem { id: item }) = (a.role, &a.pointer)
        && known(app, by.account, &board.project, &[item], &[]).await?.get(item.as_str()).is_some_and(|k| !k.pr)
    {
        return Err(bad("work attached to a row is a PR: an issue is a row of its own"));
    }
    app.db.attach(id, &a.row, a.role, &a.pointer, by.agent, now_ms(), wire::ATTACHMENTS_MAX)?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Take an attachment off its row. An agent takes off only those its own
/// machine made.
pub async fn detach(app: &Platform, by: Actor<'_>, id: &str, attachment: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    id_ok(attachment, "att").map_err(|_| bad("expected an id that starts att_"))?;
    if let (Some(machine), Some(a)) = (by.agent, app.db.attachment(id, attachment)?)
        && a.agent.as_deref() != Some(machine)
    {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent takes off only the attachments its own machine made: this one is someone else's",
        ));
    }
    app.db.detach(id, attachment, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Change a row's group and its place in it, or a group's place among
/// groups. That is planning, so people and agents alike may. A lane is never
/// set (aru-125): a move to one is refused.
pub async fn move_entry(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    entry: &str,
    m: &wire::MoveEntry,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if entry.starts_with("lane_") {
        return Err(not_a_lane());
    }
    if id_ok(entry, "zon").is_err() {
        id_ok(entry, "row").map_err(|_| bad("expected an id that starts row_ or zon_"))?;
    }
    if let Some(Some(z)) = &m.zone {
        group_id_ok(z)?;
    }
    let to = db::Move { zone: m.zone.as_ref().map(|z| z.as_deref()), before: m.before.as_deref(), rev: m.rev };
    app.db.move_entry(id, entry, &to, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Delete a row, with what is attached to it, or a group with its rows. An
/// agent deletes only rows its own machine added, and a group only when
/// every row in it is one.
pub async fn delete(app: &Platform, by: Actor<'_>, id: &str, entry: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if entry.starts_with("lane_") {
        return Err(not_a_lane());
    }
    if let (Some(machine), Some(e)) = (by.agent, app.db.board_entry(id, entry)?)
        && e.agent.as_deref() != Some(machine)
    {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent removes only the rows its own machine added: this one is someone else's",
        ));
    }
    app.db.delete_entry(id, entry, by.agent, now_ms())?.map_err(refused)?;
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
    Ok(Json(create(&app, Actor::person(&s.account), &project, &new).await?))
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
    Ok(Json(edit(&app, Actor::person(&s.account), &id, &e).await?))
}

/// `POST /api/boards/{id}/close`: close it with a done note (owners and
/// editors).
pub async fn close_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(c): Json<wire::CloseBoard>,
) -> J<wire::BoardView> {
    Ok(Json(close(&app, Actor::person(&s.account), &id, &c).await?))
}

/// `POST /api/boards/{id}/reopen`: open it again, keeping the note (owners
/// and editors).
pub async fn reopen_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<wire::BoardView> {
    Ok(Json(reopen(&app, Actor::person(&s.account), &id).await?))
}

/// `PUT /api/boards/{id}/entries`: add a row or a group, or change a row's
/// note or pointer or a group's title (owners and editors); 409 `stale_rev`
/// when it moved on.
pub async fn put_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(p): Json<wire::PutEntry>,
) -> J<wire::BoardView> {
    Ok(Json(put(&app, Actor::person(&s.account), &id, &p).await?))
}

/// `POST /api/boards/{id}/entries/{entry}/move`: a row's group and place, or
/// a group's place (owners and editors); a lane is refused, 400.
pub async fn move_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
    Json(m): Json<wire::MoveEntry>,
) -> J<wire::BoardView> {
    Ok(Json(move_entry(&app, Actor::person(&s.account), &id, &entry, &m).await?))
}

/// `DELETE /api/boards/{id}/entries/{entry}`: a row and what is attached to
/// it, or a group and its rows (owners and editors).
pub async fn delete_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
) -> J<wire::BoardView> {
    Ok(Json(delete(&app, Actor::person(&s.account), &id, &entry).await?))
}

/// `POST /api/boards/{id}/attachments`: attach work or proof to a row (owners
/// and editors).
pub async fn attach_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(a): Json<wire::AttachRow>,
) -> J<wire::BoardView> {
    Ok(Json(attach(&app, Actor::person(&s.account), &id, &a).await?))
}

/// `DELETE /api/boards/{id}/attachments/{attachment}`: take an attachment off
/// its row (owners and editors).
pub async fn detach_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, attachment)): Path<(String, String)>,
) -> J<wire::BoardView> {
    Ok(Json(detach(&app, Actor::person(&s.account), &id, &attachment).await?))
}

#[cfg(test)]
#[path = "boards_tests.rs"]
mod tests;
