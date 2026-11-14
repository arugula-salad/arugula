//! `arugula project`: your projects on Arugula control, and a project's
//! linked GitHub repos (M163). `list` lists your projects by owner, `new`
//! makes one, and `use` puts the session this runs in in one. A member of
//! the project who is an admin or maintainer of a repo on GitHub links it;
//! its PRs and issues then live on the platform. Unlinking stops that at
//! once, and its rows go within a day. `split` moves work out of a project
//! into another, after a dry run. Through control, as this CLI's account
//! (`arugula login`).

use std::{path::PathBuf, sync::OnceLock, time::Duration};

use anyhow::{Context, bail};
use arugula_control_wire::projects::{Project, ProjectCreate, ProjectOwner, ProjectOwnerRef, Projects};
use arugula_proto::{ClientMsg, Intent, ProjectRef, SessionId};
use serde_json::{Value, json};
use tungstenite::Message;

use super::issue_native::{self, Steps};
use crate::control::Enrolled;
use crate::http::{Target, request};
use crate::util::print_json;

/// This machine's daemon (`--socket`), where the session this runs in is.
pub static SOCKET: OnceLock<PathBuf> = OnceLock::new();

#[derive(clap::Subcommand)]
pub enum ProjectCmd {
    /// Your projects: Personal's, then each team's.
    ///
    /// `*` marks the current one. Archived ones are left out unless you
    /// pass `--archived`.
    List {
        /// Show archived projects too.
        #[arg(long)]
        archived: bool,
    },
    /// Make a project, and print its id and name.
    ///
    /// It's Personal's unless `--team` names a team you own.
    New {
        name: String,
        /// The team: its id or name.
        #[arg(long)]
        team: Option<String>,
    },
    /// Put the session this runs in in a project.
    ///
    /// Its id, `personal`, or its name.
    Use { project: String },
    /// The project's linked repos, and those unlinked and waiting to be
    /// purged.
    #[command(alias = "ls")]
    Repos {
        #[command(flatten)]
        which: Which,
    },
    /// Link a GitHub repo to the project.
    ///
    /// You need admin or maintain on it on GitHub, and Arugula's GitHub App
    /// installed where it lives: if it isn't, this says where to install it.
    Link {
        /// OWNER/NAME.
        repo: String,
        #[command(flatten)]
        which: Which,
    },
    /// Unlink a GitHub repo from the project.
    ///
    /// Nothing more is read from GitHub for it, and its rows go within a
    /// day unless it's linked again first.
    Unlink {
        /// OWNER/NAME.
        repo: String,
        #[command(flatten)]
        which: Which,
    },
    /// Move work out of the project: into a new one (`--name`), or one
    /// that exists (`--into`).
    ///
    /// Without `--yes` it only plans, and changes nothing: what to deal
    /// with before you move, then what comes along (a repo's boards,
    /// issues and sessions come with it; a board's issues and Docs with
    /// the board). With `--yes` it moves, then says where the work went
    /// and how its issues were renumbered.
    Split(SplitArgs),
}

#[derive(clap::Args)]
#[command(group = clap::ArgGroup::new("to").required(true).args(["name", "into"]))]
pub struct SplitArgs {
    /// The project to move work out of: its id or name. Default: the
    /// current one.
    #[arg(long)]
    project: Option<String>,
    /// Move it into a new project with this name.
    #[arg(long)]
    name: Option<String>,
    /// The new project's owner: `personal` (the default), or a team's id
    /// or name.
    #[arg(long, conflicts_with = "into")]
    owner: Option<String>,
    /// Move it into a project that exists: its id or name.
    #[arg(long)]
    into: Option<String>,
    /// A linked repo, OWNER/NAME. Repeat for more.
    #[arg(long = "repo")]
    repos: Vec<String>,
    /// A board, brd_…. Repeat for more.
    #[arg(long = "board")]
    boards: Vec<String>,
    /// An issue: #N, N or iss_…. Repeat for more.
    #[arg(long = "issue")]
    issues: Vec<String>,
    /// A project Doc, pdc_…. Repeat for more.
    #[arg(long = "doc")]
    docs: Vec<String>,
    /// Move. Without it, only plan.
    #[arg(long)]
    yes: bool,
}

#[derive(clap::Args)]
pub struct Which {
    /// The project: its id (prj_…) or name. Default: your personal one.
    #[arg(long)]
    project: Option<String>,
}

pub fn run(cmd: ProjectCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula cloud: `arugula login` first");
    };
    let project = |w: &Which| -> anyhow::Result<String> {
        match &w.project {
            None => Ok("personal".into()),
            Some(p) if p.starts_with("prj_") => Ok(p.clone()),
            Some(p) => project_id(&e.api("GET", "/api/projects", None)?, p),
        }
    };
    match cmd {
        ProjectCmd::List { archived } => return list(&e, archived, json_out),
        ProjectCmd::New { name, team } => return new(&e, &name, team.as_deref(), json_out),
        ProjectCmd::Use { project } => return use_(&e, &project, json_out),
        ProjectCmd::Split(a) => return split(&e, &a, json_out),
        ProjectCmd::Repos { which } => {
            let listed = e.api("GET", &format!("/api/projects/{}/repos", project(&which)?), None)?;
            let repos = listed["repos"].as_array().cloned().unwrap_or_default();
            if json_out {
                print_json(&listed);
            } else if repos.is_empty() {
                println!("No linked repos.");
            } else {
                for r in &repos {
                    println!("{}", line(r));
                }
            }
        }
        ProjectCmd::Link { repo, which } => {
            let path = format!("/api/projects/{}/repos", project(&which)?);
            let done = e.api("POST", &path, Some(&json!({ "repo": repo })))?;
            if json_out { print_json(&done) } else { println!("{}", line(&done)) }
        }
        ProjectCmd::Unlink { repo, which } => {
            let Some((owner, name)) = repo.split_once('/') else { bail!("repo: OWNER/NAME") };
            let path = format!("/api/projects/{}/repos/{owner}/{name}", project(&which)?);
            let done = e.api("DELETE", &path, None)?;
            if json_out { print_json(&done) } else { println!("{}", line(&done)) }
        }
    }
    Ok(0)
}

/// `acme/app  private  linked 2h ago`, or `unlinked`, and when it's purged.
fn line(r: &Value) -> String {
    let name = r["full_name"].as_str().unwrap_or_default();
    let private = if r["private"] == true { "private" } else { "public" };
    let state = match (r["unlinked_at"].as_u64(), r["purge_after"].as_u64()) {
        (Some(at), Some(purge)) => format!("unlinked {}, purged within {}", crate::hosts::ago(at), within(purge)),
        _ => format!("linked {}", r["linked_at"].as_u64().map(crate::hosts::ago).unwrap_or_default()),
    };
    format!("{name:<40} {private:<8} {state}")
}

/// How long until `ms`, roughly: `5h`, `20m`, or `0m` once it's past.
fn within(ms: u64) -> String {
    let left = ms.saturating_sub(arugula_e2e::now_ms()) / 60_000;
    if left >= 60 { format!("{}h", left.div_ceil(60)) } else { format!("{left}m") }
}

/// `project list`: by owner, the current one marked.
fn list(e: &Enrolled, archived: bool, json_out: bool) -> anyhow::Result<i32> {
    let projects: Projects = serde_json::from_value(e.api("GET", "/api/projects", None)?)?;
    let current = current(e, &projects);
    let groups = owners(&projects, archived);
    if json_out {
        let owners: Vec<Value> = groups
            .iter()
            .map(|g| {
                let mut o = match &g.team {
                    Some(id) => json!({ "kind": "team", "id": id, "name": g.name }),
                    None => json!({ "kind": "personal", "name": g.name }),
                };
                o["projects"] = json!(g.projects);
                o
            })
            .collect();
        print_json(&json!({ "current": current, "owners": owners }));
    } else {
        print!("{}", listed(&groups, current.as_deref()));
    }
    Ok(0)
}

/// The current project, by the order `arugula issue` and MCP go by
/// (`issue_native::chosen`), or `None` if it can't be told.
fn current(e: &Enrolled, projects: &Projects) -> Option<String> {
    chosen(e, projects, None).ok()
}

/// `asked`, else `$ARUGULA_PROJECT`, else the rest of the order
/// `arugula issue` and MCP go by (`issue_native::chosen`).
fn chosen(e: &Enrolled, projects: &Projects, asked: Option<&str>) -> anyhow::Result<String> {
    let env = std::env::var("ARUGULA_PROJECT").ok().filter(|p| !p.trim().is_empty());
    let home_of =
        |p: &str| Ok(serde_json::from_value(e.api("GET", &format!("/api/projects/{p}/issue-home"), None)?)?);
    let steps = Steps {
        asked: asked.or(env.as_deref()),
        session: &|| this_session()?.project.map(|p| p.id),
        dir: &issue_native::here_repo,
    };
    let chosen = issue_native::chosen(projects, &home_of, &steps, false)?;
    chosen.map(|(id, _)| id).context("no current project: name one with --project")
}

/// One owner's projects: Personal's (`team` none) or a team's.
struct Owner<'a> {
    team: Option<String>,
    name: String,
    projects: Vec<&'a Project>,
}

/// The projects by owner: Personal first, then each team in the order its
/// first project came, as the web's switcher (`platform/owners.ts`). An
/// owner left with none isn't listed.
fn owners(projects: &Projects, archived: bool) -> Vec<Owner<'_>> {
    let mut all = vec![Owner { team: None, name: "Personal".into(), projects: vec![] }];
    for p in projects.projects.iter().filter(|p| archived || !p.archived) {
        let (team, name) = match (&p.owner, &p.team) {
            (Some(ProjectOwner::Team { id, name }), _) => (Some(id.clone()), name.clone()),
            (Some(ProjectOwner::Personal { .. }), _) | (None, None) => (None, String::new()),
            (None, Some(t)) => (Some(t.id.clone()), t.name.clone()),
        };
        match all.iter_mut().find(|o| o.team == team) {
            Some(o) => o.projects.push(p),
            None => all.push(Owner { team, name, projects: vec![p] }),
        }
    }
    all.retain(|o| !o.projects.is_empty());
    all
}

/// The teams you're in, from their projects: id and name.
fn teams(projects: &Projects) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = Vec::new();
    for p in &projects.projects {
        let t = match (&p.owner, &p.team) {
            (Some(ProjectOwner::Team { id, name }), _) => (id.clone(), name.clone()),
            (None, Some(t)) => (t.id.clone(), t.name.clone()),
            _ => continue,
        };
        if !all.iter().any(|(id, _)| *id == t.0) {
            all.push(t);
        }
    }
    all
}

/// `Personal`, then `* prj_…  Notes` for each, `*` the current one.
fn listed(groups: &[Owner], current: Option<&str>) -> String {
    let mut out = String::new();
    for g in groups {
        out += &format!("{}\n", g.name);
        for p in &g.projects {
            let mark = if Some(p.id.as_str()) == current { '*' } else { ' ' };
            let archived = if p.archived { "  (archived)" } else { "" };
            out += &format!("{mark} {}  {}{archived}\n", p.id, p.name);
        }
    }
    out
}

/// `--team`: a team's id or name (any case), among yours.
fn owner_of(projects: &Projects, team: Option<&str>) -> anyhow::Result<ProjectOwnerRef> {
    let Some(want) = team else { return Ok(ProjectOwnerRef::Personal) };
    let teams = teams(projects);
    let ids: Vec<&str> = match teams.iter().find(|(id, _)| id == want) {
        Some((id, _)) => vec![id],
        None => teams.iter().filter(|(_, n)| n.eq_ignore_ascii_case(want)).map(|(id, _)| id.as_str()).collect(),
    };
    match ids.as_slice() {
        [id] => Ok(ProjectOwnerRef::Team { id: (*id).to_owned() }),
        [] => bail!("you're in no team called {want}"),
        _ => bail!("more than one of your teams is called {want}: name one by its id ({})", ids.join(", ")),
    }
}

/// `project new`.
fn new(e: &Enrolled, name: &str, team: Option<&str>, json_out: bool) -> anyhow::Result<i32> {
    let owner = match team {
        None => ProjectOwnerRef::Personal,
        Some(_) => owner_of(&serde_json::from_value(e.api("GET", "/api/projects", None)?)?, team)?,
    };
    let create = ProjectCreate { name: name.to_owned(), owner, slug: None };
    let made = e.api("POST", "/api/projects", Some(&serde_json::to_value(&create)?))?;
    if json_out {
        print_json(&made);
    } else {
        println!("{} {}", made["id"].as_str().unwrap_or_default(), made["name"].as_str().unwrap_or(name));
    }
    Ok(0)
}

/// `project use`.
fn use_(e: &Enrolled, want: &str, json_out: bool) -> anyhow::Result<i32> {
    let session = this_session().context("run it in an Arugula pane: it sets that pane's session's project")?;
    let projects: Projects = serde_json::from_value(e.api("GET", "/api/projects", None)?)?;
    let id = issue_native::project_id(&projects, want)?;
    let name = projects.projects.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
    let project = ProjectRef { id, name };
    set_session_project(&session_target()?, session.id, project.clone())?;
    if json_out {
        print_json(&json!({ "session": session.id, "project": project }));
    } else {
        println!("Session {} is in project {} ({}).", session.name, project.name, project.id);
    }
    Ok(0)
}

/// `project split`: the planner's plan, or with `--yes` the move. The
/// request is the platform's (`/api/projects/{project}/split/plan` and
/// `/split`, the web's `platform/move/plan.ts`); what comes along is the
/// planner's to say, never worked out here.
fn split(e: &Enrolled, a: &SplitArgs, json_out: bool) -> anyhow::Result<i32> {
    let projects: Projects = serde_json::from_value(e.api("GET", "/api/projects", None)?)?;
    let from = chosen(e, &projects, a.project.as_deref())?;
    let to = match (&a.name, &a.into) {
        (Some(name), _) => {
            let owner = owner_of(&projects, a.owner.as_deref().filter(|o| !o.eq_ignore_ascii_case("personal")))?;
            json!({ "kind": "new", "name": name, "owner": owner })
        }
        (None, Some(into)) => json!({ "kind": "project", "id": issue_native::project_id(&projects, into)? }),
        (None, None) => bail!("say where it goes: --name for a new project, or --into one that exists"),
    };
    let mut issues = Vec::new();
    for i in &a.issues {
        issues.push(issue_id(e, &from, i)?);
    }
    for r in &a.repos {
        if r.split_once('/').is_none_or(|(o, n)| o.is_empty() || n.is_empty() || n.contains('/')) {
            bail!("--repo: OWNER/NAME, not {r}");
        }
    }
    let ask = json!({ "to": to, "repos": a.repos, "boards": a.boards, "issues": issues, "docs": a.docs });
    if !a.yes {
        let plan = e.api("POST", &format!("/api/projects/{from}/split/plan"), Some(&ask))?;
        if json_out {
            print_json(&plan)
        } else {
            print!("{}", planned(&plan))
        }
        return Ok(0);
    }
    let (status, got) = e.api_raw("POST", &format!("/api/projects/{from}/split"), Some(&ask))?;
    let done = match crate::http::json_answer(status, got) {
        Ok(v) => v,
        Err(err) if status == 409 && !err.to_string().contains("with that name") => {
            return Err(err.context("the project changed since this was planned: run it again without --yes"));
        }
        Err(err) => return Err(err),
    };
    if json_out {
        print_json(&done)
    } else {
        print!("{}", moved(&done))
    }
    Ok(0)
}

/// `--issue`: an `iss_` id as it is, or `#N` (or `N`) in `project`.
fn issue_id(e: &Enrolled, project: &str, issue: &str) -> anyhow::Result<String> {
    let issue = issue.trim();
    if issue.starts_with("iss_") {
        return Ok(issue.to_owned());
    }
    let Ok(n) = issue.trim_start_matches('#').parse::<u64>() else {
        bail!("--issue: #N, N or iss_…, not {issue}");
    };
    let found = e.api("GET", &format!("/api/projects/{project}/issues/{n}"), None).with_context(|| format!("#{n}"))?;
    found["id"].as_str().map(str::to_owned).with_context(|| format!("#{n} has no id"))
}

/// What comes along, in the plan's and the move's words: each list, or
/// page, by its count.
const ALONG: [(&str, &str, &str); 11] = [
    ("repos", "repo", "repos"),
    ("boards", "board", "boards"),
    ("issues", "issue", "issues"),
    ("github", "GitHub issue or PR", "GitHub issues and PRs"),
    ("docs", "Doc", "Docs"),
    ("machine_docs", "Doc on a machine", "Docs on machines"),
    ("workspaces", "Cloud workspace", "Cloud workspaces"),
    ("environments", "environment", "environments"),
    ("runs", "run", "runs"),
    ("sessions", "agent session", "agent sessions"),
    ("drafts", "draft", "drafts"),
];

/// How many `v` is: a number, a page's `count`, a list's length; GitHub's
/// issues and PRs; a machine's Docs, summed.
fn count(key: &str, v: &Value) -> u64 {
    match (key, v) {
        ("github", _) => v["issues"].as_u64().unwrap_or(0) + v["prs"].as_u64().unwrap_or(0),
        ("machine_docs", Value::Array(ms)) => ms.iter().map(|m| m["docs"].as_array().map_or(0, Vec::len) as u64).sum(),
        (_, Value::Number(n)) => n.as_u64().unwrap_or(0),
        (_, Value::Array(a)) => a.len() as u64,
        (_, Value::Object(o)) => o.get("count").and_then(Value::as_u64).unwrap_or(0),
        _ => 0,
    }
}

/// `3 issues`, for each of `along`'s that isn't none, `links` last.
fn counted(along: &Value) -> Vec<String> {
    let rows = ALONG.iter().copied().chain([("links", "link", "links")]);
    rows.filter_map(|(key, one, many)| {
        let n = count(key, &along[key]);
        (n > 0).then(|| format!("{n} {}", if n == 1 { one } else { many }))
    })
    .collect()
}

/// `Acme (prj_…)`.
fn named(p: &Value) -> String {
    format!("{} ({})", p["name"].as_str().unwrap_or("?"), p["id"].as_str().unwrap_or("?"))
}

/// The plan, as the web's move screen says it: where it goes, "Before you
/// move" (the notes), then "Comes along".
fn planned(plan: &Value) -> String {
    let to = &plan["to"];
    let owner = match to["owner"]["kind"].as_str() {
        Some("team") => to["owner"]["name"].as_str().unwrap_or("a team").to_owned(),
        _ => "Personal".into(),
    };
    let into = match to["id"].as_str() {
        Some(_) => format!("{}, {owner}'s", named(to)),
        None => format!("a new project, {}, {owner}'s", to["name"].as_str().unwrap_or("?")),
    };
    let first = to["first_number"].as_u64().unwrap_or(1);
    let mut out = format!("From {} into {into}. Its issues are numbered from #{first}.\n", named(&plan["from"]));
    if plan["empty"] == true {
        out += "\nNothing would move: name a repo, board, issue or Doc of the project.\n";
        return out;
    }
    out += "\nBefore you move\n";
    let notes = plan["notes"].as_array().cloned().unwrap_or_default();
    if notes.is_empty() {
        out += "  Nothing.\n";
    }
    for n in &notes {
        out += &format!("  - {}\n", n["text"].as_str().unwrap_or_default());
    }
    out += "\nComes along\n";
    for c in counted(&plan["along"]) {
        out += &format!("  {c}\n");
    }
    out += "\nNothing has moved: run it again with --yes to move.\n";
    out
}

/// The move: where it went, what moved, and each issue's new number.
fn moved(done: &Value) -> String {
    let what = counted(&done["moved"]);
    let what = if what.is_empty() { "nothing".to_owned() } else { what.join(", ") };
    let mut out = format!("Moved into {}: {what}.\n", named(&done["to"]));
    let issues = done["issues"].as_array().cloned().unwrap_or_default();
    if !issues.is_empty() {
        out += "Renumbered:\n";
        for i in &issues {
            let n = |k: &str| i[k].as_u64().map_or("?".into(), |n| n.to_string());
            out += &format!("  #{} → #{}\n", n("from"), n("to"));
        }
    }
    if let Some(id) = done["id"].as_str() {
        out += &format!("The split is {id}.\n");
    }
    out
}

/// The session this runs in.
pub struct ThisSession {
    pub id: SessionId,
    pub name: String,
    pub project: Option<ProjectRef>,
}

fn session_target() -> anyhow::Result<Target> {
    Ok(Target::Socket(SOCKET.get().context("no daemon socket")?.clone()))
}

/// The session `$ARUGULA_PANE` is in, as this machine's daemon has it.
/// `None` outside a pane, or with no daemon.
pub fn this_session() -> Option<ThisSession> {
    let pane: u32 = std::env::var("ARUGULA_PANE").ok()?.parse().ok()?;
    let panes = request(&session_target().ok()?, "GET", "/api/panes", None).ok()?.json().ok()?;
    session_of(&panes, pane)
}

/// `pane`'s session among `GET /api/panes`'.
fn session_of(panes: &Value, pane: u32) -> Option<ThisSession> {
    let p = panes.as_array()?.iter().find(|p| p["id"] == pane)?;
    Some(ThisSession {
        id: SessionId::try_from(p["session"].as_u64()?).ok()?,
        name: p["session_name"].as_str().unwrap_or_default().to_owned(),
        project: serde_json::from_value(p["session_project"].clone()).ok(),
    })
}

/// The daemon's `set_session_project` intent, over its WebSocket. An
/// intent has no answer of its own: its error comes back with its id, and
/// a ping after it is answered once it's done.
fn set_session_project(sock: &Target, session: SessionId, project: ProjectRef) -> anyhow::Result<()> {
    let stream = sock.connect()?;
    stream.set_timeout(Some(Duration::from_secs(10)))?;
    let (mut ws, _) = tungstenite::client(sock.ws_request()?, stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(e) => anyhow::Error::from(e).context("websocket handshake"),
        tungstenite::HandshakeError::Interrupted(_) => anyhow::anyhow!("websocket handshake interrupted"),
    })?;
    let intent = Intent::SetSessionProject { session, project: Some(project) };
    for msg in [ClientMsg::Intent { id: Some(1), intent }, ClientMsg::Ping { id: 2 }] {
        ws.send(Message::Text(serde_json::to_string(&msg)?.into()))?;
    }
    loop {
        let Message::Text(t) = ws.read().context("the daemon didn't answer")? else { continue };
        let v: Value = serde_json::from_str(&t).unwrap_or_default();
        match v["type"].as_str() {
            Some("error") if v["id"] == 1 => bail!("{}", v["message"].as_str().unwrap_or("the daemon refused")),
            Some("pong") if v["id"] == 2 => return Ok(()),
            _ => {}
        }
    }
}

/// A project named on the command line, by its name (any case), among
/// the account's.
pub(super) fn project_id(listed: &Value, name: &str) -> anyhow::Result<String> {
    let ids: Vec<String> = listed["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .filter_map(|p| p["id"].as_str().map(String::from))
        .collect();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("you have no project called {name}"),
        _ => bail!("more than one of your projects is called {name}: name one by its id ({})", ids.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn a_project_is_named_by_its_name_or_id() {
        let listed = json!({ "projects": [
            { "id": "prj_a", "name": "Personal" }, { "id": "prj_b", "name": "Acme" }, { "id": "prj_c", "name": "dup" },
            { "id": "prj_d", "name": "Dup" } ] });
        assert_eq!(project_id(&listed, "acme").unwrap(), "prj_b");
        assert!(project_id(&listed, "dup").unwrap_err().to_string().contains("prj_c, prj_d"));
        assert!(project_id(&listed, "nope").is_err());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "link", "acme/app"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "link", "acme/app", "--project", "Acme"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "unlink", "acme/app"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "ls"]).is_ok());
    }

    #[test]
    fn commands_parse() {
        let ok = |argv: &[&str]| crate::Cli::try_parse_from(argv).is_ok();
        assert!(ok(&["arugula", "project", "list"]));
        assert!(ok(&["arugula", "project", "list", "--archived"]));
        assert!(ok(&["arugula", "project", "new", "Web app"]));
        assert!(ok(&["arugula", "project", "new", "Web app", "--team", "Acme"]));
        assert!(!ok(&["arugula", "project", "new"]), "it needs a name");
        assert!(ok(&["arugula", "project", "use", "personal"]));
        assert!(!ok(&["arugula", "project", "use"]), "it needs a project");
        assert!(ok(&["arugula", "project", "repos", "--project", "Acme"]));
        assert!(ok(&["arugula", "project", "split", "--name", "App", "--repo", "a/b", "--repo", "a/c", "--yes"]));
        assert!(ok(&["arugula", "project", "split", "--into", "Web", "--issue", "#4", "--doc", "pdc_1"]));
        assert!(!ok(&["arugula", "project", "split", "--repo", "a/b"]), "it needs somewhere to go");
        assert!(!ok(&["arugula", "project", "split", "--into", "Web", "--owner", "Acme"]), "--owner is a new one's");
    }

    fn listed_projects() -> Projects {
        serde_json::from_value(json!({
            "me": { "account": "acc_me", "login": "jake", "name": "Jake" },
            "projects": [
                { "id": "prj_1", "name": "Jake", "team": null, "owner": { "kind": "personal", "account": "acc_me" } },
                { "id": "prj_2", "name": "Acme", "team": { "id": "tm_1", "name": "Acme", "role": "owner", "members": [] },
                  "owner": { "kind": "team", "id": "tm_1", "name": "Acme" } },
                { "id": "prj_3", "name": "Old", "team": null, "archived": true,
                  "owner": { "kind": "personal", "account": "acc_me" } },
                { "id": "prj_4", "name": "Web", "team": null, "owner": { "kind": "team", "id": "tm_1", "name": "Acme" } },
                { "id": "prj_5", "name": "Ops", "team": { "id": "tm_2", "name": "Ops", "role": "viewer", "members": [] } },
                { "id": "prj_6", "name": "Notes", "team": null }
            ] }))
        .unwrap()
    }

    /// Personal first, then each team as its first project came; archived
    /// ones only when asked for.
    #[test]
    fn listed_by_owner() {
        let p = listed_projects();
        let shown = listed(&owners(&p, false), Some("prj_4"));
        assert_eq!(
            shown,
            "Personal\n  prj_1  Jake\n  prj_6  Notes\nAcme\n  prj_2  Acme\n* prj_4  Web\nOps\n  prj_5  Ops\n"
        );
        let shown = listed(&owners(&p, true), None);
        assert!(shown.contains("  prj_3  Old  (archived)\n  prj_6"), "{shown}");
        // An owner with only archived projects isn't listed without them.
        let only: Projects = serde_json::from_value(json!({
            "me": { "account": "a", "login": "", "name": "" },
            "projects": [{ "id": "prj_3", "name": "Old", "team": null, "archived": true }] }))
        .unwrap();
        assert!(owners(&only, false).is_empty());
    }

    #[test]
    fn a_team_by_id_or_name() {
        let p = listed_projects();
        assert_eq!(owner_of(&p, None).unwrap(), ProjectOwnerRef::Personal);
        assert_eq!(owner_of(&p, Some("acme")).unwrap(), ProjectOwnerRef::Team { id: "tm_1".into() });
        assert_eq!(owner_of(&p, Some("tm_2")).unwrap(), ProjectOwnerRef::Team { id: "tm_2".into() });
        assert!(owner_of(&p, Some("nope")).is_err());
    }

    #[test]
    fn this_pane_s_session() {
        let panes = json!([
            { "id": 1, "session": 3, "session_name": "main" },
            { "id": 2, "session": 4, "session_name": "work", "session_project": { "id": "prj_2", "name": "Acme" } }
        ]);
        let s = session_of(&panes, 2).unwrap();
        assert_eq!((s.id, s.name.as_str()), (4, "work"));
        assert_eq!(s.project.unwrap().id, "prj_2");
        assert!(session_of(&panes, 1).unwrap().project.is_none());
        assert!(session_of(&panes, 9).is_none());
    }

    #[test]
    fn lines() {
        let r = json!({ "full_name": "acme/app", "private": true, "linked_at": 0 });
        assert!(line(&r).starts_with("acme/app"), "{}", line(&r));
        assert!(line(&r).contains("private"));
        let r =
            json!({ "full_name": "acme/app", "private": false, "linked_at": 0, "unlinked_at": 1, "purge_after": 2 });
        assert!(line(&r).contains("unlinked"), "{}", line(&r));
    }
}
