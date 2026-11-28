//! Ladder starts (aru-143): a chief of staff or a board's lead starts an
//! agent for a board on one of its person's machines, this one or another,
//! under the board's standing grant.
//!
//! - **The caller** ([`start`]) asks control for a start ticket, which
//!   control signs only within the board's grant and for a target that's
//!   online, then sends it with the start: to [`START_PATH`] on another
//!   machine over the peer channel (`peer.rs`, an `agents` machine token),
//!   or straight to [`start_here`] for this one, so the grant counts every
//!   start wherever it runs.
//! - **The target** ([`start_post`], [`start_here`]) checks the ticket
//!   itself: the hub's signature, for this machine, of its own account, from
//!   the machine that sent it, taken once, and that the start is the one it
//!   names. Remote input off here refuses it, as it would a person's typing.
//!   It opens the agent block in a new worktree of its own clone of the repo,
//!   then attaches it to its issue's row with the ticket (control counts it
//!   against the grant) or makes it the board's lead, keeps who started it on
//!   the block, and writes the start to the remote sessions log.
//!
//! - **A stop** ([`stop`], [`stop_post`], Arugula #1438): the lead closes an
//!   issue agent on another machine over the same channel, with no ticket
//!   (it only frees what a start took). The target closes only a block a
//!   ladder start opened, for the machine that started it or the board's
//!   lead as control says it, and tells control at once that it closed, so
//!   the grant's slot is free for the next start. Here, `close` does the
//!   same, and lets the board's lead close one in another tab
//!   ([`lead_closes`], Arugula #1471).
//!
//! Another account's agents still send tasks (aru-050): a ticket names the
//! target's own account, and the target refuses any other.

use std::sync::Arc;

use arugula_control_wire::{
    boards::agent_board,
    boards::{BoardLead, LeadBoard, agent_board_lead},
    ladder::{
        LadderStart, LadderStarted, LadderStop, LadderStopped, Rung, START_PATH, STOP_PATH, StartClaims, StartTicket,
        StartTicketAsk, start_ticket_path,
    },
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use reqwest::Method;
use serde_json::json;

use crate::{acl::Principal, control::CloudRefused, mux::Api, server::App};

/// What a caller here asks for: a ladder start on `machine` (none: this
/// one).
#[derive(Debug, Clone, Default)]
pub struct Ask {
    /// A machine of this account, by name or id; none or this one's: here.
    pub machine: Option<String>,
    pub board: String,
    pub rung: Option<Rung>,
    pub issue: Option<String>,
    pub recipe: Option<String>,
    pub repo: Option<String>,
    pub prompt: String,
    pub session: Option<String>,
    /// Its block's title, which its lead and its row's hold go by; default
    /// its session's name.
    pub title: Option<String>,
    /// The calling session's name.
    pub by_session: Option<String>,
    /// Its session's permission mode, tools it may use without asking, and
    /// whether it takes the person's own Claude Code settings there.
    pub permission_mode: Option<String>,
    pub allow: Vec<String>,
    pub user_settings: bool,
}

/// Permission modes that skip every check: an agent never hands one to
/// another; only the person starts an agent like that.
pub const SKIPS_CHECKS: &[&str] = &["bypassPermissions", "full-access", "agent-full-access"];

/// Start an agent for a board, here or on another machine of this account,
/// under the board's grant: what the target answered, or why not, in words
/// an agent can pass on to its person.
pub async fn start(app: &Arc<App>, ask: Ask) -> Result<LadderStarted, String> {
    let e = app.control.enrolled().ok_or("this machine hasn't joined Arugula cloud: a ladder start needs it")?;
    let rung = ask.rung.unwrap_or(if ask.issue.is_some() { Rung::Issue } else { Rung::Lead });
    if rung == Rung::Issue && ask.issue.is_none() {
        return Err("an issue agent needs its issue".into());
    }
    if let Some(m) = ask.permission_mode.as_deref().map(str::trim).filter(|m| SKIPS_CHECKS.contains(m)) {
        return Err(format!("permission_mode {m} skips every check: only the user starts an agent like that"));
    }
    let here = e.keys.id();
    let target = match ask.machine.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if m == here || m == app.hosts.name() => None,
        Some(m) => Some(crate::peer::find(&app.control, m).await.map_err(|e| e.to_string())?),
        None => None,
    };
    let to = target.as_ref().map_or(here.clone(), |m| m.id.clone());
    let body = serde_json::to_value(StartTicketAsk { to, rung, issue: ask.issue.clone(), recipe: ask.recipe.clone() })
        .map_err(|e| e.to_string())?;
    let ticket: StartTicket = match app.control.cloud(Method::POST, &start_ticket_path(&ask.board), Some(&body)).await {
        Ok(v) => serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent a ticket I can't read: {e}"))?,
        Err(CloudRefused::Said { said, .. }) => return Err(said),
        Err(why) => return Err(format!("no start ticket: {why}")),
    };
    let start = LadderStart {
        ticket: ticket.ticket,
        board: ask.board,
        rung,
        issue: ask.issue,
        recipe: ask.recipe,
        repo: ask.repo,
        prompt: ask.prompt,
        session: ask.session,
        title: ask.title,
        by: arugula_control_wire::ladder::StartedBy { machine: String::new(), session: ask.by_session },
        permission_mode: ask.permission_mode,
        allow: ask.allow,
        user_settings: ask.user_settings,
    };
    let Some(m) = target else {
        let claims = app.control.check_start_ticket(&start.ticket).await.map_err(|why| format!("the ticket: {why}"))?;
        return start_here(app, claims, start, &here, app.hosts.name()).await.map_err(|(_, why)| why);
    };
    let body = serde_json::to_value(&start).map_err(|e| e.to_string())?;
    let (status, bytes) = crate::peer::request(&app.control, &m, "POST", START_PATH, Some(&body), &[])
        .await
        .map_err(|e| format!("{}: {e:#}", m.name))?;
    if status != 200 {
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let why =
            v["error"].as_str().map(str::to_owned).unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        return Err(format!("{} refused it ({status}): {why}", m.name));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("{} answered something I can't read: {e}", m.name))
}

fn refuse(status: StatusCode, why: impl Into<String>) -> Response {
    (status, Json(json!({ "error": why.into() }))).into_response()
}

/// `POST /api/a2a/start`: a ladder start here, from another machine of this
/// account over its channel, or from this machine's owner.
pub async fn start_post(
    State(app): State<Arc<App>>,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(s): Json<LadderStart>,
) -> Response {
    let Some(e) = app.control.enrolled() else {
        return refuse(StatusCode::NOT_FOUND, "this machine hasn't joined Arugula cloud");
    };
    // Another machine is let in as its account (`e2e.rs`), never as the
    // owner: it's known by what its channel proves. With no channel, only
    // this machine's owner.
    let (from, from_name) = match &dev {
        Some(d) => (d.0.device.clone(), d.0.device.clone()),
        None if who.as_ref().is_none_or(|w| w.0.is_owner()) => (e.keys.id(), app.hosts.name().to_owned()),
        None => return refuse(StatusCode::FORBIDDEN, "a ladder start comes from a machine of this account"),
    };
    let claims = match app.control.check_start_ticket(&s.ticket).await {
        Ok(c) => c,
        Err(why) => return refuse(StatusCode::FORBIDDEN, format!("the start ticket was refused: {why}")),
    };
    if let Some(d) = &dev
        && d.0.account != claims.sub
    {
        return refuse(StatusCode::FORBIDDEN, "the start ticket is another account's");
    }
    let name = match &dev {
        Some(_) => machine_name(&app, &from).await.unwrap_or(from_name),
        None => from_name,
    };
    match start_here(&app, claims, s, &from, &name).await {
        Ok(started) => Json(started).into_response(),
        Err((status, why)) => refuse(status, why),
    }
}

/// A machine's name, from control's directory.
async fn machine_name(app: &App, id: &str) -> Option<String> {
    crate::peer::machines(&app.control).await.ok()?.into_iter().find(|m| m.id == id).map(|m| m.name)
}

/// The start `s`, under its checked ticket `claims`, sent by machine `from`
/// (`from_name` to show): checked against the ticket, then opened here.
pub async fn start_here(
    app: &Arc<App>,
    claims: StartClaims,
    s: LadderStart,
    from: &str,
    from_name: &str,
) -> Result<LadderStarted, (StatusCode, String)> {
    let forbid = |why: &str| (StatusCode::FORBIDDEN, why.to_owned());
    let e = app.control.enrolled().ok_or_else(|| forbid("this machine hasn't joined Arugula cloud"))?;
    if claims.sub != e.saved.cert.account {
        return Err(forbid("the start ticket is another account's: another account sends a task"));
    }
    if claims.from != from {
        return Err(forbid("the start ticket was made for another machine to send"));
    }
    let blank = |o: &Option<String>| o.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
    if claims.board != s.board
        || claims.rung != s.rung
        || blank(&claims.issue) != blank(&s.issue)
        || blank(&claims.recipe) != blank(&s.recipe)
    {
        return Err(forbid("this start isn't the one its ticket names (board, rung, issue and recipe)"));
    }
    if !arugula_proto::sharing::remote_input(app.control.state_dir()) {
        return Err(forbid("remote input is off on this machine, so nobody starts agents here through control"));
    }
    if s.prompt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "give the agent a prompt".into()));
    }
    if let Some(m) = blank(&s.permission_mode).filter(|m| SKIPS_CHECKS.contains(&m.as_str())) {
        return Err(forbid(&format!(
            "permission_mode {m} skips every check: only the person starts an agent like that"
        )));
    }
    let worn = worn_recipe(claims.rung, blank(&s.recipe)).map_err(|why| (StatusCode::BAD_REQUEST, why))?;
    let issue = blank(&s.issue);
    let session = blank(&s.session).unwrap_or_else(|| default_session(s.rung, issue.as_deref(), &s.board));
    // A title: a post that names it finds the block (agent mentions name an
    // agent block by its title), so it's the name the lead and the row's
    // hold go by, and the board's or the issue's thread reaches it. Its own,
    // else the session's name.
    let title = blank(&s.title).unwrap_or_else(|| session.clone());
    if let Some(i) = &issue
        && let Some(held) = crate::board_attach::held(app, Some(claims.project.clone()), i, Some(title.clone())).await
    {
        return Err((StatusCode::CONFLICT, format!("{held}; no agent was started")));
    }
    let (cwd, worktree) = match blank(&s.repo) {
        Some(repo) => {
            let clone = clone_of(app, &repo)
                .await
                .ok_or_else(|| (StatusCode::NOT_FOUND, format!("{} has no clone of {repo}", app.hosts.name())))?;
            let runner = crate::worktree::local(app).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
            let mut branch = format!("ladder/{session}");
            if git_ok(&clone, &["show-ref", "--verify", "-q", &format!("refs/heads/{branch}")]).await {
                branch.push('-');
                branch.push_str(&claims.jti[..claims.jti.len().min(6)]);
            }
            let made = crate::worktree::make(&runner, &clone, &branch, "", &repo, true)
                .await
                .map_err(|e| (StatusCode::CONFLICT, format!("no worktree for it in {clone}: {e}")))?;
            let owned = crate::worktree::Owned { repo: made.repo, path: made.path.clone(), branch, base: made.base };
            (made.path, Some(owned))
        }
        None => (arugula_mux::sys::home().display().to_string(), None),
    };
    let ladder = crate::board_attach::LadderWork {
        board: claims.board.clone(),
        rung: claims.rung,
        grant: claims.grant.clone(),
        ticket: claims.jti.clone(),
        by_machine: from_name.to_owned(),
        by_id: from.to_owned(),
        by_session: s.by.session.clone(),
    };
    let mut work = json!({ "project": claims.project, "ladder": ladder });
    if let Some(i) = &issue {
        work["issue"] = json!(i);
    }
    // Arugula #1440: the rung's own recipe first, so a lead runs its board
    // and an issue agent knows it leads its issue's thread.
    let prompt = match &issue {
        Some(i) => crate::mcp::registry::issue_agent_prompt(i, Some(&claims.board), &s.prompt),
        None => crate::mcp::registry::board_lead_prompt(Some(&claims.board), &s.prompt)
            .map_err(|why| (StatusCode::SERVICE_UNAVAILABLE, why))?,
    };
    // Titled (its start's title, else its session's name), it leads its
    // issue's thread (or is found on the board's) by that name from the
    // start, not after its first turn names it (Arugula #1440, #1416).
    let mut config = json!({ "agent": "claude", "prompt": prompt, "cwd": cwd, "work": work, "title": title });
    if let Some(w) = &worktree {
        config["worktree"] = json!(w);
    }
    if let Some(r) = &worn {
        config["recipe"] = json!(r);
    }
    if let Some(m) = blank(&s.permission_mode) {
        config["permission_mode"] = json!(m);
    }
    if !s.allow.is_empty() {
        config["allow"] = s.allow.iter().map(|t| json!({ "tool": t })).collect();
    }
    if s.user_settings {
        config["user_settings"] = json!(true);
    }
    let req = arugula_proto::api::OpenRequest {
        kind: arugula_proto::BlockType::Agent,
        config,
        session: Some(session.clone()),
        local: true,
        ..Default::default()
    };
    let block = match app.mux.api(|r| Api::Open(req, None, r)).await {
        Some(Ok(b)) => b,
        Some(Err(why)) => {
            if let (Some(w), Ok(runner)) = (&worktree, crate::worktree::local(app).await) {
                crate::worktree::settle(&runner, w).await;
            }
            return Err((StatusCode::BAD_REQUEST, why));
        }
        None => return Err((StatusCode::SERVICE_UNAVAILABLE, "the daemon is stopping".into())),
    };
    let said = match (&claims.rung, &issue) {
        (Rung::Issue, Some(i)) => {
            let take = false;
            match crate::board_attach::keep_as(
                app,
                Some(claims.project.clone()),
                i,
                block,
                take,
                Some(title.clone()),
                Some(claims.jti.clone()),
            )
            .await
            {
                Ok(on) if on.is_empty() => format!("no board has a row for {i}"),
                Ok(on) => {
                    let titles: Vec<&str> = on.iter().map(|b| b.title.as_str()).collect();
                    format!("on {i}'s row on {}", titles.join(", "))
                }
                Err(why) => why.to_string(),
            }
        }
        _ => {
            // Its conversation, once the block has one (Arugula #1563); a block
            // that hasn't started one yet leads by its title.
            let conversation = crate::agent_mentions::conversation(app, block).await;
            let lead = LeadBoard { session: Some(title.clone()), pane: Some(block), conversation, release: false };
            let body = serde_json::to_value(lead).unwrap_or_default();
            match app.control.cloud(Method::POST, &agent_board_lead(&claims.board), Some(&body)).await {
                Ok(_) => "it leads the board".to_owned(),
                Err(why) => format!("it doesn't lead the board: {why}"),
            }
        }
    };
    crate::remote_log::agent_started(
        app.control.state_dir(),
        &crate::remote_log::AgentStarted {
            event: "agent",
            at: arugula_e2e::now_ms(),
            account: &claims.sub,
            from: from_name,
            board: &claims.board,
            rung: claims.rung.as_str(),
            issue: issue.as_deref(),
            recipe: blank(&s.recipe).as_deref(),
            block,
        },
    );
    tracing::info!(from = from_name, board = claims.board, rung = claims.rung.as_str(), block, "ladder start");
    Ok(LadderStarted { machine: app.hosts.name().to_owned(), id: e.keys.id(), block, session, cwd, said })
}

/// The recipe a start's block wears (`.claude/agents/NAME.md` there), from
/// the one it names: none for its rung's own recipe, which its prompt holds
/// already (Arugula #1440), and another rung's or the chief of staff's is
/// refused.
fn worn_recipe(rung: Rung, named: Option<String>) -> Result<Option<String>, String> {
    use crate::mcp::registry::{BOARD_LEAD, ISSUE_AGENT, own_recipe};
    let own = match rung {
        Rung::Lead => BOARD_LEAD,
        Rung::Issue => ISSUE_AGENT,
    };
    match named {
        Some(r) if r == own => Ok(None),
        Some(r) if own_recipe(&r) => Err(format!("recipe {r} isn't a {} start's: it runs {own}", rung.as_str())),
        other => Ok(other),
    }
}

/// Stop agent block `block` on `machine` (by name or id), one a ladder start
/// opened there: what it answered, or why not. None: `machine` is this one,
/// where `close` does it.
pub async fn stop(app: &Arc<App>, machine: &str, block: u32) -> Result<Option<LadderStopped>, String> {
    let e = app.control.enrolled().ok_or("this machine hasn't joined Arugula cloud: a stop elsewhere needs it")?;
    let machine = machine.trim();
    if machine.is_empty() || machine == e.keys.id() || machine == app.hosts.name() {
        return Ok(None);
    }
    let m = crate::peer::find(&app.control, machine).await.map_err(|e| e.to_string())?;
    let body = serde_json::to_value(LadderStop { block }).map_err(|e| e.to_string())?;
    let (status, bytes) = crate::peer::request(&app.control, &m, "POST", STOP_PATH, Some(&body), &[])
        .await
        .map_err(|e| format!("{}: {e:#}", m.name))?;
    if status != 200 {
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let why =
            v["error"].as_str().map(str::to_owned).unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        return Err(format!("{} refused it ({status}): {why}", m.name));
    }
    let mut stopped: LadderStopped =
        serde_json::from_slice(&bytes).map_err(|e| format!("{} answered something I can't read: {e}", m.name))?;
    // By its name in control, as the lead named it.
    stopped.machine = m.name;
    Ok(Some(stopped))
}

/// `POST /api/a2a/stop`: close an agent block a ladder start opened here,
/// for the machine that started it or the board's lead (another machine of
/// this account, over its channel), or this machine's owner.
pub async fn stop_post(
    State(app): State<Arc<App>>,
    who: Option<axum::Extension<Principal>>,
    dev: Option<axum::Extension<crate::e2e::Caller>>,
    Json(s): Json<LadderStop>,
) -> Response {
    let Some(e) = app.control.enrolled() else {
        return refuse(StatusCode::NOT_FOUND, "this machine hasn't joined Arugula cloud");
    };
    let from = match &dev {
        Some(d) if d.0.account != e.saved.cert.account => {
            return refuse(StatusCode::FORBIDDEN, "another account's machine stops nothing here");
        }
        Some(d) => Some(d.0.device.clone()),
        None if who.as_ref().is_none_or(|w| w.0.is_owner()) => None,
        None => return refuse(StatusCode::FORBIDDEN, "a ladder stop comes from a machine of this account"),
    };
    let block = s.block;
    let config = app.mux.api(|r| Api::Block(block, r)).await.flatten().map(|b| b.config());
    let ladder = config
        .and_then(|c| c["work"].get("ladder").cloned())
        .and_then(|l| serde_json::from_value::<crate::board_attach::LadderWork>(l).ok());
    let Some(ladder) = ladder else {
        return refuse(StatusCode::NOT_FOUND, format!("%{block} here isn't an agent a ladder start opened"));
    };
    if let Some(from) = &from
        && *from != ladder.by_id
        && lead_machine(&app, &ladder.board).await.as_deref() != Some(from.as_str())
    {
        return refuse(
            StatusCode::FORBIDDEN,
            format!("%{block} is stopped by the machine that started it ({}) or the board's lead", ladder.by_machine),
        );
    }
    let Some((closed, told)) = crate::ops::close::close(&app, block).await else {
        return refuse(StatusCode::NOT_FOUND, format!("%{block} is gone already"));
    };
    let mut said = match &closed.worktree {
        Some(w) => crate::worktree::sentence(w),
        None => "it had no worktree of its own".to_owned(),
    };
    said.push_str(match told {
        true => "; Arugula cloud was told it closed, so its slot under the board's grant is free",
        false => "; Arugula cloud wasn't told yet: its slot frees when it is",
    });
    tracing::info!(from = from.as_deref().unwrap_or("owner"), board = ladder.board, block, "ladder stop");
    Json(LadderStopped { machine: app.hosts.name().to_owned(), id: e.keys.id(), block, said }).into_response()
}

/// The machine of the session that leads `board`, as control says it.
async fn lead_machine(app: &App, board: &str) -> Option<String> {
    lead_of(app, board).await.map(|l| l.machine)
}

/// The session that leads `board`, as control says it.
async fn lead_of(app: &App, board: &str) -> Option<BoardLead> {
    let v = app.control.cloud(Method::GET, &agent_board(board), None).await.ok()?;
    serde_json::from_value(v.pointer("/board/lead")?.clone()).ok()
}

/// Whether agent block `me` here may close block `block`, in another tab
/// (Arugula #1471): `block` is one a ladder start opened here for a board,
/// and control says `me` leads that board, by its pane on this machine. A
/// lead taken without its pane (an older daemon's) doesn't count: a name
/// alone could be any session's.
pub(crate) async fn lead_closes(app: &App, me: arugula_proto::PaneId, block: arugula_proto::PaneId) -> bool {
    let Some(e) = app.control.enrolled() else { return false };
    let config = app.mux.api(|r| Api::Block(block, r)).await.flatten().map(|b| b.config());
    let ladder = config
        .and_then(|c| c["work"].get("ladder").cloned())
        .and_then(|l| serde_json::from_value::<crate::board_attach::LadderWork>(l).ok());
    let Some(ladder) = ladder else { return false };
    lead_of(app, &ladder.board).await.is_some_and(|l| l.machine == e.keys.id() && l.pane == Some(me))
}

/// A session's name for a start that gave none: its rung and its issue, or
/// the board.
fn default_session(rung: Rung, issue: Option<&str>, board: &str) -> String {
    let what = issue.unwrap_or(board);
    let what: String = what.chars().filter(char::is_ascii_alphanumeric).take(16).collect();
    format!("{}-{what}", rung.as_str())
}

/// Whether `git -C dir ARGS` succeeds.
async fn git_ok(dir: &str, args: &[&str]) -> bool {
    tokio::process::Command::new("git").arg("-C").arg(dir).args(args).output().await.is_ok_and(|o| o.status.success())
}

/// `git -C dir ARGS`'s output, trimmed, when it succeeds.
async fn git_out(dir: &str, args: &[&str]) -> Option<String> {
    let o = tokio::process::Command::new("git").arg("-C").arg(dir).args(args).output().await.ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// Whether remote `url` is `repo` (`owner/name`), as the worktree script
/// matches one.
fn remote_is(url: &str, repo: &str) -> bool {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    let tail = u.len().checked_sub(repo.len()).map(|i| (&u[..i], &u[i..]));
    matches!(tail, Some((before, r)) if r.eq_ignore_ascii_case(repo) && (before.ends_with('/') || before.ends_with(':')))
}

/// A clone of `repo` here, as [`clone_among`] finds one.
pub(crate) async fn clone_of(app: &App, repo: &str) -> Option<String> {
    clone_among(app, &[repo.to_owned()]).await.map(|(_, dir)| dir)
}

/// The first of `repos` (`owner/name`) with a clone here, and that clone: a
/// repository a hosted box's setup cloned or this machine's panes work in,
/// else one in the home folder (Arugula #1458), whose origin is it. The
/// home folder is looked in only when those have no clone of the first.
pub(crate) async fn clone_among(app: &App, repos: &[String]) -> Option<(String, String)> {
    let first = repos.first()?;
    let mut dirs: Vec<String> =
        crate::hosted::clones(app.control.state_dir()).into_iter().map(|(_, d)| d.display().to_string()).collect();
    let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
    dirs.extend(panes.into_iter().filter_map(|p| p.info.cwd));
    // Each clone once, with its origin.
    let mut clones: Vec<(String, String)> = vec![];
    for home in [false, true] {
        if home {
            let home = arugula_mux::sys::home();
            let found = tokio::task::spawn_blocking(move || repos_under(&home)).await.unwrap_or_default();
            dirs = found.into_iter().map(|d| d.display().to_string()).collect();
        }
        for d in std::mem::take(&mut dirs) {
            let Some(top) = git_out(&d, &["rev-parse", "--show-toplevel"]).await else { continue };
            // A worktree's clone is the one its common dir belongs to.
            let top = match git_out(&top, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).await {
                Some(common) if common.ends_with("/.git") => common.trim_end_matches("/.git").to_owned(),
                _ => top,
            };
            if clones.iter().any(|(t, _)| *t == top) {
                continue;
            }
            let origin = git_out(&top, &["remote", "get-url", "origin"]).await.unwrap_or_default();
            if remote_is(&origin, first) {
                return Some((first.clone(), top));
            }
            clones.push((top, origin));
        }
    }
    repos.iter().skip(1).find_map(|repo| {
        let (top, _) = clones.iter().find(|(_, origin)| remote_is(origin, repo))?;
        Some((repo.clone(), top.clone()))
    })
}

/// How deep under the home folder a clone is looked for (`~/dev/owner/name`
/// is 3), and at most how many folders are read.
const HOME_DEPTH: usize = 3;
const HOME_DIRS_MAX: usize = 4000;

/// The git repositories under `home`, nearest first: folders holding a
/// `.git`, up to [`HOME_DEPTH`] down, past hidden folders and build output.
fn repos_under(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    const SKIP: &[&str] = &["node_modules", "target", "Library", "vendor", "go"];
    let (mut found, mut read) = (vec![], 0);
    let mut level = vec![home.to_path_buf()];
    for _ in 0..HOME_DEPTH {
        let mut next = vec![];
        for dir in level {
            if read >= HOME_DIRS_MAX {
                return found;
            }
            read += 1;
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut subs: Vec<std::path::PathBuf> = entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| e.file_name().to_str().is_some_and(|n| !n.starts_with('.') && !SKIP.contains(&n)))
                .map(|e| e.path())
                .collect();
            subs.sort();
            for sub in subs {
                match sub.join(".git").exists() {
                    true => found.push(sub),
                    false => next.push(sub),
                }
            }
        }
        level = next;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_is_its_repo_by_the_last_two_parts() {
        for u in [
            "https://github.com/acme/widgets",
            "https://github.com/acme/widgets.git",
            "git@github.com:acme/widgets.git",
            "ssh://git@github.com/acme/widgets/",
            "/srv/git/acme/widgets.git",
        ] {
            assert!(remote_is(u, "acme/widgets"), "{u}");
        }
        assert!(!remote_is("https://github.com/acme/widgets-two", "acme/widgets"));
        assert!(!remote_is("https://github.com/notacme/widgets", "acme/widgets"));
        assert!(!remote_is("widgets", "acme/widgets"));
    }

    #[test]
    fn repos_under_home_are_found_nearest_first_past_hidden_and_build_folders() {
        let home = std::env::temp_dir().join(format!("arugula-repos-under-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        for d in ["dev/acme/widgets/.git", "src/site/.git", ".cache/x/.git", "code/node_modules/y/.git", "a/b/c/d/.git"]
        {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        // A repo's own folders aren't looked in.
        std::fs::create_dir_all(home.join("src/site/inner/.git")).unwrap();
        assert_eq!(repos_under(&home), [home.join("src").join("site"), home.join("dev").join("acme").join("widgets")]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_start_wears_only_a_recipe_of_its_own() {
        assert_eq!(worn_recipe(Rung::Lead, None), Ok(None));
        assert_eq!(worn_recipe(Rung::Lead, Some("board-lead".into())), Ok(None));
        assert_eq!(worn_recipe(Rung::Issue, Some("issue-agent".into())), Ok(None));
        assert_eq!(worn_recipe(Rung::Issue, Some("arugula-hand".into())), Ok(Some("arugula-hand".into())));
        assert!(worn_recipe(Rung::Issue, Some("board-lead".into())).unwrap_err().contains("runs issue-agent"));
        assert!(worn_recipe(Rung::Lead, Some("chief-of-staff".into())).is_err());
    }

    #[test]
    fn a_default_session_says_its_rung_and_work() {
        assert_eq!(default_session(Rung::Issue, Some("#1415"), "brd_1"), "issue-1415");
        assert_eq!(default_session(Rung::Lead, None, "brd_e3bdf5fbb0a5e3e334"), "lead-brde3bdf5fbb0a5e");
    }
}
