//! PRs and issues on linked repos from agents, with no block open (M169):
//! `forge_read` (a PR, an issue, a list of them, a PR's checks, drafts,
//! and the user's inbox, M171)
//! and `forge_draft` (a comment, a review or a new issue, and discarding
//! a draft).
//!
//! They live on control's platform: each call is one typed operation
//! (`arugula_control_wire::forge_ops`), `POST /api/forge/ops` signed with
//! this machine's key, as the Cloud tools' are. The platform acts for the
//! machine's owner while it holds its live `cloud.drive` grant, and shows
//! only what GitHub lets the owner read (M165).
//!
//! An agent a guest stands behind may not use them: they read as the
//! owner, private repos too.
//!
//! **No send.** "Agents draft, people send" (aru-041): no kind here maps
//! to an operation that sends ([`op`]'s test says so), and the platform
//! refuses a machine's send anyway.

use arugula_control_wire::forge_ops::{DraftQuery, DraftRef, InboxQuery, ItemQuery, ItemRef, Op};
use arugula_control_wire::projects::NewDraft;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::tools::{Call, Kind, Out, done};
use crate::{control::CloudRefused, mux::Api};

#[derive(Deserialize, JsonSchema)]
pub struct ItemArgs {
    /// The PR or issue: OWNER/NAME#N, or its link on github.com.
    pub item: String,
}

/// Kind `issue`'s: GitHub's, or a native one (M200).
#[derive(Deserialize, JsonSchema)]
pub struct IssueItemArgs {
    /// The issue: OWNER/NAME#N, or its link on github.com; or a native
    /// issue in Arugula control, "#N" (in the current project) or "iss_…".
    pub item: String,
    /// A native issue's project for "#N": its id (prj_…) or personal.
    /// Default: the one this clone's GitHub repo is linked to, else the
    /// user's personal project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ItemsArgs {
    /// The project's id (prj_…). Default: the repo's project, else the
    /// user's personal one.
    #[serde(default)]
    pub project: Option<String>,
    /// One linked repo, OWNER/NAME.
    #[serde(default)]
    pub repo: Option<String>,
    /// pr or issue.
    #[serde(default)]
    pub item_kind: Option<String>,
    /// open, closed or merged.
    #[serde(default)]
    pub state: Option<String>,
    /// A GitHub login.
    #[serde(default)]
    pub author: Option<String>,
    /// A GitHub login among the requested reviewers.
    #[serde(default)]
    pub review_requested: Option<String>,
    /// Text in the title.
    #[serde(default)]
    pub q: Option<String>,
    /// At most this many (default 100, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DraftsArgs {
    /// One PR's or issue's (OWNER/NAME#N), or one repo's (OWNER/NAME).
    /// Neither: the user's own drafts, across their projects.
    #[serde(default)]
    pub item: Option<String>,
    /// The project's id (prj_…): all of its drafts.
    #[serde(default)]
    pub project: Option<String>,
    /// waiting (the default), sent, discarded, failed or all.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct InboxArgs {
    /// Include what the user called done.
    #[serde(default)]
    pub done: bool,
    /// At most this many (default 200, at most 500).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DraftIdArgs {
    /// The draft's id (dft_…).
    pub draft: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CommentArgs {
    /// The PR or issue: OWNER/NAME#N, or its link on github.com.
    pub item: String,
    /// The comment, in markdown.
    pub body: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReviewArgs {
    /// The PR: OWNER/NAME#N, or its link on github.com.
    pub item: String,
    pub event: Event,
    /// The review's text, in markdown (needed unless it approves).
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct IssueArgs {
    /// The linked repo, OWNER/NAME.
    pub repo: String,
    pub title: String,
    /// In markdown.
    #[serde(default)]
    pub body: Option<String>,
}

pub(crate) const READ: &[Kind] = &[
    Kind {
        name: "pr",
        description: "A pull request: its state, title, author, labels, reviewers, branches, what happened to it (reviews, pushes) and its waiting drafts. Metadata only, no body or comments.",
        schema: schema_for_type::<ItemArgs>,
        flag: None,
    },
    Kind {
        name: "issue",
        description: "An issue: its state, title, author, labels, assignees, what happened to it and its waiting drafts. A native issue (\"#N\" or \"iss_…\"): it, its timeline and its newest comments.",
        schema: schema_for_type::<IssueItemArgs>,
        flag: None,
    },
    Kind {
        name: "items",
        description: "The PRs and issues of one linked repo, or of a project's linked repos, last updated first; item_kind, state, author, review_requested and q narrow it. Repos the user can't read on GitHub are named as withheld.",
        schema: schema_for_type::<ItemsArgs>,
        flag: None,
    },
    Kind {
        name: "checks",
        description: "A pull request's checks on its head commit: each one's status and conclusion.",
        schema: schema_for_type::<ItemArgs>,
        flag: None,
    },
    Kind {
        name: "inbox",
        description: "The user's inbox: PRs and issues on their linked repos that want something of them (a review requested, their checks failed, changes requested, a mention, assigned, merged or ready, a draft waiting), newest first, each with whether they've seen it.",
        schema: schema_for_type::<InboxArgs>,
        flag: None,
    },
    Kind {
        name: "drafts",
        description: "Drafts waiting for a person to send: on a PR or issue, a repo or a project, or the user's own.",
        schema: schema_for_type::<DraftsArgs>,
        flag: None,
    },
    Kind {
        name: "draft",
        description: "One draft: what it says and what became of it (waiting, sent with its link, discarded or failed).",
        schema: schema_for_type::<DraftIdArgs>,
        flag: None,
    },
    super::issues::LIST,
];

pub(crate) const DRAFT: &[Kind] = &[
    Kind {
        name: "comment",
        description: "A comment on a PR or issue.",
        schema: schema_for_type::<CommentArgs>,
        flag: None,
    },
    Kind {
        name: "review",
        description: "A review of a PR: approve, request_changes or comment.",
        schema: schema_for_type::<ReviewArgs>,
        flag: None,
    },
    Kind {
        name: "issue",
        description: "A new issue on a linked repo.",
        schema: schema_for_type::<IssueArgs>,
        flag: None,
    },
    Kind {
        name: "discard",
        description: "Discard a waiting draft (yours, or one on a repo the user reads).",
        schema: schema_for_type::<DraftIdArgs>,
        flag: None,
    },
];

fn item(s: &str) -> Result<ItemRef, String> {
    ItemRef::parse(s).ok_or_else(|| format!("item: OWNER/NAME#N, or its link on github.com, not {s:?}"))
}

fn args<T: serde::de::DeserializeOwned>(a: Value) -> Result<T, String> {
    serde_json::from_value(a).map_err(|e| e.to_string())
}

/// The operation a kind is: a read or a draft, never a send. `by` names
/// the agent on a draft.
pub(crate) fn op(tool: &str, kind: &str, a: Value, by: &str) -> Result<Op, String> {
    let draft = |number, kind: &str, event: Option<&str>, title, body: String| NewDraft {
        repo: String::new(),
        number,
        kind: kind.into(),
        event: event.map(Into::into),
        title,
        body,
        agent: Some(by.to_owned()),
        import_key: None,
    };
    Ok(match (tool, kind) {
        ("forge_read", "pr") => Op::PrGet(item(&args::<ItemArgs>(a)?.item)?),
        ("forge_read", "issue") => Op::IssueGet(item(&args::<IssueItemArgs>(a)?.item)?),
        ("forge_read", "checks") => Op::ChecksGet(item(&args::<ItemArgs>(a)?.item)?),
        ("forge_read", "items") => {
            let a: ItemsArgs = args(a)?;
            Op::ItemList(ItemQuery {
                project: a.project,
                repo: a.repo,
                kind: a.item_kind,
                state: a.state,
                author: a.author,
                review_requested: a.review_requested,
                q: a.q,
                limit: a.limit,
            })
        }
        ("forge_read", "drafts") => {
            let a: DraftsArgs = args(a)?;
            let (repo, number) = match a.item.as_deref().map(str::trim) {
                None | Some("") => (None, None),
                Some(s) if s.contains('#') || s.contains("://") => {
                    let r = item(s)?;
                    (Some(r.repo), Some(r.number))
                }
                Some(s) => (Some(s.to_owned()), None),
            };
            Op::DraftList(DraftQuery { project: a.project, repo, number, state: a.state, ..Default::default() })
        }
        ("forge_read", "inbox") => {
            let a: InboxArgs = args(a)?;
            Op::InboxList(InboxQuery { done: a.done, limit: a.limit })
        }
        ("forge_read", "draft") => Op::DraftGet(DraftRef { draft: args::<DraftIdArgs>(a)?.draft }),
        ("forge_draft", "comment") => {
            let a: CommentArgs = args(a)?;
            let at = item(&a.item)?;
            Op::DraftCreate(NewDraft { repo: at.repo, ..draft(Some(at.number), "comment", None, None, a.body) })
        }
        ("forge_draft", "review") => {
            let a: ReviewArgs = args(a)?;
            let at = item(&a.item)?;
            let event = match a.event {
                Event::Approve => "APPROVE",
                Event::RequestChanges => "REQUEST_CHANGES",
                Event::Comment => "COMMENT",
            };
            let body = a.body.unwrap_or_default();
            Op::DraftCreate(NewDraft { repo: at.repo, ..draft(Some(at.number), "review", Some(event), None, body) })
        }
        ("forge_draft", "issue") => {
            let a: IssueArgs = args(a)?;
            let body = a.body.unwrap_or_default();
            Op::DraftCreate(NewDraft {
                repo: a.repo.trim().to_owned(),
                ..draft(None, "issue", None, Some(a.title), body)
            })
        }
        ("forge_draft", "discard") => Op::DraftDiscard(DraftRef { draft: args::<DraftIdArgs>(a)?.draft }),
        _ => return Err(format!("{tool} kind {kind} isn't wired up")),
    })
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where linked repos' PRs and issues live: ask your person to join it (Getting started on the web page, or `arugulad join`); show (kind pr or issue) opens one as a block instead".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't have forge operations yet: show (kind pr or issue) opens one as a block instead".into()
        }
        e => e.to_string(),
    }
}

/// `acme/app#7 [pr, open] Fix it` and who wrote it.
fn line(i: &Value) -> String {
    let mut s = format!(
        "{}#{} [{}, {}{}] {}",
        i["repo"].as_str().unwrap_or("?"),
        i["number"],
        i["kind"].as_str().unwrap_or("?"),
        if i["draft"] == true { "draft, " } else { "" },
        i["state"].as_str().unwrap_or("?"),
        i["title"].as_str().unwrap_or(""),
    );
    if let Some(a) = i["author"].as_str() {
        s += &format!(" by {a}");
    }
    s
}

fn draft_line(d: &Value) -> String {
    let what = match (d["kind"].as_str(), d["event"].as_str()) {
        (Some("review"), Some(e)) => format!("review ({})", e.to_ascii_lowercase()),
        (Some("issue"), _) => format!("new issue {:?}", d["title"].as_str().unwrap_or("")),
        (k, _) => k.unwrap_or("?").to_owned(),
    };
    let on = match d["number"].as_u64() {
        Some(n) => format!("{}#{n}", d["repo"].as_str().unwrap_or("?")),
        None => d["repo"].as_str().unwrap_or("?").to_owned(),
    };
    format!("{} {what} on {on}, {}", d["id"].as_str().unwrap_or("?"), d["state"].as_str().unwrap_or("?"))
}

/// What a kind's answer says in a sentence.
fn summary(o: &Op, r: &Value) -> String {
    match o {
        Op::PrGet(_) | Op::IssueGet(_) => {
            let drafts = r["drafts"].as_array().map_or(0, Vec::len);
            let events = r["events"].as_array().map_or(0, Vec::len);
            format!("{}; {events} event(s), {drafts} draft(s) waiting", line(&r["item"]))
        }
        Op::ItemList(_) => {
            let items = r["items"].as_array().cloned().unwrap_or_default();
            let mut s = match items.len() {
                0 => "No PRs or issues match.".to_owned(),
                n => format!("{n}: {}", items.iter().map(line).collect::<Vec<_>>().join("; ")),
            };
            for w in r["withheld"].as_array().into_iter().flatten() {
                s +=
                    &format!(" ({} withheld: {})", w["repo"].as_str().unwrap_or("?"), w["why"].as_str().unwrap_or("?"));
            }
            s
        }
        Op::ChecksGet(at) => {
            let checks = r["checks"].as_array().cloned().unwrap_or_default();
            let each = checks.iter().map(|c| {
                let how = c["conclusion"].as_str().or(c["status"].as_str()).unwrap_or("?");
                format!("{} {how}", c["name"].as_str().unwrap_or("?"))
            });
            match checks.len() {
                0 => format!("{at} has no checks"),
                n => format!("{at}: {n} check(s): {}", each.collect::<Vec<_>>().join(", ")),
            }
        }
        Op::DraftList(_) => {
            let drafts = r["drafts"].as_array().cloned().unwrap_or_default();
            match drafts.len() {
                0 => "No drafts.".to_owned(),
                n => format!("{n}: {}", drafts.iter().map(draft_line).collect::<Vec<_>>().join("; ")),
            }
        }
        Op::InboxList(_) => {
            let entries = r["entries"].as_array().cloned().unwrap_or_default();
            let each = entries.iter().map(|e| {
                let seen = if e["seen_at"].is_u64() { "" } else { " (new)" };
                format!(
                    "{}#{} {}{seen}: {}",
                    e["repo"].as_str().unwrap_or("?"),
                    e["number"],
                    e["reason"].as_str().unwrap_or("?").replace('_', " "),
                    e["title"].as_str().unwrap_or("")
                )
            });
            match entries.len() {
                0 => "Nothing in the inbox.".to_owned(),
                n => format!("{n}: {}", each.collect::<Vec<_>>().join("; ")),
            }
        }
        Op::DraftGet(_) => draft_line(r),
        Op::DraftCreate(_) => format!(
            "Drafted {}: it waits for a person to send or discard it (forge_read kind draft shows what became of it)",
            draft_line(&r["draft"])
        ),
        Op::DraftDiscard(d) => format!("Discarded {}", d.draft),
        _ => String::new(),
    }
}

/// `forge_read` and `forge_draft`. They read and draft as this
/// machine's owner, private repos too: an agent a guest stands behind
/// may not.
pub(crate) async fn forge_ops(c: &Call<'_>, tool: &str, kind: &str, a: Value) -> Out {
    if let Some(me) = c.me()
        && c.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
    {
        return Err(format!("{tool} reads and drafts as this machine's owner: only their own agents may"));
    }
    // M200: native issues are read here too, and never drafted on.
    let native = a["item"].as_str().filter(|i| crate::mcp::issues::Native::parse(i).is_some()).map(str::to_owned);
    match (tool, kind, native) {
        ("forge_read", "native_issues", _) => return super::issues::list_native(c, a).await,
        ("forge_read", "issue", Some(_)) => {
            let a: IssueItemArgs = args(a)?;
            let (summary, v) = c.read_native(&a.item, a.project.as_deref()).await?;
            return done(summary, v);
        }
        ("forge_draft", _, Some(item)) => return Err(crate::mcp::issues::not_drafted(&item)),
        _ => {}
    }
    let o = op(tool, kind, a, &c.by())?;
    let r = c.forge_op(o.clone(), said).await?;
    let mut out = match r {
        Value::Object(_) => r,
        other => json!({ "result": other }),
    };
    let s = summary(&o, &out);
    out["op"] = json!(o.name());
    done(s, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind of both tools is a read or a draft: MCP never sends.
    #[test]
    fn no_kind_sends() {
        let a = json!({ "item": "acme/app#1", "body": "b", "event": "approve", "repo": "acme/app",
                        "title": "t", "draft": "dft_1" });
        for (tool, kinds) in [("forge_read", READ), ("forge_draft", DRAFT)] {
            for k in kinds.iter().filter(|k| k.name != super::super::issues::LIST.name) {
                let o = op(tool, k.name, a.clone(), "mcp:x").unwrap_or_else(|e| panic!("{tool} {}: {e}", k.name));
                assert!(!o.sends(), "{tool} {} is {}", k.name, o.name());
            }
        }
        assert_eq!(DRAFT.iter().map(|k| k.name).collect::<Vec<_>>(), ["comment", "review", "issue", "discard"]);
    }

    #[test]
    fn kinds_to_operations() {
        let r = op(
            "forge_draft",
            "review",
            json!({ "item": "https://github.com/acme/app/pull/4", "event": "request_changes", "body": "split" }),
            "mcp:claude-code",
        )
        .unwrap();
        let Op::DraftCreate(d) = r else { panic!() };
        assert_eq!(
            (d.repo.as_str(), d.number, d.kind.as_str(), d.event.as_deref(), d.agent.as_deref()),
            ("acme/app", Some(4), "review", Some("REQUEST_CHANGES"), Some("mcp:claude-code"))
        );
        assert_eq!(
            op("forge_read", "inbox", json!({ "done": true }), "x").unwrap(),
            Op::InboxList(InboxQuery { done: true, limit: None })
        );
        let l = op("forge_read", "drafts", json!({ "item": "acme/app" }), "x").unwrap();
        assert_eq!(l, Op::DraftList(DraftQuery { repo: Some("acme/app".into()), ..Default::default() }));
        let l = op("forge_read", "drafts", json!({ "item": "acme/app#2" }), "x").unwrap();
        assert_eq!(
            l,
            Op::DraftList(DraftQuery { repo: Some("acme/app".into()), number: Some(2), ..Default::default() })
        );
        assert!(op("forge_read", "pr", json!({ "item": "%7" }), "x").unwrap_err().contains("OWNER/NAME#N"));
    }
}
