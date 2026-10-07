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
