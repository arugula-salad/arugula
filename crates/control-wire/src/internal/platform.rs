//! The platform's answers to the hub (`PlatformPort` in `arugula-platform`),
//! and what the hub hands it beyond the `*Args` objects: deleted accounts,
//! GitHub tokens and sends, hosted boxes' turns and transcripts, public
//! previews and hud boxes.

use serde::{Deserialize, Serialize};

use crate::{forge::GithubToken, projects::DraftSent};

/// An account the hub deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Deleted {
    pub account: String,
    /// Its machines, which left with it.
    pub machines: Vec<String>,
    /// The teams it disbanded.
    pub disbanded: Vec<String>,
}

/// What the platform answers a hosted box's ask for a token: one, or why
/// not, as the hub's route answers it. No `Debug`: a token is never
/// printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TokenAnswer {
    Token(GithubToken),
    Refused { status: u16, error: String },
}

/// A person's GitHub user token, for one call. Not `Clone`, never printed,
/// and it crosses from the hub only inside a signed internal call.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct UserToken(String);

impl UserToken {
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    /// The token itself, for the one GitHub call it's for.
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for UserToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UserToken(…)")
    }
}

/// What a send did, as the hub's callback hears it: the draft sent, or why
/// not (the draft says it too).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Outcome {
    #[serde(default)]
    pub sent: Option<DraftSent>,
    #[serde(default)]
    pub refused: Option<String>,
    /// A session's PR (M259) that was opened: its URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<String>,
}

/// What a hosted box's turn is told (`hosted::inference::turn` in the
/// platform): its turn id, whose key pays (`project` or `platform`) and
/// what kind it is (`Auth::as_str`; absent from an older platform, a key),
/// or why it can't run, as the hub answers the box: an HTTP status, a
/// reason (`no_key`, `cap`) and a sentence for the person. It crosses
/// `PlatformPort`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnAnswer {
    Given {
        turn: String,
        pays: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth: Option<String>,
    },
    Refused {
        status: u16,
        reason: Option<String>,
        error: String,
    },
}

/// What the platform answers a box's transcript, through the hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Archived {
    Kept { id: String },
    Refused { status: u16, error: String },
}

/// A port that's public now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PublicPreview {
    /// Its host label under the preview domain.
    pub id: String,
    pub workspace: String,
    pub project: String,
    /// The hosted machine its workspace's box enrolled as.
    pub machine: Option<String>,
    pub port: u16,
    /// Its workspace's state (`ready`, `paused`, …).
    pub state: String,
    /// Who made it public or last renewed it (`None` once that account
    /// is deleted).
    pub by: Option<String>,
    pub until: u64,
}

/// A hosted machine, as the hub checked its signature (its hosted
/// certificate): the project and workspace it was enrolled into, its device
/// id, and its signing key, hex, which its reports are checked with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HostedMachine {
    pub project: String,
    pub workspace: String,
    pub machine: String,
    pub sign: String,
}

/// A member of a box's project (M237), as the platform knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoxMember {
    pub account: String,
    /// `owner` for a personal project's account; else their team role
    /// (`owner`, `editor`, `viewer`, ...).
    pub role: String,
    /// What the roster calls them (a personal project: the account's name).
    pub name: String,
}

/// A review a box posts into an object's thread (M237).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoxPost {
    /// The project the box is bound to: the object has to be in it.
    pub project: String,
    /// The box's name, kept beside the author.
    pub via: String,
    /// The object's global id (`iss_…`, `fi_…`, `hw_…`, `run_…`).
    pub target: String,
    /// Whose review it is: a member who signed in to the box.
    pub author: String,
    pub text: String,
}

/// What became of a [`BoxPost`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoxPosted {
    /// In the thread `room` as message `seq`.
    Posted { room: String, seq: i64 },
    /// Refused, as the author posting themselves would be: the status and
    /// why.
    Refused { status: u16, error: String },
}
