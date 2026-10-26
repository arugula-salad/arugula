//! Native issues from agents (M200): a project's own issues in Arugula
//! control (M194), read with `read_forge` (`issue`) and `forge_read`
//! (kinds `issue` and `native_issues`), written with `issue_write`.
//!
//! **Which issue:** `#12` is a number in the current project, `iss_…` an
//! issue anywhere the owner reads. **The current project** is the one the
//! call names (`project`: an id, or `personal`), else the one the caller's
//! clone's GitHub repo is linked to, else the owner's personal project. A
//! clone of a GitHub repo that no project of the owner's links is
//! refused, not guessed (as `arugula issue`'s `#N` there is GitHub's).
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
    issues::{
        AGENT_ISSUE_HOME, IssueListEdit, IssuePatch, IssueQuery, NewIssue, agent_issue, agent_issue_events,
        agent_project_issue, agent_project_issues,
    },
};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::tools::{Call, Kind, Out, done};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const WRITE_TOOL: &str = "issue_write";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to Arugula's own (native) issues in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. An issue is #N (in the current project: project, else the one this clone's GitHub repo is linked to, else your person's personal project) or iss_…. Their grants say what you may do (issues.write to file and change, threads.post to comment; changing reads it first, with threads.read), and never more than their role. GitHub's PRs and issues aren't written here: draft and forge_draft make drafts that wait for a person to send them.";

/// A native issue: `#N` in the current project, or `iss_…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Native {
    Id(String),
    Number(u64),
}

impl Native {
    /// `None`: not a native issue's (GitHub's, a block's).
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.strip_prefix("iss_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Some(Native::Id(s.to_owned()));
        }
        s.strip_prefix('#').and_then(|n| n.parse().ok()).map(Native::Number)
    }
}

fn native(s: &str) -> Result<Native, String> {
    Native::parse(s).ok_or_else(|| format!("issue: #N (in the current project) or iss_…, not {s:?}"))
}

/// What `draft` and `forge_draft` say to a native issue.
pub(crate) fn not_drafted(s: &str) -> String {
    format!(
        "{} is a native issue in Arugula control: those aren't drafted, issue_write writes to them at once (kind comment, edit, label, assign, close or reopen). Drafts are for GitHub, where a person sends them.",
        s.trim()
    )
}

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

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where native issues live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't let agents use native issues yet".into()
        }
        e => e.to_string(),
    }
}

/// `#12 [open] Title`, its labels and who opened it.
fn line(i: &Value) -> String {
    let mut s = format!(
        "#{} [{}{}] {} ({})",
        i["number"],
        i["state"].as_str().unwrap_or("?"),
        i["close_reason"].as_str().map(|r| format!(", {}", r.replace('_', " "))).unwrap_or_default(),
        i["title"].as_str().unwrap_or(""),
        i["id"].as_str().unwrap_or("?"),
    );
    let labels: Vec<&str> = i["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !labels.is_empty() {
        s += &format!(" labels {}", labels.join(", "));
    }
    s
}

/// Who: an account, or "<account>'s agent on <machine>".
fn who(account: &Value, agent: &Value) -> String {
    let a = account.as_str().filter(|a| !a.is_empty()).unwrap_or("a deleted account");
    match agent["machine_name"].as_str().filter(|n| !n.is_empty()).or(agent["machine"].as_str()) {
        Some(m) => format!("{a}'s agent on {m}"),
        None => a.to_owned(),
    }
}

/// An issue, its timeline and its newest comments as text.
fn text(issue: &Value, events: &[Value], comments: &[Value]) -> String {
    let mut out = format!("{}\nOpened by {}", line(issue), who(&issue["author"], &issue["agent"]));
    let assignees: Vec<&str> = issue["assignees"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !assignees.is_empty() {
        out += &format!(", assigned to {}", assignees.join(", "));
    }
    if let Some(b) = issue["body"].as_str().filter(|b| !b.is_empty()) {
        out += &format!("\n\n{b}");
    }
    if !events.is_empty() {
        out += "\n\nTimeline:";
        for e in events {
            let d = &e["detail"];
            let what = d["label"].as_str().or(d["assignee"].as_str()).or(d["reason"].as_str());
            out += &format!(
                "\n- {} {}{}",
                who(&e["actor"], &e["agent"]),
                e["kind"].as_str().unwrap_or("?"),
                what.map(|w| format!(" {w}")).unwrap_or_default()
            );
        }
    }
    if !comments.is_empty() {
        out += "\n\nComments:";
        for m in comments {
            let by = match m["agent"]["machine_name"].as_str() {
                Some(machine) => format!("{}'s agent on {machine}", m["name"].as_str().unwrap_or("?")),
                None => m["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("someone").to_owned(),
            };
            // Posted for them by a product (M237: a hud box's review).
            let by = match m["via"].as_str() {
                Some(v) => format!("{by} via {v}"),
                None => by,
            };
            out += &format!("\n[{}] {by}: {}", m["seq"], m["text"].as_str().unwrap_or(""));
        }
    }
    out
}

fn query(pairs: &[(&str, String)]) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        q.append_pair(k, v);
    }
    q.finish()
}

impl Call<'_> {
    /// They act as this machine's owner: only the owner's own agents may.
    async fn owners_own(&self, what: &str) -> Result<(), String> {
        if let Some(me) = self.me()
            && self.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err(format!("{what} acts as this machine's owner: only their own agents may"));
        }
        Ok(())
    }

    async fn issues_call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.app().control.cloud(method, path, body.as_ref()).await.map_err(said)
    }

    /// The GitHub repo (`owner/name`) of the caller's own clone, if its
    /// pane is on this host in a clone of one.
    async fn my_github_repo(&self) -> Option<String> {
        let pane = self.own_pane()?;
        let info = self.readable(pane).await.ok()?.info;
        if info.host.is_some() {
            return None;
        }
        github_repo_of(&info.cwd?).await
    }

    /// M203: the project whose own issue `#N` in `dir` is, when the clone's
    /// GitHub repo is linked to one whose issue home is native (M199's and
    /// this module's rule). `None`: the clone's forge's, as before.
    pub(crate) async fn native_home_of(&self, dir: &str) -> Option<String> {
        let repo = github_repo_of(dir).await?;
        let path = format!("{AGENT_ISSUE_HOME}?{}", query(&[("repo", repo)]));
        let v = self.issues_call(Method::GET, &path, None).await.ok()?;
        (v["home"] == "native").then(|| v["project"].as_str().map(str::to_owned)).flatten()
    }

    /// The current project's id (module docs).
    pub(super) async fn current_project(&self, project: Option<&str>) -> Result<String, String> {
        let mut q = vec![];
        match project.map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => q.push(("project", p.to_owned())),
            None => {
                if let Some(repo) = self.my_github_repo().await {
                    q.push(("repo", repo));
                }
            }
        }
        let path = if q.is_empty() { AGENT_ISSUE_HOME.to_owned() } else { format!("{AGENT_ISSUE_HOME}?{}", query(&q)) };
        let v = self.issues_call(Method::GET, &path, None).await?;
        v["project"].as_str().map(str::to_owned).ok_or_else(|| "Arugula control named no project".into())
    }

    /// A native issue, as the platform answers it.
    async fn native_issue(&self, at: &Native, project: Option<&str>) -> Result<Value, String> {
        let path = match at {
            Native::Id(id) => agent_issue(id),
            Native::Number(n) => agent_project_issue(&self.current_project(project).await?, *n),
        };
        self.issues_call(Method::GET, &path, None).await
    }

    /// `read_forge`'s `issue` and `forge_read`'s kind `issue` on a native
    /// one: it, its timeline and its newest comments (with threads.read;
    /// left out, saying why, if they can't be read).
    pub(crate) async fn read_native(&self, issue: &str, project: Option<&str>) -> Result<(String, Value), String> {
        self.owners_own("reading a native issue").await?;
        let i = self.native_issue(&native(issue)?, project).await?;
        let id = i["id"].as_str().unwrap_or_default().to_owned();
        let events = self.issues_call(Method::GET, &agent_issue_events(&id), None).await?;
        let events = events["events"].as_array().cloned().unwrap_or_default();
        let (comments, more, note) = match self.issues_call(Method::GET, &agent_messages(&id), None).await {
            Ok(v) => (v["messages"].as_array().cloned().unwrap_or_default(), v["more"].clone(), None),
            Err(e) => (vec![], json!(false), Some(e)),
        };
        let mut t = text(&i, &events, &comments);
        if let Some(n) = &note {
            t += &format!("\n\n(Its comments weren't read: {n})");
        }
        let summary = format!(
            "Native issue {} in project {}: {} event(s), {} comment(s){}; read_thread with room {id} pages its comments, issue_write writes to it at once",
            line(&i),
            i["project"].as_str().unwrap_or("?"),
            events.len(),
            comments.len(),
            if more == true { " (the newest)" } else { "" },
        );
        let v = json!({ "issue": i, "events": events, "comments": comments, "more_comments": more,
                        "comments_note": note, "text": t });
        Ok((summary, v))
    }

    /// `forge_read` kind `native_issues`.
    pub(crate) async fn list_native(&self, a: Value) -> Out {
        self.owners_own("forge_read kind native_issues").await?;
        let a: NativeListArgs = args(a)?;
        let project = self.current_project(a.project.as_deref()).await?;
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
        let v = self.issues_call(Method::GET, &format!("{}?{qs}", agent_project_issues(&project)), None).await?;
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
    pub(crate) async fn issue_write(&self, kind: &str, a: Value) -> Out {
        self.owners_own(WRITE_TOOL).await?;
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
                let project = self.current_project(a.project.as_deref()).await?;
                let new = NewIssue {
                    title: a.title,
                    body: a.body.unwrap_or_default(),
                    labels: a.labels,
                    assignees: a.assignees,
                };
                let body = serde_json::to_value(new).map_err(|e| e.to_string())?;
                let i = self.issues_call(Method::POST, &agent_project_issues(&project), Some(body)).await?;
                let s = format!("Filed {} in project {project}, now, as your person's agent on this machine", line(&i));
                return done(s, json!({ "issue": i }));
            }
            "comment" => {
                let a: CommentArgs = args(a)?;
                let i = self.native_issue(&native(&a.issue)?, a.project.as_deref()).await?;
                return self.comment_on(&i, a.text).await;
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
                if let Some(c) = a.comment.filter(|c| !c.trim().is_empty()) {
                    // Closed first: a refused close leaves no comment behind.
                    let i = self.native_issue(&native(&a.issue)?, a.project.as_deref()).await?;
                    let id = i["id"].as_str().unwrap_or_default().to_owned();
                    let p = IssuePatch { state: Some("closed".into()), reason: a.reason, ..Default::default() };
                    let mut closed = self.change(&id, patch(p)?).await?;
                    let said = closed["summary"].as_str().unwrap_or_default().to_owned();
                    closed["summary"] = json!(match self.comment_on(&closed["issue"], c).await {
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
            at => self.native_issue(&at, project.as_deref()).await?["id"].as_str().unwrap_or_default().to_owned(),
        };
        self.change(&id, body).await
    }

    async fn change(&self, id: &str, patch: Value) -> Out {
        let v = self.issues_call(Method::PATCH, &agent_issue(id), Some(patch)).await?;
        let kinds: Vec<&str> =
            v["events"].as_array().into_iter().flatten().filter_map(|e| e["kind"].as_str()).collect();
        let s = match kinds.len() {
            0 => format!("Nothing changed on {}", line(&v["issue"])),
            _ => format!("{} {}, now, as your person's agent on this machine", kinds.join(", "), line(&v["issue"])),
        };
        done(s, v)
    }

    /// A comment: a post in the issue's thread, by its id (M198).
    async fn comment_on(&self, issue: &Value, text: String) -> Out {
        let id = issue["id"].as_str().unwrap_or_default();
        let v = self.issues_call(Method::POST, &agent_messages(id), Some(json!({ "text": text }))).await?;
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

/// The GitHub repo (`owner/name`) of the clone at `dir`: its `origin`'s,
/// else the first remote's that's on github.com.
async fn github_repo_of(dir: &str) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["remote", "-v"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .await
        .ok()?;
    let mut remotes: Vec<(String, String)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            Some((p.next()?.to_owned(), p.next()?.to_owned()))
        })
        .collect();
    remotes.sort_by_key(|(name, _)| name != "origin");
    remotes
        .iter()
        .filter_map(|(_, url)| crate::forge::login::parse_remote(url))
        .find(|(host, _)| host == "github.com")
        .map(|(_, repo)| repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_issues_are_named_by_number_or_id() {
        let id = format!("iss_{}", "0a".repeat(16));
        assert_eq!(Native::parse(&id), Some(Native::Id(id.clone())));
        assert_eq!(Native::parse(" #12 "), Some(Native::Number(12)));
        for not in ["12", "%12", "acme/app#12", "https://github.com/acme/app/issues/12", "iss_12", "#x"] {
            assert_eq!(Native::parse(not), None, "{not}");
        }
        assert!(native("acme/app#1").unwrap_err().contains("#N"));
    }

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
