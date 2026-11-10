//! What daemons and control say about GitHub: the repositories a daemon
//! watches, where each stands, the pokes control relays from the App's
//! webhooks, and the installation token a daemon with no `gh` login asks for.
//!
//! The three relay messages travel as text frames on the relay socket,
//! which other subsystems share (`trust`, `guest.routes.ok`, ...). Each
//! carries its tag as a `t` field that serializes as today; a frame with
//! another `t` doesn't parse as one of these, and the receiver passes it on
//! as it always did. Receivers read each message leniently, as both sides
//! did before: see the notes on the fields.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A daemon asks for a GitHub installation token (`POST`),
/// [`GithubTokenRequest`], answered [`GithubToken`].
pub const GITHUB_TOKEN: &str = "/api/daemon/github/token";

/// A `t` that is always the same string: serializes as it, and parses only
/// from it.
macro_rules! tag {
    ($(#[$m:meta])* $name:ident = $s:literal) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str($s)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let t = String::deserialize(d)?;
                if t == $s { Ok($name) } else { Err(serde::de::Error::custom(format!("not a {}", $s))) }
            }
        }

        #[cfg(feature = "schema")]
        crate::const_schema!($name = $s);
    };
}

tag!(
    /// `"forge.watch"`.
    WatchTag = "forge.watch"
);
tag!(
    /// `"forge.watching"`.
    WatchingTag = "forge.watching"
);
tag!(
    /// `"forge.poke"`.
    PokeTag = "forge.poke"
);

/// `{"t": "forge.watch", "repos": […]}`, daemon to control: the github.com
/// repositories its forge blocks have open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ForgeWatch {
    pub t: WatchTag,
    #[serde(default)]
    pub repos: Vec<String>,
}

impl ForgeWatch {
    pub fn new(repos: Vec<String>) -> Self {
        ForgeWatch { t: WatchTag, repos }
    }
}

/// Whether a repository's events reach a daemon, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Standing {
    /// Read leniently by a daemon, which skips an entry without one.
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub live: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// `{"t": "forge.watching", "repos": [{repo, live, why?}]}`, control to
/// daemon: where each watched repository stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ForgeWatching {
    pub t: WatchingTag,
    #[serde(default)]
    pub repos: Vec<Standing>,
}

impl ForgeWatching {
    pub fn new(repos: Vec<Standing>) -> Self {
        ForgeWatching { t: WatchingTag, repos }
    }
}

fn github_com() -> String {
    "github.com".to_owned()
}

/// What control relays of a webhook event: no more than this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Poke {
    pub provider: String,
    /// A daemon takes `github.com` when it is left out.
    #[serde(default = "github_com")]
    pub host: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub delivery: String,
}

/// `{"t": "forge.poke", "poke": {…}}`, control to daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ForgePoke {
    pub t: PokeTag,
    pub poke: Poke,
}

impl ForgePoke {
    pub fn new(poke: Poke) -> Self {
        ForgePoke { t: PokeTag, poke }
    }
}

/// `POST /api/daemon/github/token`: the repository (`OWNER/NAME`) to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GithubTokenRequest {
    pub repo: String,
}

/// A read-only installation token, who it is for, and when it stops
/// working. Never logged. A daemon takes what it finds: no token is an error
/// of its own, a missing `login` is empty and a missing `expires_at` is ten
/// minutes.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GithubToken {
    #[serde(default)]
    pub token: String,
    /// RFC 3339, `2026-10-03T12:34:56Z`.
    #[serde(default)]
    pub expires_at: String,
    /// The account's GitHub login.
    #[serde(default)]
    pub login: String,
    /// The App's slug.
    #[serde(default)]
    pub app: String,
}

impl std::fmt::Debug for GithubToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GithubToken")
            .field("token", &"…")
            .field("expires_at", &self.expires_at)
            .field("login", &self.login)
            .field("app", &self.app)
            .finish()
    }
}

// ---------------------------------------------------------------- ops on a machine

tag!(
    /// `"ops"`.
    OpsTag = "ops"
);
tag!(
    /// `"op"`.
    OpTag = "op"
);
tag!(
    /// `"op.done"`.
    OpDoneTag = "op.done"
);

/// The typed ops this build's daemon does when control asks (M168).
pub const OPS: &[&str] = &["forge.send"];

/// The run ops (M211) every daemon of this build knows: one that isn't a
/// hosted machine answers them `cant`.
pub const RUN_OPS: &[&str] = &["runs.start", "runs.revoke"];

/// The hosted ops (Arugula #209) every daemon of this build knows: one
/// that isn't a hosted machine answers them `cant`.
pub const HOSTED_OPS: &[&str] = &["hosted.archive", "hosted.archive_blocks"];

/// The agents' op (Arugula #394) every daemon of this build knows.
pub const AGENT_OPS: &[&str] = &["agents.mention", "agents.sessions", "agents.grant_ask"];

/// `{"t": "ops", "ops": [...]}`, daemon to control, on connecting: the
/// [`DaemonOp`]s it knows. A daemon that never says knows none, and control
/// answers [`OpAnswer::Unknown`] for it without asking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Ops {
    pub t: OpsTag,
    #[serde(default)]
    pub ops: Vec<String>,
}

impl Ops {
    /// What this build knows.
    pub fn ours() -> Self {
        Ops {
            t: OpsTag,
            ops: OPS.iter().chain(RUN_OPS).chain(HOSTED_OPS).chain(AGENT_OPS).map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// `{"t": "op", "id": ..., "op": {...}}`, control to daemon: do this and
/// answer with an [`OpDone`] of the same `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OpAsk {
    pub t: OpTag,
    pub id: String,
    pub op: DaemonOp,
}

impl OpAsk {
    pub fn new(id: String, op: DaemonOp) -> Self {
        OpAsk { t: OpTag, id, op }
    }
}

/// `{"t": "op.done", "id": ..., "answer": {...}}`, daemon to control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OpDone {
    pub t: OpDoneTag,
    pub id: String,
    pub answer: OpAnswer,
}

impl OpDone {
    pub fn new(id: String, answer: OpAnswer) -> Self {
        OpDone { t: OpDoneTag, id, answer }
    }
}

/// An op control asks a machine for, by its name (`op`). Only the platform
/// asks, through the hub, and only for a person's own action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "op")]
pub enum DaemonOp {
    /// Send a draft on a linked repo as the machine's owner, with their own
    /// `gh` login, because the person pressed Send (M168).
    #[serde(rename = "forge.send")]
    ForgeSend(ForgeSend),
    /// Start under the project's standing grant, on a hosted machine
    /// (M211; [`crate::runs`]).
    #[serde(rename = "runs.start")]
    RunStart(Box<crate::runs::RunStart>),
    /// The project's standing grant was revoked (M211).
    #[serde(rename = "runs.revoke")]
    RunRevoke(crate::runs::RunRevoke),
    /// Archive every open agent block's transcript now, as each would be
    /// on closing (M184): the platform is about to rebuild the hosted box,
    /// which takes them with it (Arugula #209).
    #[serde(rename = "hosted.archive")]
    HostedArchive(crate::transcripts::ArchiveOpen),
    /// Archive just these agent blocks' transcripts now: one session moves
    /// to another box (Arugula #214, Move to…), and the box stays.
    #[serde(rename = "hosted.archive_blocks")]
    HostedArchiveBlocks(crate::transcripts::ArchiveBlocks),
    /// A teammate's message in a thread is for this machine's agents
    /// (Arugula #394): an `@<machine>`, or a reply to one of them. The
    /// poster holds `threads.post` there; the machine's owner may refuse
    /// them all (`cant`).
    #[serde(rename = "agents.mention")]
    AgentMention(AgentMention),
    /// Which of this machine's sessions a teammate's agent may mention
    /// (`@<machine>/<session>`, #577). The owner decides what is said
    /// (`sharing.json`'s `agent_sessions`).
    #[serde(rename = "agents.sessions")]
    MachineAgents(MachineAgentsAsk),
    /// A teammate mentioned this machine in a room its agents can't read
    /// (#578): the daemon raises a card asking its owner to let them. The
    /// grant itself is the owner's own act, made over their control session.
    #[serde(rename = "agents.grant_ask")]
    AgentGrantAsk(AgentGrantAsk),
}

impl DaemonOp {
    pub fn name(&self) -> &'static str {
        match self {
            DaemonOp::ForgeSend(_) => "forge.send",
            DaemonOp::RunStart(_) => "runs.start",
            DaemonOp::RunRevoke(_) => "runs.revoke",
            DaemonOp::HostedArchive(_) => "hosted.archive",
            DaemonOp::HostedArchiveBlocks(_) => "hosted.archive_blocks",
            DaemonOp::AgentMention(_) => "agents.mention",
            DaemonOp::MachineAgents(_) => "agents.sessions",
            DaemonOp::AgentGrantAsk(_) => "agents.grant_ask",
        }
    }
}

/// A message in a thread for a machine's agents (Arugula #394).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentMention {
    /// The thread's room.
    pub room: String,
    /// What an issue's, PR's or box's thread hangs on (`iss_…`), the id
    /// an agent posts to it by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// What the room is called, for the one reading.
    #[serde(default)]
    pub room_name: String,
    /// The message's number in the room.
    pub seq: i64,
    /// The message it replies under, when it's in a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<i64>,
    /// Who wrote it, as people read it: a person's name, or "Ada's agent
    /// on geek".
    pub author: String,
    pub text: String,
    /// The one session it names (#429): `ux-improvements` of
    /// `@geek/ux-improvements`, as the poster typed it, lowercased. The
    /// daemon matches it against its sessions' names, exactly. An older
    /// daemon ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// With an agent's post (#575): the name of the session that wrote it.
    /// The daemon never gives the message to a session of this name, so an
    /// agent's mention of its own machine doesn't come back to it. An older
    /// daemon ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_session: Option<String>,
    /// With a reply (#580): the first line of the thread's root message,
    /// about 120 characters at most. The daemon names the thread by it when
    /// replies come in together; without it, by the root's number. An older
    /// daemon ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_line: Option<String>,
    /// With a reply (#649): the other agents in the thread, as people read
    /// them ("wake (Aaron's, Mac mini)"), so a session can see who's here
    /// before it starts on something. An older daemon ignores it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<String>,
    /// Whether this machine's owner wrote it themselves (#665): a person's
    /// post, signed in as the account that owns the machine. Never set for an
    /// agent's post, the owner's own agents' included. The daemon tells the
    /// session it's from its person; else from a teammate. An older daemon
    /// ignores it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_owner: bool,
}

/// What a machine's session is doing (#645).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MachineAgentState {
    Working,
    Idle,
    NeedsInput,
}

/// A mention of this machine that its agents can't read the room for (#578).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentGrantAsk {
    pub room: String,
    pub room_name: String,
    /// The mention's number in the room.
    pub seq: i64,
    /// Who wrote it, as people read it.
    pub author: String,
    /// Its first line.
    pub line: String,
    /// This machine's id and name, for the card.
    pub machine: String,
    pub machine_name: String,
    /// What a grant of `threads.read` is over, as `POST /api/grants` says it:
    /// `project` and the project's id, or `account`.
    pub scope_kind: String,
    #[serde(default)]
    pub scope_id: String,
    /// The scope's name as its owner reads it (the team's, or "your DMs"),
    /// for the card to say what the grant reaches. Empty: just the room.
    #[serde(default)]
    pub scope_name: String,
}

/// An agent asks which sessions of a machine it may mention (#577).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineAgentsAsk {
    /// The room the mention would be in.
    pub room: String,
    /// When the asker's session is on this machine (#575): its name, so the
    /// list leaves it out, as a mention never reaches the session that
    /// wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_session: Option<String>,
    /// For a thread's strip (#645): only the sessions of these names
    /// ([`session_slug`]), silenced or not, and however many. Empty: the list
    /// a mention picks from. An older daemon ignores it and answers that.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// A session's name as `@machine/name` matches it: lowercase, its letters and
/// digits, the rest one dash.
pub fn session_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The most sessions an `agents.sessions` answer lists.
pub const SESSIONS_LISTED: usize = 20;

/// A session of a machine a teammate's agent may mention (#577).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineAgent {
    /// What `@machine/name` matches: the session's name, as a slug.
    pub name: String,
    /// The tab it is in, when the tab has a name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    /// How long ago it last printed, in ms; absent when the machine does not
    /// know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_ms_ago: Option<u64>,
    /// With the owner's `work`: the issue it was started on (`#540`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_on: Option<String>,
    /// What it's doing now (#645); absent from an older daemon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<MachineAgentState>,
    /// With the owner's `work`: the name of the tool it last ran (#645),
    /// never its arguments or output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// A draft to send: what it is and where it goes. `account` is its person,
/// who must be the machine's owner.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ForgeSend {
    /// The platform's draft id.
    pub draft: String,
    pub account: String,
    /// `OWNER/NAME` on github.com.
    pub repo: String,
    /// The PR or issue; `None` for a new issue or PR.
    #[serde(default)]
    pub number: Option<u64>,
    /// `comment`, `review`, `issue` or `pr` (an agent session's PR, M259:
    /// its draft's id is `pdr_…`).
    pub kind: String,
    /// A review's: `COMMENT`, `APPROVE` or `REQUEST_CHANGES`.
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: String,
    /// A PR's: the branch it's from, and the one it goes into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// A PR's: opened as a draft on GitHub.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub draft_pr: bool,
}

/// How a daemon routed an `agents.mention` (#502): which of its four steps
/// took it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Routed {
    pub step: RoutedStep,
    /// The session it went to, by its name, when it went to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The session the message named (`@geek/slowness`), when the daemon
    /// found no such session (or more than one): the message waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
    /// The session's hook wasn't waiting, so the mention only sits in its
    /// queue (#571). A daemon from before it says nothing: handed over.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    /// With `asked`: the names a mention could have used (sessions and
    /// tabs, at most 10), since no session was called that (#576).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// The session it named is there, but its owner silenced mentions for it
    /// (#579): the message waits in the inbox. With `asked`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub silenced: bool,
}

/// Which of the daemon's steps took an `agents.mention`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RoutedStep {
    /// The session it names.
    Named,
    /// The session that posted in its thread.
    Thread,
    /// The session that last posted in its room (a fallback).
    Room,
    /// No session took it: the machine's inbox.
    Inbox,
}

/// How a machine (or control, for it) answered an op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum OpAnswer {
    /// Done: what it made on the forge, when it says.
    Done {
        #[serde(default)]
        url: Option<String>,
        /// An `agents.mention`'s: how the daemon routed it (#502). A daemon
        /// from before it says nothing, and control shows only "delivered".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routed: Option<Routed>,
    },
    /// The forge refused it, or the call failed: the person's to see.
    Failed { error: String },
    /// This machine can't (no `gh` login, not its owner's): ask another.
    Cant { why: String },
    /// An `agents.sessions`' answer (#577): the sessions the owner shows,
    /// newest first. `shared` is false when the owner shows none.
    Sessions {
        shared: bool,
        #[serde(default)]
        sessions: Vec<MachineAgent>,
    },
    /// It doesn't know the op (a daemon from before it).
    Unknown,
    /// It isn't on control's relay now, or didn't answer in time.
    Offline,
}
