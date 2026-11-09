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
//! Boards are on for everyone: they left Labs (#191), and the old `boards`
//! flag is read by nothing here.
//!
//! **Lanes** (aru-125): a board is rows, a row is one piece of work, and
//! nobody sets its lane. A read answers the board as four synthetic zones
//! (`lane_next`, `lane_doing`, `lane_review`, `lane_done`), each row in the
//! lane [`lanes::lane`] works out from the live state of its items, never
//! stored. What a board's author sets is its **groups** (phases, waves,
//! areas): named, ordered sections, made, renamed, moved and deleted by
//! people and agents alike, with a row's group and its place in it changed
//! by a move. A read answers them as `groups`, with each row's `group`, and
//! a row's `marks` say what its lane doesn't: `merged_not_closed`, `cancelled`, `after`
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
//! **Whose blocks:** an agent attaches only its own machine's blocks; a
//! person, only a block on a machine control routes them to (the hub's
//! `may_reach`, asked on every attach, and refused while the hub can't
//! say). A read shows a block's state, and its machine's id, as a machine
//! Doc names its machine ([`docs`]): one its own machine's agent attached
//! while that machine writes the project, and any to a reader who may reach
//! its machine. To anyone else the chip is a plain agent block, so an
//! attachment made before the check says nothing to those it doesn't
//! concern. That is a label: it waits on the hub [`LABEL_WAIT`] at most,
//! and on an error shows less and goes on; a read asks once for its Docs
//! and blocks together. A row's lane comes from its blocks whoever reads,
//! as from an item the reader can't read, but its stalled mark only from
//! the blocks the reader is told: one they aren't told never stalls it.
//!
//! **Work** ([`work`]): a project's issues, PRs and runs, for a row to point
//! at, and the open issues that no board of the project has a row for.
//!
//! **Live:** a change to a board, to an issue a row points at, or (a
//! webhook) to a mirrored item it points at pushes an object change
//! ([`changed`]), so clients following the board read it again.
//!
//! [`db::boards`]: crate::db::boards

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::{boards as wire, chat::ChatAgent};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::db::boards_docs as db_docs;
use crate::{
    Platform,
    auth::{Machine, Person},
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
use arugula_control_wire::boards::Category;
use lanes::{Fact, Pr, Primary, github_category, plane_category};
use refs::{IssueKey, Refs, Source, references};

pub mod agent;
pub mod asks;
pub mod auto;
pub mod docs;
pub mod epics;
pub mod lanes;
pub mod refs;
pub mod relations;
pub mod resolve;
pub mod steps;
pub mod work;

/// The reason a refusal gives for a write made on a stale rev.
pub const STALE_REV: &str = "stale_rev";
/// The reason a move gives when the group's rows are not in the order it read.
pub const STALE_ORDER: &str = "stale_order";

/// A move that wasn't made: refused as any change is, or its group's rows
/// are in another order than the move read, which answers 409 `stale_order`
/// with the rows as they are ([`wire::StaleOrder`]).
pub enum Unmoved {
    Refused(ApiError),
    Order(Vec<String>),
}

impl From<ApiError> for Unmoved {
    fn from(e: ApiError) -> Self {
        Unmoved::Refused(e)
    }
}

const ORDER_MOVED: &str = "someone moved these rows first: read the board again";

impl From<Unmoved> for ApiError {
    fn from(u: Unmoved) -> Self {
        match u {
            Unmoved::Refused(e) => e,
            Unmoved::Order(_) => refusal(StatusCode::CONFLICT, STALE_ORDER, ORDER_MOVED),
        }
    }
}

impl IntoResponse for Unmoved {
    fn into_response(self) -> Response {
        match self {
            Unmoved::Refused(e) => e.into_response(),
            Unmoved::Order(rows) => (
                StatusCode::CONFLICT,
                Json(wire::StaleOrder { error: ORDER_MOVED.to_owned(), reason: STALE_ORDER.to_owned(), rows }),
            )
                .into_response(),
        }
    }
}

type J<T> = Result<Json<T>, ApiError>;

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

fn no_board() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such board")
}

fn bad(msg: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

/// The project `asked` names (its id, or [`PERSONAL`]) as `account` sees
/// it: 404 for a stranger; with `write`, 403 for a
/// viewer, and the team's roster comes from the hub, not the cache. An
/// agent also needs its machine's grant over the project (`boards.read`, or
/// `boards.write` to change; a write reads too).
async fn place(app: &Platform, by: Actor<'_>, asked: &str, write: bool) -> Result<String, ApiError> {
    let account = by.account;
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
        Refused::Order { .. } => refusal(StatusCode::CONFLICT, STALE_ORDER, ORDER_MOVED),
        Refused::Bad(why) => err(StatusCode::BAD_REQUEST, &why),
        Refused::Forbidden(why) => err(StatusCode::FORBIDDEN, &why),
        Refused::Conflict(why) => err(StatusCode::CONFLICT, &why),
        Refused::Unprocessable(why) => err(StatusCode::UNPROCESSABLE_ENTITY, &why),
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

/// A block pointer's pane as a machine reports it: the plain number. `%N`
/// and `N` both name pane N.
pub(crate) fn plain_pane(pane: &str) -> &str {
    let pane = pane.trim();
    pane.strip_prefix('%').unwrap_or(pane)
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
        wire::Pointer::Doc { id } => id_ok(id, "pdc"),
        wire::Pointer::MachineDoc { machine, doc, title } => {
            name_ok(machine, "machine")?;
            name_ok(doc, "Doc")?;
            title_ok(title).map(|_| ())
        }
    }
}

/// A block pointer as `by` may attach it: an agent attaches only a block on
/// its own machine, a person only one on a machine they may reach (asked of
/// the hub each time, so a machine just joined or shared counts; an error
/// refuses).
async fn block_ok(app: &Platform, by: Actor<'_>, machine: &str) -> Result<(), ApiError> {
    match by.agent {
        Some(own) if own != machine => {
            Err(err(StatusCode::FORBIDDEN, "an agent attaches only blocks on its own machine"))
        }
        Some(_) => Ok(()),
        None => {
            let asked = [machine.to_owned()];
            if app.cache.may_reach(&*app.hub, by.account, &asked).await?.contains(machine) {
                Ok(())
            } else {
                Err(err(StatusCode::FORBIDDEN, "a person attaches only blocks on a machine they can reach"))
            }
        }
    }
}

/// The most a read waits on the hub to say which machines its reader may
/// reach, for what it shows them.
const LABEL_WAIT: Duration = Duration::from_secs(2);

/// Of `machines`, those `account` may reach, for what a read shows them, not
/// a check: [`crate::cache::Cache::may_reach_label`] within [`LABEL_WAIT`].
/// An error or a longer wait is none, with a warning, so the read shows
/// less rather than failing. `what` says what it was for, in the warning.
async fn reach_label(app: &Platform, account: &str, machines: &[String], what: &str) -> HashSet<String> {
    if machines.is_empty() {
        return HashSet::new();
    }
    match tokio::time::timeout(LABEL_WAIT, app.cache.may_reach_label(&*app.hub, account, machines)).await {
        Ok(Ok(reach)) => reach,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "boards: asking whom the hub routes to {what}: showing less");
            HashSet::new()
        }
        Err(_) => {
            tracing::warn!("boards: the hub took over {LABEL_WAIT:?} to say whom it routes to {what}: showing less");
            HashSet::new()
        }
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

/// Name the machine on each board that an agent made or closed, or whose
/// lead runs there.
async fn name_boards(app: &Platform, boards: &mut [wire::Board]) -> Result<(), ApiError> {
    let mut machines: Vec<String> = boards
        .iter()
        .flat_map(|b| [&b.created_by_agent, &b.closed_by_agent])
        .flatten()
        .map(|a| a.machine.clone())
        .chain(boards.iter().filter_map(|b| b.lead.as_ref()).map(|l| l.machine.clone()))
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
    for l in boards.iter_mut().filter_map(|b| b.lead.as_mut()) {
        l.machine_name = names.get(&l.machine).map(|d| d.name.clone()).unwrap_or_default();
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
        labels: serde_json::from_str(&i.labels).unwrap_or_default(),
        provider: None,
        identifier: None,
        state_name: None,
        url: None,
        category: (i.kind == "issue").then(|| github_category(&i.state, i.state_reason.as_deref())),
        closed_with: None,
        closed_by: None,
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
        labels: i.labels.clone(),
        provider: Some("plane".to_owned()),
        identifier: Some(c.identifier.clone()),
        state_name: Some(i.state_name.clone()),
        url: Some(trackers::item_url(&c.workspace_slug, &c.identifier, i.number)),
        category: Some(plane_category(Some(&i.state_group), &i.state)),
        closed_with: None,
        closed_by: None,
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
        labels: vec![],
        provider: None,
        identifier: None,
        state_name: None,
        url: None,
        category: None,
        closed_with: None,
        closed_by: None,
    }
}

/// Whether `machine` holds a live `boards.write` grant over `project`, one
/// its owner may still give ([`agent_may`]). An error is a lookup that
/// failed, not a refusal: the caller can't tell, and mustn't act as if the
/// grant were gone (Arugula #745).
async fn machine_writes(app: &Platform, machine: &str, project: &str) -> Result<bool, ApiError> {
    let grants = app.db.machine_write_projects(machine, BOARDS_WRITE)?;
    for (scope, owner) in grants.into_iter().filter(|(p, _)| p == project) {
        let m = Machine { account: owner, daemon: machine.to_owned() };
        match agent_may(app, &m, BOARDS_WRITE, &AgentScope::Project(scope)).await {
            Ok(_) => return Ok(true),
            // Refused: not this grant.
            Err(e) if e.0.is_client_error() => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
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
        let mut viewer = Viewer::of(app, account).await?;
        let checked = viewer::check(app, &mut viewer, &repos).await?;
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
        // Which PR closed each closed one, and by which words (Arugula #743).
        let closed_ids: Vec<&str> = found.iter().filter(|f| f.closed).map(|f| f.id.as_str()).collect();
        let mut prs = app.db.closing_prs(&closed_ids)?;
        for db::IssueState { id, number, title, closed, category, assignees: who, labels } in found {
            let (closed_with, closed_by) = match prs.remove(&id) {
                Some((pr, by)) => (Some(pr), by),
                None => (None, None),
            };
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
                    labels,
                    provider: None,
                    identifier: None,
                    state_name: None,
                    url: None,
                    category: Some(category),
                    closed_with,
                    closed_by,
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
fn facts(app: &Platform, project: &str, ids: &Ids, blocks: &Blocks) -> Result<Facts, ApiError> {
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
            _ if i.provider == "plane" => {
                let category = plane_category(i.state_group.as_deref(), &i.state);
                let review =
                    category == Category::Started && lanes::names_review(i.state_name.as_deref().unwrap_or_default());
                Fact::Issue { category, review }
            }
            _ => Fact::Issue { category: github_category(&i.state, i.state_reason.as_deref()), review: false },
        };
        f.by_id.insert(i.id, fact);
    }
    f.after = app.db.stacked_on(project, &found)?.into_iter().collect();
    let issues: Vec<&str> = ids.issues.iter().map(String::as_str).collect();
    for i in app.db.issue_states(project, &issues)? {
        f.by_id.insert(i.id, Fact::Issue { category: i.category, review: false });
    }
    for id in &ids.runs {
        if let Some(run) = app.db.run(id)? {
            f.by_id.insert(id.clone(), Fact::Run { running: run.state == RunState::Running });
        }
    }
    // A stale report is unknown: the block moves no lane. One whose machine
    // left is released: closed.
    for (k, b) in blocks.state.iter().filter(|(k, _)| !blocks.stale.contains(*k)) {
        let closed = b.state == wire::BlockWord::Closed || blocks.gone.contains(k);
        f.blocks.insert(k.clone(), Fact::Block { closed });
    }
    Ok(f)
}

impl Facts {
    /// [`Self::of`] for an attachment: a block a newer agent took the row
    /// from (`released`) is closed to this row.
    fn of_attached(&self, p: &wire::Pointer, released: bool) -> Option<Fact> {
        match (p, released) {
            (wire::Pointer::Block { .. }, true) => Some(Fact::Block { closed: true }),
            _ => self.of(p),
        }
    }

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

/// What is attached to a row, as a lane reads it: its role, its pointer, and
/// whether a newer agent took the row from it (`released`).
type Attach<'a> = (wire::AttachRole, &'a wire::Pointer, bool);

/// A row as its facts read it.
struct Assessed {
    lane: wire::Lane,
    /// On a Done row.
    proof: Option<wire::Proof>,
    merged_not_closed: bool,
    /// The primary issue is canceled: Done, never proven.
    cancelled: bool,
}

/// A row's lane and, on Done, its proof: from its primary pointer and what
/// is attached to it as (role, pointer).
fn assess(pointer: &wire::Pointer, attached: &[Attach<'_>], facts: &Facts) -> Assessed {
    let counted = matches!(pointer, wire::Pointer::Issue { .. } | wire::Pointer::ForgeItem { .. });
    let item = counted.then(|| facts.of(pointer)).flatten();
    let primary = if counted { Primary::Item(item.as_ref()) } else { Primary::Note };
    let work: Vec<(&wire::Pointer, Fact)> = attached
        .iter()
        .filter(|(r, ..)| *r == wire::AttachRole::Work)
        .filter_map(|(_, p, released)| facts.of_attached(p, *released).map(|f| (*p, f)))
        .collect();
    let seen: Vec<Fact> = work.iter().map(|(_, f)| *f).collect();
    let lane = lanes::lane(primary, &seen);
    let merged_not_closed = lanes::merged_not_closed(primary, &seen);
    let cancelled = lanes::cancelled(primary);
    if lane != wire::Lane::Done || !counted {
        return Assessed { lane, proof: None, merged_not_closed, cancelled };
    }
    let evidence = attached.iter().find(|(r, ..)| *r == wire::AttachRole::Proof).map(|(_, p, _)| *p);
    Assessed { lane, proof: lanes::proof((pointer, item.as_ref()), &work, evidence), merged_not_closed, cancelled }
}

/// How far a row's derived lane can be trusted, from its blocks' reports
/// (Arugula #745).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Doubt {
    Sure,
    /// A block of it is stale: the lane follows the rest of the work for
    /// now, and a fresh report may bring the block back.
    Stale,
    /// A block of it is on a machine whose grant couldn't be looked up: the
    /// lane may be wrong.
    Unsure,
}

/// The lane a row shows, and the override behind it (#494). A person's lane
/// holds only while the work still derives the lane it did when they set it:
/// once that changes, the row goes back to following the work. While a
/// block of the row couldn't be looked up, it holds as it is.
fn shown(derived: wire::Lane, set: Option<&db::LaneSet>, doubt: Doubt) -> (wire::Lane, Option<&db::LaneSet>) {
    match set.filter(|s| doubt == Doubt::Unsure || s.holds(derived)) {
        Some(s) => (s.lane, Some(s)),
        None => (derived, None),
    }
}

/// Drop the override of `row` that `derived` has outgrown, so it can't come
/// back if the work returns to the lane it was set in. Not while a block of
/// the row is stale or couldn't be looked up: `derived` may change back
/// without the work changing.
fn forget_stale(app: &Platform, board: &str, row: &str, derived: wire::Lane, set: Option<&db::LaneSet>, doubt: Doubt) {
    if doubt != Doubt::Sure {
        return;
    }
    let Some(set) = set.filter(|s| !s.holds(derived)) else { return };
    if let Err(e) = app.db.clear_stale_lane(board, row, set.at) {
        tracing::warn!(error = %e, "boards: clearing a lane that was set by hand");
    }
}

/// A row's marks (see [`wire::Mark`]), from what [`assess`] found:
/// `merged_not_closed`; `cancelled`; `after` the
/// first open PR of the row (its own, then those attached) that is stacked
/// on another, with the repo as `repo_of` says it; the `blocked` marks of
/// its item's open blockers ([`relations`]); `stalled` when [`stall_of`]
/// says so; `earlier` for each closed or taken-over block attached.
fn marks_of(
    assessed: &Assessed,
    stalled: Option<u64>,
    pointer: &wire::Pointer,
    attachments: &[&db::Attached],
    facts: &Facts,
    blocked: Vec<wire::Mark>,
    repo_of: &dyn Fn(&wire::Pointer) -> Option<String>,
) -> Vec<wire::Mark> {
    let mut marks = vec![];
    if assessed.merged_not_closed {
        marks.push(wire::Mark::MergedNotClosed);
    }
    if assessed.cancelled {
        marks.push(wire::Mark::Cancelled);
    }
    let work = attachments.iter().filter(|a| a.role == wire::AttachRole::Work).map(|a| &a.pointer);
    let stacked = std::iter::once(pointer).chain(work).find_map(|p| match p {
        wire::Pointer::ForgeItem { id } => facts.after.get(id).map(|n| (*n, repo_of(p))),
        _ => None,
    });
    if let Some((number, repo)) = stacked {
        marks.push(wire::Mark::After { number, repo });
    }
    marks.extend(stalled.map(|since| wire::Mark::Stalled { since }));
    marks.extend(blocked);
    marks.extend(
        attachments.iter().filter(|a| is_earlier(a, facts)).map(|a| wire::Mark::Earlier { attachment: a.id.clone() }),
    );
    marks
}

/// Whether an attachment is an agent block that was closed.
fn is_earlier(a: &db::Attached, facts: &Facts) -> bool {
    a.role == wire::AttachRole::Work
        && matches!(a.pointer, wire::Pointer::Block { .. })
        && (a.released_at.is_some() || matches!(facts.of(&a.pointer), Some(Fact::Block { closed: true })))
}

/// When the row stalled (see [`lanes::stalled`]), from the blocks as they
/// reported and `minutes`, the project's setting. Only blocks the reader is
/// told count as idle ([`Shown`]), so a block they aren't told holds no
/// stall up to them, and its idle time says nothing.
fn stall_of(
    attachments: &[&db::Attached],
    lane: wire::Lane,
    facts: &Facts,
    told: &Shown<'_>,
    now: u64,
    minutes: u32,
) -> Option<u64> {
    let work = attachments.iter().filter(|a| a.role == wire::AttachRole::Work);
    let seen: Vec<Fact> = work.clone().filter_map(|a| facts.of_attached(&a.pointer, a.released_at.is_some())).collect();
    let idle: Vec<Option<u64>> = work
        .filter(|a| a.released_at.is_none())
        .filter(|a| matches!(facts.of(&a.pointer), Some(Fact::Block { closed: false })))
        .map(|a| match &a.pointer {
            wire::Pointer::Block { machine, pane } => told.idle_since(machine, pane, a.agent.as_deref()),
            _ => None,
        })
        .collect();
    lanes::stalled(lane, &seen, &idle, now, minutes)
}

/// What each machine's blocks reported ([`block_states`]), and whose they
/// are.
#[derive(Default)]
struct Blocks {
    state: HashMap<(String, String), wire::BlockState>,
    /// The account of the machine that reported the block: its owner.
    owner: HashMap<(String, String), String>,
    /// The machines that hold `boards.write` over the project now.
    writing: HashSet<String>,
    /// Reports older than [`wire::BLOCK_STATE_STALE_MS`] from a machine that
    /// sends again, and not of a close: shown, but unknown to lanes and
    /// claims (Arugula #745).
    stale: HashSet<(String, String)>,
    /// Machines whose grant couldn't be looked up: what they reported isn't
    /// here, and isn't known to be gone.
    unsure: HashSet<String>,
    /// Stale blocks whose machine left control: released, so a lane reads
    /// them as closed and no doubt holds the row.
    gone: HashSet<(String, String)>,
}

impl Blocks {
    /// How far the lane of a row with `attached` can be trusted: only the
    /// blocks that are work on it and weren't taken over count.
    fn doubt(&self, attached: &[Attach<'_>]) -> Doubt {
        let mut doubt = Doubt::Sure;
        for (role, p, released) in attached {
            let wire::Pointer::Block { machine, pane } = p else { continue };
            if *role != wire::AttachRole::Work || *released {
                continue;
            }
            if self.unsure.contains(machine) {
                return Doubt::Unsure;
            }
            if self.stale.contains(&(machine.clone(), pane.clone())) {
                doubt = Doubt::Stale;
            }
        }
        doubt
    }

    fn idle_since(&self, machine: &str, pane: &str) -> Option<u64> {
        let b = self.state.get(&(machine.to_owned(), pane.to_owned()))?;
        (b.state == wire::BlockWord::Idle).then_some(b.since).flatten()
    }

    fn owned_by(&self, machine: &str, pane: &str, account: &str) -> bool {
        self.owner.get(&(machine.to_owned(), pane.to_owned())).is_some_and(|o| o == account)
    }
}

/// Which blocks a read shows its reader (the module's top says which): those
/// on a machine the reader may reach, and those its own machine's agent
/// attached while that machine writes the project.
struct Shown<'a> {
    blocks: &'a Blocks,
    reach: HashSet<String>,
}

impl<'a> Shown<'a> {
    /// The machines of the blocks `pointers` point at, each with the machine
    /// whose agent attached it (none: a person), that a read asks the hub
    /// about: those the attachment doesn't settle.
    fn to_ask<'p>(
        blocks: &Blocks,
        pointers: impl Iterator<Item = (&'p wire::Pointer, Option<&'p str>)>,
    ) -> Vec<&'p str> {
        pointers
            .filter_map(|(p, by)| match p {
                wire::Pointer::Block { machine, .. } if !Self::own(blocks, machine, by) => Some(machine.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Its own machine's agent attached it, and the machine writes the
    /// project.
    fn own(blocks: &Blocks, machine: &str, by: Option<&str>) -> bool {
        by == Some(machine) && blocks.writing.contains(machine)
    }

    fn shows(&self, machine: &str, by: Option<&str>) -> bool {
        Self::own(self.blocks, machine, by) || self.reach.contains(machine)
    }

    /// The block's state as this reader is told it: none when they aren't.
    fn state(&self, p: &wire::Pointer, by: Option<&str>) -> Option<wire::BlockState> {
        match p {
            wire::Pointer::Block { machine, pane } if self.shows(machine, by) => {
                self.blocks.state.get(&(machine.clone(), pane.clone())).cloned()
            }
            _ => None,
        }
    }

    /// Since when the block has been idle, as this reader is told it: none
    /// (not idle, so no stall) when they aren't told the block.
    fn idle_since(&self, machine: &str, pane: &str, by: Option<&str>) -> Option<u64> {
        if self.shows(machine, by) { self.blocks.idle_since(machine, pane) } else { None }
    }

    /// The pointer as this reader is told it: a block's machine blanked when
    /// they aren't told the block.
    fn pointer(&self, p: &wire::Pointer, by: Option<&str>) -> wire::Pointer {
        match p {
            wire::Pointer::Block { machine, pane } if !self.shows(machine, by) => {
                wire::Pointer::Block { machine: String::new(), pane: pane.clone() }
            }
            p => p.clone(),
        }
    }
}

/// What each agent block reported, as a board shows it: only when its
/// machine holds `boards.write` over the project now (the grant is asked on
/// every read, so a revoked one hides what it reported).
async fn block_states(app: &Platform, project: &str, blocks: &[(String, String)]) -> Result<Blocks, ApiError> {
    let mut by_machine: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for (machine, pane) in blocks {
        by_machine.entry(machine).or_default().push(pane);
    }
    let mut found = Blocks::default();
    let now = now_ms();
    for (machine, panes) in by_machine {
        match machine_writes(app, machine, project).await {
            Ok(true) => {
                found.writing.insert(machine.to_owned());
            }
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!(machine, error = %e.1, "boards: looking up a machine's grant");
                found.unsure.insert(machine.to_owned());
                continue;
            }
        }
        for r in app.db.block_states(machine, &panes)? {
            let stale = r.stale(now);
            let key = (machine.to_owned(), r.pane);
            if stale {
                found.stale.insert(key.clone());
            }
            found.owner.insert(key.clone(), r.owner);
            found.state.insert(key, r.state);
        }
    }
    release_gone(app, &mut found).await;
    Ok(found)
}

/// Move the stale blocks of machines that left control to `gone`: nothing
/// will be heard from them again, so a row isn't left in doubt. A machine's
/// grants go when it leaves, which hides its reports anyway; this covers
/// the time before control has read that. On an error they stay stale.
async fn release_gone(app: &Platform, found: &mut Blocks) {
    let mut machines: Vec<String> = found.stale.iter().map(|(m, _)| m.clone()).collect();
    if machines.is_empty() {
        return;
    }
    machines.sort();
    machines.dedup();
    let joined = match agent_names(app, &machines).await {
        Ok(joined) => joined,
        Err(e) => {
            tracing::warn!(error = %e, "boards: asking which machines are still in control");
            return;
        }
    };
    let gone: Vec<_> = found.stale.iter().filter(|(m, _)| !joined.contains_key(m)).cloned().collect();
    for k in gone {
        found.stale.remove(&k);
        found.gone.insert(k);
    }
}

/// Fill each board's progress: its rows with an issue or a PR as their item,
/// the ones in the Done, Review and Doing lanes, and the ones of the done
/// that are proven. A lane
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
            let attached: Vec<Attach<'_>> = things
                .iter()
                .filter(|a| a.board == b.id && a.row == t.row)
                .filter_map(|a| a.role.map(|r| (r, &a.pointer, false)))
                .collect();
            let a = assess(&t.pointer, &attached, &facts);
            let doubt = blocks.doubt(&attached);
            forget_stale(app, &b.id, &t.row, a.lane, t.lane_set.as_ref(), doubt);
            let (lane, _) = shown(a.lane, t.lane_set.as_ref(), doubt);
            b.progress.total += 1;
            b.progress.done += u32::from(lane == wire::Lane::Done);
            b.progress.review += u32::from(lane == wire::Lane::Review);
            b.progress.doing += u32::from(lane == wire::Lane::Doing);
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
    machines.extend(stored.attachments.iter().filter_map(|a| match &a.pointer {
        wire::Pointer::Block { machine, .. } => Some(machine.clone()),
        _ => None,
    }));
    let strip = app.db.board_docs(id)?;
    machines.extend(strip.iter().filter_map(|d| match &d.pointer {
        wire::Pointer::MachineDoc { machine, .. } => Some(machine.clone()),
        _ => None,
    }));
    machines.extend(strip.iter().filter_map(|d| d.agent.clone()));
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
    let row_items: Vec<&str> = stored.rows.iter().filter_map(|r| item_id(&r.pointer)).collect();
    let edges = relations::Edges::load(app, account, &project, &row_items).await?;
    let others = edges.others(&row_items);
    let ids = Ids::of(
        stored.rows.iter().map(|r| &r.pointer).chain(stored.attachments.iter().map(|a| &a.pointer)).chain(&others),
    );
    let live = chips(app, account, &project, &ids).await?;
    let blocks = block_states(app, &project, &ids.blocks).await?;
    let facts = facts(app, &project, &ids, &blocks)?;
    let (now, minutes) = (now_ms(), app.db.stall_minutes(&project, wire::STALL_MINUTES_DEFAULT)?);
    // One label call for the board: the machines of its Docs and blocks
    // that the reader is told of only if they may reach them.
    let pointers = stored
        .rows
        .iter()
        .map(|r| (&r.pointer, r.agent.as_deref()))
        .chain(stored.attachments.iter().map(|a| (&a.pointer, a.agent.as_deref())));
    let mut ask: Vec<String> =
        docs::to_ask(&strip).chain(Shown::to_ask(&blocks, pointers)).map(str::to_owned).collect();
    ask.sort();
    ask.dedup();
    let reach = reach_label(app, account, &ask, "a machine on a board").await;
    let named = docs::named(app, &project, &strip, reach.clone()).await;
    let told = Shown { blocks: &blocks, reach };
    // `by`: the machine whose agent put the pointer there, none for a person.
    let chip = |p: &wire::Pointer, by: Option<&str>| -> (Option<wire::Live>, Option<wire::BlockState>) {
        match p {
            wire::Pointer::ForgeItem { id } | wire::Pointer::Issue { id } | wire::Pointer::Run { id } => {
                (live.get(id).cloned(), None)
            }
            wire::Pointer::Block { .. } => (None, told.state(p, by)),
            _ => (None, None),
        }
    };
    let asks = asks::Asks::load(app, account, &project, id).await?;
    let copies = if strip.iter().any(|d| matches!(d.pointer, wire::Pointer::MachineDoc { .. })) {
        docs::copies_in(app, &stored.board.project)?
    } else {
        Default::default()
    };
    let mut row_docs: HashMap<String, Vec<db_docs::Strip>> = HashMap::new();
    let mut strip_only = vec![];
    for d in strip {
        match d.row.clone() {
            Some(r) => row_docs.entry(r).or_default().push(d),
            None => strip_only.push(d),
        }
    }
    let mut zones: Vec<wire::LaneView> = wire::Lane::ALL
        .iter()
        .map(|l| wire::LaneView {
            id: l.id().to_owned(),
            rev: 0,
            title: l.title().to_owned(),
            kind: Some(wire::ZoneKind::Lane),
            rows: vec![],
        })
        .collect();
    let elsewhere = elsewhere_of(app, &project, id, &others)?;
    let at_item: HashMap<&str, usize> =
        stored.rows.iter().enumerate().filter_map(|(i, r)| item_id(&r.pointer).map(|x| (x, i))).collect();
    // For each row, in order: where it sits in `zones`, and what its steps
    // are worked out from.
    let mut placed: Vec<((usize, usize), steps::Node)> = vec![];
    let mut prs: HashMap<(Option<String>, u64), usize> = HashMap::new();
    let mut ready: Vec<String> = vec![];
    let mut groups: Vec<wire::Group> = stored
        .groups
        .iter()
        .map(|g| wire::Group { id: g.id.clone(), rev: g.rev, title: g.title.clone(), progress: Default::default() })
        .collect();
    for e in &stored.rows {
        let mine: Vec<&db::Attached> = stored.attachments.iter().filter(|a| a.row == e.id).collect();
        let attached: Vec<Attach<'_>> = mine.iter().map(|a| (a.role, &a.pointer, a.released_at.is_some())).collect();
        let mut a = assess(&e.pointer, &attached, &facts);
        let stalled = stall_of(&mine, a.lane, &facts, &told, now, minutes);
        let doubt = blocks.doubt(&attached);
        forget_stale(app, id, &e.id, a.lane, e.lane_set.as_ref(), doubt);
        let (lane, set) = shown(a.lane, e.lane_set.as_ref(), doubt);
        a.lane = lane;
        let (row_live, row_block) = chip(&e.pointer, e.agent.as_deref());
        // The attachments of a replaced agent come last.
        let mut ordered = mine.clone();
        ordered.sort_by_key(|a| is_earlier(a, &facts));
        let (relations, blocked) = match item_id(&e.pointer) {
            Some(item) => edges.of(item, &facts, &live, &elsewhere),
            None => (vec![], vec![]),
        };
        let mut node = steps::Node {
            open: item_id(&e.pointer).is_some() && a.lane != wire::Lane::Done,
            started: matches!(a.lane, wire::Lane::Doing | wire::Lane::Review),
            ..Default::default()
        };
        if let Some(item) = item_id(&e.pointer) {
            for other in edges.before(item, &facts) {
                match at_item.get(other) {
                    Some(&j) => node.before.push(j),
                    None => node.off = true,
                }
            }
        }
        let work = mine.iter().filter(|a| a.role == wire::AttachRole::Work).map(|a| &a.pointer);
        for p in std::iter::once(&e.pointer).chain(work) {
            if let (wire::Pointer::ForgeItem { .. }, Some(l)) = (p, chip(p, None).0) {
                prs.entry((l.repo, l.number)).or_insert(placed.len());
            }
        }
        if a.lane == wire::Lane::Next && item_id(&e.pointer).is_some() && blocked.is_empty() {
            ready.push(e.id.clone());
        }
        let marks =
            marks_of(&a, stalled, &e.pointer, &mine, &facts, blocked, &|p| chip(p, None).0.and_then(|l| l.repo));
        let attachments: Vec<wire::Attachment> = ordered
            .iter()
            .map(|a| {
                let by = a.agent.as_deref();
                let (live, block) = chip(&a.pointer, by);
                let released = a.released_at.is_some();
                let pointer = told.pointer(&a.pointer, by);
                wire::Attachment { id: a.id.clone(), role: a.role, pointer, live, block, released }
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
        placed.push(((at, zones[at].rows.len()), node));
        // Needs you, when the reader owns a block that stalled the row.
        let stalled_yours =
            stalled.zip(mine.iter().filter(|a| a.role == wire::AttachRole::Work && a.released_at.is_none()).find_map(
                |a| match &a.pointer {
                    wire::Pointer::Block { machine, pane } if blocks.owned_by(machine, pane, account) => {
                        Some(machine.clone())
                    }
                    _ => None,
                },
            ));
        let raised = asks::raised(row_live.as_ref(), &attachments, stalled_yours, e.updated_at, &|m| {
            names.get(m).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| "a machine".to_owned())
        });
        let (ask, asked) = asks.row(&e.id, raised);
        zones[at].rows.push(wire::Row {
            id: e.id.clone(),
            rev: e.rev,
            pointer: told.pointer(&e.pointer, e.agent.as_deref()),
            note: e.note.clone(),
            // What old rows kept from before lanes doesn't reach clients.
            status: if ask.is_some() { wire::Status::NeedsInput } else { wire::Status::None },
            lane: a.lane,
            lane_set: set.map(|s| wire::LaneSet { lane: s.lane, by: s.by.clone(), at: s.at }),
            attachments,
            docs: docs::view(row_docs.remove(&e.id).unwrap_or_default(), &names, &named, &copies),
            proof: a.proof,
            group: e.zone.clone(),
            marks,
            ask,
            asked,
            relations,
            step: None,
            live: row_live,
            block: row_block,
            agent: e.agent.clone().map(|m| ChatAgent {
                machine_name: names.get(&m).cloned().unwrap_or_default(),
                machine: m,
                session: None,
            }),
            created_at: e.created_at,
            updated_at: e.updated_at,
        });
    }
    // A PR stacked on another waits for it, so its row comes after the row
    // that holds the lower PR.
    for (i, ((z, k), node)) in placed.iter_mut().enumerate() {
        let after = zones[*z].rows[*k].marks.iter().find_map(|m| match m {
            wire::Mark::After { number, repo } => Some((repo.clone(), *number)),
            _ => None,
        });
        node.before.extend(after.and_then(|key| prs.get(&key).copied()).filter(|&j| j != i));
    }
    let nodes: Vec<steps::Node> = placed.iter().map(|(_, n)| n.clone()).collect();
    let found = steps::work_out(&nodes);
    for (i, ((z, k), _)) in placed.iter().enumerate() {
        let row = &mut zones[*z].rows[*k];
        row.step = found.step[i];
        if found.cycle[i] {
            row.marks.push(wire::Mark::Cycle);
        }
    }
    let row_id = |i: &usize| stored.rows[*i].id.clone();
    let (sequence, chain) = (found.sequence.iter().map(row_id).collect(), found.chain.iter().map(row_id).collect());
    let order = stored.rows.iter().map(|r| r.id.clone()).collect();
    let docs = docs::view(strip_only, &names, &named, &copies);
    Ok(wire::BoardView { board, zones, groups, docs, ready, sequence, chain, order, said: None })
}

/// For each of these items (the other ends of a board's edges) that a row
/// of another board of `project` points at, that board's title and lead.
fn elsewhere_of(
    app: &Platform,
    project: &str,
    board: &str,
    others: &[wire::Pointer],
) -> Result<HashMap<String, relations::Elsewhere>, ApiError> {
    let held = app.db.boards_holding(project, &others.iter().filter_map(item_id).collect::<Vec<_>>())?;
    let mut out = HashMap::new();
    if held.iter().all(|(_, b)| b == board) {
        return Ok(out);
    }
    let boards = app.db.boards(project)?;
    for (item, b) in held.into_iter().filter(|(_, b)| b != board) {
        if let (false, Some(b)) = (out.contains_key(&item), boards.iter().find(|x| x.id == b)) {
            out.insert(
                item,
                relations::Elsewhere { board: b.title.clone(), lead: b.lead.as_ref().map(|l| l.session.clone()) },
            );
        }
    }
    Ok(out)
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
        .create_board(&id, title, goal, by, session_ok(new.lead_session.as_deref())?, now_ms(), wire::BOARDS_MAX)?
        .ok_or_else(|| bad(&format!("a project has at most {} boards", wire::BOARDS_MAX)))?;
    read(app, by.account, &board.id).await
}

/// A session's name as a lead: trimmed, one line, at most
/// [`wire::LEAD_SESSION_MAX`] characters. Empty is none.
fn session_ok(session: Option<&str>) -> Result<Option<&str>, ApiError> {
    let Some(s) = session.map(str::trim).filter(|s| !s.is_empty()) else { return Ok(None) };
    if !one_line(s) || s.chars().count() > wire::LEAD_SESSION_MAX {
        return Err(bad(&format!("a session's name is one line of at most {} characters", wire::LEAD_SESSION_MAX)));
    }
    Ok(Some(s))
}

/// The calling session takes a board's lead, or its lead is released
/// (#414). Taking is an agent's: it names the session, and the machine is
/// the one that signed. Releasing is for the lead's own machine or a
/// person; releasing a board nobody leads changes nothing.
pub async fn lead(app: &Platform, by: Actor<'_>, id: &str, l: &wire::LeadBoard) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if l.release {
        if let (Some(machine), Some(lead)) = (by.agent, &board.lead)
            && lead.machine != machine
        {
            return Err(err(
                StatusCode::FORBIDDEN,
                "only the machine of the session that leads this board, or a person, can release it",
            ));
        }
        if board.lead.is_some() && !app.db.set_board_lead(id, None, now_ms())? {
            return Err(no_board());
        }
    } else {
        let (Some(machine), Some(session)) = (by.agent, session_ok(l.session.as_deref())?) else {
            return Err(bad("a lead is a session of an agent's machine, and it has a name"));
        };
        if !app.db.set_board_lead(id, Some((machine, session)), now_ms())? {
            return Err(no_board());
        }
    }
    changed(app, &board.project, id);
    read(app, by.account, id).await
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
        wire::Pointer::Doc { .. } | wire::Pointer::MachineDoc { .. } => {
            Err(bad("a row's item is an issue or a PR: link it to the board's Docs, or to a row's"))
        }
        wire::Pointer::Run { .. } | wire::Pointer::Block { .. } | wire::Pointer::Workspace { .. } => Err(bad(
            "a row's item is an issue or a PR (a note row may carry a link): attach a run or an agent block to the row \
             that it is for, with attach",
        )),
        p => pointer_ok(p),
    }
}

/// Refuse what sets a lane: a lane for a group, a kind, or a status. A
/// question for a person is an ask (#367), not `needs_input`. A person's
/// one way to move a row is [`set_lane`], which no agent has.
fn refuse_lane_writes(p: &wire::PutEntry) -> Result<(), ApiError> {
    if let Some(z) = &p.zone {
        group_id_ok(z)?;
    }
    if p.kind.is_some() {
        return Err(bad("a group has a title and no kind"));
    }
    if p.status == Some(wire::Status::NeedsInput) {
        return Err(bad(
            "ask with a question: a row that needs a person asks them on the board (an ask names who, the question \
             and its options), and that replaces the needs_input status",
        ));
    }
    if p.status.is_some_and(|s| s != wire::Status::None) {
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
    let mut viewer = Viewer::of(app, account).await?;
    let checked = viewer::check(app, &mut viewer, &app.db.linked_repos(project)?).await?;
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

/// Whether another machine's block holds `row` against a block of `machine`
/// (#404): one that is work on the row, was not taken over and isn't
/// reported closed. A block that hasn't reported yet holds it for
/// [`wire::BLOCK_STATE_STALE_MS`] after it was attached (or while its
/// machine's grant can't be looked up); after that, and once its report is
/// stale, it is unknown and holds nothing. Nor does a block whose machine
/// left control (Arugula #745). The machine's own other panes are no
/// conflict (a pane it restarted). Refused 409 `claimed`, saying that
/// `what` ("#12") is being worked by the holder, since when it attached.
pub(crate) async fn claim_conflict(
    app: &Platform,
    project: &str,
    stored: &db::Stored,
    row: &str,
    machine: &str,
    what: &str,
) -> Result<(), ApiError> {
    match claim_holder(app, project, stored, row, machine, what).await? {
        Some(said) => Err(refusal(StatusCode::CONFLICT, wire::CLAIMED, &said)),
        None => Ok(()),
    }
}

/// [`claim_conflict`]'s sentence, none when nothing holds the row.
pub(crate) async fn claim_holder(
    app: &Platform,
    project: &str,
    stored: &db::Stored,
    row: &str,
    machine: &str,
    what: &str,
) -> Result<Option<String>, ApiError> {
    let holders: Vec<&db::Attached> = stored
        .attachments
        .iter()
        .filter(|a| a.row == row && a.role == wire::AttachRole::Work && a.released_at.is_none())
        .filter(|a| matches!(&a.pointer, wire::Pointer::Block { machine: m, .. } if m != machine))
        .collect();
    let panes: Vec<(String, String)> = holders
        .iter()
        .filter_map(|a| match &a.pointer {
            wire::Pointer::Block { machine, pane } => Some((machine.clone(), pane.clone())),
            _ => None,
        })
        .collect();
    let blocks = block_states(app, project, &panes).await?;
    let mut machines: Vec<String> = panes.iter().map(|(m, _)| m.clone()).collect();
    machines.dedup();
    // Only machines still in control: one that left is missing.
    let joined = agent_names(app, &machines).await?;
    let now = now_ms();
    for a in holders {
        let wire::Pointer::Block { machine: m, pane } = &a.pointer else { continue };
        let Some(d) = joined.get(m) else { continue };
        let key = (m.clone(), pane.clone());
        let state = blocks.state.get(&key);
        let holds = match state {
            Some(b) => b.state != wire::BlockWord::Closed && !blocks.stale.contains(&key),
            None => blocks.unsure.contains(m) || now.saturating_sub(a.created_at) <= wire::BLOCK_STATE_STALE_MS,
        };
        if !holds {
            continue;
        }
        // Its title and machine only for a block its own machine's agent
        // attached while the machine writes the project, as a board shows it
        // to everyone.
        let own = Shown::own(&blocks, m, a.agent.as_deref());
        let title = state.filter(|_| own).and_then(|b| b.title.clone()).unwrap_or_else(|| "an agent".to_owned());
        let on = if own && !d.name.is_empty() { d.name.clone() } else { "another machine".to_owned() };
        let at = (a.created_at / 60_000) % 1440;
        return Ok(Some(format!(
            "{what} is being worked by {title} on {on}, since {:02}:{:02} UTC: pass take: true to take it over",
            at / 60,
            at % 60
        )));
    }
    Ok(None)
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
    // A board has one row for a piece of work: the native issue and the
    // GitHub issue it was exported to are one.
    let work_item = app.db.work_item_for(mine)?;
    if let Some(wi) = &work_item
        && let Some(row) = app.db.row_of_work_item(&stored.board.id, wi)?
    {
        return Ok(Plan::Already(format!("that is row {row} on this board already: it was not added again")));
    }
    let found = known(app, account, &stored.board.project, &items, &issues).await?;
    let Some(me) = found.get(mine) else { return Ok(Plan::Add { absorb: vec![], said: None }) };
    let on_board = app.db.board_row_keys(&stored.board.id)?;
    let candidates: Vec<refs::Candidate<'_>> = on_board
        .iter()
        .map(|k| refs::Candidate { row: &k.row, work_item: k.work_item.as_deref(), keys: &k.keys })
        .collect();
    if me.pr {
        // A PR for an issue that is a row.
        if let Some(row) = refs::rows_named(&me.refs, me.repo, &candidates).first() {
            let said = format!(
                "that PR references an issue that is row {row} on this board, so it is attached to that row as \
                 work instead of added as a row of its own"
            );
            return Ok(Plan::Attach { row: (*row).to_owned(), said });
        }
    } else if let Some(key) = &me.key {
        // An issue that PR rows reference: they become its work.
        let keys = match &work_item {
            Some(wi) => app.db.issue_keys_of(mine, wi)?,
            None => vec![key.clone()],
        };
        let new = refs::Candidate { row: NEW_ROW, work_item: work_item.as_deref(), keys: &keys };
        let mut all = candidates.clone();
        all.push(new);
        let absorb: Vec<String> = rows
            .iter()
            .filter(|(_, id)| {
                found.get(*id).is_some_and(|k| k.pr && refs::rows_named(&k.refs, k.repo, &all).contains(&NEW_ROW))
            })
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

/// The row an issue being added would be, among a board's, when a PR row
/// asks which row it names.
const NEW_ROW: &str = "new";

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
    let (mut said, mut change) = (None, true);
    if p.id.is_some() || p.entry == Some(wire::EntryKind::Zone) {
        app.db.put_entry(id, &put, &[], now_ms(), wire::ROWS_MAX, wire::GROUPS_MAX)?.map_err(refused)?;
    } else {
        let (did, s) = add_row(app, by, id, &put, pointer).await?;
        change = did != Did::Already;
        said = s;
    }
    if change {
        changed(app, &board.project, id);
    }
    let mut v = read(app, by.account, id).await?;
    v.said = said;
    Ok(v)
}

/// What adding an item to a board did.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Did {
    Row,
    Attached,
    Already,
}

/// Add one item as a new row, last: [`plan_add`]'s rules, for a single add
/// and for [`add_many`]. What it did, and what it says about it.
async fn add_row(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    put: &Put<'_>,
    pointer: Option<&wire::Pointer>,
) -> Result<(Did, Option<String>), ApiError> {
    let stored = app.db.board_stored(id)?.ok_or_else(no_board)?;
    let plan = match pointer {
        Some(ptr) => plan_add(app, by.account, &stored, ptr).await?,
        None => Plan::Add { absorb: vec![], said: None },
    };
    Ok(match plan {
        Plan::Already(s) => (Did::Already, Some(s)),
        Plan::Attach { row, said } => {
            let ptr = pointer.ok_or_else(|| bad("an attachment points at something"))?;
            app.db
                .attach(id, &row, wire::AttachRole::Work, ptr, by.agent, now_ms(), wire::ATTACHMENTS_MAX)?
                .map_err(refused)?;
            (Did::Attached, Some(said))
        }
        Plan::Add { absorb, said } => {
            app.db.put_entry(id, put, &absorb, now_ms(), wire::ROWS_MAX, wire::GROUPS_MAX)?.map_err(refused)?;
            (Did::Row, said)
        }
    })
}

/// `POST /api/boards/{id}/entries/many`: add the pointers, or the untriaged
/// items that match and aren't in `except`, each as a single add does (owners
/// and editors, people only). More than [`wire::ADD_MANY_MAX`] is a 400.
pub async fn add_many(
    app: &Platform,
    account: &str,
    id: &str,
    body: &wire::AddMany,
) -> Result<wire::AddedMany, ApiError> {
    let by = Actor::person(account);
    let board = board_place(app, by, id, true).await?;
    let pointers: Vec<wire::Pointer> = match (&body.pointers, &body.untriaged) {
        (Some(p), None) if body.except.is_empty() => p.clone(),
        (Some(_), None) => return Err(bad("except goes with untriaged")),
        (None, Some(u)) => {
            let (project, items) =
                work::untriaged(app, account, &u.project, u.q.as_deref(), u.label.as_deref()).await?;
            if project != board.project {
                return Err(bad("untriaged names the board's own project"));
            }
            items.into_iter().map(|i| i.pointer).filter(|p| !body.except.contains(p)).collect()
        }
        _ => return Err(bad("add either pointers or untriaged, one of the two")),
    };
    if pointers.len() > wire::ADD_MANY_MAX {
        return Err(bad(&format!(
            "that is {} items and one call adds at most {}: narrow the filter, or add them in several calls",
            pointers.len(),
            wire::ADD_MANY_MAX
        )));
    }
    for p in &pointers {
        primary_ok(p)?;
    }
    let (mut added, mut attached) = (0, 0);
    let mut failed = None;
    for ptr in &pointers {
        let put = Put { pointer: Some(ptr), ..Put::default() };
        match add_row(app, by, id, &put, Some(ptr)).await {
            Ok((Did::Row, _)) => added += 1,
            Ok((Did::Attached, _)) => attached += 1,
            Ok((Did::Already, _)) => {}
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    if added + attached > 0 {
        changed(app, &board.project, id);
    }
    if let Some(e) = failed {
        return Err(e);
    }
    Ok(wire::AddedMany { added, attached, board: read(app, account, id).await? })
}

/// `POST /api/boards/{id}/entries/many`; see [`add_many`].
pub async fn add_many_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<wire::AddMany>,
) -> J<wire::AddedMany> {
    Ok(Json(add_many(&app, &s.account, &id, &b).await?))
}

/// Attach work or proof to a row: a PR, an agent block or a run as `work`
/// (it moves the row's lane), a run or a link as `proof` (it marks a Done
/// row proven). An item that is on the board already isn't attached again.
pub async fn attach(app: &Platform, by: Actor<'_>, id: &str, a: &wire::AttachRow) -> Result<wire::BoardView, ApiError> {
    let mut a = a.clone();
    if let wire::Pointer::Block { pane, .. } = &mut a.pointer {
        *pane = plain_pane(pane).to_owned();
    }
    let a = &a;
    let board = board_place(app, by, id, true).await?;
    id_ok(&a.row, "row").map_err(|_| bad("expected a row id that starts row_"))?;
    if !a.role.takes(&a.pointer) {
        return Err(bad(match a.role {
            wire::AttachRole::Work => "work attached to a row is a PR, an agent block or a run",
            wire::AttachRole::Proof => "proof attached to a row is a run or a link",
        }));
    }
    pointer_ok(&a.pointer)?;
    if let wire::Pointer::Block { machine, .. } = &a.pointer {
        block_ok(app, by, machine).await?;
    }
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
    let claiming = match (&a.pointer, a.role, by.agent) {
        (wire::Pointer::Block { machine, .. }, wire::AttachRole::Work, Some(_)) => Some(machine.as_str()),
        _ => None,
    };
    if let (Some(machine), false) = (claiming, a.take) {
        let what = match stored.rows.iter().find(|r| r.id == a.row).and_then(|r| item_id(&r.pointer)) {
            Some(item) => {
                let (items, issues): (Vec<&str>, Vec<&str>) =
                    if item.starts_with("iss_") { (vec![], vec![item]) } else { (vec![item], vec![]) };
                match known(app, by.account, &board.project, &items, &issues).await?.get(item) {
                    Some(k) => k.key.as_ref().map_or("this row".to_owned(), |k| format!("#{}", k.number)),
                    None => "this row".to_owned(),
                }
            }
            None => "this row".to_owned(),
        };
        claim_conflict(app, &board.project, &stored, &a.row, machine, &what).await?;
    }
    app.db.attach(id, &a.row, a.role, &a.pointer, by.agent, now_ms(), wire::ATTACHMENTS_MAX)?.map_err(refused)?;
    if let (Some(machine), true) = (claiming, a.take) {
        app.db.release_blocks(&a.row, machine, now_ms())?;
    }
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
) -> Result<wire::BoardView, Unmoved> {
    let board = board_place(app, by, id, true).await?;
    if entry.starts_with("lane_") {
        return Err(not_a_lane().into());
    }
    if id_ok(entry, "zon").is_err() {
        id_ok(entry, "row").map_err(|_| bad("expected an id that starts row_ or zon_"))?;
    }
    if let Some(Some(z)) = &m.zone {
        group_id_ok(z)?;
    }
    let to = db::Move {
        zone: m.zone.as_ref().map(|z| z.as_deref()),
        before: m.before.as_deref(),
        rev: m.rev,
        seen: m.seen.as_deref(),
    };
    match app.db.move_entry(id, entry, &to, now_ms()).map_err(ApiError::from)? {
        Err(Refused::Order { rows }) => return Err(Unmoved::Order(rows)),
        moved => moved.map_err(refused)?,
    }
    changed(app, &board.project, id);
    Ok(read(app, by.account, id).await?)
}

/// The lane `row` derives from its work now, as the board reads it before a
/// person's lane.
async fn derived_lane(
    app: &Platform,
    project: &str,
    stored: &db::Stored,
    row: &db::Entry,
) -> Result<wire::Lane, ApiError> {
    let mine: Vec<&db::Attached> = stored.attachments.iter().filter(|a| a.row == row.id).collect();
    let attached: Vec<Attach<'_>> = mine.iter().map(|a| (a.role, &a.pointer, a.released_at.is_some())).collect();
    let ids = Ids::of(std::iter::once(&row.pointer).chain(mine.iter().map(|a| &a.pointer)));
    let blocks = block_states(app, project, &ids.blocks).await?;
    if blocks.doubt(&attached) == Doubt::Unsure {
        // A lane set against a wrong basis would hold, or lapse, wrongly.
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "couldn't read the row's work just now: try again"));
    }
    let facts = facts(app, project, &ids, &blocks)?;
    Ok(assess(&row.pointer, &attached, &facts).lane)
}

/// Move a row to `s.lane` as a person (#494), the escape hatch for a lane
/// that is wrong or stale; lanes follow the work otherwise.
///
/// - Done closes the row's native issue (as `s.reason` says, completed when
///   none) and sets nothing; a GitHub or Plane item is closed there, and a
///   row with no issue has nothing to close (422).
/// - Next, Doing and Review set a lane that shows while the work still
///   derives the lane it did now ([`db::LaneSet`]). On a closed native issue
///   the move reopens it first; a closed GitHub or Plane item is reopened
///   there (422). Moving a row to the lane it derives takes the override off.
///
/// Owners and editors; closing or reopening is `issues::change`, under the
/// same role. No agent moves a row.
pub async fn set_lane(
    app: &Platform,
    account: &str,
    id: &str,
    row: &str,
    s: &wire::SetLane,
) -> Result<wire::BoardView, ApiError> {
    let by = Actor::person(account);
    let board = board_place(app, by, id, true).await?;
    id_ok(row, "row").map_err(|_| bad("expected a row id that starts row_"))?;
    if s.reason.is_some() && s.lane != wire::Lane::Done {
        return Err(bad("a reason goes with moving a row to Done"));
    }
    let stored = app.db.board_stored(id)?.ok_or_else(no_board)?;
    let entry = stored.rows.iter().find(|r| r.id == row).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such row"))?;
    let unprocessable = |why: &str| err(StatusCode::UNPROCESSABLE_ENTITY, why);
    let mut derived = derived_lane(app, &board.project, &stored, entry).await?;
    let set = match (s.lane, &entry.pointer) {
        (wire::Lane::Done, wire::Pointer::Issue { id: issue }) => {
            if derived != wire::Lane::Done {
                let patch = arugula_control_wire::issues::IssuePatch {
                    state: Some("closed".to_owned()),
                    reason: s.reason.clone(),
                    ..Default::default()
                };
                crate::issues::change(app, by, issue, &patch).await?;
            }
            None
        }
        (wire::Lane::Done, wire::Pointer::ForgeItem { .. }) => {
            return Err(unprocessable("close it in GitHub/Plane to move it to Done"));
        }
        (wire::Lane::Done, _) => {
            return Err(unprocessable("a row with no issue can't be moved to Done: link its issue first"));
        }
        (lane, pointer) => {
            if derived == wire::Lane::Done {
                match pointer {
                    wire::Pointer::Issue { id: issue } => {
                        let patch = arugula_control_wire::issues::IssuePatch {
                            state: Some("open".to_owned()),
                            ..Default::default()
                        };
                        crate::issues::change(app, by, issue, &patch).await?;
                        derived = derived_lane(app, &board.project, &stored, entry).await?;
                    }
                    _ => return Err(unprocessable("reopen it in GitHub/Plane to move it out of Done")),
                }
            }
            if derived == wire::Lane::Done {
                return Err(err(StatusCode::CONFLICT, "the row is still Done: its work closes it"));
            }
            (lane != derived).then(|| db::LaneSet { lane, basis: derived, by: account.to_owned(), at: now_ms() })
        }
    };
    app.db.set_lane(id, row, set.as_ref(), now_ms())?;
    changed(app, &board.project, id);
    read(app, account, id).await
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

/// `GET /api/projects/{project}/board-settings`: a project's board settings
/// (members).
pub async fn settings_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
) -> J<wire::BoardSettings> {
    let project = place(&app, Actor::person(&s.account), &project, false).await?;
    Ok(Json(wire::BoardSettings { stall_minutes: app.db.stall_minutes(&project, wire::STALL_MINUTES_DEFAULT)? }))
}

/// `PUT /api/projects/{project}/board-settings`: set them (owners and
/// editors). `stall_minutes` is 1 to a week; the boards of the project are
/// told, for a row's stalled mark moves with it.
pub async fn put_settings_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(n): Json<wire::BoardSettings>,
) -> J<wire::BoardSettings> {
    let project = place(&app, Actor::person(&s.account), &project, true).await?;
    if !(1..=wire::STALL_MINUTES_MAX).contains(&n.stall_minutes) {
        return Err(bad(&format!("stall_minutes is 1 to {}", wire::STALL_MINUTES_MAX)));
    }
    app.db.set_stall_minutes(&project, n.stall_minutes, now_ms())?;
    for b in app.db.boards(&project)? {
        changed(&app, &project, &b.id);
    }
    Ok(Json(n))
}

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
/// a group's place (owners and editors); a lane is refused, 400. With
/// `seen`, 409 `stale_order` when the group's rows have moved on.
pub async fn move_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
    Json(m): Json<wire::MoveEntry>,
) -> Result<Json<wire::BoardView>, Unmoved> {
    Ok(Json(move_entry(&app, Actor::person(&s.account), &id, &entry, &m).await?))
}

/// `PUT /api/boards/{id}/rows/{row}/lane`: a person moves a row to a lane
/// (owners and editors); see [`set_lane`].
pub async fn set_lane_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, row)): Path<(String, String)>,
    Json(l): Json<wire::SetLane>,
) -> J<wire::BoardView> {
    Ok(Json(set_lane(&app, &s.account, &id, &row, &l).await?))
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
#[path = "boards_asks_tests.rs"]
mod asks_tests;
#[cfg(test)]
#[path = "boards_docs_tests.rs"]
mod docs_tests;
#[cfg(test)]
#[path = "boards_relations_tests.rs"]
mod relations_tests;
#[cfg(test)]
#[path = "boards_tests.rs"]
mod tests;
