//! Hosted compute (M176, #1237): the substrate a hosted workspace's box
//! runs on, and its one adapter, [`E2b`], which drives E2B's cloud and
//! loam's E2B-compatible API alike (only the URL and key differ).
//!
//! The substrate does a box's lifecycle and nothing else: create, resume,
//! pause, delete, status, list and hold, and (when the provider keeps them)
//! the runs it billed. No exec, files or ports: the
//! daemon in the box (`arugulad --hosted`, M180) dials out and brings
//! those. Beside the lifecycle, egress rules (M185): [`Rules`] that set
//! headers on a box's outbound requests at the provider's egress proxy, so
//! an inference key never enters the box ("Hosted inference: strangers
//! bring their own key", aru-096). A box always pauses when its timeout passes, never dies, and only
//! [`Substrate::resume`] wakes it: no read here does, and traffic doesn't
//! either (auto-resume is off).
//!
//! The provider's key is a platform secret (`ARUGULA_E2B_API_KEY`). It is
//! never sent to a client or put in a box, and nothing a provider answers
//! but its status and its error code reaches an [`Error`]: E2B's messages
//! and bodies can echo what was sent.

use std::{collections::BTreeMap, time::Duration};

use futures_util::future::BoxFuture;

mod e2b;
#[cfg(test)]
mod e2b_tests;
#[cfg(test)]
mod egress_tests;
#[cfg(test)]
mod live_tests;
pub mod template;

pub use e2b::E2b;

/// Hosted compute's flags and their env vars, held only by the platform.
#[derive(clap::Args, Debug, Clone)]
#[group(id = "compute")]
pub struct Args {
    /// Hosted workspaces' boxes (M176): E2B's API, or loam's E2B-compatible
    /// one. Off without a key.
    #[arg(long, env = "ARUGULA_E2B_URL", default_value = "https://api.e2b.dev")]
    pub e2b_url: String,
    #[arg(long, env = "ARUGULA_E2B_API_KEY", hide_env_values = true)]
    pub e2b_api_key: Option<String>,
}

impl Args {
    /// The substrate these flags set up: none without a key.
    pub fn substrate(self) -> anyhow::Result<Option<std::sync::Arc<dyn Substrate>>> {
        Ok(match self.e2b_api_key.filter(|k| !k.trim().is_empty()) {
            Some(key) => Some(std::sync::Arc::new(E2b::new(&self.e2b_url, key)?)),
            None => None,
        })
    }
}

/// What a box's egress proxy writes into its outbound HTTPS requests
/// (M185), as E2B's network rules do: for each host (exact, or `*.` and a
/// suffix), headers set over whatever the box sent. A value is a key
/// itself (a project's: the proxy holds it, the box never sees it), or
/// names a secret in the provider's broker with [`fill`] (the platform's,
/// which the operator keeps there), resolved at request time; a secret
/// that's gone is no header at all (E2B fails open, and never sends the
/// reference on). Plain HTTP is never filled. Rules can hold keys: never
/// log them.
pub type Rules = BTreeMap<String, BTreeMap<String, String>>;

/// A header value that the egress proxy fills with the broker's secret
/// `name` (E2B's `${e2b.secrets.<name>}`).
pub fn fill(name: &str) -> String {
    format!("${{e2b.secrets.{name}}}")
}

/// What a box is made from and carries. Its `Debug` names the rules'
/// hosts and headers, never their values.
#[derive(Clone, Default)]
pub struct Create {
    /// The template id (M177's per release; `base` in the live suite).
    pub template: String,
    /// The box's environment. Never an inference key or the provider's.
    pub env: BTreeMap<String, String>,
    /// What [`Substrate::list`] filters on: `arugula_workspace`,
    /// `arugula_project`, `arugula_release`, `arugula_template`.
    pub metadata: BTreeMap<String, String>,
    /// How long it runs before it pauses itself, unless held.
    pub timeout: Duration,
    /// Its egress rules from the start (M185): none, unless set.
    pub rules: Rules,
}

impl std::fmt::Debug for Create {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let rules: BTreeMap<&String, Vec<&String>> = self.rules.iter().map(|(h, v)| (h, v.keys().collect())).collect();
        f.debug_struct("Create")
            .field("template", &self.template)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("rules", &rules)
            .finish()
    }
}

/// A box as made: its id and the template the provider says it's from.
/// E2B answers the template's resolved id (`base` is `rki5dems…`), loam
/// the name asked for: the one asked for is the box's `arugula_template`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub id: String,
    pub template: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Paused,
}

/// A box as the provider reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    pub id: String,
    pub template: String,
    pub state: State,
    pub metadata: BTreeMap<String, String>,
    /// When it started (RFC 3339), and when its timeout passes: the time it
    /// paused, once it has.
    pub started_at: String,
    pub end_at: String,
    /// Whether it pauses at its timeout (`lifecycle.onTimeout`), when the
    /// provider says: [`Substrate::status`] does, a listing doesn't.
    pub pauses_on_timeout: Option<bool>,
    /// Its size, when the provider says (E2B and loam do): vCPUs and
    /// memory in MiB, what it's billed by ([`crate::hosted::usage`]).
    pub cpus: Option<u32>,
    pub memory_mb: Option<u32>,
}

/// One run of a box as the provider billed it, from its start or a resume
/// to a pause or its delete (M188).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// When it started, ms since the epoch, and how long it ran, in ms.
    pub started_at: u64,
    pub ms: u64,
    pub cpus: u32,
    pub memory_mb: u32,
}

/// What [`Substrate::resume`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resumed {
    /// It was paused, and runs now.
    Woke,
    /// It was running: only its timeout moved, and only later.
    Running,
}

/// What [`Substrate::pause`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paused {
    Paused,
    /// It was paused already.
    Already,
}

/// A provider's no, with only what's safe to keep: the HTTP status and
/// E2B's machine-readable `error_code`, never its message or a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No such box, or not one this key may see.
    NotFound,
    /// The provider refused, and nothing changed: a 4xx (but 404, 408 and
    /// 429), as `409` for a box mid-transition.
    Refused { status: u16, code: Option<String> },
    /// The provider couldn't, for now: 408, 429 or a 5xx. `retry_after`
    /// is its `Retry-After`, in seconds, when it gave one.
    Unavailable { status: u16, code: Option<String>, retry_after: Option<u64> },
    /// No answer: the request may or may not have happened.
    Unreachable,
    /// An answer that isn't what the API says it is.
    Invalid,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = |c: &Option<String>| c.as_deref().map(|c| format!(" {c}")).unwrap_or_default();
        match self {
            Error::NotFound => write!(f, "the compute provider has no such box"),
            Error::Refused { status, code: c } => write!(f, "the compute provider refused: HTTP {status}{}", code(c)),
            Error::Unavailable { status, code: c, .. } => {
                write!(f, "the compute provider is unavailable: HTTP {status}{}", code(c))
            }
            Error::Unreachable => write!(f, "the compute provider didn't answer"),
            Error::Invalid => write!(f, "the compute provider's answer wasn't understood"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A box's lifecycle on a provider, shaped after E2B's sandbox API.
pub trait Substrate: Send + Sync {
    /// Make a box from a template. It pauses (with its memory) when its
    /// timeout passes, and only [`Substrate::resume`] wakes it. Never
    /// retried: after a lost answer, find it by its metadata.
    fn create<'a>(&'a self, box_: &'a Create) -> BoxFuture<'a, Result<Created>>;

    /// Wake a paused box, its processes as they were, with `timeout` until
    /// it pauses again. A running box only has its timeout extended.
    fn resume<'a>(&'a self, id: &'a str, timeout: Duration) -> BoxFuture<'a, Result<Resumed>>;

    /// Pause a box, memory and disk. Only between turns: never one whose
    /// daemon reports busy.
    fn pause<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Paused>>;

    /// Delete a box, disk and all. One already gone is fine.
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<()>>;

    /// A box as it is, `None` when it's gone. Never wakes it.
    fn status<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Sandbox>>>;

    /// Every box, running or paused, whose metadata has all of `metadata`.
    fn list<'a>(&'a self, metadata: &'a BTreeMap<String, String>) -> BoxFuture<'a, Result<Vec<Sandbox>>>;

    /// Keep a running box up: it pauses `timeout` from now, not before
    /// (and not after: this sets the time, as E2B's timeout does). A paused
    /// box is [`Error::NotFound`] on E2B and loam alike: hold never wakes.
    fn hold<'a>(&'a self, id: &'a str, timeout: Duration) -> BoxFuture<'a, Result<()>>;

    /// A box's finished runs as the provider keeps them, to reconcile
    /// what the platform metered with (M188): `None` when it keeps no such
    /// record (loam), or none for this box. Never wakes it. Optional: a
    /// provider without one leaves this as it is.
    fn runs<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Result<Option<Vec<Run>>>> {
        Box::pin(async { Ok(None) })
    }

    /// Replace a running box's egress rules (M185): they take effect on
    /// its next request, and last through a pause. A paused box is
    /// [`Error::NotFound`], as with hold: set them again after a wake. This
    /// replaces the box's whole network config on E2B, so egress limits
    /// (M192) will have to ride along.
    fn set_rules<'a>(&'a self, id: &'a str, rules: &'a Rules) -> BoxFuture<'a, Result<()>> {
        let _ = (id, rules);
        no_rules()
    }
}

/// What a provider without egress rules answers.
fn no_rules<'a>() -> BoxFuture<'a, Result<()>> {
    Box::pin(async { Err(Error::Refused { status: 404, code: None }) })
}
