//! The event bus's envelope and catalog (Arugula #1224; the plan is the Doc
//! "Event bus: watch on one event log"). Every source writes its events in
//! one envelope, [`Event`], to one log, and `watch` waits on a filter over
//! it: an event type, a subject and a few of the type's match fields.
//!
//! The catalog, [`CATALOG`], lists the types this side knows: what each
//! means, the fields a watch may match on, where it's served (the daemon,
//! for a machine's own panes and agent blocks, which work offline; or the
//! cloud) and the grant power an agent needs to watch it. Cloud-only types
//! may also live in the cloud's repository; a type this side doesn't list
//! still reads as an [`Event`].
//!
//! Naming: Arugula's own types are `<object>.<verb>` (`issue.closed`,
//! `session.settled`, `pane.command.ended` for an object's part); a third
//! party's carry its name first (`github.pr.merged`, `plane.issue.moved`).
//! Each part is lowercase ASCII letters, digits and `_`
//! ([`name_well_formed`]).
//!
//! An event's `data` is metadata only: ids, states, logins and times, at
//! most [`DATA_MAX_BYTES`]. Never a title, body, comment or diff: what an
//! event carries reaches agents, and text written by someone else is
//! untrusted. [`data_check`] says whether some `data` may go in.

use serde::{Deserialize, Serialize};

/// An event's id prefix: `evt_…`.
pub const ID_PREFIX: &str = "evt_";

/// [`Event::source`] for Arugula's own events.
pub const SOURCE_ARUGULA: &str = "arugula";
/// [`Event::source`] for GitHub's, through the App's webhook or the poller.
pub const SOURCE_GITHUB: &str = "github";
/// [`Event::source`] for Plane's.
pub const SOURCE_PLANE: &str = "plane";

/// The most an event's `data` may be, as compact JSON: 4 KB.
pub const DATA_MAX_BYTES: usize = 4096;

/// Keys `data` may not have, at any depth: the words for text someone
/// wrote. [`data_check`] refuses them; it can't see every such field, so a
/// producer still puts only ids, states, logins and times in.
pub const DATA_TEXT_KEYS: &[&str] = &["title", "body", "comment", "diff", "text", "message", "description", "note"];

/// One event in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LogEvent"))]
pub struct Event {
    /// `evt_…`.
    pub id: String,
    /// Its place in the log, in commit order: a reader by `seq` misses
    /// none.
    pub seq: u64,
    /// Its type, a name in the catalog (`issue.closed`), or one this side
    /// doesn't know yet.
    #[serde(rename = "type")]
    pub kind: String,
    /// The version of its type's `data`, from 1.
    #[serde(default = "v1")]
    pub v: u32,
    /// Who it came from: [`SOURCE_ARUGULA`], [`SOURCE_GITHUB`],
    /// [`SOURCE_PLANE`], or another adapter's name.
    pub source: String,
    pub scope: EventScope,
    /// What it's about, as a ref Arugula already reads: `iss_…`,
    /// `owner/repo#12`, `run_…`, `dep_…`, `ses_…`, a pane's id, or a room
    /// and thread (`room_…#12`).
    pub subject: String,
    /// Who did it, when anyone did.
    #[serde(default)]
    pub actor: Option<Actor>,
    /// When it happened, as its source says, in ms since the epoch.
    pub at_ms: i64,
    /// When the log took it, in ms since the epoch.
    pub recorded_ms: i64,
    /// The type's fields: metadata only, at most [`DATA_MAX_BYTES`]
    /// ([`data_check`]). Null when it has none.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown> | null"))]
    pub data: serde_json::Value,
    /// Its identity for deduplication: a second event with the same key is
    /// dropped (a webhook's delivery id, `issue.closed:iss_…:3`).
    pub key: String,
}

fn v1() -> u32 {
    1
}

/// Where an event belongs: who may see it is decided from here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LogEventScope"))]
pub struct EventScope {
    /// The team's id.
    pub team: String,
    /// The project's id (`prj_…`), when it's in one.
    #[serde(default)]
    pub project: Option<String>,
}

/// Who did what an event says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LogActor"))]
pub struct Actor {
    pub kind: ActorKind,
    /// The account's or agent session's id; for an external actor, its id
    /// at the source.
    pub id: String,
    /// Their login as people read it, when they have one. An external
    /// one's is what the source said, untrusted.
    #[serde(default)]
    pub login: Option<String>,
}

/// What kind of actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LogActorKind"))]
#[cfg_attr(feature = "schema", schemars(transform = crate::open_enum))]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// A person's Arugula account.
    Account,
    /// An agent, acting for its owner.
    Agent,
    /// Someone at a third party (a GitHub login), not known to Arugula.
    External,
    /// A kind this side doesn't know yet.
    #[serde(other)]
    Other,
}

impl Event {
    /// Whether its `data` may go in the log ([`data_check`]).
    pub fn data_check(&self) -> Result<(), String> {
        data_check(&self.data)
    }
}

/// Whether `data` may be an event's: null or an object, at most
/// [`DATA_MAX_BYTES`] as compact JSON, with none of [`DATA_TEXT_KEYS`].
pub fn data_check(data: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    match data {
        Value::Null => return Ok(()),
        Value::Object(_) => {}
        _ => return Err("an event's data is an object".into()),
    }
    let len = serde_json::to_vec(data).map(|b| b.len()).unwrap_or(usize::MAX);
    if len > DATA_MAX_BYTES {
        return Err(format!("an event's data is {len} bytes, more than {DATA_MAX_BYTES}"));
    }
    fn text_key(v: &Value) -> Option<&str> {
        match v {
            Value::Object(o) => o
                .iter()
                .find_map(|(k, v)| DATA_TEXT_KEYS.contains(&k.as_str()).then_some(k.as_str()).or_else(|| text_key(v))),
            Value::Array(a) => a.iter().find_map(text_key),
            _ => None,
        }
    }
    match text_key(data) {
        Some(k) => Err(format!("an event's data is metadata only: no {k}")),
        None => Ok(()),
    }
}

/// Where a type is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    /// The machine's daemon, from its panes' and blocks' states: works
    /// with no account.
    Daemon,
    /// The cloud's log: needs the machine to be joined.
    Cloud,
}

/// One field a watch may match an event on, by equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchField {
    pub name: &'static str,
    /// What it holds, in a line.
    pub meaning: &'static str,
}

/// One type in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventType {
    /// `issue.closed` ([`name_well_formed`]).
    pub name: &'static str,
    /// What it means, in a line.
    pub meaning: &'static str,
    /// The fields a watch may match on.
    pub matches: &'static [MatchField],
    pub home: Home,
    /// The grant power an agent needs to watch it (`agent_may`), checked
    /// when the watch is made and when it fires. None for a daemon's own
    /// types: the machine's owner's agents watch its panes.
    pub power: Option<&'static str>,
}

const fn m(name: &'static str, meaning: &'static str) -> MatchField {
    MatchField { name, meaning }
}

const ISSUE: MatchField = m("issue", "the issue, iss_… or its #N in the project");
const PROJECT: MatchField = m("project", "the project, prj_…");
const PANE: MatchField = m("pane", "the pane or agent block, by its id");
const REPO: MatchField = m("repo", "the linked repo, owner/name");
const PR: MatchField = m("pr", "the pull request's number");
const BRANCH: MatchField = m("branch", "the branch: a PR's head, or the one pushed to");
const BY: MatchField = m("by", "the GitHub login of who did it");
const OUTSIDE: MatchField =
    m("outside", "true for a fork's PR or a non-member's: such an event matches only a watch that asks for it");
const IDENT: MatchField = m("ident", "the Plane work item, as IDENT-N (such as ALL-2212)");
const BOARD: MatchField = m("board", "the board, brd_…");
const GITHUB_PR: &[MatchField] = &[REPO, PR, BRANCH, BY, OUTSIDE];
const GITHUB_CHECKS: &[MatchField] = &[REPO, PR, BRANCH, OUTSIDE];
const BASE: MatchField = m("base", "the branch it's to merge into");
const BLOCK: MatchField = m("session", "the agent block, as <machine>/<pane>");
const MACHINE: MatchField = m("machine", "the machine, by its id");

/// Every type this side knows. Issues and sessions ride `cloud.drive` until
/// they have read powers of their own, as PR watches do.
pub const CATALOG: &[EventType] = &[
    EventType {
        name: "issue.created",
        meaning: "A native issue was opened.",
        matches: &[PROJECT, m("author", "the login of who opened it"), m("label", "a label it was opened with")],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "issue.closed",
        meaning: "A native issue was closed.",
        matches: &[ISSUE, PROJECT, m("by", "the login of who closed it")],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "issue.reopened",
        meaning: "A closed native issue was opened again.",
        matches: &[ISSUE, PROJECT, m("by", "the login of who reopened it")],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "issue.assigned",
        meaning: "Someone was assigned a native issue.",
        matches: &[ISSUE, PROJECT, m("assignee", "the login of who was assigned")],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "issue.labeled",
        meaning: "A native issue gained a label.",
        matches: &[ISSUE, PROJECT, m("label", "the label it gained"), m("by", "the login of who labelled it")],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "session.settled",
        meaning: "A hosted agent session stopped working: done, waiting for input, failed or stopped.",
        matches: &[
            m("session", "the session, ses_…"),
            PROJECT,
            m("state", "how it settled: done, waiting, failed or stopped"),
        ],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    // A machine's agent blocks, from the state its daemon reports for boards
    // (Arugula #1274): ids, a kind and times, never what is asked. Who may
    // watch them is who sees a block's state on a board.
    EventType {
        name: "session.needs_input",
        meaning: "An agent block on a machine started asking a person something: a question or a permission.",
        matches: &[BLOCK, PROJECT, MACHINE, PANE, m("kind", "what it asks for: ask or permission")],
        home: Home::Cloud,
        power: Some("boards.read"),
    },
    EventType {
        name: "session.idle",
        meaning: "An agent block on a machine finished its turn, or was answered and went idle.",
        matches: &[BLOCK, PROJECT, MACHINE, PANE],
        home: Home::Cloud,
        power: Some("boards.read"),
    },
    EventType {
        name: "run.ended",
        meaning: "A run finished, however it finished.",
        matches: &[m("run", "the run, run_…"), PROJECT, m("state", "how it ended: succeeded, failed or cancelled")],
        home: Home::Cloud,
        power: Some("runs.read"),
    },
    EventType {
        name: "deploy.ended",
        meaning: "A deploy finished, however it finished.",
        matches: &[
            m("deploy", "the deploy, dep_…"),
            PROJECT,
            m("environment", "the environment it deployed to"),
            m("state", "how it ended: succeeded, failed or cancelled"),
        ],
        home: Home::Cloud,
        power: Some("runs.read"),
    },
    EventType {
        name: "chat.reply",
        meaning: "A message was posted in a chat thread, or in a room.",
        matches: &[
            m("room", "the room, by its id"),
            m("thread", "the thread, its root message's number"),
            m("from", "the login of who wrote it"),
        ],
        home: Home::Cloud,
        power: Some("threads.read"),
    },
    EventType {
        name: "draft.answered",
        meaning: "A forge draft was sent, discarded or failed.",
        matches: &[
            m("draft", "the draft, by its id"),
            m("state", "how it was answered: sent, discarded or failed"),
            REPO,
        ],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    // An agent session's proposal (Arugula #1278) answered on its card:
    // the card is in its asker's staff room, so threads.read.
    EventType {
        name: "ask.answered",
        meaning: "A person answered their agent's approval card (an agent session's, or a machine's agent's): approved it, denied it, or approved it and doing it failed; or a to-do: done, can't, or cancelled by its agent. Never the person's note.",
        matches: &[
            m("ask", "the card, prp_…"),
            PROJECT,
            m("session", "the session that proposed it: ses_…, or a machine's agent's session name"),
            m("action", "what it proposed: start, close, run_board or todo"),
            m("answer", "approved, denied or failed; a to-do's done, cant or cancelled"),
        ],
        home: Home::Cloud,
        power: Some("threads.read"),
    },
    EventType {
        name: "machine.updated",
        meaning: "A machine's daemon version changed.",
        matches: &[m("machine", "the machine, by its id"), m("version", "the daemon version it runs now")],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.opened",
        meaning: "A pull request was opened on a linked GitHub repo.",
        matches: &[REPO, PR, BRANCH, BASE, BY, OUTSIDE],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.closed",
        meaning: "A pull request on a linked GitHub repo was closed, merged or not (merged says which).",
        matches: GITHUB_PR,
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.merged",
        meaning: "A pull request on a linked GitHub repo was merged.",
        matches: GITHUB_PR,
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.checks_done",
        meaning: "None of the checks on a pull request's head commit is still running (once per head).",
        matches: GITHUB_CHECKS,
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.checks_failed",
        meaning: "A check on a pull request's head commit failed (once per head).",
        matches: GITHUB_CHECKS,
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.ready",
        meaning: "A pull request's checks are green and GitHub says it can merge cleanly (once per head).",
        matches: GITHUB_CHECKS,
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.check.failed",
        meaning: "A check run or commit status failed on a linked repo's commit, on any branch.",
        matches: &[
            REPO,
            m("sha", "the commit it ran on"),
            m("branches", "a branch it's on: its head's, or one of its PRs' bases"),
            m("heads", "a branch whose own head it ran on (a PR's head, never its base): main, for main's red"),
            m("conclusion", "failure or timed_out for a check run; failure or error for a status"),
            OUTSIDE,
        ],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.reviewed",
        meaning: "A review was submitted on a linked repo's pull request.",
        matches: &[REPO, PR, m("state", "approved, changes_requested, commented or dismissed"), BY, OUTSIDE],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.pr.review_commented",
        meaning: "An inline review comment was made on a linked repo's pull request.",
        matches: &[REPO, PR, BY, OUTSIDE],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.push",
        meaning: "A branch of a linked GitHub repo was pushed to: its new head.",
        matches: &[REPO, BRANCH, BY],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "github.release.published",
        meaning: "A release of a linked GitHub repo was published, a draft's included (once per release).",
        matches: &[
            REPO,
            m("tag", "the release's tag, such as v1.2.0"),
            m("prerelease", "true for a release marked as a pre-release"),
            BY,
        ],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "plane.issue.created",
        meaning: "A work item was made in a connected Plane project.",
        matches: &[
            PROJECT,
            m("state", "the name of the state it was made in, as the Plane project names it"),
            m("state_group", "its state's group: backlog, unstarted, started, completed or cancelled"),
            m("labels", "a label it was made with, by name"),
            m("assignees", "the Plane user id of someone it was assigned to"),
        ],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "plane.issue.updated",
        meaning: "A connected Plane work item's state, assignees or labels changed.",
        matches: &[
            IDENT,
            PROJECT,
            m("state", "the name of the state it is in now, such as Done"),
            m("state_group", "its state's group now: backlog, unstarted, started, completed or cancelled"),
            m("closed", "whether it is closed now (true once its group is completed or cancelled)"),
            m("changed", "what changed: state, assignees or labels"),
            m("labels", "a label it has now, by name"),
            m("assignees", "the Plane user id of someone it is assigned to now"),
            m("by", "the Plane user id of who changed it"),
        ],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "plane.issue.deleted",
        meaning: "A connected Plane work item was deleted.",
        matches: &[IDENT, PROJECT],
        home: Home::Cloud,
        power: Some("cloud.drive"),
    },
    EventType {
        name: "board.row.added",
        meaning: "A row was added to a board, by a person or their agent.",
        matches: &[BOARD, PROJECT, m("by", "the login of who added it, or whose agent did")],
        home: Home::Cloud,
        power: Some("boards.read"),
    },
    EventType {
        name: "board.lane.overridden",
        meaning: "A person moved a board's row to a lane its work doesn't derive.",
        matches: &[
            BOARD,
            PROJECT,
            m("row", "the row, row_…"),
            m("lane", "the lane it was moved to: next, doing or review"),
            m("by", "the login of who moved it"),
        ],
        home: Home::Cloud,
        power: Some("boards.read"),
    },
    EventType {
        name: "board.closed",
        meaning: "A board was closed, its goal met.",
        matches: &[BOARD, PROJECT, m("by", "the login of who closed it, or whose agent did")],
        home: Home::Cloud,
        power: Some("boards.read"),
    },
    EventType {
        name: "agent.idle",
        meaning: "An agent on this machine finished its turn and waits for a prompt.",
        matches: &[PANE],
        home: Home::Daemon,
        power: None,
    },
    EventType {
        name: "agent.needs_input",
        meaning: "An agent on this machine asks a person something: a permission or a question.",
        matches: &[PANE],
        home: Home::Daemon,
        power: None,
    },
    EventType {
        name: "pane.command.ended",
        meaning: "A command in a pane's shell finished.",
        matches: &[PANE, m("exit", "its exit code")],
        home: Home::Daemon,
        power: None,
    },
    EventType {
        name: "pane.exited",
        meaning: "A pane's process exited.",
        matches: &[PANE, m("exit", "its exit code")],
        home: Home::Daemon,
        power: None,
    },
    EventType {
        name: "pane.output.matched",
        meaning: "A new line of a pane's output matches a pattern: lines already on screen or in scrollback never do.",
        matches: &[PANE, m("pattern", "a regex, matched against each new line of its output, ANSI stripped")],
        home: Home::Daemon,
        power: None,
    },
];

/// `GET` with [`EventsQuery`]: a project's events after a number, for an
/// agent on the machine (Arugula #1273), answered [`EventsPage`]. A hosted
/// session reads the same through its watch route
/// (`work::SessionWatchOp::Events`), in its own project. Each event is
/// one the agent could watch: its type's [`EventType::power`] is the
/// machine's over the project, and its owner may read what it's about now,
/// as a watch on it is checked when it's made and when it fires.
pub const AGENT_EVENTS: &str = "/api/daemon/events";

/// `GET` with [`ProjectEventsQuery`]: the same read for a person, on the
/// page (Arugula #1277): their project's events after a number that their
/// account may read now, answered [`EventsPage`]. A person has no machine
/// grants to check, so it's their sight alone, as a hosted session's read
/// is its asker's. The Overview counts its briefing line from it.
pub const PROJECT_EVENTS: &str = "/api/projects/{project}/events";

/// How many events a read returns when it isn't told.
pub const READ_LIMIT_DEFAULT: u32 = 50;
/// The most one read returns.
pub const READ_LIMIT_MAX: u32 = 200;

/// `GET /api/daemon/events`: which events.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EventsQuery {
    /// The project's id, `prj_…`.
    pub project: String,
    /// Events numbered after this: 0 for the oldest the log keeps, or a
    /// page's [`EventsPage::next_after`].
    #[serde(default)]
    pub after: u64,
    /// Only these types, joined by `,` ([`read_types`]). None: every type
    /// the agent may watch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    /// At most this many: [`READ_LIMIT_DEFAULT`] when none, at most
    /// [`READ_LIMIT_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// [`PROJECT_EVENTS`]: which events, as [`EventsQuery`] says, the project
/// being the path's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProjectEventsQuery {
    #[serde(default)]
    pub after: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// A read of the log: its events in `seq` order, as stored (metadata
/// only), and where to read on from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EventsPage {
    pub events: Vec<Event>,
    /// The next read's `after`: past every event this one looked at,
    /// including those it left out (another project's or type's, or one
    /// the agent may not see), so none is read twice.
    pub next_after: u64,
    /// The log's newest number when it was read. `next_after` below it:
    /// there's more to read.
    pub head: u64,
    /// The log no longer keeps the events after `after` numbered below
    /// this: they were older than it keeps them, and are gone. None when it
    /// had them all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gone_before: Option<u64>,
}

/// A read's `types`, from a list joined by `,` or given one by one: each
/// a type in the catalog that the cloud's log holds. Empty: every type.
pub fn read_types<'a>(types: impl IntoIterator<Item = &'a str>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = vec![];
    for name in types.into_iter().flat_map(|t| t.split(',')).map(str::trim).filter(|t| !t.is_empty()) {
        match lookup(name) {
            None => return Err(format!("type {name}: no such type; watch types lists them")),
            Some(t) if t.home == Home::Daemon => {
                return Err(format!("type {name}: a machine's own, never in the cloud's log; watch it instead"));
            }
            Some(_) if out.iter().any(|o| o == name) => {}
            Some(_) => out.push(name.to_owned()),
        }
    }
    Ok(out)
}

/// A read's events as an agent reads them: one line each, then where to
/// read on from.
pub fn page_text(p: &EventsPage) -> String {
    let mut lines = vec![];
    if let Some(n) = p.gone_before {
        lines.push(format!(
            "The log no longer keeps events before #{n}: those after your after were older than it keeps, and are gone."
        ));
    }
    for e in &p.events {
        let by = e.actor.as_ref().map(|a| format!(" by {}", a.login.as_deref().unwrap_or(&a.id))).unwrap_or_default();
        let data = match &e.data {
            serde_json::Value::Null => String::new(),
            d => format!(" {d}"),
        };
        lines.push(format!("#{} {} {}{by}{data}", e.seq, e.kind, e.subject));
    }
    if p.events.is_empty() {
        lines.push("No events.".into());
    }
    let more = if p.next_after < p.head { "; there's more" } else { "" };
    lines.push(format!("Read on with after {} (the log's head is #{}{more}).", p.next_after, p.head));
    lines.join("\n")
}

/// The catalog's entry for a type, by its name.
pub fn lookup(name: &str) -> Option<&'static EventType> {
    CATALOG.iter().find(|t| t.name == name)
}

/// Whether `name` is a type's name: at least two parts joined by `.`, each
/// lowercase ASCII letters, digits and `_`, starting with a letter.
pub fn name_well_formed(name: &str) -> bool {
    let mut parts = 0;
    for part in name.split('.') {
        parts += 1;
        let mut chars = part.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_lowercase())
            || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return false;
        }
    }
    parts >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn closed() -> serde_json::Value {
        json!({
            "id": "evt_01", "seq": 42, "type": "issue.closed", "v": 1, "source": "arugula",
            "scope": { "team": "team_1", "project": "prj_1" },
            "subject": "iss_90dd",
            "actor": { "kind": "account", "id": "acc_1", "login": "jdoe" },
            "at_ms": 1_763_500_000_000_i64, "recorded_ms": 1_763_500_000_015_i64,
            "data": { "issue": "iss_90dd", "number": 812, "state": "closed", "by": "jdoe" },
            "key": "issue.closed:iss_90dd:3",
        })
    }

    #[test]
    fn an_event_round_trips() {
        let e: Event = serde_json::from_value(closed()).unwrap();
        assert_eq!(e.kind, "issue.closed");
        assert_eq!(e.actor.as_ref().unwrap().kind, ActorKind::Account);
        assert_eq!(serde_json::to_value(&e).unwrap(), closed());
        assert!(e.data_check().is_ok());
    }

    #[test]
    fn an_event_reads_leniently() {
        let mut v = closed();
        let o = v.as_object_mut().unwrap();
        for k in ["v", "actor", "data"] {
            o.remove(k);
        }
        o.insert("source".into(), json!("linear"));
        o.insert("scope".into(), json!({ "team": "team_1" }));
        o.insert("later".into(), json!(true));
        let e: Event = serde_json::from_value(v).unwrap();
        assert_eq!((e.v, e.source.as_str(), &e.actor, &e.data), (1, "linear", &None, &serde_json::Value::Null));
        assert_eq!(e.scope.project, None);
        // Nulls stay nulls on the way out.
        let out = serde_json::to_value(&e).unwrap();
        assert_eq!((&out["actor"], &out["scope"]["project"]), (&json!(null), &json!(null)));
        let a: Actor = serde_json::from_value(json!({ "kind": "bot", "id": "1" })).unwrap();
        assert_eq!((a.kind, a.login), (ActorKind::Other, None));
    }

    #[test]
    fn data_is_metadata_only() {
        assert!(data_check(&json!(null)).is_ok());
        assert!(data_check(&json!({ "pr": "o/r#1", "checks": [{ "name": "ci", "state": "failed" }] })).is_ok());
        assert!(data_check(&json!("closed")).is_err());
        assert!(data_check(&json!({ "title": "x" })).unwrap_err().contains("title"));
        assert!(data_check(&json!({ "reviews": [{ "body": "lgtm" }] })).unwrap_err().contains("body"));
        // A to-do's note (Arugula #1462) is the person's words: never an event's.
        assert!(data_check(&json!({ "ask": "prp_1", "note": "14,203 rows" })).unwrap_err().contains("note"));
        let fits = json!({ "s": "a".repeat(DATA_MAX_BYTES - 8) });
        assert_eq!(serde_json::to_vec(&fits).unwrap().len(), DATA_MAX_BYTES);
        assert!(data_check(&fits).is_ok());
        let over = json!({ "s": "a".repeat(DATA_MAX_BYTES - 7) });
        assert!(data_check(&over).unwrap_err().contains("more than"));
    }

    #[test]
    fn the_catalogs_names_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for t in CATALOG {
            assert!(name_well_formed(t.name), "{}", t.name);
            assert!(seen.insert(t.name), "{} twice", t.name);
            assert!(!t.meaning.is_empty() && !t.matches.is_empty(), "{}", t.name);
            let mut fields = std::collections::HashSet::new();
            assert!(t.matches.iter().all(|f| name_well_formed(&format!("x.{}", f.name)) && fields.insert(f.name)));
            assert_eq!(t.power.is_none(), t.home == Home::Daemon, "{}: a cloud type needs a power", t.name);
            // A third party's carry its name first, and are the cloud's.
            if [SOURCE_GITHUB, SOURCE_PLANE].contains(&t.name.split('.').next().unwrap()) {
                assert_eq!(t.home, Home::Cloud, "{}", t.name);
            }
        }
        for good in ["issue.closed", "pane.command.ended", "github.pr.merged", "agent.needs_input"] {
            assert!(name_well_formed(good), "{good}");
        }
        for bad in ["issue", "Issue.closed", "issue..closed", ".closed", "issue.closed.", "issue.1st", "issue-x.closed"]
        {
            assert!(!name_well_formed(bad), "{bad}");
        }
    }

    #[test]
    fn a_reads_types_are_the_clouds() {
        assert_eq!(read_types(["issue.closed, run.ended", "issue.closed"]).unwrap(), ["issue.closed", "run.ended"]);
        assert!(read_types(["", " "]).unwrap().is_empty());
        assert!(read_types(["issue.shut"]).unwrap_err().contains("no such type"));
        assert!(read_types(["pane.exited"]).unwrap_err().contains("never in the cloud's log"));
        let q = EventsQuery { project: "prj_1".into(), after: 3, types: None, limit: None };
        assert_eq!(serde_json::to_value(&q).unwrap(), json!({ "project": "prj_1", "after": 3 }));
    }

    #[test]
    fn a_page_reads_as_a_line_an_event_and_where_to_read_on() {
        let e: Event = serde_json::from_value(closed()).unwrap();
        let p = EventsPage { events: vec![e], next_after: 42, head: 50, gone_before: Some(40) };
        let text = page_text(&p);
        assert!(text.starts_with("The log no longer keeps events before #40"), "{text}");
        assert!(text.contains("#42 issue.closed iss_90dd by jdoe {"), "{text}");
        assert!(text.ends_with("Read on with after 42 (the log's head is #50; there's more)."), "{text}");
        let none = page_text(&EventsPage { next_after: 50, head: 50, ..Default::default() });
        assert_eq!(none, "No events.\nRead on with after 50 (the log's head is #50).");
        let v = serde_json::to_value(EventsPage::default()).unwrap();
        assert!(v.get("gone_before").is_none(), "{v}");
    }

    #[test]
    fn lookup_finds_by_name() {
        let t = lookup("run.ended").unwrap();
        assert_eq!((t.home, t.power), (Home::Cloud, Some("runs.read")));
        assert_eq!(lookup("chat.reply").unwrap().power, Some("threads.read"));
        assert_eq!(lookup("pane.exited").unwrap().home, Home::Daemon);
        assert!(lookup("issue.closed").unwrap().matches.iter().any(|f| f.name == "issue"));
        for name in ["issue.labeled", "draft.answered", "machine.updated", "github.pr.checks_failed", "github.push"] {
            assert_eq!(lookup(name).unwrap().power, Some("cloud.drive"), "{name}");
        }
        assert!(lookup("doc.share.answered").is_none(), "no producer yet");
        let t = lookup("ask.answered").unwrap();
        assert_eq!((t.home, t.power), (Home::Cloud, Some("threads.read")));
        assert!(
            ["ask", "project", "session", "action", "answer"].iter().all(|f| t.matches.iter().any(|m| m.name == *f))
        );
        assert!(!lookup("github.push").unwrap().matches.iter().any(|f| f.name == "outside"), "a push needs write");
        assert!(lookup("github.pr.merged").unwrap().matches.iter().any(|f| f.name == "outside"));
        assert!(lookup("github.push").unwrap().matches.iter().any(|f| f.name == "branch"));
        assert!(lookup("github.pr.reopened").is_none());
        assert_eq!(lookup("plane.issue.updated").unwrap().home, Home::Cloud);
        assert_eq!(lookup("board.closed").unwrap().power, Some("boards.read"));
        for name in ["session.needs_input", "session.idle"] {
            let t = lookup(name).unwrap();
            assert_eq!((t.home, t.power), (Home::Cloud, Some("boards.read")), "{name}");
            assert!(["session", "project", "machine", "pane"].iter().all(|f| t.matches.iter().any(|m| m.name == *f)));
        }
        assert!(lookup("session.needs_input").unwrap().matches.iter().any(|f| f.name == "kind"));
        for name in ["github.check.failed", "github.pr.reviewed", "github.pr.review_commented"] {
            assert_eq!(lookup(name).unwrap().home, Home::Cloud, "{name}");
        }
        let failed = lookup("github.check.failed").unwrap();
        assert!(["branches", "heads"].iter().all(|f| failed.matches.iter().any(|m| m.name == *f)), "#1291");
        assert!(lookup("github.pr.opened").unwrap().matches.iter().any(|f| f.name == "base"));
        let release = lookup("github.release.published").unwrap();
        assert_eq!((release.home, release.power), (Home::Cloud, Some("cloud.drive")));
        assert!(release.matches.iter().any(|f| f.name == "tag"));
        assert!(!release.matches.iter().any(|f| f.name == "outside"), "a release needs write");
        assert!(lookup("").is_none());
    }
}
