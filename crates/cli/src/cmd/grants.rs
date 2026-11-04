//! `arugula grants`: the powers your machines' agents hold over your things
//! on Arugula control (M130). `cloud.drive`: the agents on a machine drive your Cloud
//! workspaces; every machine you join gets it, and you revoke it from one,
//! and give it back, here. And (M197) what its agents may do in
//! conversations: `threads.read` and `threads.post` in a project's threads
//! (a team's channels are its project's) or your DMs, `issues.write` on
//! a project's issues, and (M218) `deploys.plan`: plan a project's deploys
//! and ask an owner to approve them, never approve one. And (M216) its
//! agents and a project's runs: `runs.read` reads them, `runs.start`
//! starts its run specs, and `runs.command` starts a command or an
//! instruction of the agent's own (each start as yours, never past your
//! role). Those are off until you `add` them, and only over what you can
//! read yourself. Through control, as this CLI's account (`arugula
//! login`). `ls --project X` is a project's Access view (Arugula #682):
//! who can do what there, for any of its members, read-only.

use anyhow::bail;
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

/// The powers an agent gets over a scope (M197, M218's deploys.plan,
/// M216's over runs, and aru-121's over sessions).
const AGENT_POWERS: [&str; 12] = [
    "threads.read",
    "threads.post",
    "issues.write",
    "deploys.plan",
    "runs.read",
    "runs.start",
    "runs.command",
    "boards.read",
    "boards.write",
    "trackers.write",
    "sessions.start",
    "sessions.reply",
];

/// The powers that do nothing at all for a viewer's agents (Arugula #683):
/// the platform refuses to give one over a project you only view, so `add`
/// checks the whole list first and gives nothing rather than part of it.
/// The wire's list, the platform's own.
use arugula_control_wire::projects::VIEWER_REFUSED;

/// Those over a project only.
const PROJECT_ONLY: [&str; 10] = [
    "issues.write",
    "deploys.plan",
    "runs.read",
    "runs.start",
    "runs.command",
    "boards.read",
    "boards.write",
    "trackers.write",
    "sessions.start",
    "sessions.reply",
];

/// Where an agent power reaches: one of the two.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(multiple = false)]
pub struct Scope {
    /// A project, by its name or id (a team's channels are its project's).
    #[arg(long)]
    project: Option<String>,
    /// Your DMs.
    #[arg(long)]
    dms: bool,
}

#[derive(clap::Subcommand)]
pub enum GrantsCmd {
    /// Your machines' grants, live and revoked; or who can do what in a project.
    ///
    /// With --project, the project's Access view, for any of its members:
    /// its team roles (the roster, as control's copy stands; each machine
    /// checks the roster itself), the powers over it (every live grant,
    /// whose machine or agent token holds it, and whether it reaches
    /// anything now), what an owner allowed (the delegation for cloud
    /// workspaces, the standing run grant and its expiry) and its cloud
    /// workspaces, where an editor has full use. Read-only.
    Ls {
        /// A project, by its name or id: its Access view.
        #[arg(long)]
        project: Option<String>,
    },
    /// Give a machine's agents a power over a project or your DMs.
    ///
    /// The powers are threads.read, threads.post or issues.write over a
    /// project (`--project`) or your DMs (`--dms`), deploys.plan over a
    /// project (plan deploys and ask for approval, never approve),
    /// runs.read, runs.start (its run specs) or runs.command (its own
    /// commands) over a project, boards.read (a project's boards) or
    /// boards.write (add rows, set notes and statuses, move rows; never
    /// make or delete a zone) over a project, trackers.write (create,
    /// comment on, close, reopen and edit a connected Plane project's work
    /// items; each may wait for a person to approve it) over a project, sessions.start (start agent
    /// sessions as you: you're the asker, and you pay) or sessions.reply
    /// (answer a session's question, or prompt it) over a project, or
    /// cloud.drive again.
    /// Every room it reaches says whose agents read and post there, and what
    /// they write is labelled as your agent's on that machine.
    Add {
        /// The machine: its name or id.
        machine: String,
        /// One or more of threads.read, threads.post, issues.write,
        /// deploys.plan, runs.read, runs.start, runs.command, boards.read,
        /// boards.write, trackers.write, sessions.start, sessions.reply (or cloud.drive,
        /// with no scope).
        #[arg(required = true)]
        powers: Vec<String>,
        #[command(flatten)]
        scope: Scope,
    },
    /// Take a machine's grants back, or one grant by its id.
    ///
    /// All of a machine's live grants, or one power, over one scope. Or
    /// one grant by its id (grt_…): as an owner of the project it's over,
    /// a teammate's machine's or token's too; they're told, and can give
    /// it again (an owner who means it takes them out of the team).
    ///
    /// Its agents are refused from their next call; a run, an agent
    /// session or an approval already under way carries on.
    Revoke {
        /// The machine: its name or id. Or a grant's id (grt_…), which a
        /// project's owner may revoke whoever gave it: the project's
        /// Access view lists them.
        machine: String,
        /// Only this power.
        power: Option<String>,
        #[command(flatten)]
        scope: Scope,
    },
    /// Give a machine cloud.drive again.
    Restore {
        /// The machine: its name or id.
        machine: String,
    },
}

pub fn run(cmd: Option<GrantsCmd>, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    run_with(&e, cmd, json_out)
}

/// [`run`], as this enrolled CLI.
fn run_with(e: &Enrolled, cmd: Option<GrantsCmd>, json_out: bool) -> anyhow::Result<i32> {
    if let Some(GrantsCmd::Ls { project: Some(want) }) = &cmd {
        let projects = e.api("GET", "/api/projects", None)?;
        let id = project_id(&projects, want)?;
        let (status, body) = e.api_raw("GET", &format!("/api/projects/{id}/grants"), None)?;
        let view = project_access(status, body)?;
        if json_out {
            print_json(&view);
        } else {
            print!("{}", access_text(&view));
        }
        return Ok(0);
    }
    let listed = e.api("GET", "/api/grants", None)?;
    let grants = listed["grants"].as_array().cloned().unwrap_or_default();
    let show = |g: &Value| if json_out { print_json(g) } else { println!("{}", line(g)) };
    match cmd.unwrap_or(GrantsCmd::Ls { project: None }) {
        GrantsCmd::Ls { .. } => {
            if json_out {
                print_json(&listed);
            } else if grants.is_empty() {
                println!("No grants.");
            } else {
                for g in &grants {
                    println!("{}", line(g));
                }
            }
        }
        GrantsCmd::Add { machine, powers, scope } => {
            let id = machine_id(&grants, &machine)?;
            let (scope, role) = scope_of(e, &scope)?;
            // The whole list first: a power the platform would refuse
            // means nothing is given, not the ones before it.
            let bodies = powers.iter().map(|p| give(&id, p, scope.as_ref())).collect::<anyhow::Result<Vec<_>>>()?;
            let refused = refused_for_viewer(&powers, role.as_deref());
            if !refused.is_empty() {
                let name = scope.as_ref().and_then(|s| s["name"].as_str()).unwrap_or("that project");
                bail!(
                    "you only view {name}, so {} would do nothing for {machine}'s agents: nothing was given. Add the \
                     rest without {}",
                    refused.join(", "),
                    if refused.len() == 1 { "it" } else { "them" }
                );
            }
            for body in &bodies {
                show(&e.api("POST", "/api/grants", Some(body))?);
            }
        }
        GrantsCmd::Revoke { machine, power, scope } if machine.starts_with("grt_") => {
            // One grant by its id: as its giver, or as an owner of the
            // project it's over (Arugula #683), so it needn't be in the
            // list above.
            if power.is_some() || scope.project.is_some() || scope.dms {
                bail!("a grant's id names one grant: no power, --project or --dms with it");
            }
            let r = e.api("POST", &format!("/api/grants/{machine}/revoke"), None).map_err(|err| {
                if err.to_string().contains("no such grant") {
                    err.context(
                        "no grant by that id is yours to revoke: not over a project you own, or this control is \
                         from before owners' revokes (0.66.15): ask its giver",
                    )
                } else {
                    err
                }
            })?;
            show(&r);
        }
        GrantsCmd::Revoke { machine, power, scope } => {
            let id = machine_id(&grants, &machine)?;
            let (scope, _) = scope_of(e, &scope)?;
            let live: Vec<&Value> = grants
                .iter()
                .filter(|g| g["subject"]["id"] == id && g["live"] == true)
                .filter(|g| {
                    power.as_ref().is_none_or(|p| g["power"] == p.as_str()) && scope.as_ref().is_none_or(|s| over(g, s))
                })
                .collect();
            if live.is_empty() {
                println!("{machine} has no live grant like that.");
            }
            for g in live {
                let path = format!("/api/grants/{}/revoke", g["id"].as_str().unwrap_or_default());
                show(&e.api("POST", &path, None)?);
            }
        }
        GrantsCmd::Restore { machine } => {
            let id = machine_id(&grants, &machine)?;
            show(&e.api("POST", "/api/grants", Some(&json!({ "daemon": id, "power": "cloud.drive" })))?);
        }
    }
    Ok(0)
}

/// `GET /api/projects/{id}/grants`' answer, or why not: a control from
/// before the view answers 404 with no "no such project" in it, which
/// is told apart from a project this account isn't in.
fn project_access(status: u16, body: Vec<u8>) -> anyhow::Result<Value> {
    let text = String::from_utf8_lossy(&body).into_owned();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let error = v.get("error").and_then(Value::as_str);
    if status == 404 && error != Some("no such project") {
        bail!("this control doesn't list a project's access yet: update it, then `arugula grants ls --project` again");
    }
    if !(200..300).contains(&status) {
        bail!("{}", error.map(str::to_owned).unwrap_or(text));
    }
    Ok(v)
}

/// The Access view as text: one part per line of the question, each
/// saying whose word it is. A giver who left is "a former member". Ids
/// show when the answer has them (a project owner's, to revoke).
fn access_text(v: &Value) -> String {
    let s = |x: &Value| x.as_str().unwrap_or_default().to_owned();
    let name = |who: &Value| {
        if who["former"] == true {
            return "a former member".to_owned();
        }
        let n = who["name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| s(&who["account"]), str::to_owned);
        if who["locked_out"] == true { format!("{n} (locked out)") } else { n }
    };
    let mut out = String::new();
    let project = &v["project"];
    let kind = if project["owner"]["kind"] == "team" { "team project" } else { "personal project" };
    out.push_str(&format!("{} ({kind}); you're its {}.\n", s(&project["name"]), s(&v["role"])));

    out.push_str(&format!("\nProject roles, by {}:\n", s(&v["roles"]["says"])));
    for m in v["roles"]["members"].as_array().into_iter().flatten() {
        let locked = if m["locked_out"] == true { "  (locked out)" } else { "" };
        out.push_str(&format!("  {:<24} {}{locked}\n", s(&m["name"]), s(&m["role"])));
    }
    if project["owner"]["locked"] == true {
        out.push_str("  (the team is locked: only its owners get in while it is)\n");
    }

    // Every member's machines drive the project's cloud workspaces by
    // cloud.drive, as their role allows: said first, so "none" below
    // never reads as "no machine can do anything here".
    out.push_str(&format!("\nCloud workspaces' drivers, by {}:\n", s(&v["cloud_drive"]["says"])));
    let drivers = v["cloud_drive"]["members"].as_array().cloned().unwrap_or_default();
    if drivers.is_empty() {
        out.push_str("  none\n");
    }
    for m in &drivers {
        let (machines, driving) = (m["machines"].as_u64().unwrap_or(0), m["driving"].as_u64().unwrap_or(0));
        let what = match (machines, driving) {
            (0, _) => "no machine joined".to_owned(),
            (n, d) if n == d => format!("{n} machine{} drive as an {}", if n == 1 { "" } else { "s" }, s(&m["role"])),
            (n, d) => format!(
                "{d} of {n} machines drive as an {} (the rest: cloud.drive revoked by its owner)",
                s(&m["role"])
            ),
        };
        out.push_str(&format!("  {:<24} {what}\n", name(&m["owner"])));
    }

    out.push_str(&format!("\nPowers over it, by {}:\n", s(&v["powers"]["says"])));
    let grants = v["powers"]["grants"].as_array().cloned().unwrap_or_default();
    if grants.is_empty() {
        out.push_str("  no other power: no machine's or agent token's agents hold one here\n");
    }
    for g in &grants {
        // A former member's machine or token isn't named: "a former
        // member's machine".
        let token = g["subject"]["kind"] == "agent";
        let subject = match g["subject"]["name"].as_str().or(g["subject"]["id"].as_str()) {
            Some(n) if token => format!("token \u{201c}{}\u{201d}", n.trim_end_matches(" (MCP)")),
            Some(n) => n.to_owned(),
            None if token => "a token".to_owned(),
            None => "a machine".to_owned(),
        };
        let whose = format!("{subject} ({}'s)", name(&g["owner"]));
        let state = match g["limited"]["why"].as_str() {
            Some(why) => why.to_owned(),
            None => format!("since {}", crate::hosts::ago(g["created_at"].as_u64().unwrap_or(0))),
        };
        let id = g["id"].as_str().unwrap_or_default();
        out.push_str(&format!("  {whose:<34} {:<14} {state} {id}\n", s(&g["power"])));
    }

    out.push_str(&format!("\nAllowed, {}:\n", s(&v["allow"]["says"])));
    let d = &v["allow"]["delegation"];
    out.push_str(&format!(
        "  cloud workspaces: {}\n",
        if d.is_null() {
            "control couldn't say whether an owner allowed them".to_owned()
        } else if d["live"] == true {
            format!("allowed (delegation {})", s(&d["id"]))
        } else {
            "not allowed: no live delegation".to_owned()
        }
    ));
    let rg = &v["allow"]["run_grant"];
    out.push_str(&format!(
        "  runs with every device asleep: {}\n",
        if rg.is_null() {
            "not allowed: no live standing grant".to_owned()
        } else if rg["stale"] == true {
            "not now: the standing grant names an earlier delegation, so workspaces enrolled since refuse it; an \
             owner signs it again"
                .to_owned()
        } else {
            format!(
                "allowed by {} {}, {}",
                if rg["by"].is_null() { "an owner".to_owned() } else { name(&rg["by"]) },
                crate::hosts::ago(rg["made"].as_u64().unwrap_or(0)),
                match rg["expires"].as_u64() {
                    Some(at) => format!("expires in {}", in_(at)),
                    None => "until it's revoked".to_owned(),
                }
            )
        }
    ));

    let list = v["workspaces"]["list"].as_array().cloned().unwrap_or_default();
    out.push_str(&format!("\nCloud workspaces, by {}:\n", s(&v["workspaces"]["says"])));
    if list.is_empty() {
        out.push_str("  none\n");
    }
    for w in &list {
        let by =
            if w["created_by"].is_null() { String::new() } else { format!(", made by {}", name(&w["created_by"])) };
        out.push_str(&format!("  {:<24} {:<10} {}{by}\n", s(&w["name"]), s(&w["state"]), s(&w["full_use"])));
    }
    out
}

/// How long until `ms`: `3d`, `5h`, `12m`, or `now` once passed.
fn in_(ms: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let secs = ms.saturating_sub(now) / 1000;
    match secs {
        0 => "now".to_owned(),
        1..3600 => format!("{}m", secs.div_ceil(60)),
        3600..86400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// `POST /api/grants`'s body: an agent power needs a scope, cloud.drive
/// takes none.
fn give(daemon: &str, power: &str, scope: Option<&Value>) -> anyhow::Result<Value> {
    match (power, scope) {
        ("cloud.drive", None) => Ok(json!({ "daemon": daemon, "power": power })),
        ("cloud.drive", Some(_)) => bail!("cloud.drive is over your account: no --project or --dms"),
        (p, Some(s)) if PROJECT_ONLY.contains(&p) && s["kind"] == "account" => {
            bail!("{p} is over a project, not your DMs: --project NAME")
        }
        (p, Some(s)) if AGENT_POWERS.contains(&p) => Ok(json!({ "daemon": daemon, "power": power, "scope": s })),
        (p, None) if AGENT_POWERS.contains(&p) => {
            bail!("{p} is over a project or your DMs: add --project NAME or --dms")
        }
        (p, _) => bail!(
            "there's no power called {p}: threads.read, threads.post, issues.write, deploys.plan, runs.read, \
             runs.start, runs.command, boards.read, boards.write, trackers.write, sessions.start, sessions.reply or cloud.drive"
        ),
    }
}

/// The scope asked for, as `POST /api/grants` takes it: a project named by
/// its name or id among yours (`GET /api/projects`), with its name and
/// your role there beside it, or your DMs.
fn scope_of(e: &Enrolled, s: &Scope) -> anyhow::Result<(Option<Value>, Option<String>)> {
    if s.dms {
        return Ok((Some(json!({ "kind": "account" })), None));
    }
    let Some(want) = &s.project else { return Ok((None, None)) };
    let projects = e.api("GET", "/api/projects", None)?;
    let id = project_id(&projects, want)?;
    let p = projects["projects"].as_array().and_then(|all| all.iter().find(|p| p["id"] == id.as_str()));
    let name = p.and_then(|p| p["name"].as_str()).unwrap_or(&id).to_owned();
    let role = p.and_then(|p| p["team"]["role"].as_str().map(str::to_owned));
    Ok((Some(json!({ "kind": "project", "id": id, "name": name })), role))
}

/// The powers among `powers` the platform refuses to a viewer's machine
/// ([`VIEWER_REFUSED`]) when `role` there is viewer: for `add` to give
/// nothing rather than part of the list.
fn refused_for_viewer(powers: &[String], role: Option<&str>) -> Vec<String> {
    if role != Some("viewer") {
        return vec![];
    }
    powers.iter().filter(|p| VIEWER_REFUSED.contains(&p.as_str())).cloned().collect()
}

/// A project named on the command line, by its id or name (any case).
fn project_id(projects: &Value, want: &str) -> anyhow::Result<String> {
    let all = projects["projects"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = all
        .iter()
        .filter(|p| p["id"] == want || p["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(want)))
        .filter_map(|p| p["id"].as_str().map(String::from))
        .collect();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("you have no project called {want} (the platform page lists yours)"),
        _ => bail!("more than one project is called {want}: name one by its id ({})", ids.join(", ")),
    }
}

/// Whether a grant is over this scope.
fn over(g: &Value, scope: &Value) -> bool {
    match scope["kind"].as_str() {
        Some("account") => g["scope"]["kind"] == "account" && g["power"] != "cloud.drive",
        _ => g["scope"]["kind"] == "project" && g["scope"]["id"] == scope["id"],
    }
}

/// `geek  threads.post  project acme  live  grt_…`, or `revoked` and when
/// (and by whom, when someone other than its giver did), or a live one's
/// limit while you only view the project (Arugula #683: `idle`, `reads
/// only`; `--json` says why).
fn line(g: &Value) -> String {
    let name = g["subject"]["name"].as_str().or(g["subject"]["id"].as_str()).unwrap_or_default();
    let state = match g["revoked_at"].as_u64() {
        Some(at) => {
            let by = &g["revoked_by"];
            let giver = g["granted_by"].as_str().unwrap_or_default();
            let other = by.is_object() && by["account"].as_str() != Some(giver);
            match (other, by["name"].as_str().or(by["account"].as_str())) {
                (true, Some(who)) => format!("revoked {} by {who}", crate::hosts::ago(at)),
                _ => format!("revoked {}", crate::hosts::ago(at)),
            }
        }
        None => g["limited"]["label"].as_str().unwrap_or("live").to_owned(),
    };
    let scope = match (g["scope"]["kind"].as_str(), g["power"].as_str()) {
        (Some("project"), _) => {
            format!("project {}", g["scope"]["name"].as_str().or(g["scope"]["id"].as_str()).unwrap_or_default())
        }
        (_, Some("cloud.drive")) => "your account".into(),
        _ => "your DMs".into(),
    };
    format!(
        "{name:<20} {:<13} {scope:<24} {state:<16} {}",
        g["power"].as_str().unwrap_or_default(),
        g["id"].as_str().unwrap_or_default()
    )
}

/// A machine named on the command line, by its name or id, among those
/// with grants (every machine on the account has had one since it joined).
fn machine_id(grants: &[Value], machine: &str) -> anyhow::Result<String> {
    let mut ids: Vec<String> = grants
        .iter()
        .filter(|g| g["subject"]["id"] == machine || g["subject"]["name"] == machine)
        .filter_map(|g| g["subject"]["id"].as_str().map(String::from))
        .collect();
    ids.sort();
    ids.dedup();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("no machine on this account is called {machine} (`arugula grants` lists them)"),
        _ => bail!("more than one machine is called {machine}: name one by its id ({})", ids.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn a_machine_is_named_by_its_name_or_id_once() {
        let g = |id: &str, name: &str, live: bool| json!({ "subject": { "kind": "daemon", "id": id, "name": name }, "live": live });
        let grants = [g("d1", "geek", true), g("d1", "geek", false), g("d2", "box", true), g("d3", "box", true)];
        assert_eq!(machine_id(&grants, "geek").unwrap(), "d1");
        assert_eq!(machine_id(&grants, "d2").unwrap(), "d2");
        assert!(machine_id(&grants, "box").unwrap_err().to_string().contains("d2, d3"));
        assert!(machine_id(&grants, "mini").is_err());
        assert!(crate::Cli::try_parse_from(["arugula", "grants", "revoke", "geek"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "grants", "revoke", "grt_1"]).is_ok(), "one grant by its id");
        assert!(crate::Cli::try_parse_from(["arugula", "grants"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "grants", "ls", "--project", "acme"]).is_ok());
    }

    /// `ls --project`: the project's Access view as text, each part saying
    /// whose word it is; a control from before the view is told apart
    /// from a project this account isn't in.
    #[test]
    fn a_projects_access_view_reads_as_text_and_an_old_control_says_so() {
        let old = project_access(404, br#"{"error":"not found"}"#.to_vec()).unwrap_err().to_string();
        assert!(old.contains("doesn't list a project's access yet"), "{old}");
        let html = project_access(404, b"<html>nope</html>".to_vec()).unwrap_err().to_string();
        assert!(html.contains("update it"), "{html}");
        let none = project_access(404, br#"{"error":"no such project"}"#.to_vec()).unwrap_err().to_string();
        assert_eq!(none, "no such project");
        assert!(project_access(403, br#"{"error":"refused"}"#.to_vec()).unwrap_err().to_string().contains("refused"));
        let v = json!({
            "project": { "id": "prj_1", "name": "Acme", "owner": { "kind": "team", "id": "t", "locked": false } },
            "role": "viewer",
            "roles": { "says": "the team's roster", "members": [
                { "account": "a1", "name": "alice", "role": "owner" }, { "account": "b1", "name": "bob", "role": "editor" } ] },
            "cloud_drive": { "says": "the platform's cloud.drive grants", "members": [
                { "owner": { "account": "a1", "name": "alice", "former": false }, "role": "owner", "power": "cloud.drive", "machines": 2, "driving": 1 },
                { "owner": { "account": "b1", "name": "bob", "former": false }, "role": "editor", "power": "cloud.drive", "machines": 1, "driving": 1 } ] },
            "powers": { "says": "the platform's grants", "grants": [
                { "id": null, "subject": { "kind": "daemon", "id": "d1", "name": "geek" }, "owner": { "account": "a1", "name": "alice", "former": false },
                  "owner_role": "owner", "power": "issues.write", "created_at": 1, "limited": null },
                { "id": null, "subject": { "kind": "agent", "id": null, "name": null }, "owner": { "account": null, "name": null, "former": true },
                  "owner_role": null, "power": "boards.write", "created_at": 1,
                  "limited": { "label": "idle", "why": "does nothing: its giver doesn't read this project now" } },
                { "id": null, "subject": { "kind": "daemon", "id": "d2", "name": "box" }, "owner": { "account": "b1", "name": "bob", "locked_out": true },
                  "owner_role": null, "power": "threads.post", "created_at": 1,
                  "limited": { "label": "idle", "why": "does nothing while the team is locked" } } ] },
            "allow": { "says": "signed by a project owner", "delegation": { "live": true, "id": "dlg_1" },
                "run_grant": { "id": "pg_1", "made": 1, "expires": u64::MAX, "by": { "account": "a1", "name": "alice" }, "created_at": 1, "stale": false } },
            "workspaces": { "says": "the platform's list", "full_use": "owners and editors have full use; viewers watch", "list": [
                { "id": "hw_1", "name": "dev", "state": "ready", "created_by": { "account": "b1", "name": "bob", "former": false }, "created_at": 1,
                  "full_use": "owners and editors have full use; viewers watch" } ] }
        });
        let text = access_text(&project_access(200, v.to_string().into_bytes()).unwrap());
        for want in [
            "Acme (team project); you're its viewer.",
            "Project roles, by the team's roster:",
            "alice                    owner",
            "Cloud workspaces' drivers, by the platform's cloud.drive grants:",
            "alice                    1 of 2 machines drive as an owner (the rest: cloud.drive revoked by its owner)",
            "bob                      1 machine drive as an editor",
            "Powers over it, by the platform's grants:",
            "geek (alice's)",
            "issues.write",
            "a token (a former member's)",
            "does nothing: its giver doesn't read this project now",
            "box (bob (locked out)'s)",
            "does nothing while the team is locked",
            "cloud workspaces: allowed (delegation dlg_1)",
            "runs with every device asleep: allowed by alice",
            "dev                      ready      owners and editors have full use; viewers watch, made by bob",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
        assert!(
            !text.contains("agt_") && !text.contains("(MCP)") && !text.contains("grt_") && !text.contains("ci"),
            "{text}"
        );
        let mut stale = v.clone();
        stale["allow"]["run_grant"]["stale"] = json!(true);
        assert!(access_text(&stale).contains("names an earlier delegation"));
        let empty = json!({ "project": { "name": "Personal", "owner": { "kind": "account", "id": "a1" } }, "role": "owner",
            "roles": { "says": "this account's own project", "members": [{ "account": "a1", "name": "alice", "role": "owner" }] },
            "cloud_drive": { "says": "the platform's cloud.drive grants", "members": [
                { "owner": { "account": "a1", "name": "alice", "former": false }, "role": "owner", "power": "cloud.drive", "machines": 0, "driving": 0 } ] },
            "powers": { "says": "the platform's grants", "grants": [] },
            "allow": { "says": "signed by a project owner", "delegation": null, "run_grant": null },
            "workspaces": { "says": "the platform's list", "list": [] } });
        let text = access_text(&empty);
        for want in [
            "Personal (personal project)",
            "alice                    no machine joined",
            "no other power: no machine's",
            "control couldn't say",
            "no live standing grant",
            "  none\n",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
    }

    /// A revoked line says who, when it wasn't the giver; a live grant that
    /// does nothing says idle (Arugula #683).
    #[test]
    fn a_line_says_who_revoked_and_what_is_idle() {
        let g = |revoked_at: Option<u64>, by: Value, idle: Value| {
            json!({ "subject": { "name": "geek" }, "power": "issues.write", "granted_by": "b",
                "scope": { "kind": "project", "id": "prj_2", "name": "Acme" }, "id": "grt_1",
                "revoked_at": revoked_at, "revoked_by": by, "limited": idle })
        };
        assert!(line(&g(None, Value::Null, Value::Null)).contains(" live "));
        assert!(line(&g(None, Value::Null, json!({ "label": "idle", "why": "does nothing" }))).contains(" idle "));
        assert!(line(&g(None, Value::Null, json!({ "label": "reads only", "why": "…" }))).contains(" reads only "));
        let own = line(&g(Some(1), json!({ "account": "b", "name": "Bea" }), Value::Null));
        assert!(own.contains("revoked") && !own.contains(" by "), "{own}");
        let owner = line(&g(Some(1), json!({ "account": "a", "name": "Ana" }), Value::Null));
        assert!(owner.contains("by Ana"), "{owner}");
        let unnamed = line(&g(Some(1), json!({ "account": "a", "name": null }), Value::Null));
        assert!(unnamed.contains("by a"), "{unnamed}");
        let erased = line(&g(Some(1), json!({ "account": null, "name": "an account since deleted" }), Value::Null));
        assert!(erased.contains("by an account since deleted"), "{erased}");
        let older = line(&g(Some(1), Value::Null, Value::Null));
        assert!(older.contains("revoked") && !older.contains(" by "), "an older platform: {older}");
    }

    /// A control of the test's own: it answers `GET /api/grants` and `GET
    /// /api/projects` as given, `200 {}` to anything else, and keeps every
    /// request it saw (method, path, body). One connection per request,
    /// as the CLI makes them.
    fn control(grants: Value, projects: Value) -> (Enrolled, std::sync::mpsc::Receiver<(String, String, String)>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for sock in l.incoming() {
                let sock = sock.unwrap();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut words = line.split_whitespace();
                let (method, path) =
                    (words.next().unwrap_or_default().to_owned(), words.next().unwrap_or_default().to_owned());
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if line.trim().is_empty() {
                        break;
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let answer = match path.as_str() {
                    "/api/grants" if method == "GET" => grants.to_string(),
                    "/api/projects" => projects.to_string(),
                    _ => "{}".to_owned(),
                };
                // Kept before it's answered, so the CLI's return finds it.
                if tx.send((method, path, String::from_utf8_lossy(&body).into_owned())).is_err() {
                    break;
                }
                let mut sock = sock;
                write!(sock, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{answer}", answer.len()).unwrap();
            }
        });
        (Enrolled::for_tests(url), rx)
    }

    /// `add` gives nothing when any power in the list would be refused to
    /// a viewer's machine (Arugula #683): the whole list is checked first,
    /// before any grant is asked for; an editor's list is given whole, one
    /// POST each, in order. The refused powers are the wire's list.
    #[test]
    fn add_gives_nothing_rather_than_part_of_a_list() {
        let powers = |l: &[&str]| l.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            refused_for_viewer(&powers(&["threads.read", "issues.write", "runs.start"]), Some("viewer")),
            ["issues.write", "runs.start"]
        );
        assert!(
            refused_for_viewer(&powers(&["threads.read", "deploys.plan", "boards.write"]), Some("viewer")).is_empty()
        );
        assert!(refused_for_viewer(&powers(&["issues.write"]), Some("editor")).is_empty());
        assert!(refused_for_viewer(&powers(&["issues.write"]), None).is_empty(), "your DMs, or an older platform");
        assert_eq!(
            refused_for_viewer(&powers(&["secrets.write"]), Some("viewer")),
            ["secrets.write"],
            "the wire's list"
        );

        let grants = json!({ "grants": [{ "id": "grt_0", "subject": { "kind": "daemon", "id": "d1", "name": "geek" },
            "power": "cloud.drive", "scope": { "kind": "account", "id": "a" }, "live": true }] });
        let projects = |role: &str| json!({ "projects": [{ "id": "prj_2", "name": "Acme", "team": { "id": "t", "name": "Acme", "role": role } }] });
        let add = |powers: &[&str]| GrantsCmd::Add {
            machine: "geek".into(),
            powers: powers.iter().map(|p| (*p).to_owned()).collect(),
            scope: Scope { project: Some("acme".into()), dms: false },
        };
        let (e, seen) = control(grants.clone(), projects("viewer"));
        let err = run_with(&e, Some(add(&["threads.read", "issues.write", "runs.start"])), true).unwrap_err();
        assert!(err.to_string().contains("issues.write, runs.start would do nothing"), "{err:#}");
        let seen: Vec<_> = seen.try_iter().map(|(m, p, _)| format!("{m} {p}")).collect();
        assert_eq!(seen, ["GET /api/grants", "GET /api/projects"], "nothing was given");

        let (e, seen) = control(grants, projects("editor"));
        assert_eq!(run_with(&e, Some(add(&["threads.read", "issues.write"])), true).unwrap(), 0);
        let seen: Vec<_> = seen.try_iter().collect();
        let posts: Vec<&str> = seen.iter().filter(|(m, _, _)| m == "POST").map(|(_, _, b)| b.as_str()).collect();
        assert_eq!(seen.len(), 4, "{seen:?}");
        assert!(posts[0].contains("\"threads.read\"") && posts[1].contains("\"issues.write\""), "{posts:?}");
        assert!(posts.iter().all(|b| b.contains("\"prj_2\"")), "{posts:?}");
    }

    /// `add` takes one or more powers and one scope; an agent power needs
    /// one, cloud.drive none.
    #[test]
    fn an_agent_power_is_given_over_a_project_or_the_dms() {
        let parse = |args: &[&str]| crate::Cli::try_parse_from([&["arugula", "grants"], args].concat());
        assert!(parse(&["add", "geek", "threads.read", "threads.post", "--project", "acme"]).is_ok());
        assert!(parse(&["add", "geek", "threads.read", "--dms"]).is_ok());
        assert!(parse(&["add", "geek", "threads.read", "--dms", "--project", "acme"]).is_err(), "one scope");
        assert!(parse(&["add", "geek"]).is_err(), "a power");
        assert!(parse(&["revoke", "geek", "threads.post", "--project", "acme"]).is_ok());

        let dms = json!({ "kind": "account" });
        assert_eq!(give("d1", "threads.post", Some(&dms)).unwrap()["scope"], dms);
        assert_eq!(give("d1", "cloud.drive", None).unwrap(), json!({ "daemon": "d1", "power": "cloud.drive" }));
        assert!(give("d1", "threads.post", None).unwrap_err().to_string().contains("--project NAME or --dms"));
        assert!(give("d1", "cloud.drive", Some(&dms)).is_err());
        assert!(give("d1", "pane.drive", Some(&dms)).is_err());
        let acme = json!({ "kind": "project", "id": "prj_2" });
        assert_eq!(give("d1", "runs.start", Some(&acme)).unwrap()["power"], "runs.start");
        assert!(give("d1", "runs.read", Some(&dms)).unwrap_err().to_string().contains("over a project"));
        assert!(parse(&["add", "geek", "runs.read", "runs.start", "--project", "acme"]).is_ok());
        assert_eq!(give("d1", "runs.command", Some(&acme)).unwrap()["power"], "runs.command");
        assert!(give("d1", "runs.command", Some(&dms)).is_err());

        let projects = json!({ "projects": [
            { "id": "prj_1", "name": "Personal" }, { "id": "prj_2", "name": "Acme" }, { "id": "prj_3", "name": "acme" },
        ] });
        assert_eq!(project_id(&projects, "personal").unwrap(), "prj_1");
        assert_eq!(project_id(&projects, "prj_3").unwrap(), "prj_3");
        assert!(project_id(&projects, "acme").unwrap_err().to_string().contains("prj_2, prj_3"));
        assert!(project_id(&projects, "none").is_err());

        let g = |power: &str, kind: &str, id: &str| json!({ "power": power, "scope": { "kind": kind, "id": id } });
        assert!(over(&g("threads.read", "account", "a"), &dms));
        assert!(!over(&g("cloud.drive", "account", "a"), &dms), "--dms isn't cloud.drive");
        assert!(over(&g("threads.read", "project", "prj_2"), &json!({ "kind": "project", "id": "prj_2" })));
        assert!(!over(&g("threads.read", "project", "prj_3"), &json!({ "kind": "project", "id": "prj_2" })));
        assert!(
            line(&json!({ "subject": { "name": "geek" }, "power": "threads.post",
            "scope": { "kind": "project", "id": "prj_2", "name": "Acme" }, "id": "grt_1" }))
            .contains("project Acme")
        );
    }
}
