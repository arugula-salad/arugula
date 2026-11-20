//! A hosted session's MCP (aru-115, amended): the tools a session's agent
//! has for its project's native issues and Docs (aru-118), on a listener of
//! their own.
//!
//! A session's agent runs as one of the box's session users, behind the
//! firewall that keeps it off the daemon's ports (`work::firewall`), and
//! gets none of `/mcp`'s tools: those start and drive panes as the daemon.
//! What it gets instead is this: a loopback listener that serves only
//! [`READ`], [`COMMENT`], [`CREATE`], [`CREATE_DOC`], [`FILL_DOC`] and
//! [`READ_DOC`], each a call to control's `POST
//! /api/hosted/work/session/issues` for the agent's session, and [`WATCH`]
//! (Arugula #1035), its `POST /api/hosted/work/session/watch`.
//!
//! - **Who is calling is the socket's owner, not a secret.** The listener
//!   finds the uid that owns the client end of the connection
//!   (`/proc/net/tcp`), and the session that user runs in the ledger
//!   (`crate::work::sessions`). A token would sit on the command line of
//!   the agent's `sudo env -i …`, which the box's other users can read, and
//!   they are other sessions, asked by other people. A caller that is no
//!   session's user (the run user, the daemon's) is refused.
//! - **The session is the authority, not this listener.** Control checks
//!   the session is this box's and live, takes the project from it, and
//!   acts as its asker: here the session id is all that is sent, with an
//!   issue's number or a Doc's id in the project, never a project.
//! - **Not `/mcp`.** The main server keeps refusing a session's agent (its
//!   port is fenced, and its block token isn't given out); this one has no
//!   other tools to add to by accident.
//!
//! It starts only on a box that takes sessions, and its address is
//! [`url`].

use std::{
    net::{SocketAddr, SocketAddrV4},
    sync::Arc,
};

use arugula_control_wire::{
    project_docs::{DocView, DocWrite, NewDoc, NewSection},
    watch::{EventTarget, WatchState, WatchTarget, Watcher, event_target, event_types, expires_ms, until_word_parse},
    work::{
        SessionIssuesAsk, SessionIssuesGiven, SessionIssuesOp, SessionWatchAsk, SessionWatchGiven, SessionWatchOp,
        WORK_SESSION_ISSUES, WORK_SESSION_WATCH,
    },
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::common::schema_for_type,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::mcp::{
    session::{COMMENT, CREATE, CREATE_DOC, FILL_DOC, READ, READ_DOC, WATCH},
    tools::{Out, done, parse},
};
use crate::server::App;

/// Said in place of an issue's comments when chat is off for the asker.
const COMMENTS_IN_CHAT: &str =
    "its comments are in chat, which is off for the person who asked you, so they aren't shown";

const INSTRUCTIONS: &str = "You are an agent session of an Arugula project. read_session_issues lists the project's own (native) issues, or reads one with its timeline and newest comments; comment_session_issue comments on one (mention a person by @login when you need them to decide or answer: a comment that names nobody reaches no one's inbox); create_session_issue opens one and returns its link. create_session_doc makes a Doc in the project (a title and its sections, each a heading and its markdown, or an intent for later) and returns its link; fill_session_doc writes a section of a Doc you made, or adds one; read_project_doc lists the project's Docs or reads one. When someone asks for a native Doc, make one with create_session_doc and post its link. They act with the rights of the person who asked you, in this project only, and a comment, issue or Doc you make is this session's own; an issue is its number, #N, and a Doc its id, pdc_ and 32 hex digits. watch wakes this session later, when a PR merges or its checks get somewhere, another session settles, a thread gets a reply, or a run or deploy ends: create one, end your turn, and Arugula sends you what happened as your next prompt, once.";

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// An issue to read, as #N or N: it, its timeline and its newest
    /// comments. Without it: a list of the project's issues, newest first.
    #[serde(default)]
    pub issue: Option<String>,
    /// For a list: open (the default), closed or all.
    #[serde(default)]
    pub state: Option<String>,
    /// For a list: issues with each of these labels.
    #[serde(default)]
    pub labels: Vec<String>,
    /// For a list: words in the title or body (dep*: a word starting with dep).
    #[serde(default)]
    pub q: Option<String>,
    /// For a list: at most this many (default 50, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
    /// For a list: issues numbered below this, the next page.
    #[serde(default)]
    pub before: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CommentArgs {
    /// The issue, as #N or N.
    pub issue: String,
    /// The comment, in markdown.
    pub text: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateArgs {
    /// The issue's title: one line, at most 256 characters.
    pub title: String,
    /// Its body, in markdown (at most 64 KB).
    #[serde(default)]
    pub body: String,
    /// Labels, each one the project's issues already have (a label that isn't is refused, with the ones there are).
    #[serde(default)]
    pub labels: Vec<String>,
}

/// One section of a new Doc.
#[derive(Deserialize, JsonSchema)]
pub struct DocSection {
    /// The section's heading.
    pub heading: String,
    /// One line on what it will hold, shown while it's pending.
    #[serde(default)]
    pub intent: String,
    /// Its body now, in Markdown (at most 64 KB): headings, lists, tables, ```chart and ```svg fences, as a Doc's. Leave it out to fill it later with fill_session_doc.
    #[serde(default)]
    pub markdown: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateDocArgs {
    /// The Doc's title: one line.
    pub title: String,
    /// Every section, in order (at most 40; the Doc at most 1 MiB).
    #[serde(default)]
    pub sections: Vec<DocSection>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FillDocArgs {
    /// The Doc's id (pdc_...), one this session made.
    pub doc: String,
    /// The section's id (s1, s2, ...): its body is replaced. Leave it out, and give heading, to add a section at the end.
    #[serde(default)]
    pub section: Option<String>,
    /// A new section's heading, or a new heading for the section.
    #[serde(default)]
    pub heading: Option<String>,
    /// The section's body, in Markdown (at most 64 KB).
    #[serde(default)]
    pub markdown: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadDocArgs {
    /// The Doc's id (pdc_...). Without it: a list of the project's Docs.
    #[serde(default)]
    pub doc: Option<String>,
}

/// One id, or several.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Ids {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, JsonSchema)]
pub struct WatchArgs {
    /// create (the default), list, cancel, or types (the event types event
    /// takes).
    #[serde(default)]
    pub kind: Option<String>,
    /// create: a pull request, OWNER/REPO#N or its link, or several.
    #[serde(default)]
    pub pr: Option<Ids>,
    /// create, with pr: merged (the default), closed, checks_done,
    /// checks_failed or ready.
    #[serde(default)]
    pub until: Option<String>,
    /// create: another agent session (ses_…), or several: until it settles.
    #[serde(default)]
    pub agent_session: Option<Ids>,
    /// create: a run (run_…) or a deploy (dep_…), or several: until it ends.
    #[serde(default)]
    pub run: Option<Ids>,
    /// create: a chat room's id: until a reply in it that you didn't write.
    #[serde(default)]
    pub room: Option<String>,
    /// create, with room: only under this thread (its root message's number).
    #[serde(default)]
    pub parent: Option<i64>,
    /// create, with room: only a reply by this person (login or name).
    #[serde(default)]
    pub from: Option<String>,
    /// create: an event type (kind types lists them): fires when an event
    /// of it happens that has every match field given.
    #[serde(default)]
    pub event: Option<String>,
    /// create, with event: field and value pairs the event must have, from
    /// the type's match fields ({"issue": "#12"}, this project's #12).
    #[serde(default, rename = "match")]
    pub matches: Option<serde_json::Map<String, Value>>,
    /// create, with event: why you watch, at most 200 characters: it comes
    /// back with the event when it fires.
    #[serde(default)]
    pub note: Option<String>,
    /// create: how long before it ends without firing: 30m, 6h, 3d. Default
    /// 24h, at most 7d.
    #[serde(default)]
    pub expires: Option<String>,
    /// cancel: the watch's id (wch_…).
    #[serde(default)]
    pub id: Option<String>,
    /// cancel: all of this session's watches.
    #[serde(default)]
    pub all: bool,
}

/// What `watch create` asks for, from its arguments: its targets and its
/// event target, checked as the machine's `watch` checks them.
fn watch_create(a: WatchArgs) -> Result<SessionWatchOp, String> {
    let targets = watch_targets(&a)?;
    // The session's project is control's to say, here as everywhere.
    if a.matches.as_ref().is_some_and(|m| m.contains_key("project")) {
        return Err("match project: a session watches its own project's events, and #N is its issue".into());
    }
    let EventTarget { event, matches, note } = event_target(Watcher::HostedSession, a.event, a.matches, a.note)?;
    if targets.is_empty() && event.is_none() {
        return Err("watch create needs something to watch: pr, agent_session, run, room or event".into());
    }
    let expires_in_ms = a.expires.as_deref().map(expires_ms).transpose()?;
    Ok(SessionWatchOp::Create { targets, expires_in_ms, event, matches, note })
}

/// What `watch create` waits for besides an event, from its arguments.
fn watch_targets(a: &WatchArgs) -> Result<Vec<WatchTarget>, String> {
    let until = until_word_parse(a.until.as_deref())?;
    let ids = |i: &Option<Ids>| match i {
        None => vec![],
        Some(Ids::One(s)) => vec![s.trim().to_owned()],
        Some(Ids::Many(v)) => v.iter().map(|s| s.trim().to_owned()).collect(),
    };
    let mut out = vec![];
    for p in ids(&a.pr) {
        let r = arugulad_base::forge_wait::PrRef::parse(&p)
            .filter(|r| r.host == "github.com")
            .ok_or_else(|| format!("pr {p}: OWNER/REPO#N, or its link on github.com"))?;
        out.push(WatchTarget::Pr { repo: r.repo, number: r.number, until });
    }
    for id in ids(&a.agent_session) {
        if !id.starts_with("ses_") {
            return Err(format!("agent_session {id}: an agent session's id, ses_…"));
        }
        out.push(WatchTarget::Session { id });
    }
    for id in ids(&a.run) {
        if !id.starts_with("run_") && !id.starts_with("dep_") {
            return Err(format!("run {id}: a run's id (run_…) or a deploy's (dep_…)"));
        }
        out.push(WatchTarget::Run { id });
    }
    match a.room.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        Some(room) => out.push(WatchTarget::Thread { room: room.to_owned(), parent: a.parent, from: a.from.clone() }),
        None if a.parent.is_some() || a.from.is_some() => return Err("parent and from go with a room".into()),
        None => {}
    }
    Ok(out)
}

fn tools() -> Vec<Tool> {
    let read = ToolAnnotations::with_title("Read the project's issues").read_only(true).open_world(false);
    let comment =
        ToolAnnotations::with_title("Comment on an issue").read_only(false).destructive(false).open_world(false);
    let create = ToolAnnotations::with_title("Open an issue").read_only(false).destructive(false).open_world(false);
    let create_doc = ToolAnnotations::with_title("Make a Doc").read_only(false).destructive(false).open_world(false);
    let fill_doc =
        ToolAnnotations::with_title("Write a section of a Doc").read_only(false).destructive(false).open_world(false);
    let read_doc = ToolAnnotations::with_title("Read the project's Docs").read_only(true).open_world(false);
    let watch = ToolAnnotations::with_title("Watch for something to happen")
        .read_only(false)
        .destructive(false)
        .open_world(true);
    vec![
        Tool::new(
            READ,
            "List this project's own (native) issues in Arugula, newest first, or read one (issue: #N) with its timeline and newest comments. Only this project's.",
            schema_for_type::<ReadArgs>(),
        )
        .with_title("Read the project's issues")
        .with_annotations(read),
        Tool::new(
            COMMENT,
            "Comment on one of this project's issues (issue: #N). It lands at once as this agent session's own comment, with the rights of the person who asked you. You can't edit, label or close issues.",
            schema_for_type::<CommentArgs>(),
        )
        .with_title("Comment on an issue")
        .with_annotations(comment),
        Tool::new(
            CREATE,
            "Open a new issue in this project (title, a markdown body, labels the project already uses). It is this agent session's own issue, made with the rights of the person who asked you; nobody is assigned or notified. Returns its number and link, to post.",
            schema_for_type::<CreateArgs>(),
        )
        .with_title("Open an issue")
        .with_annotations(create),
        Tool::new(
            CREATE_DOC,
            "Make a Doc in this project, a report, plan or write-up that its members read on Arugula's page: a title and every section in order, each a heading with its markdown, or an intent to fill later. It is this agent session's own Doc, made with the rights of the person who asked you. Returns its id, its sections' ids and its link, to post.",
            schema_for_type::<CreateDocArgs>(),
        )
        .with_title("Make a Doc")
        .with_annotations(create_doc),
        Tool::new(
            FILL_DOC,
            "Write one section of a Doc this session made (doc: pdc_..., section: s1, s2, ..., markdown): filling a section again replaces it. With no section and a heading, add a section at the end. You can't write other people's Docs, or delete one.",
            schema_for_type::<FillDocArgs>(),
        )
        .with_title("Write a section of a Doc")
        .with_annotations(fill_doc),
        Tool::new(
            READ_DOC,
            "List this project's Docs, the latest changed first, or read one (doc: pdc_...) as markdown. Only this project's.",
            schema_for_type::<ReadDocArgs>(),
        )
        .with_title("Read the project's Docs")
        .with_annotations(read_doc),
        Tool::new(
            WATCH,
            "Wake this session later, when something happens, without polling or holding your turn open: create a watch (kind create), then end your turn; what happened (ids and states only) comes as your next prompt, from Arugula, once. Targets, any of which fires it: pr (OWNER/REPO#N, in a repo linked to Arugula's GitHub App) until merged, closed, checks_done, checks_failed or ready; agent_session (another ses_) until it settles; run (run_ or dep_) until it ends; room (a chat room's id, with parent for one thread and from for one person) until a reply you didn't write; event (a type from kind types, like issue.closed) with match (some of the type's match fields, like {\"issue\": \"#12\"}, this project's #12) and note (why you watch, at most 200 characters, handed back when it fires). It expires after 24 hours unless you say (expires, at most 7d), and then you're told it ended without firing. If it has happened already, create says so and makes none. kind list shows this session's watches; kind cancel takes one by id, or all; kind types lists the event types and their match fields.",
            schema_for_type::<WatchArgs>(),
        )
        .with_title("Watch for something to happen")
        .with_annotations(watch),
    ]
}

/// `#12` or `12`.
fn number(s: &str) -> Result<u64, String> {
    s.trim().trim_start_matches('#').parse().map_err(|_| format!("an issue is #N, not {s:?}"))
}

#[derive(Clone)]
struct Server {
    app: Arc<App>,
}

/// The session whose agent is calling, found by its socket.
#[derive(Clone)]
struct Caller(String);

impl Server {
    /// `watch`: make, list or cancel this session's watches, or list the
    /// event types.
    async fn watch(&self, session: &str, args: Value) -> Out {
        let a: WatchArgs = parse(args)?;
        let kind = a.kind.clone().unwrap_or_default();
        let op = match kind.trim() {
            "" | "create" => watch_create(a)?,
            "list" => SessionWatchOp::List,
            "cancel" => {
                let id = a.id.as_deref().map(str::trim).filter(|i| !i.is_empty()).map(str::to_owned);
                if id.is_none() && !a.all {
                    return Err("cancel needs an id (wch_…), or all".into());
                }
                SessionWatchOp::Cancel { id, all: a.all }
            }
            // A hosted session's box has joined: every cloud type is
            // watchable, and no machine's own.
            "types" => {
                let text = event_types(Watcher::HostedSession, true);
                return done(text.clone(), json!({ "types": text }));
            }
            k => return Err(format!("watch kind {k}: create, list, cancel or types")),
        };
        let ask = SessionWatchAsk { session: session.to_owned(), op };
        let got: SessionWatchGiven = self
            .app
            .control
            .post_signed(WORK_SESSION_WATCH, &ask)
            .await
            .map_err(|e| format!("asking Arugula cloud: {e:#}"))?;
        match got {
            SessionWatchGiven::Refused { why } => Err(why),
            SessionWatchGiven::Made { made } => {
                let text = match (made.state, &made.id) {
                    (WatchState::AlreadyMet, _) => {
                        format!("It has already happened: {}. No watch was made.", made.met.as_deref().unwrap_or("met"))
                    }
                    (_, Some(id)) => format!(
                        "Watch {id} is live. End your turn now: what happens comes as your next prompt, or word that it ended without firing."
                    ),
                    (_, None) => "Arugula cloud made no watch, and said nothing of why".into(),
                };
                done(text, made)
            }
            SessionWatchGiven::List { list } => {
                let lines: Vec<String> =
                    list.watches.iter().map(|w| format!("{}: {}", w.id, w.targets.join(" or "))).collect();
                let text = match lines.len() {
                    0 => "No live watches".to_owned(),
                    n => format!("{n} live watch{}:\n{}", if n == 1 { "" } else { "es" }, lines.join("\n")),
                };
                done(text, list)
            }
            SessionWatchGiven::Cancelled { cancelled } => {
                let text = match cancelled.cancelled.as_slice() {
                    [] => "No live watch of this session's was cancelled".to_owned(),
                    ids => format!("Cancelled {}", ids.join(", ")),
                };
                done(text, cancelled)
            }
        }
    }

    async fn ask(&self, session: &str, op: SessionIssuesOp) -> Result<SessionIssuesGiven, String> {
        let ask = SessionIssuesAsk { session: session.to_owned(), op };
        let got: SessionIssuesGiven = self
            .app
            .control
            .post_signed(WORK_SESSION_ISSUES, &ask)
            .await
            .map_err(|e| format!("asking Arugula cloud: {e:#}"))?;
        match got {
            SessionIssuesGiven::Refused { why } => Err(why),
            g => Ok(g),
        }
    }

    async fn read(&self, session: &str, args: Value) -> Out {
        let a: ReadArgs = parse(args)?;
        let Some(issue) = a.issue.as_deref().filter(|i| !i.trim().is_empty()) else {
            let query = arugula_control_wire::issues::IssueQuery {
                state: a.state,
                labels: (!a.labels.is_empty()).then(|| a.labels.join(",")),
                q: a.q,
                before: a.before,
                limit: a.limit,
                ..Default::default()
            };
            let SessionIssuesGiven::List { list } = self.ask(session, SessionIssuesOp::List { query }).await? else {
                return Err("control answered something else".into());
            };
            let issues = serde_json::to_value(&list.issues).map_err(|e| e.to_string())?;
            let text = list.issues.iter().map(|i| crate::mcp::issues::line(&json!(i))).collect::<Vec<_>>().join("\n");
            let summary = format!(
                "{} issue(s) in project {}{}",
                list.issues.len(),
                list.project,
                if list.more { " (more: before the last number)" } else { "" }
            );
            return done(summary, json!({ "issues": issues, "more": list.more, "text": text }));
        };
        let number = number(issue)?;
        let SessionIssuesGiven::Shown { issue, events, comments, more_comments, comments_in_chat } =
            self.ask(session, SessionIssuesOp::Show { number }).await?
        else {
            return Err("control answered something else".into());
        };
        let (issue, events, comments) = (json!(issue), json!(events), json!(comments));
        let events_v = events.as_array().cloned().unwrap_or_default();
        let comments_v = comments.as_array().cloned().unwrap_or_default();
        let mut text = crate::mcp::issues::text(&issue, &events_v, &comments_v);
        let summary = if comments_in_chat {
            text.push_str(&format!("\n{COMMENTS_IN_CHAT}\n"));
            format!(
                "Native issue {}: {} event(s); {COMMENTS_IN_CHAT}",
                crate::mcp::issues::line(&issue),
                events_v.len()
            )
        } else {
            format!(
                "Native issue {}: {} event(s), {} comment(s){}",
                crate::mcp::issues::line(&issue),
                events_v.len(),
                comments_v.len(),
                if more_comments { " (the newest)" } else { "" }
            )
        };
        done(
            summary,
            json!({ "issue": issue, "events": events, "comments": comments, "more_comments": more_comments,
                    "comments_in_chat": comments_in_chat, "text": text }),
        )
    }

    async fn comment(&self, session: &str, args: Value) -> Out {
        let a: CommentArgs = parse(args)?;
        let number = number(&a.issue)?;
        match self.ask(session, SessionIssuesOp::Comment { number, text: a.text }).await? {
            SessionIssuesGiven::Commented { seq } => {
                done(format!("Commented on #{number} (message {seq})"), json!({ "issue": number, "seq": seq }))
            }
            _ => Err("control answered something else".into()),
        }
    }

    async fn create(&self, session: &str, args: Value) -> Out {
        let a: CreateArgs = parse(args)?;
        let op = SessionIssuesOp::Create { title: a.title, body: a.body, labels: a.labels };
        match self.ask(session, op).await? {
            SessionIssuesGiven::Created { issue, url } => done(
                format!("Opened #{}: {} ({url})", issue.number, issue.title),
                json!({ "issue": issue.number, "id": issue.id, "url": url }),
            ),
            _ => Err("control answered something else".into()),
        }
    }

    async fn create_doc(&self, session: &str, args: Value) -> Out {
        let a: CreateDocArgs = parse(args)?;
        let sections = a
            .sections
            .into_iter()
            .map(|s| NewSection { heading: s.heading, intent: s.intent, markdown: s.markdown })
            .collect();
        let doc = NewDoc { title: a.title, sections, layout: None, moved_from: None, session: None };
        let (view, url) = self.doc(session, SessionIssuesOp::DocCreate { doc }).await?;
        doc_done("Made", &view, &url)
    }

    async fn fill_doc(&self, session: &str, args: Value) -> Out {
        let a: FillDocArgs = parse(args)?;
        let write = DocWrite { section: a.section, heading: a.heading, markdown: a.markdown, ..Default::default() };
        let (view, url) = self.doc(session, SessionIssuesOp::DocWrite { id: a.doc, write }).await?;
        doc_done("Wrote", &view, &url)
    }

    async fn read_doc(&self, session: &str, args: Value) -> Out {
        let a: ReadDocArgs = parse(args)?;
        if let Some(id) = a.doc.filter(|d| !d.trim().is_empty()) {
            let (view, url) = self.doc(session, SessionIssuesOp::DocRead { id: Some(id) }).await?;
            let text = doc_text(&view);
            let sections = json!(view.sections);
            return done(
                format!("Doc {} ({} sections), {url}", view.doc.title, view.sections.len()),
                json!({ "doc": view.doc.id, "title": view.doc.title, "url": url, "writer": view.writer, "sections": sections, "text": text }),
            );
        }
        let SessionIssuesGiven::Docs { list } = self.ask(session, SessionIssuesOp::DocRead { id: None }).await? else {
            return Err("control answered something else".into());
        };
        let text = list
            .docs
            .iter()
            .map(|d| format!("{} {} ({} of {} sections written)", d.id, d.title, d.written, d.sections))
            .collect::<Vec<_>>()
            .join("\n");
        done(
            format!("{} Doc(s) in project {}", list.docs.len(), list.project),
            json!({ "docs": json!(list.docs), "text": text }),
        )
    }

    async fn doc(&self, session: &str, op: SessionIssuesOp) -> Result<(DocView, String), String> {
        match self.ask(session, op).await? {
            SessionIssuesGiven::Doc { view, url } => Ok((view, url)),
            _ => Err("control answered something else".into()),
        }
    }
}

/// A made or written Doc's answer: its id, sections and link.
fn doc_done(what: &str, view: &DocView, url: &str) -> Out {
    let ids: Vec<&str> = view.sections.iter().map(|s| s.id.as_str()).collect();
    let pending: Vec<&str> = view.sections.iter().filter(|s| s.body.is_none()).map(|s| s.id.as_str()).collect();
    let left = if pending.is_empty() { String::new() } else { format!("; to fill: {}", pending.join(", ")) };
    done(
        format!("{what} Doc {} ({}), sections {}{left}: {url}", view.doc.title, view.doc.id, ids.join(", ")),
        json!({ "doc": view.doc.id, "url": url, "sections": ids, "pending": pending }),
    )
}

/// A Doc as markdown: its title, then each section's heading and body.
fn doc_text(view: &DocView) -> String {
    let mut out = format!("# {}\n", view.doc.title);
    for s in &view.sections {
        let body = match &s.body {
            Some(b) => b.trim_end().to_owned(),
            None if s.intent.is_empty() => "_(pending)_".to_owned(),
            None => format!("_(pending: {})_", s.intent),
        };
        out.push_str(&format!("\n## {} [{}]\n\n{body}\n", s.heading, s.id));
    }
    out
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("arugula", arugula_core::version::get()))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tools()).with_ttl_ms(0).with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let session = ctx
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<Caller>())
            .map(|c| c.0.clone())
            .ok_or_else(|| McpError::invalid_request("not a session's agent", None))?;
        info!(tool = %request.name, session, "session mcp call");
        let args = request.arguments.map(Value::Object).unwrap_or_else(|| json!({}));
        let out = match &*request.name {
            READ => self.read(&session, args).await,
            COMMENT => self.comment(&session, args).await,
            CREATE => self.create(&session, args).await,
            CREATE_DOC => self.create_doc(&session, args).await,
            FILL_DOC => self.fill_doc(&session, args).await,
            READ_DOC => self.read_doc(&session, args).await,
            WATCH => self.watch(&session, args).await,
            other => Err(format!("no tool {other}")),
        };
        let res = match out {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        };
        Ok(res.into())
    }
}

// ------------------------------------------------------------ who's calling

/// The uid that owns the client end of a loopback connection from `peer` to
/// `local`, in `/proc/net/tcp`'s text `table`.
fn uid_in(table: &str, peer: SocketAddrV4, local: SocketAddrV4) -> Option<u32> {
    let hex = |a: SocketAddrV4| format!("{:08X}:{:04X}", u32::from_le_bytes(a.ip().octets()), a.port());
    let (mine, theirs) = (hex(peer), hex(local));
    table.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.get(1) == Some(&mine.as_str()) && f.get(2) == Some(&theirs.as_str())).then(|| f.get(7)?.parse().ok())?
    })
}

#[cfg(unix)]
fn uid_of_user(name: &str) -> Option<u32> {
    nix::unistd::User::from_name(name).ok().flatten().map(|u| u.uid.as_raw())
}

#[cfg(not(unix))]
fn uid_of_user(_: &str) -> Option<u32> {
    None
}

#[cfg(unix)]
fn own_uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

#[cfg(not(unix))]
fn own_uid() -> u32 {
    0
}

/// The live session run by `uid`, if any: `uid` is one of the box's
/// session users (any of [`slots`](crate::work::sessions::slots)' users,
/// however many the box has), and each holds at most one session. A test's
/// stand-in users (`Slots::real` is false) are the daemon's own user, so
/// their sessions are told apart by nothing: with more than one live, none
/// is found.
fn session_of_uid(state: &std::path::Path, uid: u32) -> Option<String> {
    let slots = crate::work::sessions::slots()?;
    let user = if slots.real {
        Some(slots.users.iter().find(|u| uid_of_user(&u.user) == Some(uid))?.user.as_str())
    } else {
        (own_uid() == uid).then_some(None)?
    };
    let mut mine = crate::work::sessions::live(state).into_iter().filter(|s| user.is_none_or(|u| s.user.user == u));
    let found = mine.next()?;
    mine.next().is_none().then_some(found.session)
}

#[derive(Clone)]
struct Door {
    app: Arc<App>,
    local: SocketAddrV4,
}

fn refuse(why: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": why }))).into_response()
}

async fn authenticate(
    State(door): State<Door>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let SocketAddr::V4(peer) = peer else { return refuse("not a session's agent") };
    let table = std::fs::read_to_string("/proc/net/tcp").unwrap_or_default();
    let found = uid_in(&table, peer, door.local).and_then(|uid| session_of_uid(door.app.control.state_dir(), uid));
    let Some(session) = found else { return refuse("this MCP is for a session's agent, as its session user") };
    req.extensions_mut().insert(Caller(session));
    next.run(req).await
}

// ---------------------------------------------------------------- the server

/// Serve it, on a box that takes sessions (module docs).
pub fn start(app: Arc<App>) {
    if !crate::work::sessions::takes() {
        return;
    }
    crate::task::spawn(async move {
        if let Err(e) = serve(app).await {
            warn!(error = %format!("{e:#}"), "hosted: sessions' MCP isn't up; their agents have no issue tools");
        }
    });
}

async fn serve(app: Arc<App>) -> anyhow::Result<()> {
    // A port the firewall doesn't fence: not a privileged one, nor the
    // provider's (`work::firewall_ports`).
    let (listener, local) = loop {
        let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let SocketAddr::V4(a) = l.local_addr()? else { anyhow::bail!("not an IPv4 address") };
        if !crate::work::fenced_port(a.port()) {
            break (l, a);
        }
    };
    crate::mcp::session::serving(format!("http://{local}/mcp"));
    let config = StreamableHttpServerConfig::default().disable_allowed_hosts();
    let server = Server { app: app.clone() };
    let service: StreamableHttpService<Server, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(server.clone()), Default::default(), config);
    let router = Router::new()
        .nest_service(crate::mcp::PATH, service)
        .layer(middleware::from_fn_with_state(Door { app, local }, authenticate));
    info!(%local, "hosted: sessions' MCP is up");
    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const TABLE: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:D431 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1004        0 7001 1 0000000000000000 100 0 0 10 0
   1: 0100007F:D432 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1005        0 7002 1 0000000000000000 100 0 0 10 0
   2: 0100007F:9C40 0100007F:D431 01 00000000:00000000 00:00000000 00000000     0        0 7003 1 0000000000000000 100 0 0 10 0
";

    /// The uid is the one whose socket is the client end: not the server's
    /// end of the same connection, and not another connection.
    #[test]
    fn a_connections_uid_is_its_clients() {
        let local = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0x9C40);
        let at = |port| SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        assert_eq!(uid_in(TABLE, at(0xD431), local), Some(1004));
        assert_eq!(uid_in(TABLE, at(0xD432), local), Some(1005));
        assert_eq!(uid_in(TABLE, at(0xD433), local), None, "no such connection");
        assert_eq!(uid_in(TABLE, local, at(0xD431)), Some(0), "the server's own end is root's here");
        assert_eq!(uid_in("", at(0xD431), local), None);
    }

    #[test]
    fn exactly_the_session_tools_and_no_project_argument() {
        let t = tools();
        let names: Vec<&str> = t.iter().map(|t| &*t.name).collect();
        assert_eq!(
            names,
            [
                "read_session_issues",
                "comment_session_issue",
                "create_session_issue",
                "create_session_doc",
                "fill_session_doc",
                "read_project_doc",
                "watch",
            ]
        );
        for t in &t {
            let schema = serde_json::to_value(&t.input_schema).unwrap();
            let props = schema["properties"].as_object().unwrap();
            assert!(props.keys().all(|k| !k.contains("project")), "{}: a session names no project: {props:?}", t.name);
        }
        assert_eq!(number("#12"), Ok(12));
        assert_eq!(number(" 7 "), Ok(7));
        assert!(number("iss_0123456789abcdef0123456789abcdef").is_err(), "only a number in the project");
    }

    fn create(v: Value) -> Result<SessionWatchOp, String> {
        watch_create(serde_json::from_value(v).unwrap())
    }

    /// An event target goes to control as the machine's watch sends it,
    /// checked the same way, and a watch without one is the bytes it was.
    #[test]
    fn watch_create_takes_an_event_target() {
        let op = create(json!({
            "event": "issue.closed",
            "match": { "issue": "#12" },
            "note": " ship once it's closed ",
        }))
        .unwrap();
        let ask = serde_json::to_value(SessionWatchAsk { session: "ses_1".into(), op }).unwrap();
        assert_eq!(
            ask,
            json!({ "session": "ses_1", "op": "create", "targets": [], "expires_in_ms": null,
                    "event": "issue.closed", "match": { "issue": "#12" }, "note": "ship once it's closed" })
        );
        let old = serde_json::to_value(create(json!({ "run": "run_1" })).unwrap()).unwrap();
        let keys: Vec<&str> = old.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["expires_in_ms", "op", "targets"]);
        let err = |v| create(v).err().unwrap();
        assert_eq!(
            err(json!({ "event": "issue.renamed" })),
            "event issue.renamed: no such type; watch types lists them"
        );
        assert_eq!(
            err(json!({ "event": "issue.closed", "match": { "label": "bug" } })),
            "match label: issue.closed matches on issue, project, by"
        );
        assert_eq!(
            err(json!({ "event": "pane.exited" })),
            "event pane.exited: a machine's own event, watched by an agent on that machine; a hosted session watches the cloud's types"
        );
        assert!(err(json!({ "event": "issue.closed", "note": "x".repeat(201) })).contains("at most 200"));
        assert_eq!(err(json!({ "run": "run_1", "match": { "issue": "#1" } })), "match and note go with an event");
        assert!(err(json!({ "event": "issue.closed", "match": { "project": "prj_2" } })).starts_with("match project"));
        assert!(err(json!({})).ends_with("room or event"));
    }
}
