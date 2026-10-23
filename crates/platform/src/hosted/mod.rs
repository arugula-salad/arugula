//! Hosted workspaces (M179, #1240): `arugulad --hosted` in a box on the
//! compute provider ([`crate::compute`], M176), in a project ("A hosted
//! machine is arugulad in a rented sandbox", aru-092). The platform keeps
//! what a workspace is (its [`Spec`]), where it runs (its box), and drives
//! the box's lifecycle on its leased [`worker`]: create, wake, pause,
//! delete and rebuild. Nothing reaches into the box from here: its daemon
//! dials out and brings terminals, agents and previews itself.
//!
//! - **Who.** A project's members list and open its workspaces; its owners
//!   and editors (not a team's viewers) create, rebuild and delete them
//!   ([`routes`]). Making one also needs the account on
//!   `ARUGULA_HOSTED_ACCOUNTS`.
//! - **A box** is made from the current release's template ([`template`])
//!   with the metadata `arugula_workspace`, `arugula_project`,
//!   `arugula_release` and `arugula_template`. It pauses (memory and disk
//!   kept) when its timeout passes, and only a wake resumes it. A create is
//!   never sent twice: after a lost answer the box is found by its metadata.
//! - **Enrolling** ("Control enrolls hosted machines under a project
//!   delegation", aru-095). A box from a running snapshot can't read the env
//!   it was made with, so it enrolls by its sandbox id: once the box is made,
//!   the platform asks the hub to record a one-time enrollment for that id
//!   ([`enroll`], about two minutes). The sandbox id is a bearer secret for
//!   that window: it's never logged, answered or put in a URL. A box that
//!   doesn't enroll in time, or whose enrollment the hub refuses, is deleted
//!   (fail closed). Where the box isn't a snapshot (loam, tests:
//!   `ARUGULA_HOSTED_ENROLL=ticket`), the hub makes a one-time ticket that
//!   goes in the box's env instead.
//! - **Agents** (M184): an agent block on the box is the daemon's, as on
//!   any machine; when one closes, its transcript is archived here, read
//!   only under the project ([`transcripts`]).
//! - **Usage** (M188): how long each box ran, by its size, is metered from
//!   the worker's checkpoints and checked against E2B's own record
//!   ([`usage`]).
//! - **Waking on connect** (M182): when someone opens a workspace's machine
//!   and its daemon isn't connected, the hub asks the platform to wake its
//!   box, and the worker resumes it ([`wake`]).
//! - **Pausing only between turns** (M181, "Hosted machines pause only
//!   between turns", aru-094): the leader [`hold`]s a box while its daemon
//!   says it's busy, and lets it pause itself when it's idle or silent.
//! - **Inference** ([`inference`], M185): agents in the box reach their
//!   model through the provider's egress proxy, which fills in the key:
//!   the project's own, or the platform's, under caps, for workspaces made
//!   by an account on the beta allowlist. No key is ever in the box.
//! - **Setup** (M183): on its first boot the box reads its spec, with what
//!   to clone each repo with, and runs it in a pane; its daemon says how it
//!   goes, kept on the workspace ([`setup`]). A spec changed since the box
//!   was built shows as [`HostedWorkspace::rebuild_needed`].
//! - **Public previews** ([`previews`], M187): a port an owner or editor
//!   makes public is served to anyone at `<id>.<preview domain>`, through
//!   the hub, for 7 days unless renewed.
//!
//! The provider's key never reaches a client or a box, and neither does an
//! inference key ([`crate::compute`]).

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{Platform, secrets::SecretCipher};

pub mod enroll;
pub mod hold;
pub mod inference;
#[cfg(test)]
mod inference_tests;
pub mod previews;
#[cfg(test)]
mod previews_tests;
pub mod routes;
pub mod setup;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
pub(crate) mod tests;
pub mod transcripts;
pub mod usage;
pub mod wake;
#[cfg(test)]
mod wake_tests;
pub mod worker;

pub use crate::db::hosted::HostedWorkspace;

/// Hosted workspaces' flags and their env vars.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(id = "hosted")]
pub struct Args {
    /// The template new boxes are made from, in place of this release's
    /// (M177's `arugula-workspace:vX.Y.Z`): for loam and testnet, whose
    /// templates are named by their operator.
    #[arg(long, env = "ARUGULA_HOSTED_TEMPLATE")]
    pub hosted_template: Option<String>,
    /// How a new box enrolls: `sandbox` (by its sandbox id, for a box made
    /// from a running snapshot) or `ticket` (a one-time ticket in its env,
    /// for loam and testnet, whose boxes boot fresh).
    #[arg(long, env = "ARUGULA_HOSTED_ENROLL", default_value = "sandbox", value_parser = ["sandbox", "ticket"])]
    pub hosted_enroll: String,
    /// Who may make hosted workspaces: account ids, comma-separated, or `*`.
    /// Empty: nobody.
    #[arg(long, env = "ARUGULA_HOSTED_ACCOUNTS", value_delimiter = ',')]
    pub hosted_accounts: Vec<String>,
    /// Seconds a box runs after its daemon last said it was busy before it
    /// pauses itself ([`worker::IDLE_TIMEOUT`] unless set).
    #[arg(long, env = "ARUGULA_HOSTED_IDLE_TIMEOUT")]
    pub hosted_idle_timeout: Option<u64>,
    /// Seconds a box may run without a pause on the provider's plan
    /// ([`hold::E2B_PRO_RUNTIME`] unless set; E2B's Hobby plan is 3600).
    #[arg(long, env = "ARUGULA_HOSTED_RUNTIME_CAP")]
    pub hosted_runtime_cap: Option<u64>,
    /// Which providers the platform's own inference key covers (M185), for
    /// workspaces made by accounts on ARUGULA_CLOUD_ACCOUNTS: `anthropic`,
    /// `openai`, comma-separated. Each is a dedicated key with a hard
    /// monthly spend limit at the provider, which the operator keeps as an
    /// E2B secret named `arugula-platform-<provider>` (docs/control-ops.md);
    /// the platform never holds it. Empty: the platform's key pays for
    /// nothing, and every project brings its own.
    #[arg(long, env = "ARUGULA_HOSTED_PLATFORM_KEYS", value_delimiter = ',', value_parser = ["anthropic", "openai"])]
    pub hosted_platform_keys: Vec<String>,
    /// Caps on agent turns that run on the platform's key, per account
    /// (M185): at once, per UTC day, and minutes per UTC day.
    #[arg(long, env = "ARUGULA_HOSTED_CONCURRENT_TURNS", default_value_t = inference::CAPS.concurrent)]
    pub hosted_concurrent_turns: u64,
    #[arg(long, env = "ARUGULA_HOSTED_DAILY_TURNS", default_value_t = inference::CAPS.daily)]
    pub hosted_daily_turns: u64,
    #[arg(long, env = "ARUGULA_HOSTED_TURN_MINUTES", default_value_t = inference::CAPS.minutes)]
    pub hosted_turn_minutes: u64,
    /// Public previews (M187): a port a member makes public is served at
    /// `<id>.<this>` (`arugula.site` for the hosted one), a registrable
    /// domain of its own with a wildcard DNS name and certificate pointed
    /// at the hub. The hub and the platform both read it. Off when unset
    /// (docs/control-ops.md, "Public previews").
    #[arg(long, env = "ARUGULA_PREVIEW_DOMAIN")]
    pub preview_domain: Option<String>,
}

impl Args {
    /// The config these flags set up, with two of Cloud's: the master key
    /// (`ARUGULA_CLOUD_SECRET_KEY`, and the old one while it's rotated),
    /// which wraps the project keys that seal hosted workspaces' secrets
    /// (M209), and `ARUGULA_CLOUD_ACCOUNTS`, the beta allowlist, whose
    /// workspaces may run on the platform's key (M185).
    pub fn config(self, cloud: &crate::cloud::Args) -> anyhow::Result<Config> {
        let inference = inference::Settings::new(
            &cloud.cloud_accounts,
            &self.hosted_platform_keys,
            inference::Caps {
                concurrent: self.hosted_concurrent_turns,
                daily: self.hosted_daily_turns,
                minutes: self.hosted_turn_minutes,
            },
        );
        Ok(Config {
            inference,
            template: self.hosted_template.filter(|t| !t.trim().is_empty()),
            enroll: if self.hosted_enroll == "ticket" { EnrollBy::Ticket } else { EnrollBy::Sandbox },
            accounts: self.hosted_accounts.into_iter().map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect(),
            cipher: cloud.master_key()?.map(std::sync::Arc::new),
            idle_timeout: self.hosted_idle_timeout.filter(|s| *s > 0).map(Duration::from_secs),
            runtime_cap: self.hosted_runtime_cap.filter(|s| *s > 0).map(Duration::from_secs),
            preview_domain: previews::domain(self.preview_domain.as_deref().unwrap_or(""))?,
        })
    }
}

/// How a new box enrolls ([`Args::hosted_enroll`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EnrollBy {
    /// By its sandbox id, which it reads from `/run/e2b/.E2B_SANDBOX_ID`.
    #[default]
    Sandbox,
    /// By a one-time ticket in its env (`ARUGULA_HOSTED_TICKET`).
    Ticket,
}

/// What hosted workspaces run with, beside [`Platform::compute`].
#[derive(Clone, Default)]
pub struct Config {
    pub template: Option<String>,
    pub enroll: EnrollBy,
    pub accounts: Vec<String>,
    /// Seals [`crate::db::hosted`]'s secrets, when there's a key.
    pub cipher: Option<std::sync::Arc<SecretCipher>>,
    /// [`Args::hosted_idle_timeout`] and [`Args::hosted_runtime_cap`], when
    /// set: [`Config::idle_timeout`] and [`Config::runtime_cap`] say what
    /// holds.
    pub idle_timeout: Option<Duration>,
    pub runtime_cap: Option<Duration>,
    /// Whose key agents run on, and the caps on the platform's (M185).
    pub inference: inference::Settings,
    /// Public previews' domain (M187), when they're on.
    pub preview_domain: Option<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("template", &self.template)
            .field("enroll", &self.enroll)
            .field("accounts", &self.accounts)
            .field("idle_timeout", &self.idle_timeout)
            .field("runtime_cap", &self.runtime_cap)
            .field("inference", &self.inference)
            .field("preview_domain", &self.preview_domain)
            .finish_non_exhaustive()
    }
}

impl Config {
    /// Whether this account may make hosted workspaces.
    pub fn allows(&self, account: &str) -> bool {
        self.accounts.iter().any(|a| a == account || a == "*")
    }

    /// How long a box runs without being held before it pauses itself.
    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout.unwrap_or(worker::IDLE_TIMEOUT)
    }

    /// How long the provider lets a box run without a pause.
    pub fn runtime_cap(&self) -> Duration {
        self.runtime_cap.unwrap_or(hold::E2B_PRO_RUNTIME)
    }
}

/// The template a new box is made from: the current release's
/// ([`crate::compute::template::current`]), unless `ARUGULA_HOSTED_TEMPLATE`
/// names another.
pub fn template(app: &Platform) -> Option<String> {
    Some(app.hosted.template.clone().unwrap_or_else(crate::compute::template::current))
}

/// What the box carries for [`crate::compute::Substrate::list`].
pub fn metadata(workspace: &str, project: &str, template: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("arugula_workspace".into(), workspace.into()),
        ("arugula_project".into(), project.into()),
        ("arugula_release".into(), env!("CARGO_PKG_VERSION").into()),
        ("arugula_template".into(), template.into()),
    ])
}

/// A new hosted workspace's id: `hw_` and 32 hex characters, beside
/// `prj_`'s.
pub fn new_id() -> String {
    format!("hw_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// What a hosted workspace is built from ([`setup`] runs it in a visible
/// pane).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    /// HTTPS clone URLs, with no credentials in them.
    #[serde(default)]
    pub repos: Vec<String>,
    /// Packages to install.
    #[serde(default)]
    pub packages: Vec<String>,
    /// A shell script run after both.
    #[serde(default)]
    pub setup: String,
    /// The names of env vars it takes from its secrets.
    #[serde(default)]
    pub env: Vec<String>,
}

const MAX_REPOS: usize = 10;
const MAX_PACKAGES: usize = 50;
const MAX_SETUP: usize = 64 * 1024;
const MAX_ENV: usize = 32;

impl Spec {
    /// Why it can't be kept, if it can't.
    pub fn check(&self) -> Result<(), String> {
        if self.repos.len() > MAX_REPOS {
            return Err(format!("at most {MAX_REPOS} repos"));
        }
        if let Some(r) = self.repos.iter().find(|r| !crate::cloud::https_repository(r)) {
            return Err(format!("{r:?} isn't an HTTPS clone URL without credentials"));
        }
        if self.packages.len() > MAX_PACKAGES {
            return Err(format!("at most {MAX_PACKAGES} packages"));
        }
        let package = |p: &str| {
            !p.is_empty()
                && p.len() <= 100
                && !p.starts_with('-')
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-:=@/".contains(&b))
        };
        if let Some(p) = self.packages.iter().find(|p| !package(p)) {
            return Err(format!("{p:?} isn't a package name"));
        }
        if self.setup.len() > MAX_SETUP {
            return Err(format!("the setup script is over {} KiB", MAX_SETUP / 1024));
        }
        if self.env.len() > MAX_ENV {
            return Err(format!("at most {MAX_ENV} env names"));
        }
        if let Some(n) = self.env.iter().find(|n| !crate::secrets::valid_name(n)) {
            return Err(format!("{n:?} isn't an env name a workspace may take"));
        }
        Ok(())
    }

    /// Its SHA-256, hex: what "rebuild needed" compares.
    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(serde_json::to_vec(self).unwrap_or_default()))
    }
}

/// A workspace's name: 1–64 letters, digits, `-`, `_` and `.`, starting
/// with a letter or digit.
pub fn good_name(name: &str) -> bool {
    name.len() <= 64
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// What someone may ask of a workspace that the worker does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Resume a paused box, or one that may have paused itself (M182's
    /// wake on connect).
    Wake,
    /// Pause a running box. Only between turns: while its daemon says
    /// busy, the ask is done without a pause, and the box pauses itself
    /// after the turn ([`hold`]).
    Pause,
    /// Delete the box and make it again from the spec.
    Rebuild,
    Delete,
}

impl Ask {
    pub fn as_str(self) -> &'static str {
        match self {
            Ask::Wake => "wake",
            Ask::Pause => "pause",
            Ask::Rebuild => "rebuild",
            Ask::Delete => "delete",
        }
    }
}

/// Ask the worker to do `what` to a workspace: whether it was taken. A
/// delete always is (until it's done), a rebuild unless a delete is asked,
/// and a wake or a pause only of a box that's paused or running with
/// nothing else asked.
pub fn ask(app: &Platform, workspace: &str, what: Ask) -> anyhow::Result<bool> {
    app.db.ask_hosted(workspace, what, arugula_e2e::now_ms())
}

/// The lifecycle's background work, when this platform has a compute
/// provider.
pub fn start(app: &std::sync::Arc<Platform>) {
    if app.compute.is_some() {
        worker::start(app.clone());
        usage::start(app.clone());
        hold::start(app.clone());
        inference::start(app.clone());
    }
}
