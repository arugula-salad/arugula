//! `issue_write` and `forge_read` kind `native_issues` (M200): agents file,
//! change and list a project's own issues in Arugula control (M194).
//! Reading one, which `read_forge` does too, and the rules for which issue
//! and which project, are the base's (`mcp::issues`).
//!
//! **Native writes land at once**, as the owner's, labelled "<owner>'s
//! agent on <machine>": the platform checks the machine's grants (M197:
//! `threads.read` to read, `issues.write` to change, `threads.post` to
//! comment) and never goes past the owner's role. **GitHub's stay
//! drafts** a person sends (aru-041, aru-085): `draft` and `forge_draft`
//! refuse a native issue and say to use `issue_write`.
//!
//! Each call is a machine-signed route on the platform
//! (`/api/daemon/issues/...`, `crates/platform/src/issues/agent.rs`); a
//! comment is a post to the issue's thread (M198). An agent a guest stands
//! behind may not use them: they act as the owner.

use arugula_control_wire::{
    chat::agent_messages,
    issues::{IssueListEdit, IssuePatch, IssueQuery, NewIssue, agent_issue, agent_project_issues},
};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::{
    issues::{Native, line, native, query},
    tools::{Call, Kind, Out, done},
};

pub(crate) const WRITE_TOOL: &str = "issue_write";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to Arugula's own (native) issues in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. An issue is #N (in the current project: project, else the one this clone's GitHub repo is linked to, else your person's personal project) or iss_…. Their grants say what you may do (issues.write to file and change, threads.post to comment; changing reads it first, with threads.read), and never more than their role. GitHub's PRs and issues aren't written here: draft and forge_draft make drafts that wait for a person to send them.";

#[derive(Deserialize, JsonSchema)]
pub struct CreateArgs {
    pub title: String,
    /// In markdown.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Accounts in the project.
    #[serde(default)]
    pub assignees: Vec<String>,
    /// The project's id (prj_…) or personal. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CommentArgs {
    /// The issue: "#N" (in the current project) or "iss_…".
    pub issue: String,
    /// The comment, in markdown; @login mentions a person there.
    pub text: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditArgs {
    /// The issue: "#N" (in the current project) or "iss_…".
    pub issue: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// The issue: "#N" (in the current project) or "iss_…".
    pub issue: String,
    /// label: labels; assign: accounts in the project.
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloseArgs {
    /// The issue: "#N" (in the current project) or "iss_…".
    pub issue: String,
    /// completed (the default), not_planned or duplicate.
    #[serde(default)]
    pub reason: Option<String>,
    /// A comment to leave with it, posted once the close has landed.
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReopenArgs {
    /// The issue: "#N" (in the current project) or "iss_…".
    pub issue: String,
    #[serde(default)]
    pub project: Option<String>,
}

/// `forge_read` kind `native_issues`.
#[derive(Deserialize, JsonSchema)]
pub struct NativeListArgs {
    /// The project's id (prj_…) or personal. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
    /// open (the default), closed or all.
    #[serde(default)]
    pub state: Option<String>,
    /// Issues with each of these labels.
    #[serde(default)]
    pub labels: Vec<String>,
    /// An account.
    #[serde(default)]
    pub assignee: Option<String>,
    /// An account.
    #[serde(default)]
    pub author: Option<String>,
    /// Words in the title or body (dep*: a word starting with dep).
    #[serde(default)]
    pub q: Option<String>,
    /// At most this many (default 50, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Issues numbered below this: the next page.
    #[serde(default)]
    pub before: Option<u64>,
}

pub(crate) const WRITE: &[Kind] = &[
    Kind {
        name: "create",
        description: "File a new issue in a project (issues.write).",
        schema: schema_for_type::<CreateArgs>,
        flag: None,
    },
    Kind {
        name: "comment",
        description: "Comment on an issue: a post in its thread (threads.post).",
        schema: schema_for_type::<CommentArgs>,
        flag: None,
    },
    Kind {
        name: "edit",
        description: "Change an issue's title or body.",
        schema: schema_for_type::<EditArgs>,
        flag: None,
    },
    Kind {
        name: "label",
        description: "Add labels to an issue or take them off.",
        schema: schema_for_type::<ListArgs>,
        flag: None,
    },
    Kind {
        name: "assign",
        description: "Assign people in the project to an issue, or unassign them.",
        schema: schema_for_type::<ListArgs>,
        flag: None,
    },
    Kind {
        name: "close",
        description: "Close an issue (completed, not_planned or duplicate), with a comment if given, posted once it's closed.",
        schema: schema_for_type::<CloseArgs>,
        flag: None,
    },
    Kind {
        name: "reopen",
        description: "Open a closed issue again.",
        schema: schema_for_type::<ReopenArgs>,
        flag: None,
    },
];

/// `forge_read`'s kind for a project's native issues.
pub(crate) const LIST: Kind = Kind {
    name: "native_issues",
    description: "Arugula's own (native) issues of a project in Arugula control (the current one unless project names it), newest first; state, labels, assignee, author and q (search words) narrow it. issue_write writes to them at once.",
    schema: schema_for_type::<NativeListArgs>,
    flag: None,
};

fn args<T: serde::de::DeserializeOwned>(a: Value) -> Result<T, String> {
    serde_json::from_value(a).map_err(|e| format!("bad arguments: {e}"))
}
/// `forge_read` kind `native_issues`.
pub(crate) async fn list_native(c: &Call<'_>, a: Value) -> Out {
    c.owners_own("forge_read kind native_issues").await?;
    let a: NativeListArgs = args(a)?;
    let project = c.current_project(a.project.as_deref()).await?;
    let q = IssueQuery {
        state: a.state,
        labels: (!a.labels.is_empty()).then(|| a.labels.join(",")),
        assignee: a.assignee,
        author: a.author,
        q: a.q,
        before: a.before,
        limit: a.limit,
    };
    let qs = issue_query(&q);
    let v = c.issues_call(Method::GET, &format!("{}?{qs}", agent_project_issues(&project)), None).await?;
    let issues = v["issues"].as_array().cloned().unwrap_or_default();
    let mut s = match issues.len() {
        0 => format!("No native issues in project {project} match."),
        n => format!("{n} in project {project}: {}", issues.iter().map(line).collect::<Vec<_>>().join("; ")),
    };
    if v["more"] == true {
        s += " (more: before the last one's number)";
    }
    done(s, v)
}

/// `issue_write`.
pub(crate) async fn issue_write(c: &Call<'_>, kind: &str, a: Value) -> Out {
    c.owners_own(WRITE_TOOL).await?;
    // Only what changes.
    let patch = |p: IssuePatch| {
        let mut v = serde_json::to_value(p).map_err(|e| e.to_string())?;
        if let Some(o) = v.as_object_mut() {
            o.retain(|_, v| !v.is_null());
        }
        Ok::<_, String>(v)
    };
    let (issue, project, body) = match kind {
        "create" => {
            let a: CreateArgs = args(a)?;
            let project = c.current_project(a.project.as_deref()).await?;
            let new =
                NewIssue { title: a.title, body: a.body.unwrap_or_default(), labels: a.labels, assignees: a.assignees };
            let body = serde_json::to_value(new).map_err(|e| e.to_string())?;
            let i = c.issues_call(Method::POST, &agent_project_issues(&project), Some(body)).await?;
            let s = format!("Filed {} in project {project}, now, as your person's agent on this machine", line(&i));
            return done(s, json!({ "issue": i }));
        }
        "comment" => {
            let a: CommentArgs = args(a)?;
            let i = c.native_issue(&native(&a.issue)?, a.project.as_deref()).await?;
            return comment_on(c, &i, a.text).await;
        }
        "edit" => {
            let a: EditArgs = args(a)?;
            if a.title.is_none() && a.body.is_none() {
                return Err("edit needs title or body".into());
            }
            (a.issue, a.project, patch(IssuePatch { title: a.title, body: a.body, ..Default::default() })?)
        }
        "label" | "assign" => {
            let a: ListArgs = args(a)?;
            let (add, remove) = (a.add, a.remove);
            if add.is_empty() == remove.is_empty() {
                return Err(format!("{kind} needs add or remove (one of them)"));
            }
            let edit = if add.is_empty() { IssueListEdit::Remove(remove) } else { IssueListEdit::Add(add) };
            let p = if kind == "label" {
                IssuePatch { labels: Some(edit), ..Default::default() }
            } else {
                IssuePatch { assignees: Some(edit), ..Default::default() }
            };
            (a.issue, a.project, patch(p)?)
        }
        "close" => {
            let a: CloseArgs = args(a)?;
            if let Some(comment) = a.comment.filter(|t| !t.trim().is_empty()) {
                // Closed first: a refused close leaves no comment behind.
                let i = c.native_issue(&native(&a.issue)?, a.project.as_deref()).await?;
                let id = i["id"].as_str().unwrap_or_default().to_owned();
                let p = IssuePatch { state: Some("closed".into()), reason: a.reason, ..Default::default() };
                let mut closed = change(c, &id, patch(p)?).await?;
                let said = closed["summary"].as_str().unwrap_or_default().to_owned();
                closed["summary"] = json!(match comment_on(c, &closed["issue"], comment).await {
                    Ok(v) => {
                        closed["comment"] = v["comment"].clone();
                        format!("{said}; commented as seq {} in its thread", v["comment"]["seq"])
                    }
                    Err(e) => format!("{said}; the comment wasn't posted: {e}"),
                });
                return Ok(closed);
            }
            let p = IssuePatch { state: Some("closed".into()), reason: a.reason, ..Default::default() };
            (a.issue, a.project, patch(p)?)
        }
        "reopen" => {
            let a: ReopenArgs = args(a)?;
            (a.issue, a.project, patch(IssuePatch { state: Some("open".into()), ..Default::default() })?)
        }
        _ => return Err(format!("{WRITE_TOOL} kind {kind} isn't wired up")),
    };
    let id = match native(&issue)? {
        Native::Id(id) => id,
        at => c.native_issue(&at, project.as_deref()).await?["id"].as_str().unwrap_or_default().to_owned(),
    };
    change(c, &id, body).await
}

async fn change(c: &Call<'_>, id: &str, patch: Value) -> Out {
    let v = c.issues_call(Method::PATCH, &agent_issue(id), Some(patch)).await?;
    let kinds: Vec<&str> = v["events"].as_array().into_iter().flatten().filter_map(|e| e["kind"].as_str()).collect();
    let s = match kinds.len() {
        0 => format!("Nothing changed on {}", line(&v["issue"])),
        _ => format!("{} {}, now, as your person's agent on this machine", kinds.join(", "), line(&v["issue"])),
    };
    done(s, v)
}

/// A comment: a post in the issue's thread, by its id (M198).
async fn comment_on(c: &Call<'_>, issue: &Value, text: String) -> Out {
    let id = issue["id"].as_str().unwrap_or_default();
    let v = c.issues_call(Method::POST, &agent_messages(id), Some(json!({ "text": text }))).await?;
    let mut s = format!(
        "Commented on {} as seq {} in its thread, now, as your person's agent on this machine",
        line(issue),
        v["seq"]
    );
    if let Some(u) = v["unreached"].as_array().filter(|u| !u.is_empty()) {
        let words: Vec<&str> = u.iter().filter_map(Value::as_str).collect();
        s += &format!(" (@{} reached nobody there)", words.join(", @"));
    }
    done(s, json!({ "issue": issue, "comment": v }))
}

/// A list's query as `a=b&c=d`, leaving out what isn't given.
fn issue_query(q: &IssueQuery) -> String {
    let given = [
        ("state", q.state.clone()),
        ("labels", q.labels.clone()),
        ("assignee", q.assignee.clone()),
        ("author", q.author.clone()),
        ("q", q.q.clone()),
        ("before", q.before.map(|b| b.to_string())),
        ("limit", q.limit.map(|l| l.to_string())),
    ];
    let pairs: Vec<(&str, String)> = given.into_iter().filter_map(|(k, v)| Some((k, v?))).collect();
    query(&pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_leaves_out_what_isnt_given() {
        let q = IssueQuery {
            state: Some("all".into()),
            q: Some("dep* fails".into()),
            limit: Some(5),
            ..Default::default()
        };
        assert_eq!(issue_query(&q), "state=all&q=dep*+fails&limit=5");
    }
}
