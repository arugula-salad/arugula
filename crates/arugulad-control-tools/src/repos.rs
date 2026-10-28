//! A project's linked GitHub repos from agents (M163): `list` kind
//! `linked_repos`, and `link_repo` kinds `link` and `unlink`.
//!
//! They live in control: each call is control's
//! `/api/projects/{project}/repos`, signed with this machine's key, as
//! the Cloud tools' are ([`Control::cloud`](crate::control::Control::cloud)).
//! Control acts for the machine's owner while the machine holds its live
//! `cloud.drive` grant, and links a repo only when the owner is an admin
//! or maintainer of it on GitHub.

use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::tools::{Call, Kind, Out, done};

#[derive(Deserialize, JsonSchema)]
pub struct ProjectArgs {
    /// The project's id (prj_…). Default: the user's personal project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RepoArgs {
    /// The repo on GitHub, OWNER/NAME.
    pub repo: String,
    /// The project's id (prj_…). Default: the user's personal project.
    #[serde(default)]
    pub project: Option<String>,
}

pub(crate) const LIST_LINKED: Kind = Kind {
    name: "linked_repos",
    description: "The GitHub repos linked to one of the user's projects in Arugula control (default their personal one): each one's name, whether it's private, and, for one unlinked, when its rows are purged.",
    schema: schema_for_type::<ProjectArgs>,
    flag: None,
};

pub(crate) const LINK: &[Kind] = &[
    Kind {
        name: "link",
        description: "Link a GitHub repo to one of the user's projects (default their personal one), as the user: they must be an admin or maintainer of it on GitHub. If Arugula's GitHub App isn't installed where it lives, the answer says where the user installs it.",
        schema: schema_for_type::<RepoArgs>,
        flag: None,
    },
    Kind {
        name: "unlink",
        description: "Unlink a GitHub repo from the project: nothing more is read from GitHub for it, and its rows are purged within a day unless it's linked again first.",
        schema: schema_for_type::<RepoArgs>,
        flag: None,
    },
];

/// The project in a path: its id, or `personal`. Nothing that isn't an id
/// reaches the path.
fn project_path(project: Option<&str>) -> Result<String, String> {
    match project {
        None => Ok("/api/projects/personal/repos".into()),
        Some(p)
            if p.strip_prefix("prj_")
                .is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric())) =>
        {
            Ok(format!("/api/projects/{p}/repos"))
        }
        Some(p) => Err(format!("project: a project's id (prj_…), not {p:?}")),
    }
}

/// OWNER and NAME, as GitHub spells them.
fn owner_name(repo: &str) -> Result<(&str, &str), String> {
    let ok = |s: &str| {
        !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    match repo.trim().split_once('/') {
        Some((o, n)) if ok(o) && ok(n) => Ok((o, n)),
        _ => Err(format!("repo: OWNER/NAME on GitHub, not {repo:?}")),
    }
}

fn line(r: &Value) -> String {
    let name = r["full_name"].as_str().unwrap_or("?");
    match r["purge_after"].as_u64() {
        Some(_) => format!("{name} (unlinked, waiting to be purged)"),
        None => format!("{name}{}", if r["private"] == true { " (private)" } else { "" }),
    }
}

async fn repos_call(c: &Call<'_>, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    c.app().control.cloud(method, path, body.as_ref()).await.map_err(|e| e.to_string())
}

pub(crate) async fn list_linked_repos(c: &Call<'_>, a: ProjectArgs) -> Out {
    let v = repos_call(c, Method::GET, &project_path(a.project.as_deref())?, None).await?;
    let repos = v["repos"].as_array().cloned().unwrap_or_default();
    let summary = if repos.is_empty() {
        "No GitHub repos are linked to the project.".to_owned()
    } else {
        format!("{} linked: {}", repos.len(), repos.iter().map(line).collect::<Vec<_>>().join(", "))
    };
    done(summary, json!({ "project": v["project"], "repos": repos }))
}

pub(crate) async fn link_repo(c: &Call<'_>, kind: &str, a: RepoArgs) -> Out {
    let (owner, name) = owner_name(&a.repo)?;
    let path = project_path(a.project.as_deref())?;
    let v = match kind {
        "link" => repos_call(c, Method::POST, &path, Some(json!({ "repo": format!("{owner}/{name}") }))).await?,
        _ => repos_call(c, Method::DELETE, &format!("{path}/{owner}/{name}"), None).await?,
    };
    let said = if kind == "link" { "Linked" } else { "Unlinked" };
    done(format!("{said} {}", line(&v)), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ids_and_repo_names_reach_the_path() {
        assert_eq!(project_path(None).unwrap(), "/api/projects/personal/repos");
        assert_eq!(project_path(Some("prj_ab12")).unwrap(), "/api/projects/prj_ab12/repos");
        assert!(project_path(Some("prj_../x")).is_err());
        assert!(project_path(Some("Acme")).is_err());
        assert_eq!(owner_name("acme/app").unwrap(), ("acme", "app"));
        assert!(owner_name("acme/app/x").is_err());
        assert!(owner_name("../x").is_err());
        assert!(owner_name("acme").is_err());
    }
}
