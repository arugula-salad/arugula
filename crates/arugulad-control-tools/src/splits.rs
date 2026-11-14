//! Moving work out of a project from agents: `project_split` (Arugula
//! #973). It plans unless `confirm` is true, as `arugula project split`
//! does without `--yes`: the planner's answer says what to deal with
//! before moving and what comes along, and nothing changes.
//!
//! Each call is the platform's `/api/projects/{project}/split/plan` or
//! `/split` (the web's `platform/move/plan.ts` has their shapes), signed
//! with this machine's key and acting as its owner. What comes along is
//! the planner's to say: nothing here works it out. A move the project
//! changed under since its plan is refused (409), and says to plan again.

use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    control::CloudRefused,
    mcp::{
        issues::{self, native},
        tools::{Call, Out, done},
    },
};

pub(crate) const TOOL: &str = "project_split";

pub(crate) const DESCRIPTION: &str = "Move work out of one of the user's projects in Arugula cloud (default the current one) into a new project (name, and owner: personal or a team's tm_ id) or one that exists (into, its prj_ id): linked repos (OWNER/NAME), boards (brd_…), issues (#N in the project, or iss_…) and Docs (pdc_…). A repo's boards, issues and sessions come with it; a board's issues and Docs with the board. Without confirm: true it only plans and changes nothing: it says what to deal with before moving (notes) and what comes along, with each issue's new number. Show the person the plan and move only once they agree; a move refused because the project changed since the plan needs a new plan.";

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
    /// Move. Default false: only plan.
    #[serde(default)]
    pub confirm: bool,
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

/// The plan in a few sentences: where, the notes, what comes along.
fn planned(plan: &Value) -> String {
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
        "Planned, nothing moved: from {from} into {into}, its issues numbered from #{first}. Before you move: {before} Comes along: {}. Call again with confirm: true to move, once your person agrees.",
        counted(&plan["along"])
    )
}

/// The move: where it went, what moved and the issues' new numbers.
fn moved(done: &Value) -> String {
    let to = &done["to"];
    let mut out = format!(
        "Moved into {} ({}): {}.",
        to["name"].as_str().unwrap_or("?"),
        to["id"].as_str().unwrap_or("?"),
        counted(&done["moved"])
    );
    let renumbered: Vec<String> =
        done["issues"].as_array().into_iter().flatten().map(|i| format!("#{} is #{}", i["from"], i["to"])).collect();
    if !renumbered.is_empty() {
        out += &format!(" Renumbered: {}.", renumbered.join(", "));
    }
    out
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
    if !a.confirm {
        let plan = c.issues_call(Method::POST, &format!("/api/projects/{from}/split/plan"), Some(ask)).await?;
        return done(planned(&plan), plan);
    }
    let v = match c.app().control.cloud(Method::POST, &format!("/api/projects/{from}/split"), Some(&ask)).await {
        Err(CloudRefused::Said { status: 409, said, .. }) if !said.contains("with that name") => {
            return Err(format!(
                "{said}: the project changed since its plan; plan again (confirm false) and show your person"
            ));
        }
        r => r.map_err(issues::said)?,
    };
    done(moved(&v), v)
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
        assert!(!args(json!({ "name": "A" })).confirm, "it plans unless asked");
    }

    #[test]
    fn the_plan_and_the_move_in_words() {
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
            planned(&plan),
            "Planned, nothing moved: from Acme into a new project, App, its issues numbered from #1. Before you move: Two people aren't in App. Comes along: 1 repo, 3 GitHub issues and PRs, 1 Doc on a machine, 2 links. Call again with confirm: true to move, once your person agrees."
        );
        let done = json!({ "id": "spl_1", "to": { "id": "prj_2", "name": "App" },
                           "moved": { "repos": 1, "issues": 2, "runs": 0 },
                           "issues": [{ "id": "iss_a", "from": 3, "to": 1 }] });
        assert_eq!(moved(&done), "Moved into App (prj_2): 1 repo, 2 issues. Renumbered: #3 is #1.");
    }
}
