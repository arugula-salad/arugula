//! Planning to move work out of a project, for agents: `project_split`
//! (Arugula #973). It only plans, as `arugula project split` does without
//! `--yes`: the planner's answer says what to deal with before moving and
//! what comes along, and nothing changes. An agent never moves work between
//! projects: the answer ends with the command its person runs to move it,
//! or where on the page they do.
//!
//! The call is the platform's planner for machines, `/api/daemon/projects/
//! {project}/split/plan` (the web's `platform/move/plan.ts` has its shape),
//! signed with this machine's key and acting as its owner with the
//! machine's grant over the project. What comes along is the planner's to
//! say: nothing here works it out.

use arugula_control_wire::projects::agent_split_plan;
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::{
    issues::native,
    tools::{Call, Out, done},
};

pub(crate) const TOOL: &str = "project_split";

pub(crate) const DESCRIPTION: &str = "Plan moving work out of one of the user's projects in Arugula cloud (default the current one) into a new project (name, and owner: personal or a team's tm_ id) or one that exists (into, its prj_ id): linked repos (OWNER/NAME), boards (brd_…), issues (#N in the project, or iss_…) and Docs (pdc_…). A repo's boards, issues and sessions come with it; a board's issues and Docs with the board. It only plans and changes nothing: it says what to deal with before moving (notes) and what comes along, with each issue's new number. Agents never move it: show the person the plan and the command it ends with, which they run themselves (or they use Move to project… in the project's settings).";

#[derive(Deserialize, JsonSchema)]
pub struct SplitArgs {
    /// The project to move work out of: its id (prj_…) or personal.
    /// Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
    /// Move it into a new project with this name.
    #[serde(default)]
    pub name: Option<String>,
    /// The new project's owner: personal (the default) or a team's id (tm_…).
    #[serde(default)]
    pub owner: Option<String>,
    /// Move it into a project that exists: its id (prj_…).
    #[serde(default)]
    pub into: Option<String>,
    /// Linked repos, OWNER/NAME.
    #[serde(default)]
    pub repos: Vec<String>,
    /// Boards, brd_….
    #[serde(default)]
    pub boards: Vec<String>,
    /// Issues: "#N" (in the project) or "iss_…".
    #[serde(default)]
    pub issues: Vec<String>,
    /// Project Docs, pdc_….
    #[serde(default)]
    pub docs: Vec<String>,
}

/// Where it goes, as the platform's request says it.
fn to_of(a: &SplitArgs) -> Result<Value, String> {
    let name = a.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let into = a.into.as_deref().map(str::trim).filter(|p| !p.is_empty());
    match (name, into) {
        (Some(_), Some(_)) => Err("name a new project or into one that exists, not both".into()),
        (Some(name), None) => {
            let owner = match a.owner.as_deref().map(str::trim) {
                None | Some("" | "personal") => json!({ "kind": "personal" }),
                Some(t) if t.starts_with("tm_") => json!({ "kind": "team", "id": t }),
                Some(t) => return Err(format!("owner: personal or a team's id (tm_…), not {t:?}")),
            };
            Ok(json!({ "kind": "new", "name": name, "owner": owner }))
        }
        (None, Some(p)) if p.strip_prefix("prj_").is_some_and(|h| !h.is_empty()) => {
            Ok(json!({ "kind": "project", "id": p }))
        }
        (None, Some(p)) => Err(format!("into: a project's id (prj_…), not {p:?}")),
        (None, None) => Err("say where it goes: name for a new project, or into for one that exists".into()),
    }
}

/// The plan's and the move's lists, by count; `links` last.
const ALONG: [(&str, &str, &str); 12] = [
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
    ("links", "link", "links"),
];

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

/// `1 repo, 3 issues`, or `nothing`.
fn counted(along: &Value) -> String {
    let all: Vec<String> = ALONG
        .iter()
        .filter_map(|(key, one, many)| {
            let n = count(key, &along[*key]);
            (n > 0).then(|| format!("{n} {}", if n == 1 { one } else { many }))
        })
        .collect();
    if all.is_empty() { "nothing".into() } else { all.join(", ") }
}

/// `s` as one shell word: as it is when that's safe, else in single quotes.
fn word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:@+".contains(c)) {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The command that moves what `a` planned out of `from`, for the person
/// to run: the same flags, with `--yes`.
fn command(from: &str, a: &SplitArgs) -> String {
    let mut out = format!("arugula project split --project {}", word(from));
    let mut flag = |name: &str, v: &str| {
        let v = v.trim();
        if !v.is_empty() {
            out += &format!(" --{name} {}", word(v));
        }
    };
    if let Some(n) = &a.name {
        flag("name", n);
    }
    if let Some(o) = a.owner.as_deref().filter(|o| o.trim() != "personal") {
        flag("owner", o);
    }
    if let Some(p) = &a.into {
        flag("into", p);
    }
    for (name, all) in [("repo", &a.repos), ("board", &a.boards), ("issue", &a.issues), ("doc", &a.docs)] {
        for v in all {
            flag(name, v);
        }
    }
    out + " --yes"
}

/// The plan in a few sentences: where, the notes, what comes along, and
/// how the person moves it (`how`).
fn planned(plan: &Value, how: &str) -> String {
    let to = &plan["to"];
    let name = to["name"].as_str().unwrap_or("?");
    let into = match to["id"].as_str() {
        Some(id) => format!("{name} ({id})"),
        None => format!("a new project, {name}"),
    };
    let from = plan["from"]["name"].as_str().unwrap_or("?");
    let first = to["first_number"].as_u64().unwrap_or(1);
    if plan["empty"] == true {
        return format!("Nothing would move from {from} into {into}: name a repo, board, issue or Doc of the project.");
    }
    let notes: Vec<&str> = plan["notes"].as_array().into_iter().flatten().filter_map(|n| n["text"].as_str()).collect();
    let before = if notes.is_empty() { "nothing".to_owned() } else { notes.join(" ") };
    format!(
        "Planned, nothing moved: from {from} into {into}, its issues numbered from #{first}. Before you move: {before} Comes along: {}.\nTo move it, your person runs `{how}` or opens Move to project… in the project's settings.",
        counted(&plan["along"])
    )
}

pub(crate) async fn project_split(c: &Call<'_>, a: SplitArgs) -> Out {
    c.owners_own("project_split").await?;
    let to = to_of(&a)?;
    let from = c.chosen_project(a.project.as_deref()).await?.id;
    let mut issues = Vec::with_capacity(a.issues.len());
    for i in &a.issues {
        let issue = c.native_issue(&native(i)?, Some(&from)).await?;
        issues.push(issue["id"].as_str().ok_or_else(|| format!("{i} has no id"))?.to_owned());
    }
    let ask = json!({ "to": to, "repos": a.repos, "boards": a.boards, "issues": issues, "docs": a.docs });
    let plan = c.issues_call(Method::POST, &agent_split_plan(&from), Some(ask)).await?;
    done(planned(&plan, &command(&from, &a)), plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> SplitArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn where_it_goes() {
        let new = to_of(&args(json!({ "name": "App" }))).unwrap();
        assert_eq!(new, json!({ "kind": "new", "name": "App", "owner": { "kind": "personal" } }));
        let team = to_of(&args(json!({ "name": "App", "owner": "tm_1" }))).unwrap();
        assert_eq!(team["owner"], json!({ "kind": "team", "id": "tm_1" }));
        assert_eq!(to_of(&args(json!({ "into": "prj_ab" }))).unwrap(), json!({ "kind": "project", "id": "prj_ab" }));
        assert!(to_of(&args(json!({}))).is_err());
        assert!(to_of(&args(json!({ "name": "A", "into": "prj_ab" }))).is_err());
        assert!(to_of(&args(json!({ "into": "Acme" }))).is_err());
        assert!(to_of(&args(json!({ "name": "A", "owner": "Acme" }))).is_err());
    }

    #[test]
    fn the_person_moves_it_with_the_same_flags() {
        let a = args(json!({ "name": "My App", "owner": "tm_1", "repos": ["acme/app"], "issues": ["#3", "iss_x"] }));
        assert_eq!(
            command("prj_1", &a),
            "arugula project split --project prj_1 --name 'My App' --owner tm_1 --repo acme/app --issue '#3' --issue iss_x --yes"
        );
        let a = args(json!({ "into": "prj_2", "owner": "personal", "boards": ["brd_1"], "docs": ["pdc_1"] }));
        assert_eq!(
            command("prj_1", &a),
            "arugula project split --project prj_1 --into prj_2 --board brd_1 --doc pdc_1 --yes"
        );
        assert_eq!(word("it's"), r"'it'\''s'");
    }

    #[test]
    fn the_plan_in_words() {
        let plan = json!({
            "from": { "id": "prj_1", "name": "Acme" },
            "to": { "name": "App", "first_number": 1, "owner": { "kind": "personal", "account": "a" } },
            "empty": false,
            "notes": [{ "key": "people", "text": "Two people aren't in App." }],
            "along": { "repos": { "count": 1, "items": [] }, "github": { "issues": 2, "prs": 1, "repos": [] },
                       "machine_docs": [{ "machine": "m", "name": "m", "docs": [{ "id": "d", "title": "t" }] }],
                       "workspaces": [], "sessions": 0, "links": 2 }
        });
        assert_eq!(
            planned(&plan, "arugula project split --project prj_1 --name App --repo acme/app --yes"),
            "Planned, nothing moved: from Acme into a new project, App, its issues numbered from #1. Before you move: Two people aren't in App. Comes along: 1 repo, 3 GitHub issues and PRs, 1 Doc on a machine, 2 links.\nTo move it, your person runs `arugula project split --project prj_1 --name App --repo acme/app --yes` or opens Move to project… in the project's settings."
        );
    }
}
