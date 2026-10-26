//! Agent sessions (M254, thread = session, Arugula #129): a thread's agent
//! session on a project's hosted workspace, its history and the prompts
//! that start its turns. Who may start, prompt or stop one is the routes'
//! (M256); this is storage and the one place a session's state changes
//! ([`Db::move_session`]). The tables, with notes on their columns, are
//! made by `0050_agent_sessions`; the states, the moves and the signed
//! event are `arugula_e2e::session`'s, and what the hosted machine pulls
//! and reports is `arugula_control_wire::work`'s.
//!
//! A session is not a run. It runs as its asker, on the project's key, on
//! the workspace the project's agents run on ([`Db::agents_run_on`]), and
//! lives across many turns, each started by a prompt.
//!
//! - **States**: `queued`, then `working` ⇄ `needs_input`; `working` to
//!   `done` when a turn ends, and back to `working` when a new prompt's
//!   turn starts; `failed` and `stopped` are the ends. One transaction
//!   checks a move and writes the session's new state and its event, or
//!   refuses and writes nothing ([`Refused`]).
//! - **`unresponsive`** is the platform's: a session whose turn is under
//!   way (`working`, `needs_input`) and that hasn't heard from its machine
//!   for [`QUIET_MS`] is marked so ([`Db::mark_unresponsive`]), and the
//!   machine's next report, whatever it is, moves it back to the state it
//!   was in (`was_state`) before that report is taken, as control's event.
//!   So a machine's report is always from the state it last saw.
//! - **Who reports what** ([`Move`]): the machine only what it knows
//!   (taking it, a turn's start, question and end, a failure, stopping the
//!   agent), signed over `arugula session event v1` and checked into a
//!   [`DaemonReport`]; control the rest (a session nobody took fails, a
//!   person stops it, the box goes quiet), unsigned
//!   (`State::daemon_may`, `State::control_may`).
//! - **Which machine:** the session's workspace's hosted machine, as it is
//!   when the first report arrives; from then on that machine only
//!   (`machine`).
//! - **No replays:** each of the machine's reports carries its number in
//!   the session, `n`, from 1, which must be past the last one taken: a
//!   signed report is good once. A report control refused leaves a gap the
//!   next one steps over, rather than stopping every report after it (a
//!   refused heartbeat wedged a live session, 0.63.3). Control's events
//!   don't take a number, so they never make the machine's next one wrong.
//! - **Events** are append-only: the tables refuse an `UPDATE`, and a
//!   `DELETE` while the event's session is there. A heartbeat isn't one:
//!   it takes its number and moves `heard_at` and `last_tool` (cut to
//!   [`TOOL_MAX`] characters), and leaves
//!   no row, so a session waiting hours on a question doesn't fill the
//!   table.
//! - **What each report holds** ([`SessionDetail`]): a JSON object of at
//!   most [`MAX_DETAIL`] bytes. A turn's start names the queued prompts it
//!   starts with, which become `sent`; taking a session names its agent
//!   block; `failed` says why; a `turn` names the session's own branch;
//!   an `error` says what. The wire module's docs list them.
//! - **At most [`MAX_SESSIONS_PER_BOX`]** sessions on a workspace hold a
//!   slot on its box: taken and not ended (`working`, `needs_input`,
//!   `unresponsive` or `done`; a done one keeps its checkout for its next
//!   turn). A machine's take past that is refused ([`Refused::Busy`]) and
//!   may be sent again later.
//! - **Prompts** are `queued` until a turn starts with them (`sent`), or
//!   `dropped`: their author took them back, or the session ended first.
//! - **Actions** (M257, [`Action`]) are what people ask of a session
//!   that only its box can do: answer its open question, approve or deny
//!   its open permission, cancel the turn. Each is `pending`, listed in
//!   the box's work, until a report of the machine names it in its
//!   detail's `actions` (then `done`, with that report's number), or the
//!   session ends first (`dropped`). One answer to an open request at a
//!   time: the first wins. A turn that leaves `needs_input` drops the
//!   answers it didn't name: what they answered isn't open any more.
//! - **The job** that follows a session is its `session` job on
//!   `platform_jobs` ([`dispatch`]); one that ends other than `done` fails
//!   its session (M223's rule for runs). Its handler is M256's.
//! - **Siblings and reviews** (M264, [`Role`]): a Try 3 makes two
//!   sessions under one ([`start_tries_in`]), on its thread, box and repo,
//!   each re-running its first prompt, on the harness asked for. A thread
//!   still has one session of its own going at a time; its siblings and
//!   reviews run beside it, each in a slot of the box's. A pick
//!   ([`pick_in`]) keeps one of the three and stops the others; a picked
//!   sibling is the thread's session from then on ([`Db::session_on_thread`]).
//!   A review (`@codex review`) is a session under the one it reviews,
//!   read-only, that ends once its review is posted.

use anyhow::{Context as _, Result, bail};
pub use arugula_control_wire::work::{MAX_SESSIONS_PER_BOX, SessionAction, SessionDetail, SessionPrompt, SessionWork};
pub use arugula_e2e::session::{Kind, QUIET_MS, State};

use super::{
    Db,
    sql::{Optional as _, Row, Sql, params},
    taken,
};
use crate::Deleted;

/// The most a report's or control's detail may hold.
pub const MAX_DETAIL: usize = 16 * 1024;

/// The most a prompt may hold.
pub const MAX_PROMPT: usize = 32 * 1024;

/// The most of a heartbeat's tool title kept, in characters.
pub const TOOL_MAX: usize = 200;

/// The longest slug a session's branch takes.
pub const MAX_SLUG: usize = 40;

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A slug for a session's branch from `text` (its first prompt, say): its
/// first words, lowercase ASCII letters and digits joined by `-`, at most
/// [`MAX_SLUG`] bytes. Empty when nothing in it fits.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for word in text.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        let word = word.to_ascii_lowercase();
        let more = if out.is_empty() { word.len() } else { word.len() + 1 };
        if out.len() + more > MAX_SLUG {
            if out.is_empty() {
                out.push_str(&word[..MAX_SLUG]);
            }
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&word);
    }
    out
}

/// A slug for a session on native issue `number` (M265), as `arugula issue
/// open` names an issue's branch: `i<number>-` and the title's first
/// words (at most five, about 28 characters), else `i<number>`. At most
/// [`MAX_SLUG`] bytes.
pub fn issue_slug(number: u64, title: &str) -> String {
    let title = title.to_lowercase().replace(['\'', '’'], "");
    let mut words: Vec<String> = Vec::new();
    for w in title.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        let len: usize = words.iter().map(|w| w.len() + 1).sum();
        if words.len() >= 5 || (len + w.len() > 28 && !words.is_empty()) {
            break;
        }
        words.push(w.chars().take(28).collect());
    }
    let mut out = format!("i{number}");
    for w in words {
        if out.len() + 1 + w.len() > MAX_SLUG {
            break;
        }
        out.push('-');
        out.push_str(&w);
    }
    out
}

/// How many hex characters of a session's id its branch takes.
pub const BRANCH_HEX: usize = 8;

/// Session `id`'s branch: `arugula/<the first 8 hex of its id>-<slug>`,
/// or `arugula/<8 hex>` with no slug. Short enough to read in a PR's
/// header; the full id stays on the session's row, and the push is checked
/// against the row's branch.
pub fn branch_for(id: &str, slug: &str) -> String {
    let hex = id.strip_prefix("ses_").unwrap_or(id);
    let short = &hex[..hex.len().min(BRANCH_HEX)];
    if slug.is_empty() { format!("arugula/{short}") } else { format!("arugula/{short}-{slug}") }
}

fn is_slug(s: &str) -> bool {
    s.len() <= MAX_SLUG
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One word for a column the machine names (an agent block): printable,
/// no space, at most 128 bytes.
fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// What a session is to its thread (M264).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The thread's own: `@arugula` made it.
    Thread,
    /// A Try 3's sibling of its `parent_session`.
    Try,
    /// `@codex review` of its `parent_session`: read-only.
    Review,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Thread => "thread",
            Role::Try => "try",
            Role::Review => "review",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Role::Thread, Role::Try, Role::Review].into_iter().find(|r| r.as_str() == s)
    }
}

/// Whether `h` is an agent a session runs on (`claude`, `codex`).
pub fn is_harness(h: &str) -> bool {
    arugula_control_wire::work::HARNESSES.contains(&h)
}

/// Why a stopped try wasn't kept, first the code (the card knows it).
pub const NOT_PICKED: &str = "not_picked: another try was kept";

/// How many sessions a Try 3 runs: the one it's asked of and two
/// siblings.
pub const TRIES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub project: String,
    /// Its thread: the room, and the message the thread hangs off.
    pub room: String,
    pub parent_seq: u64,
    /// The account it runs as, whose mention started it. Empty once that
    /// account is deleted.
    pub asker: String,
    /// The hosted workspace it runs on.
    pub workspace: String,
    /// The workspace's machine, once it reported: the only one whose
    /// reports move the session.
    pub machine: Option<String>,
    /// The agent block it runs in on the box.
    pub block: Option<String>,
    /// The linked repo it works in (`owner/name`).
    pub repo: Option<String>,
    /// The one ref it may push ([`branch_for`]):
    /// `arugula/<8 hex of its id>-<slug>`.
    pub branch: String,
    pub state: State,
    /// The state an `unresponsive` session goes back to.
    pub was: Option<State>,
    /// The latest event's number.
    pub last_seq: u64,
    /// The machine's latest report's number.
    pub daemon_n: u64,
    /// When the platform last heard from the machine (its own clock).
    pub heard_at: Option<u64>,
    pub last_tool: Option<String>,
    pub last_error: Option<String>,
    /// The draft PR's link, once its asker sent it.
    pub pr: Option<String>,
    /// How many times CI's failure went back to it.
    pub ci_rounds: u32,
    pub created_at: u64,
    pub updated_at: u64,
    /// What it is to its thread (M264), and the session it's a sibling or
    /// a review of.
    pub role: Role,
    pub parent_session: Option<String>,
    /// The agent asked for: `claude` or `codex`.
    pub harness: String,
    /// On a Try 3's parent: the session kept, who picked it, and when.
    pub picked: Option<String>,
    pub picked_by: Option<String>,
    pub picked_at: Option<u64>,
    /// When the mirror saw its PR merged (M265): its thread is done.
    pub merged_at: Option<u64>,
}

impl Session {
    /// The state its machine sees it in: an `unresponsive` one's is the
    /// state it was in.
    pub fn seen_as(&self) -> State {
        match (self.state, self.was) {
            (State::Unresponsive, Some(was)) => was,
            (s, _) => s,
        }
    }
}

/// One entry in a session's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub session: String,
    pub seq: u64,
    /// `state`, `error` or `turn` (a heartbeat leaves no event).
    pub kind: Kind,
    /// `None` for the first: the session made, as `queued`.
    pub from: Option<State>,
    pub to: State,
    pub at: u64,
    /// A JSON object's text, as the reporter wrote it (and signed it).
    pub detail: String,
    /// The machine that reported it, its report number and its signature
    /// over [`Event::signed`], hex; all `None` when control recorded it.
    pub daemon: Option<String>,
    pub daemon_n: Option<u64>,
    pub sig: Option<String>,
}

impl Event {
    /// What its machine signed (`None` when control recorded it).
    pub fn signed(&self) -> Option<arugula_e2e::session::Event<'_>> {
        Some(arugula_e2e::session::Event {
            session: &self.session,
            n: self.daemon_n?,
            kind: self.kind,
            from: self.from?,
            to: self.to,
            at: self.at,
            detail: &self.detail,
            daemon: self.daemon.as_deref()?,
        })
    }
}

/// Where a prompt's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A project member, in the thread: given to the agent plain.
    Member,
    /// GitHub (a review, a check's log): given fenced as quoted data, with
    /// its `author_association`.
    Github,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Member => "member",
            Source::Github => "github",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "member" => Some(Source::Member),
            "github" => Some(Source::Github),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptState {
    Queued,
    Sent,
    Dropped,
}

impl PromptState {
    pub fn as_str(self) -> &'static str {
        match self {
            PromptState::Queued => "queued",
            PromptState::Sent => "sent",
            PromptState::Dropped => "dropped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(PromptState::Queued),
            "sent" => Some(PromptState::Sent),
            "dropped" => Some(PromptState::Dropped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub id: String,
    pub session: String,
    /// An account (empty once it's deleted), or `github:<login>`.
    pub author: String,
    pub source: Source,
    /// GitHub's `author_association`, for GitHub's text.
    pub association: Option<String>,
    pub text: String,
    pub state: PromptState,
    pub created_at: u64,
    pub sent_at: Option<u64>,
    pub dropped_at: Option<u64>,
}

impl Prompt {
    /// As the machine is given it.
    pub fn wire(&self) -> SessionPrompt {
        SessionPrompt {
            id: self.id.clone(),
            author: self.author.clone(),
            // Filled by the pull (`work::work`): the hub knows names.
            author_name: String::new(),
            source: self.source.as_str().into(),
            association: self.association.clone(),
            text: self.text.clone(),
            at: self.created_at,
        }
    }
}

/// What an action asks of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Answer the open question.
    Answer,
    /// Allow the open permission, with one of its options.
    Approve,
    /// Refuse the open permission.
    Deny,
    /// Cancel the turn under way; the session goes on.
    Cancel,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Answer => "answer",
            ActionKind::Approve => "approve",
            ActionKind::Deny => "deny",
            ActionKind::Cancel => "cancel",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [ActionKind::Answer, ActionKind::Approve, ActionKind::Deny, ActionKind::Cancel]
            .into_iter()
            .find(|k| k.as_str() == s)
    }

    /// It answers the open request (a question or a permission).
    pub fn answers(self) -> bool {
        self != ActionKind::Cancel
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Pending,
    Done,
    Dropped,
}

impl ActionState {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionState::Pending => "pending",
            ActionState::Done => "done",
            ActionState::Dropped => "dropped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [ActionState::Pending, ActionState::Done, ActionState::Dropped].into_iter().find(|k| k.as_str() == s)
    }
}

/// Something a person asked of a session that only its box can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub session: String,
    /// The account that asked; empty once it's deleted.
    pub by: String,
    pub kind: ActionKind,
    /// The open request it answers; `None`: whichever is open.
    pub request: Option<String>,
    /// An answer's JSON object, as text.
    pub answer: Option<String>,
    /// An approval's option.
    pub option: Option<String>,
    /// A denial's why.
    pub reason: Option<String>,
    pub state: ActionState,
    pub at: u64,
    pub done_at: Option<u64>,
    /// The number of the machine's report that did it.
    pub done_n: Option<u64>,
}

impl Action {
    /// As the machine is given it.
    pub fn wire(&self) -> SessionAction {
        SessionAction {
            id: self.id.clone(),
            by: self.by.clone(),
            by_name: String::new(),
            kind: self.kind.as_str().into(),
            request: self.request.clone(),
            answer: self.answer.as_deref().and_then(|a| serde_json::from_str(a).ok()),
            option: self.option.clone(),
            reason: self.reason.clone(),
            at: self.at,
        }
    }
}

/// An action to ask.
#[derive(Debug, Clone, Copy)]
pub struct NewAction<'a> {
    pub by: &'a str,
    pub kind: ActionKind,
    /// The open request it answers, as the session's `needs_input` named
    /// it; `None`: whichever is open.
    pub request: Option<&'a str>,
    /// `answer`'s: a JSON object.
    pub answer: Option<&'a serde_json::Value>,
    /// `approve`'s: one of the permission's options.
    pub option: Option<&'a str>,
    /// `deny`'s: why, at most [`MAX_REASON`] characters.
    pub reason: Option<&'a str>,
}

/// The most a denial's reason holds, in characters.
pub const MAX_REASON: usize = 1000;

/// A prompt to queue.
#[derive(Debug, Clone, Copy)]
pub struct NewPrompt<'a> {
    /// An account, or `github:<login>` for [`Source::Github`].
    pub author: &'a str,
    pub source: Source,
    pub association: Option<&'a str>,
    pub text: &'a str,
}

/// A session to make, with its first prompt.
#[derive(Debug, Clone, Copy)]
pub struct NewSession<'a> {
    pub project: &'a str,
    /// Its thread.
    pub room: &'a str,
    pub parent_seq: u64,
    pub asker: &'a str,
    /// One of the project's live workspaces.
    pub workspace: &'a str,
    pub repo: Option<&'a str>,
    /// Its branch's slug ([`slug`]): the branch is [`branch_for`] its id
    /// and this.
    pub slug: &'a str,
    pub prompt: NewPrompt<'a>,
}

/// A report a session's machine made, its signature checked: only
/// [`DaemonReport::verify`] makes one, so storage is never handed a report
/// nobody checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonReport {
    session: String,
    daemon: String,
    n: u64,
    kind: Kind,
    from: State,
    to: State,
    at: u64,
    detail: String,
    sig: String,
}

impl DaemonReport {
    /// `report`, if `sig` (hex) is the signature over it of the machine it
    /// names, whose Ed25519 key (hex) is `sign_hex`. The caller finds that
    /// key for `report.daemon` in the hub's directory; whether that machine
    /// is the session's own, [`Db::move_session`] checks.
    pub fn verify(report: &arugula_e2e::session::Event, sign_hex: &str, sig: &str) -> Option<Self> {
        report.verify(sign_hex, sig).then(|| Self {
            session: report.session.into(),
            daemon: report.daemon.into(),
            n: report.n,
            kind: report.kind,
            from: report.from,
            to: report.to,
            at: report.at,
            detail: report.detail.into(),
            sig: sig.into(),
        })
    }
}

/// A change asked for.
#[derive(Debug, Clone, Copy)]
pub enum Move<'a> {
    /// Control records it: `failed` (with a `reason`), `stopped`,
    /// `unresponsive` (as [`Db::mark_unresponsive`] does), or out of
    /// `unresponsive` to the state it was in. `from` is the state control
    /// saw it in: a move from any other is refused. `detail` is a JSON
    /// object's text, at most [`MAX_DETAIL`] bytes.
    Control { from: State, to: State, at: u64, detail: &'a str },
    /// The session's machine reported it, signed; `now` is when the
    /// platform heard it, by its own clock.
    Daemon { report: &'a DaemonReport, now: u64 },
}

/// Why a move, or a prompt, was refused. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// There's no such session.
    NoSession,
    /// The session isn't in the state the move is from (another move won,
    /// or it ended). For the machine, the state it would see.
    Moved { now: State },
    /// A session can't move so.
    Disallowed { from: State, to: State },
    /// Not a move this reporter may make.
    Reporter,
    /// A report from another machine than the session's, or about another
    /// session.
    OtherDaemon,
    /// A machine's report that isn't its next.
    Seq { next: u64 },
    /// The box holds [`MAX_SESSIONS_PER_BOX`] sessions: later.
    Busy,
    /// The detail isn't a JSON object of at most [`MAX_DETAIL`] bytes, or
    /// doesn't fit the move.
    Detail(&'static str),
    /// A prompt that can't be queued as it is.
    Prompt(&'static str),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NoSession => f.write_str("no such session"),
            Refused::Moved { now } => write!(f, "the session is {now} now"),
            Refused::Disallowed { from, to } => write!(f, "a session can't go from {from} to {to}"),
            Refused::Reporter => f.write_str("not a move this reporter may make"),
            Refused::OtherDaemon => f.write_str("the report isn't from this session's machine"),
            Refused::Seq { next } => write!(f, "the machine's next report is {next}"),
            Refused::Busy => write!(f, "the box holds {MAX_SESSIONS_PER_BOX} sessions"),
            Refused::Detail(why) | Refused::Prompt(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Refused {}

/// Where a project's agents run (its setting "agents run on").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsOn {
    /// The workspace the project chose.
    Chosen(String),
    /// None chosen, and the project has one workspace: that one.
    Only(String),
    /// None chosen, and the project has several: the thread is asked
    /// (M256), choosing from these.
    Ask(Vec<String>),
    /// The project has no workspace.
    Nowhere,
}

impl AgentsOn {
    /// The workspace a session goes to, when that's settled.
    pub fn workspace(&self) -> Option<&str> {
        match self {
            AgentsOn::Chosen(w) | AgentsOn::Only(w) => Some(w),
            AgentsOn::Ask(_) | AgentsOn::Nowhere => None,
        }
    }
}

pub(super) const COLS: &str = "id, project_id, room_id, parent_seq, asker, workspace_id, machine, block, repo, \
                               branch, state, was_state, last_seq, daemon_n, heard_at, last_tool, last_error, pr, \
                               ci_rounds, created_at, updated_at, role, parent_session, harness, picked, \
                               picked_by, picked_at, merged_at";
const EVENT_COLS: &str = "session_id, seq, kind, from_state, to_state, at, detail, daemon, daemon_n, sig";
const PROMPT_COLS: &str = "id, session_id, author, source, association, text, state, created_at, sent_at, dropped_at";
const ACTION_COLS: &str =
    "id, session_id, by_account, kind, request, answer, option_id, reason, state, at, done_at, done_n";

fn state(s: &str) -> Result<State> {
    State::parse(s).with_context(|| format!("a session in state {s:?}"))
}

pub(super) fn session_row(r: &Row) -> Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        project: r.get(1)?,
        room: r.get(2)?,
        parent_seq: r.get(3)?,
        asker: r.get(4)?,
        workspace: r.get(5)?,
        machine: r.get(6)?,
        block: r.get(7)?,
        repo: r.get(8)?,
        branch: r.get(9)?,
        state: state(&r.get::<String>(10)?)?,
        was: r.get::<Option<String>>(11)?.as_deref().map(state).transpose()?,
        last_seq: r.get(12)?,
        daemon_n: r.get(13)?,
        heard_at: r.get(14)?,
        last_tool: r.get(15)?,
        last_error: r.get(16)?,
        pr: r.get(17)?,
        ci_rounds: r.get(18)?,
        created_at: r.get(19)?,
        updated_at: r.get(20)?,
        role: {
            let role: String = r.get(21)?;
            Role::parse(&role).with_context(|| format!("a session of role {role:?}"))?
        },
        parent_session: r.get(22)?,
        harness: r.get(23)?,
        picked: r.get(24)?,
        picked_by: r.get(25)?,
        picked_at: r.get(26)?,
        merged_at: r.get(27)?,
    })
}

fn event_row(r: &Row) -> Result<Event> {
    let kind: String = r.get(2)?;
    Ok(Event {
        session: r.get(0)?,
        seq: r.get(1)?,
        kind: Kind::parse(&kind).with_context(|| format!("a session event of kind {kind:?}"))?,
        from: r.get::<Option<String>>(3)?.as_deref().map(state).transpose()?,
        to: state(&r.get::<String>(4)?)?,
        at: r.get(5)?,
        detail: r.get(6)?,
        daemon: r.get(7)?,
        daemon_n: r.get(8)?,
        sig: r.get(9)?,
    })
}

fn prompt_row(r: &Row) -> Result<Prompt> {
    let (source, st): (String, String) = (r.get(3)?, r.get(6)?);
    Ok(Prompt {
        id: r.get(0)?,
        session: r.get(1)?,
        author: r.get(2)?,
        source: Source::parse(&source).with_context(|| format!("a prompt from {source:?}"))?,
        association: r.get(4)?,
        text: r.get(5)?,
        state: PromptState::parse(&st).with_context(|| format!("a prompt in state {st:?}"))?,
        created_at: r.get(7)?,
        sent_at: r.get(8)?,
        dropped_at: r.get(9)?,
    })
}

fn action_row(r: &Row) -> Result<Action> {
    let (kind, st): (String, String) = (r.get(3)?, r.get(8)?);
    Ok(Action {
        id: r.get(0)?,
        session: r.get(1)?,
        by: r.get(2)?,
        kind: ActionKind::parse(&kind).with_context(|| format!("a session action of kind {kind:?}"))?,
        request: r.get(4)?,
        answer: r.get(5)?,
        option: r.get(6)?,
        reason: r.get(7)?,
        state: ActionState::parse(&st).with_context(|| format!("a session action in state {st:?}"))?,
        at: r.get(9)?,
        done_at: r.get(10)?,
        done_n: r.get(11)?,
    })
}

fn pending_actions(c: &impl Sql, session: &str) -> Result<Vec<Action>> {
    c.query_map(
        &format!(
            "SELECT {ACTION_COLS} FROM session_actions WHERE session_id = ?1 AND state = 'pending' ORDER BY at, id"
        ),
        params![session],
        action_row,
    )
}

/// The latest move to `needs_input`: what it asked.
fn last_ask(c: &impl Sql, session: &str) -> Result<Option<Event>> {
    c.query_row(
        &format!(
            "SELECT {EVENT_COLS} FROM session_events WHERE session_id = ?1 AND kind = 'state' AND to_state = 'needs_input'
             ORDER BY seq DESC LIMIT 1"
        ),
        params![session],
        event_row,
    )
    .optional()
}

fn add_event(c: &impl Sql, e: &Event) -> Result<()> {
    c.execute(
        &format!("INSERT INTO session_events ({EVENT_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"),
        params![
            e.session,
            e.seq,
            e.kind.as_str(),
            e.from.map(State::as_str),
            e.to.as_str(),
            e.at,
            e.detail,
            e.daemon,
            e.daemon_n,
            e.sig
        ],
    )?;
    Ok(())
}

/// `detail` as a [`SessionDetail`], if it's a JSON object of at most
/// [`MAX_DETAIL`] bytes.
fn detail_of(detail: &str) -> Result<SessionDetail, Refused> {
    if detail.len() > MAX_DETAIL {
        return Err(Refused::Detail("the detail is too long"));
    }
    match serde_json::from_str::<serde_json::Value>(detail) {
        Ok(v @ serde_json::Value::Object(_)) => {
            serde_json::from_value(v).map_err(|_| Refused::Detail("the detail's fields aren't what they should be"))
        }
        _ => Err(Refused::Detail("the detail isn't a JSON object")),
    }
}

fn said(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| !s.trim().is_empty())
}

/// `s`, cut to at most `max` characters, the last an ellipsis when cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// The queued prompts of `session` among `ids`, each once: refused when
/// one isn't.
fn queued_among(c: &impl Sql, session: &str, ids: &[String]) -> Result<Result<(), Refused>> {
    for (i, id) in ids.iter().enumerate() {
        if ids[..i].contains(id) {
            return Ok(Err(Refused::Detail("a prompt is named twice")));
        }
        let queued: Option<String> = c
            .query_row(
                "SELECT id FROM session_prompts WHERE id = ?1 AND session_id = ?2 AND state = 'queued'",
                params![id, session],
                |r| r.get(0),
            )
            .optional()?;
        if queued.is_none() {
            return Ok(Err(Refused::Detail("a prompt it names isn't queued in this session")));
        }
    }
    Ok(Ok(()))
}

/// How many sessions on `workspace` other than `but` hold a slot on its
/// box: taken and not ended, a done one too.
fn under_way(c: &impl Sql, workspace: &str, but: &str) -> Result<usize> {
    let n: u64 = c.query_row(
        "SELECT count(*) FROM agent_sessions
         WHERE workspace_id = ?1 AND id <> ?2 AND state IN ('working', 'needs_input', 'unresponsive', 'done')",
        params![workspace, but],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

/// Whether a machine's report fits its move, against the session as it
/// is: `Ok` with the prompts a turn starts with.
fn check_daemon(c: &impl Sql, s: &Session, r: &DaemonReport) -> Result<Result<Vec<String>, Refused>> {
    let d = match detail_of(&r.detail) {
        Ok(d) => d,
        Err(e) => return Ok(Err(e)),
    };
    let refuse = |why| Ok(Err(Refused::Detail(why)));
    if !d.actions.is_empty() {
        let pending = pending_actions(c, &s.id)?;
        for (i, a) in d.actions.iter().enumerate() {
            if d.actions[..i].contains(a) {
                return refuse("an action is named twice");
            }
            if !pending.iter().any(|p| p.id == *a) {
                return refuse("an action it names isn't pending in this session");
            }
        }
    }
    match r.kind {
        Kind::State => {
            let (from, to) = (r.from, r.to);
            if !from.may_become(to) {
                return Ok(Err(Refused::Disallowed { from, to }));
            }
            if !from.daemon_may(to) {
                return Ok(Err(Refused::Reporter));
            }
            // A turn someone started in the session's block itself (its
            // Open button) names no prompts: a done session's next only.
            if d.in_block && (from != State::Done || to != State::Working || !d.prompts.is_empty()) {
                return refuse("only a done session's next turn starts in its block, naming no prompts");
            }
            match to {
                State::Working => {
                    let starts = matches!(from, State::Queued | State::Done);
                    if starts && d.prompts.is_empty() && !d.in_block {
                        return refuse("a turn starts with the queued prompts it names");
                    }
                    if d.by.as_deref().is_some_and(|b| b.chars().count() > 200) {
                        return refuse("who started a turn is at most 200 characters");
                    }
                    if from == State::Queued && !d.block.as_deref().is_some_and(word) {
                        return refuse("taking a session names its agent block");
                    }
                    if d.harness.as_deref().is_some_and(|h| from != State::Queued || !is_harness(h)) {
                        return refuse("taking a session names the agent it runs: claude or codex");
                    }
                    if let Err(e) = queued_among(c, &s.id, &d.prompts)? {
                        return Ok(Err(e));
                    }
                    if starts && under_way(c, &s.workspace, &s.id)? >= MAX_SESSIONS_PER_BOX {
                        return Ok(Err(Refused::Busy));
                    }
                }
                State::Failed if !said(&d.reason) => return refuse("a failed session says why (reason)"),
                _ if !d.prompts.is_empty() => return refuse("only a turn's start names prompts"),
                _ => {}
            }
            Ok(Ok(d.prompts))
        }
        // A tool's title is the agent's (a Bash call's is its command line):
        // a long one is cut when it's kept, never refused.
        Kind::Heartbeat => Ok(Ok(vec![])),
        Kind::Error if !said(&d.error) => refuse("an error says what (error)"),
        Kind::Error => Ok(Ok(vec![])),
        Kind::Turn => {
            if d.branch.as_deref() != Some(s.branch.as_str()) {
                return refuse("a turn names the session's own branch");
            }
            if d.pushed.is_none() {
                return refuse("a turn says whether it pushed (pushed)");
            }
            if s.role == Role::Review && d.pushed == Some(true) {
                return refuse("a review pushes nothing");
            }
            if d.reply.is_some() && s.role != Role::Review {
                return refuse("only a review's turn carries a reply");
            }
            Ok(Ok(vec![]))
        }
    }
}

/// The session's own machine, as the report must name it: the one it's
/// bound to, or before its first report its workspace's machine now.
fn own_machine(c: &impl Sql, s: &Session) -> Result<Option<String>> {
    if let Some(m) = &s.machine {
        return Ok(Some(m.clone()));
    }
    Ok(c.query_row("SELECT machine FROM hosted_workspaces WHERE id = ?1", params![s.workspace], |r| {
        r.get::<Option<String>>(0)
    })
    .optional()?
    .flatten())
}

/// [`Db::create_session`] in the caller's transaction.
pub fn create_session_in(c: &impl Sql, new: &NewSession, at: u64) -> Result<Option<(Session, Prompt)>> {
    make_in(c, new, Role::Thread, None, "claude", at)
}

/// Make a session of `role` under `parent` (none for a thread's own), on
/// `harness`: [`create_session_in`]'s rules.
fn make_in(
    c: &impl Sql,
    new: &NewSession,
    role: Role,
    parent: Option<&str>,
    harness: &str,
    at: u64,
) -> Result<Option<(Session, Prompt)>> {
    if !is_harness(harness) {
        bail!("a session runs on claude or codex");
    }
    if (role == Role::Thread) != parent.is_none() {
        bail!("a sibling or a review has a parent session, and a thread's own has none");
    }
    if !is_slug(new.slug) {
        bail!("a session's slug is lowercase letters, digits and single dashes, at most {MAX_SLUG}");
    }
    if new.asker.is_empty() {
        bail!("a session has an asker");
    }
    // The branch has only 8 hex of the id: one the project's sessions
    // already have is drawn again.
    let (id, branch) = loop {
        let id = new_id("ses");
        let branch = branch_for(&id, new.slug);
        let used = c
            .query_row(
                "SELECT 1 FROM agent_sessions WHERE project_id = ?1 AND branch = ?2",
                params![new.project, branch],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !used {
            break (id, branch);
        }
    };
    let made = c
        .query_row(
            &format!(
                "INSERT INTO agent_sessions (id, project_id, room_id, parent_seq, asker, workspace_id, repo, branch,
                     state, last_seq, created_at, updated_at, role, parent_session, harness)
                 SELECT ?1, p.id, ?2, ?3, ?4, w.id, ?5, ?6, 'queued', 1, ?7, ?7, ?10, ?11, ?12
                 FROM projects p JOIN hosted_workspaces w ON w.project_id = p.id
                 WHERE p.id = ?8 AND w.id = ?9 AND w.deleted_at IS NULL AND w.purpose = 'workspace'
                 RETURNING {COLS}"
            ),
            params![
                id,
                new.room,
                new.parent_seq,
                new.asker,
                new.repo,
                branch,
                at,
                new.project,
                new.workspace,
                role.as_str(),
                parent,
                harness
            ],
            session_row,
        )
        .optional()
        .map_err(taken)?;
    let Some(session) = made else { return Ok(None) };
    add_event(
        c,
        &Event {
            session: session.id.clone(),
            seq: 1,
            kind: Kind::State,
            from: None,
            to: State::Queued,
            at,
            detail: "{}".into(),
            daemon: None,
            daemon_n: None,
            sig: None,
        },
    )?;
    let prompt = match queue_in(c, &session, &new.prompt, at)? {
        Ok(p) => p,
        Err(e) => bail!("{e}"),
    };
    Ok(Some((session, prompt)))
}

/// The slug of session `s`'s branch (`arugula/<8 hex>-<slug>`).
pub fn slug_of(s: &Session) -> &str {
    let short = s.id.strip_prefix("ses_").unwrap_or(&s.id);
    let short = &short[..short.len().min(BRANCH_HEX)];
    s.branch.strip_prefix("arugula/").and_then(|b| b.strip_prefix(short)).map_or("", |r| r.trim_start_matches('-'))
}

/// A session's first prompt: what its siblings re-run.
fn first_prompt_of(c: &impl Sql, session: &str) -> Result<Option<Prompt>> {
    c.query_row(
        &format!("SELECT {PROMPT_COLS} FROM session_prompts WHERE session_id = ?1 ORDER BY created_at, id LIMIT 1"),
        params![session],
        prompt_row,
    )
    .optional()
}

fn siblings_in(c: &impl Sql, parent: &str) -> Result<Vec<Session>> {
    c.query_map(
        &format!(
            "SELECT {COLS} FROM agent_sessions WHERE parent_session = ?1 AND role = 'try' ORDER BY created_at, id"
        ),
        params![parent],
        session_row,
    )
}

/// Try 3 (M264) of session `parent`, in the caller's transaction: two
/// siblings, one on each of `harnesses`, each re-running its first prompt
/// on its thread, workspace and repo, as its asker (so their turns are
/// the asker's), queued with their jobs. Refused unless it's a thread's
/// own session that hasn't ended and has no tries yet.
pub fn start_tries_in(
    c: &impl Sql,
    parent: &str,
    harnesses: &[&str],
    at: u64,
) -> Result<Result<Vec<Session>, Refused>> {
    let Some(p) = session_for_update(c, parent)? else { return Ok(Err(Refused::NoSession)) };
    if harnesses.len() != TRIES - 1 || !harnesses.iter().all(|h| is_harness(h)) {
        return Ok(Err(Refused::Detail("a Try 3 runs two more: claude or codex, each")));
    }
    if p.role != Role::Thread {
        return Ok(Err(Refused::Detail("only a thread's own session is tried three ways")));
    }
    if p.state.terminal() {
        return Ok(Err(Refused::Moved { now: p.state }));
    }
    if !siblings_in(c, &p.id)?.is_empty() {
        return Ok(Err(Refused::Detail("it's being tried three ways already")));
    }
    let Some(first) = first_prompt_of(c, &p.id)?.filter(|f| !f.author.is_empty() && !f.text.trim().is_empty()) else {
        return Ok(Err(Refused::Prompt("its first prompt is gone")));
    };
    let mut out = Vec::new();
    for (i, h) in harnesses.iter().enumerate() {
        let new = NewSession {
            project: &p.project,
            room: &p.room,
            parent_seq: p.parent_seq,
            asker: &p.asker,
            workspace: &p.workspace,
            repo: p.repo.as_deref(),
            slug: slug_of(&p),
            prompt: NewPrompt {
                author: &first.author,
                source: first.source,
                association: first.association.as_deref(),
                text: &first.text,
            },
        };
        // A millisecond apart, so they're listed in the order asked.
        let Some((sib, _)) = make_in(c, &new, Role::Try, Some(&p.id), h, at + i as u64)? else {
            return Ok(Err(Refused::Detail("its workspace is gone")));
        };
        dispatch(c, &sib.id, at)?;
        out.push(sib);
    }
    Ok(Ok(out))
}

/// Keep `picked` of session `parent`'s Try 3 (M264), in the caller's
/// transaction: `parent` says so, and the others that haven't ended stop
/// ([`NOT_PICKED`]), as control. The parent after, and the ones it
/// stopped. Refused unless `parent` has tries, none is picked yet, and
/// `picked` is one of them that hasn't ended.
pub fn pick_in(
    c: &impl Sql,
    parent: &str,
    picked: &str,
    by: &str,
    at: u64,
) -> Result<Result<(Session, Vec<String>), Refused>> {
    let Some(p) = session_for_update(c, parent)? else { return Ok(Err(Refused::NoSession)) };
    let siblings = siblings_in(c, &p.id)?;
    if siblings.is_empty() {
        return Ok(Err(Refused::Detail("it isn't being tried three ways")));
    }
    if p.picked.is_some() {
        return Ok(Err(Refused::Detail("one of its tries is kept already")));
    }
    let mut all = vec![p.clone()];
    all.extend(siblings);
    let Some(keep) = all.iter().find(|s| s.id == picked) else {
        return Ok(Err(Refused::Detail("that isn't one of its tries")));
    };
    if keep.state.terminal() {
        return Ok(Err(Refused::Moved { now: keep.state }));
    }
    c.execute(
        "UPDATE agent_sessions SET picked = ?2, picked_by = ?3, picked_at = ?4, updated_at = ?4 WHERE id = ?1",
        params![p.id, picked, by, at],
    )?;
    let detail = serde_json::json!({ "reason": NOT_PICKED, "picked": picked }).to_string();
    let mut stopped = Vec::new();
    for s in all.iter().filter(|s| s.id != picked && !s.state.terminal()) {
        let m = Move::Control { from: s.state, to: State::Stopped, at, detail: &detail };
        match move_session_in(c, &s.id, &m)? {
            Ok(_) => stopped.push(s.id.clone()),
            Err(e) => return Ok(Err(e)),
        }
    }
    let after = session_for_update(c, &p.id)?.context("the session it picked in")?;
    Ok(Ok((after, stopped)))
}

/// A review (M264, `@codex review`) of session `of`, asked by `asker`, on
/// `harness`, with `prompt` (the diff and the thread, fenced): a session
/// under it, on its thread, workspace and repo, queued with its job.
pub fn create_review_in(
    c: &impl Sql,
    of: &Session,
    asker: &str,
    harness: &str,
    prompt: &str,
    at: u64,
) -> Result<Option<(Session, Prompt)>> {
    let new = NewSession {
        project: &of.project,
        room: &of.room,
        parent_seq: of.parent_seq,
        asker,
        workspace: &of.workspace,
        repo: of.repo.as_deref(),
        slug: "review",
        prompt: NewPrompt { author: asker, source: Source::Member, association: None, text: prompt },
    };
    let made = make_in(c, &new, Role::Review, Some(&of.id), harness, at)?;
    if let Some((s, _)) = &made {
        dispatch(c, &s.id, at)?;
    }
    Ok(made)
}

/// Follow session `id` with its `session` job on `platform_jobs`, queued in
/// the caller's transaction beside the session. The job's id is the
/// session's, so asking twice queues one: `false` when it was there.
pub fn dispatch(c: &impl Sql, id: &str, now: u64) -> Result<bool> {
    super::jobs::enqueue(c, id, super::jobs::JobKind::Session, &serde_json::json!({ "session": id }), now, now)
}

pub(super) fn queue_in(c: &impl Sql, s: &Session, p: &NewPrompt, at: u64) -> Result<Result<Prompt, Refused>> {
    if s.state.terminal() {
        return Ok(Err(Refused::Moved { now: s.state }));
    }
    if p.text.trim().is_empty() || p.text.len() > MAX_PROMPT {
        return Ok(Err(Refused::Prompt("a prompt has text, at most 32 KiB")));
    }
    let github = p.author.strip_prefix("github:").is_some_and(|l| !l.is_empty());
    match p.source {
        Source::Member if p.author.is_empty() || github || p.association.is_some() => {
            return Ok(Err(Refused::Prompt("a member's prompt names their account, and no association")));
        }
        Source::Github if !github || !p.association.is_some_and(word) => {
            return Ok(Err(Refused::Prompt("GitHub's text names its author (github:<login>) and association")));
        }
        _ => {}
    }
    let prompt = c.query_row(
        &format!(
            "INSERT INTO session_prompts (id, session_id, author, source, association, text, state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7) RETURNING {PROMPT_COLS}"
        ),
        params![new_id("spr"), s.id, p.author, p.source.as_str(), p.association, p.text, at],
        prompt_row,
    )?;
    Ok(Ok(prompt))
}

/// Why a session is queued again on its workspace's new box.
pub const BOX_REBUILT: &str = "box_rebuilt: the box was rebuilt; starting again from the thread";
/// ...when the old box archived no transcript of its conversation
/// (Arugula #209: it was already gone, or didn't answer in time), so the
/// agent starts again without it.
pub const BOX_REBUILT_LOST: &str = "box_rebuilt: the box was rebuilt, and the agent's conversation was lost with it \
                                    (the old box sent no transcript); starting again from the thread";

/// Session `id`, last seen `from`, moves to its workspace's new box
/// `machine` (M258: the box was rebuilt) in the caller's transaction. A
/// session no box took yet only follows the workspace's machine. One that
/// was taken is queued again: control's move, saying [`BOX_REBUILT`], or
/// [`BOX_REBUILT_LOST`] when its conversation was `lost` (no transcript
/// came from the old box). Its block was the old box's and its actions
/// asked of that agent, so both go; its queued prompts are dropped for
/// `prompt` (the thread hydrated again, which holds them), which the new
/// box's first turn starts with.
/// The box's report numbers go on from where they were.
pub fn rebuilt_in(
    c: &impl Sql,
    id: &str,
    from: State,
    machine: &str,
    prompt: &NewPrompt,
    lost: bool,
    at: u64,
) -> Result<Result<Session, Refused>> {
    if from == State::Queued {
        let Some(s) = session_for_update(c, id)? else { return Ok(Err(Refused::NoSession)) };
        if s.state != State::Queued {
            return Ok(Err(Refused::Moved { now: s.state }));
        }
        c.execute("UPDATE agent_sessions SET machine = ?2 WHERE id = ?1", params![id, machine])?;
        return Ok(Ok(session_for_update(c, id)?.context("the session it moved")?));
    }
    let reason = if lost { BOX_REBUILT_LOST } else { BOX_REBUILT };
    let detail = serde_json::json!({ "reason": reason, "machine": machine }).to_string();
    let s = match move_session_in(c, id, &Move::Control { from, to: State::Queued, at, detail: &detail })? {
        Ok((s, _)) => s,
        Err(e) => return Ok(Err(e)),
    };
    c.execute("UPDATE agent_sessions SET machine = ?2, block = NULL WHERE id = ?1", params![id, machine])?;
    c.execute(
        "UPDATE session_prompts SET state = 'dropped', dropped_at = ?2 WHERE session_id = ?1 AND state = 'queued'",
        params![id, at],
    )?;
    c.execute(
        "UPDATE session_actions SET state = 'dropped', done_at = ?2 WHERE session_id = ?1 AND state = 'pending'",
        params![id, at],
    )?;
    if let Err(e) = queue_in(c, &s, prompt, at)? {
        return Ok(Err(e));
    }
    Ok(Ok(session_for_update(c, id)?.context("the session it moved")?))
}

pub(super) fn session_for_update(c: &impl Sql, id: &str) -> Result<Option<Session>> {
    c.query_row(&format!("SELECT {COLS} FROM agent_sessions WHERE id = ?1{}", c.for_update()), params![id], session_row)
        .optional()
}

/// Write a move's new state and its event (`None` for a heartbeat), and
/// what goes with it: an end drops the queued prompts.
fn write_state(c: &impl Sql, s: &Session, to: State, was: Option<State>, event: Option<&Event>, at: u64) -> Result<()> {
    if let Some(e) = event {
        add_event(c, e)?;
    }
    c.execute(
        "UPDATE agent_sessions SET state = ?2, was_state = ?3, last_seq = ?4, updated_at = ?5,
             last_tool = CASE WHEN ?6 THEN NULL ELSE last_tool END
         WHERE id = ?1",
        params![s.id, to.as_str(), was.map(State::as_str), event.map_or(s.last_seq, |e| e.seq), at, !to.watched()],
    )?;
    if to.terminal() {
        c.execute(
            "UPDATE session_prompts SET state = 'dropped', dropped_at = ?2 WHERE session_id = ?1 AND state = 'queued'",
            params![s.id, at],
        )?;
        c.execute(
            "UPDATE session_actions SET state = 'dropped', done_at = ?2 WHERE session_id = ?1 AND state = 'pending'",
            params![s.id, at],
        )?;
    }
    Ok(())
}

/// [`Db::move_session`] in a transaction the caller holds and commits:
/// the session after it, and the events it wrote (none for a heartbeat;
/// two when a report brought an `unresponsive` session back).
pub fn move_session_in(c: &impl Sql, id: &str, m: &Move) -> Result<Result<(Session, Vec<Event>), Refused>> {
    let Some(s) = session_for_update(c, id)? else { return Ok(Err(Refused::NoSession)) };
    match m {
        Move::Control { from, to, at, detail } => {
            let (from, to) = (*from, *to);
            if s.state != from {
                return Ok(Err(Refused::Moved { now: s.state }));
            }
            if !from.may_become(to) {
                return Ok(Err(Refused::Disallowed { from, to }));
            }
            if !from.control_may(to) {
                return Ok(Err(Refused::Reporter));
            }
            if from == State::Unresponsive && Some(to) != s.was && !to.terminal() && to != State::Queued {
                return Ok(Err(Refused::Disallowed { from, to }));
            }
            let d = match detail_of(detail) {
                Ok(d) => d,
                Err(e) => return Ok(Err(e)),
            };
            if to == State::Failed && !said(&d.reason) {
                return Ok(Err(Refused::Detail("control fails a session with a reason")));
            }
            let e = Event {
                session: s.id.clone(),
                seq: s.last_seq + 1,
                kind: Kind::State,
                from: Some(from),
                to,
                at: *at,
                detail: detail.to_string(),
                daemon: None,
                daemon_n: None,
                sig: None,
            };
            let was = (to == State::Unresponsive).then_some(from);
            write_state(c, &s, to, was, Some(&e), *at)?;
            let after = session_for_update(c, id)?.context("the session it moved")?;
            Ok(Ok((after, vec![e])))
        }
        Move::Daemon { report: r, now } => {
            if r.session != s.id || own_machine(c, &s)?.as_deref() != Some(r.daemon.as_str()) {
                return Ok(Err(Refused::OtherDaemon));
            }
            if r.n <= s.daemon_n {
                return Ok(Err(Refused::Seq { next: s.daemon_n + 1 }));
            }
            let seen = s.seen_as();
            if s.state.terminal() || r.from != seen {
                return Ok(Err(Refused::Moved { now: seen }));
            }
            let prompts = match check_daemon(c, &s, r)? {
                Ok(p) => p,
                Err(e) => return Ok(Err(e)),
            };
            // Everything's checked: from here on it's written.
            let mut written = Vec::new();
            let mut s = s;
            if s.state == State::Unresponsive {
                let back = Event {
                    session: s.id.clone(),
                    seq: s.last_seq + 1,
                    kind: Kind::State,
                    from: Some(State::Unresponsive),
                    to: seen,
                    at: *now,
                    detail: serde_json::json!({ "heard": r.n }).to_string(),
                    daemon: None,
                    daemon_n: None,
                    sig: None,
                };
                write_state(c, &s, seen, None, Some(&back), *now)?;
                s.last_seq = back.seq;
                written.push(back);
            }
            let d = detail_of(&r.detail).unwrap_or_default();
            let event = (r.kind != Kind::Heartbeat).then(|| Event {
                session: s.id.clone(),
                seq: s.last_seq + 1,
                kind: r.kind,
                from: Some(r.from),
                to: r.to,
                at: r.at,
                detail: r.detail.clone(),
                daemon: Some(r.daemon.clone()),
                daemon_n: Some(r.n),
                sig: Some(r.sig.clone()),
            });
            write_state(c, &s, r.to, None, event.as_ref(), *now)?;
            c.execute(
                "UPDATE agent_sessions SET machine = ?2, daemon_n = ?3, heard_at = ?4,
                     block = COALESCE(?5, block),
                     last_tool = CASE WHEN ?6 THEN ?7 ELSE last_tool END,
                     last_error = COALESCE(?8, last_error)
                 WHERE id = ?1",
                params![
                    s.id,
                    r.daemon,
                    r.n,
                    *now,
                    d.block.filter(|_| r.kind == Kind::State && r.from == State::Queued),
                    r.kind == Kind::Heartbeat,
                    d.tool.map(|t| clip(&t, TOOL_MAX)),
                    d.error.filter(|_| r.kind == Kind::Error)
                ],
            )?;
            for p in &prompts {
                c.execute(
                    "UPDATE session_prompts SET state = 'sent', sent_at = ?2 WHERE id = ?1 AND state = 'queued'",
                    params![p, *now],
                )?;
            }
            for a in &d.actions {
                c.execute(
                    "UPDATE session_actions SET state = 'done', done_at = ?2, done_n = ?3 WHERE id = ?1 AND state = 'pending'",
                    params![a, *now, r.n],
                )?;
            }
            // What the answers it didn't name answered isn't open now.
            if r.kind == Kind::State && r.from == State::NeedsInput && !r.to.terminal() {
                c.execute(
                    "UPDATE session_actions SET state = 'dropped', done_at = ?2
                     WHERE session_id = ?1 AND state = 'pending' AND kind <> 'cancel'",
                    params![s.id, *now],
                )?;
            }
            written.extend(event);
            let after = session_for_update(c, id)?.context("the session it moved")?;
            Ok(Ok((after, written)))
        }
    }
}

impl Db {
    /// Make a session on one of `project`'s live workspaces, `queued`, with
    /// its first event (control's) and its first prompt. `None`: there's no
    /// such project, or no such workspace in it. A thread with a session
    /// going already is [`super::Taken`]. Its job is the caller's
    /// ([`dispatch`], M256).
    pub fn create_session(&self, new: &NewSession, at: u64) -> Result<Option<(Session, Prompt)>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let made = create_session_in(&tx, new, at)?;
        tx.commit()?;
        Ok(made)
    }

    pub fn session(&self, id: &str) -> Result<Option<Session>> {
        self.c()
            .query_row(&format!("SELECT {COLS} FROM agent_sessions WHERE id = ?1"), params![id], session_row)
            .optional()
    }

    /// The session going on a thread (one that hasn't ended), if any: the
    /// thread's own, or once a Try 3's sibling is picked, that one (M264).
    /// Its other tries and its reviews aren't the thread's.
    pub fn session_on_thread(&self, room: &str, parent_seq: u64) -> Result<Option<Session>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {COLS} FROM agent_sessions s
                     WHERE room_id = ?1 AND parent_seq = ?2 AND state NOT IN ('failed', 'stopped')
                       AND (parent_session IS NULL OR (role = 'try' AND EXISTS (
                           SELECT 1 FROM agent_sessions p WHERE p.id = s.parent_session AND p.picked = s.id)))
                     ORDER BY (parent_session IS NULL) DESC, created_at LIMIT 1"
                ),
                params![room, parent_seq],
                session_row,
            )
            .optional()
    }

    /// Session `parent`'s Try 3 siblings (M264), oldest first.
    pub fn session_siblings(&self, parent: &str) -> Result<Vec<Session>> {
        siblings_in(&self.c(), parent)
    }

    /// [`start_tries_in`] in a transaction of its own.
    pub fn start_tries(&self, parent: &str, harnesses: &[&str], at: u64) -> Result<Result<Vec<Session>, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let made = start_tries_in(&tx, parent, harnesses, at)?;
        if made.is_ok() {
            tx.commit()?;
        }
        Ok(made)
    }

    /// [`pick_in`] in a transaction of its own.
    pub fn pick_try(
        &self,
        parent: &str,
        picked: &str,
        by: &str,
        at: u64,
    ) -> Result<Result<(Session, Vec<String>), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let done = pick_in(&tx, parent, picked, by, at)?;
        if done.is_ok() {
            tx.commit()?;
        }
        Ok(done)
    }

    /// A project's latest sessions, newest first.
    pub fn project_sessions(&self, project: &str, limit: u32) -> Result<Vec<Session>> {
        self.c().query_map(
            &format!(
                "SELECT {COLS} FROM agent_sessions WHERE project_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2"
            ),
            params![project, limit],
            session_row,
        )
    }

    /// A session's history, oldest first.
    pub fn session_events(&self, session: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM session_events WHERE session_id = ?1 ORDER BY seq"),
            params![session],
            event_row,
        )
    }

    /// A session's prompts, oldest first.
    pub fn session_prompts(&self, session: &str) -> Result<Vec<Prompt>> {
        self.c().query_map(
            &format!("SELECT {PROMPT_COLS} FROM session_prompts WHERE session_id = ?1 ORDER BY created_at, id"),
            params![session],
            prompt_row,
        )
    }

    /// Move a session: its new state and the events saying so, in one
    /// transaction. `Err(Refused)` writes nothing.
    pub fn move_session(&self, id: &str, m: &Move) -> Result<Result<(Session, Vec<Event>), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let moved = move_session_in(&tx, id, m)?;
        if moved.is_ok() {
            tx.commit()?;
        }
        Ok(moved)
    }

    /// Queue a prompt in a session's thread: refused once the session
    /// ended, or when the prompt isn't one ([`NewPrompt`]).
    pub fn queue_prompt(&self, session: &str, p: &NewPrompt, at: u64) -> Result<Result<Prompt, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(s) = session_for_update(&tx, session)? else { return Ok(Err(Refused::NoSession)) };
        let queued = queue_in(&tx, &s, p, at)?;
        if queued.is_ok() {
            tx.commit()?;
        }
        Ok(queued)
    }

    /// Take back a prompt that's still queued: `None` when there's no such
    /// prompt or it was sent or dropped already.
    pub fn drop_prompt(&self, prompt: &str, at: u64) -> Result<Option<Prompt>> {
        self.c()
            .query_row(
                &format!(
                    "UPDATE session_prompts SET state = 'dropped', dropped_at = ?2 WHERE id = ?1 AND state = 'queued'
                     RETURNING {PROMPT_COLS}"
                ),
                params![prompt, at],
                prompt_row,
            )
            .optional()
    }

    /// Ask the session's box to do something (M257): refused once the
    /// session ended; an answer, approval or denial unless it waits on
    /// one (`needs_input`), for the request that's open, nobody answered it
    /// yet, and it fits what's open (an approval's option is one of the
    /// permission's); a cancel unless a turn is under way, or when one is
    /// asked already.
    pub fn add_session_action(&self, session: &str, a: &NewAction, at: u64) -> Result<Result<Action, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(s) = session_for_update(&tx, session)? else { return Ok(Err(Refused::NoSession)) };
        let seen = s.seen_as();
        let refuse = |why| Ok(Err(Refused::Detail(why)));
        if s.state.terminal() {
            return Ok(Err(Refused::Moved { now: s.state }));
        }
        if a.by.is_empty() {
            bail!("an action names who asked");
        }
        let pending = pending_actions(&tx, &s.id)?;
        if a.kind.answers() {
            if seen != State::NeedsInput {
                return Ok(Err(Refused::Moved { now: seen }));
            }
            let open = last_ask(&tx, &s.id)?.map(|e| detail_of(&e.detail).unwrap_or_default()).unwrap_or_default();
            if a.request.is_some_and(|r| open.request.as_deref() != Some(r)) {
                return refuse("that isn't the request that's open");
            }
            if pending.iter().any(|p| p.kind.answers()) {
                return refuse("someone answered it already");
            }
            let permission = open.ask.as_deref() == Some("permission");
            match a.kind {
                ActionKind::Answer if permission => return refuse("a permission is approved or denied, not answered"),
                ActionKind::Approve | ActionKind::Deny if !permission && open.ask.is_some() => {
                    return refuse("a question is answered, not approved or denied");
                }
                ActionKind::Approve
                    if a.option.is_some_and(|o| !open.options.is_empty() && !open.options.iter().any(|x| x == o)) =>
                {
                    return refuse("that isn't one of the permission's options");
                }
                _ => {}
            }
        } else if !matches!(seen, State::Working | State::NeedsInput) {
            return Ok(Err(Refused::Moved { now: seen }));
        } else if pending.iter().any(|p| p.kind == ActionKind::Cancel) {
            return refuse("cancelling the turn is asked already");
        }
        let answer = match (a.kind, a.answer) {
            (ActionKind::Answer, Some(v @ serde_json::Value::Object(_))) => {
                let text = v.to_string();
                if text.len() > MAX_DETAIL {
                    return refuse("the answer is too long");
                }
                Some(text)
            }
            (ActionKind::Answer, _) => return refuse("an answer is a JSON object"),
            (_, None) => None,
            (_, Some(_)) => return refuse("only an answer carries one"),
        };
        if a.option.is_some() && (a.kind != ActionKind::Approve || !a.option.is_some_and(word)) {
            return refuse("only an approval names an option, one word");
        }
        if a.reason.is_some()
            && (a.kind != ActionKind::Deny || a.reason.is_some_and(|r| r.chars().count() > MAX_REASON))
        {
            return refuse("only a denial says why, at most 1000 characters");
        }
        if a.request.is_some_and(|r| !word(r)) {
            return refuse("a request's id is one word");
        }
        let made = tx.query_row(
            &format!(
                "INSERT INTO session_actions (id, session_id, by_account, kind, request, answer, option_id, reason, state, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9) RETURNING {ACTION_COLS}"
            ),
            params![new_id("sac"), s.id, a.by, a.kind.as_str(), a.request, answer, a.option, a.reason, at],
            action_row,
        )?;
        tx.commit()?;
        Ok(Ok(made))
    }

    /// A session's actions, oldest first.
    pub fn session_actions(&self, session: &str) -> Result<Vec<Action>> {
        self.c().query_map(
            &format!("SELECT {ACTION_COLS} FROM session_actions WHERE session_id = ?1 ORDER BY at, id"),
            params![session],
            action_row,
        )
    }

    /// Mark `unresponsive` every session whose turn is under way and whose
    /// machine the platform hasn't heard from for [`QUIET_MS`] (since it
    /// was made or moved, before it ever reported): the ids it marked.
    pub fn mark_unresponsive(&self, now: u64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let quiet: Vec<(String, String, Option<u64>)> = tx.query_map(
            "SELECT id, state, heard_at FROM agent_sessions
             WHERE state IN ('working', 'needs_input') AND COALESCE(heard_at, updated_at) <= ?1",
            params![now.saturating_sub(QUIET_MS)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut marked = Vec::new();
        for (id, from, heard) in quiet {
            let detail = serde_json::json!({ "heard_at": heard }).to_string();
            let m = Move::Control { from: state(&from)?, to: State::Unresponsive, at: now, detail: &detail };
            if move_session_in(&tx, &id, &m)?.is_ok() {
                marked.push(id);
            }
        }
        tx.commit()?;
        Ok(marked)
    }

    /// Link the draft PR its asker sent (a URL or a mirrored item's
    /// `fi_` id): false when there's no such session.
    pub fn link_session_pr(&self, session: &str, pr: &str, at: u64) -> Result<bool> {
        Ok(self
            .c()
            .execute("UPDATE agent_sessions SET pr = ?2, updated_at = ?3 WHERE id = ?1", params![session, pr, at])?
            == 1)
    }

    /// Count one more round of CI's failure going back to a session: its
    /// rounds now, `None` when there's no such session.
    pub fn count_ci_round(&self, session: &str, at: u64) -> Result<Option<u32>> {
        self.c()
            .query_row(
                "UPDATE agent_sessions SET ci_rounds = ci_rounds + 1, updated_at = ?2 WHERE id = ?1 RETURNING ci_rounds",
                params![session, at],
                |r| r.get(0),
            )
            .optional()
    }

    /// Every session on `workspace` that hasn't ended, oldest first.
    pub fn live_sessions_on(&self, workspace: &str) -> Result<Vec<Session>> {
        self.c().query_map(
            &format!(
                "SELECT {COLS} FROM agent_sessions WHERE workspace_id = ?1 AND state NOT IN ('failed', 'stopped')
                 ORDER BY created_at, id"
            ),
            params![workspace],
            session_row,
        )
    }

    /// [`rebuilt_in`] in a transaction of its own.
    pub fn session_rebuilt(
        &self,
        id: &str,
        from: State,
        machine: &str,
        prompt: &NewPrompt,
        lost: bool,
        at: u64,
    ) -> Result<Result<Session, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let moved = rebuilt_in(&tx, id, from, machine, prompt, lost, at)?;
        if moved.is_ok() {
            tx.commit()?;
        }
        Ok(moved)
    }

    /// What a hosted machine on `workspace` pulls (M255): every session
    /// there that hasn't ended, oldest first, with its queued prompts, as
    /// the machine sees it. Queued ones only while the box has room
    /// ([`MAX_SESSIONS_PER_BOX`]), the oldest first.
    pub fn session_work(&self, workspace: &str) -> Result<Vec<SessionWork>> {
        let c = self.c();
        let live = c.query_map(
            &format!(
                "SELECT {COLS} FROM agent_sessions WHERE workspace_id = ?1 AND state NOT IN ('failed', 'stopped')
                 ORDER BY created_at, id"
            ),
            params![workspace],
            session_row,
        )?;
        // The workspace's finish step (M261), sent with each session.
        let finish = c
            .query_row("SELECT spec FROM hosted_workspaces WHERE id = ?1", params![workspace], |r| r.get::<String>(0))
            .optional()?
            .and_then(|spec| serde_json::from_str::<crate::hosted::Spec>(&spec).ok())
            .and_then(|spec| spec.finish_step());
        // Every live session but a queued one holds a slot, a done one too.
        let busy = live.iter().filter(|s| s.state != State::Queued);
        let mut room = MAX_SESSIONS_PER_BOX.saturating_sub(busy.count());
        let mut out = Vec::new();
        for s in live {
            if s.state == State::Queued {
                if room == 0 {
                    continue;
                }
                room -= 1;
            }
            let prompts = c.query_map(
                &format!(
                    "SELECT {PROMPT_COLS} FROM session_prompts WHERE session_id = ?1 AND state = 'queued'
                     ORDER BY created_at, id"
                ),
                params![s.id],
                prompt_row,
            )?;
            out.push(SessionWork {
                session: s.id.clone(),
                project: s.project.clone(),
                workspace: s.workspace.clone(),
                asker: s.asker.clone(),
                repo: s.repo.clone(),
                branch: s.branch.clone(),
                state: s.seen_as().as_str().into(),
                n: s.daemon_n,
                prompts: prompts.iter().map(Prompt::wire).collect(),
                actions: pending_actions(&c, &s.id)?.iter().map(Action::wire).collect(),
                // A review runs no finish step (M264).
                finish: finish.clone().filter(|_| s.role != Role::Review),
                harness: Some(s.harness.clone()),
                review: s.role == Role::Review,
            });
        }
        Ok(out)
    }

    /// The sessions with these ids (`ses_…`): each one's id, project and
    /// state. One statement; an id that isn't there isn't in the list.
    pub fn session_places(&self, ids: &[&str]) -> Result<Vec<(String, String, State)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let rows: Vec<(String, String, String)> = c.query_map(
            &format!("SELECT id, project_id, state FROM agent_sessions WHERE {}", super::chat::in_json(&c, "id", 1)),
            params![super::chat::json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        rows.into_iter().map(|(id, p, s)| Ok((id, p, state(&s)?))).collect()
    }

    /// Where `project`'s agents run: the workspace it chose while that's
    /// live, else its only workspace, else the thread is asked.
    pub fn agents_run_on(&self, project: &str) -> Result<AgentsOn> {
        let c = self.c();
        let chosen: Option<String> = c
            .query_row(
                "SELECT w.id FROM projects p JOIN hosted_workspaces w ON w.id = p.agents_workspace
                 WHERE p.id = ?1 AND w.project_id = p.id AND w.deleted_at IS NULL AND w.purpose = 'workspace'",
                params![project],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(w) = chosen {
            return Ok(AgentsOn::Chosen(w));
        }
        let mut all: Vec<String> = c.query_map(
            "SELECT id FROM hosted_workspaces
             WHERE project_id = ?1 AND deleted_at IS NULL AND purpose = 'workspace' ORDER BY name, id",
            params![project],
            |r| r.get(0),
        )?;
        Ok(match all.len() {
            0 => AgentsOn::Nowhere,
            1 => AgentsOn::Only(all.remove(0)),
            _ => AgentsOn::Ask(all),
        })
    }

    /// Choose the workspace `project`'s agents run on, or `None` for the
    /// default again: false when it isn't one of the project's live
    /// workspaces (or there's no such project), and nothing changed.
    pub fn set_agents_run_on(&self, project: &str, workspace: Option<&str>) -> Result<bool> {
        let c = self.c();
        let n = match workspace {
            None => c.execute("UPDATE projects SET agents_workspace = NULL WHERE id = ?1", params![project])?,
            Some(w) => c.execute(
                "UPDATE projects SET agents_workspace = ?2 WHERE id = ?1 AND EXISTS (
                     SELECT 1 FROM hosted_workspaces
                     WHERE id = ?2 AND project_id = ?1 AND deleted_at IS NULL AND purpose = 'workspace')",
                params![project, w],
            )?,
        };
        Ok(n == 1)
    }
}

/// What deleting an account takes from here, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// the sessions of projects that went, with their events, prompts, PR
/// drafts and feedback (M260), the account's own PR drafts (a sent PR stays on GitHub, and its
/// link on its session), and on the sessions that stay, the account's name as asker and its prompts'
/// and actions' text and name (as chat blanks its messages). Events keep the machine
/// that signed them: a signature names its signer.
///
/// The sessions go first: `session_events` refuses a delete while its
/// session is there.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted, now: u64) -> Result<()> {
    c.execute("DELETE FROM agent_sessions WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("DELETE FROM session_events WHERE session_id NOT IN (SELECT id FROM agent_sessions)", params![])?;
    c.execute("DELETE FROM session_prompts WHERE session_id NOT IN (SELECT id FROM agent_sessions)", params![])?;
    c.execute("DELETE FROM session_actions WHERE session_id NOT IN (SELECT id FROM agent_sessions)", params![])?;
    c.execute("DELETE FROM session_feedback WHERE session_id NOT IN (SELECT id FROM agent_sessions)", params![])?;
    c.execute(
        "DELETE FROM session_feedback_items WHERE feedback_id NOT IN (SELECT id FROM session_feedback)",
        params![],
    )?;
    c.execute(
        "DELETE FROM pr_drafts WHERE author = ?1 OR session_id NOT IN (SELECT id FROM agent_sessions)",
        params![d.account],
    )?;
    c.execute(
        "UPDATE session_actions SET by_account = '', answer = CASE WHEN answer IS NULL THEN NULL ELSE '{}' END,
             reason = NULL,
             state = CASE WHEN state = 'pending' THEN 'dropped' ELSE state END,
             done_at = CASE WHEN state = 'pending' THEN ?2 ELSE done_at END
         WHERE by_account = ?1",
        params![d.account, now],
    )?;
    c.execute("UPDATE agent_sessions SET asker = '' WHERE asker = ?1", params![d.account])?;
    c.execute("UPDATE agent_sessions SET picked_by = '' WHERE picked_by = ?1", params![d.account])?;
    c.execute(
        "UPDATE session_prompts SET author = '', text = '',
             state = CASE WHEN state = 'queued' THEN 'dropped' ELSE state END,
             dropped_at = CASE WHEN state = 'queued' THEN ?2 ELSE dropped_at END
         WHERE author = ?1 AND source = 'member'",
        params![d.account, now],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
