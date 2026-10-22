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
pub struct GithubTokenRequest {
    pub repo: String,
}

/// A read-only installation token, who it is for, and when it stops
/// working. Never logged. A daemon takes what it finds: no token is an error
/// of its own, a missing `login` is empty and a missing `expires_at` is ten
/// minutes.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// `{"t": "ops", "ops": [...]}`, daemon to control, on connecting: the
/// [`DaemonOp`]s it knows. A daemon that never says knows none, and control
/// answers [`OpAnswer::Unknown`] for it without asking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ops {
    pub t: OpsTag,
    #[serde(default)]
    pub ops: Vec<String>,
}

impl Ops {
    /// What this build knows.
    pub fn ours() -> Self {
        Ops { t: OpsTag, ops: OPS.iter().chain(RUN_OPS).map(|s| (*s).to_owned()).collect() }
    }
}

/// `{"t": "op", "id": ..., "op": {...}}`, control to daemon: do this and
/// answer with an [`OpDone`] of the same `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(tag = "op")]
pub enum DaemonOp {
    /// Send a draft on a linked repo as the machine's owner, with their own
    /// `gh` login, because the person pressed Send (M168).
    #[serde(rename = "forge.send")]
    ForgeSend(ForgeSend),
    /// Start under the project's standing grant, on a hosted machine
    /// (M211; [`crate::runs`]).
    #[serde(rename = "runs.start")]
    RunStart(crate::runs::RunStart),
    /// The project's standing grant was revoked (M211).
    #[serde(rename = "runs.revoke")]
    RunRevoke(crate::runs::RunRevoke),
}

impl DaemonOp {
    pub fn name(&self) -> &'static str {
        match self {
            DaemonOp::ForgeSend(_) => "forge.send",
            DaemonOp::RunStart(_) => "runs.start",
            DaemonOp::RunRevoke(_) => "runs.revoke",
        }
    }
}

/// A draft to send: what it is and where it goes. `account` is its person,
/// who must be the machine's owner.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeSend {
    /// The platform's draft id.
    pub draft: String,
    pub account: String,
    /// `OWNER/NAME` on github.com.
    pub repo: String,
    /// The PR or issue; `None` for a new issue.
    #[serde(default)]
    pub number: Option<u64>,
    /// `comment`, `review` or `issue`.
    pub kind: String,
    /// A review's: `COMMENT`, `APPROVE` or `REQUEST_CHANGES`.
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: String,
}

/// How a machine (or control, for it) answered an op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum OpAnswer {
    /// Done: what it made on the forge, when it says.
    Done {
        #[serde(default)]
        url: Option<String>,
    },
    /// The forge refused it, or the call failed: the person's to see.
    Failed { error: String },
    /// This machine can't (no `gh` login, not its owner's): ask another.
    Cant { why: String },
    /// It doesn't know the op (a daemon from before it).
    Unknown,
    /// It isn't on control's relay now, or didn't answer in time.
    Offline,
}
