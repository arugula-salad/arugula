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
//! - **A box** is made from the current release's template of the
//!   workspace's size ([`template`], [`Size`], M266) with the metadata `arugula_workspace`, `arugula_project`,
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
//!   box, and the worker resumes it ([`wake`]). A woken box is `waking`
//!   until its daemon dials in again, and `ready` only then; one whose
//!   daemon doesn't within [`Config::dial_wait`] is failed as needing a
//!   rebuild, and the project's owners and editors are told (M258, Arugula #124).
//!   The template supervises the daemon; the platform never reaches in.
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
//! - **Snapshots** (M263): a workspace that set up all right has a box of
//!   its own set up from its spec, wiped and snapshotted, and its new boxes
//!   and rebuilds start from that snapshot, set up ([`snapshot`]).
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

pub mod broker;
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
pub mod snapshot;
#[cfg(test)]
pub(crate) mod tests;
pub mod transcripts;
pub mod usage;
pub mod wake;
#[cfg(test)]
mod wake_tests;
pub mod worker;

pub use crate::{compute::template::Size, db::hosted::HostedWorkspace};

/// Hosted workspaces' flags and their env vars.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(id = "hosted")]
pub struct Args {
    /// The template new boxes are made from, in place of this release's
    /// (M177's `arugula-workspace:vX.Y.Z`): for loam and testnet, whose
    /// templates are named by their operator.
    #[arg(long, env = "ARUGULA_HOSTED_TEMPLATE")]
    pub hosted_template: Option<String>,
    /// The template large workspaces' boxes are made from, in place of this
    /// release's `arugula-workspace-large:vX.Y.Z` (M266). Unset where
    /// ARUGULA_HOSTED_TEMPLATE is set: large ones are made from that one too.
    #[arg(long, env = "ARUGULA_HOSTED_TEMPLATE_LARGE")]
    pub hosted_template_large: Option<String>,
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
    /// Seconds a woken box's daemon has to dial in before the workspace is
    /// marked as needing a rebuild ([`worker::DIAL_WAIT`] unless set).
    #[arg(long, env = "ARUGULA_HOSTED_DIAL_WAIT")]
    pub hosted_dial_wait: Option<u64>,
    /// Hours between a workspace's snapshot builds (M263; 24 unless set):
    /// a box set up from its spec is snapshotted, and its new boxes and
    /// rebuilds start from that, set up. A spec change builds one at once.
    /// 0: no snapshots, and every box runs its setup.
    #[arg(long, env = "ARUGULA_HOSTED_SNAPSHOT_HOURS")]
    pub hosted_snapshot_hours: Option<u64>,
    /// Hours a deleted workspace waits for the provider to let go of its
    /// snapshots (M263; 24 unless set): past it, it's deleted, and the
    /// snapshots left go on the platform's orphans, which the worker keeps
    /// asking the provider to delete.
    #[arg(long, env = "ARUGULA_HOSTED_SNAPSHOT_HOLD_HOURS")]
    pub hosted_snapshot_hold_hours: Option<u64>,
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
    /// The platform's own Anthropic key for the inference broker (M231):
    /// it pays, under a monthly cap per account, for calls a registered
    /// client's policy covers and for accounts on ARUGULA_CLOUD_ACCOUNTS,
    /// when no project's or account's own key does. A dedicated key with a
    /// hard monthly spend limit at Anthropic (docs/control-ops.md). Unlike
    /// hosted boxes' (E2B's broker holds that one), the platform holds this
    /// one; it's never answered or logged. Unset: the platform's key pays
    /// for nothing through the broker.
    #[arg(long, env = "ARUGULA_INFERENCE_ANTHROPIC_KEY", hide_env_values = true)]
    pub inference_anthropic_key: Option<broker::Key>,
    /// Anthropic's API, for the broker (a fake on the testnet).
    #[arg(long, env = "ARUGULA_INFERENCE_UPSTREAM", default_value = broker::ANTHROPIC)]
    pub inference_upstream: String,
    /// What the platform's key may spend through the broker each UTC month
    /// for an account on ARUGULA_CLOUD_ACCOUNTS calling it itself (a
    /// client's accounts have the client's cap).
    #[arg(long, env = "ARUGULA_INFERENCE_MONTHLY_USD", default_value_t = broker::MONTHLY_USD)]
    pub inference_monthly_usd: f64,
    /// How many of an account's calls may run on the platform's key at
    /// once, through the broker.
    #[arg(long, env = "ARUGULA_INFERENCE_CONCURRENT", default_value_t = broker::CONCURRENT)]
    pub inference_concurrent: u64,
    /// The `anthropic-beta` values the broker sends on the platform's key,
    /// comma-separated; the rest are dropped, and the long-context ones
    /// refused. Empty: none.
    #[arg(long, env = "ARUGULA_INFERENCE_PLATFORM_BETAS", value_delimiter = ',')]
    pub inference_platform_betas: Vec<String>,
    /// The broker's registered clients (studio's lobby): `id:sha256[:house_usd[:payers]]`,
    /// comma-separated (`hosted::broker::clients::parse`). Empty: none.
    #[arg(long, env = "ARUGULA_INFERENCE_CLIENTS", value_delimiter = ',')]
    pub inference_clients: Vec<String>,
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
            template_large: self.hosted_template_large.filter(|t| !t.trim().is_empty()),
            enroll: if self.hosted_enroll == "ticket" { EnrollBy::Ticket } else { EnrollBy::Sandbox },
            accounts: self.hosted_accounts.into_iter().map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect(),
            cipher: cloud.master_key()?.map(std::sync::Arc::new),
            idle_timeout: self.hosted_idle_timeout.filter(|s| *s > 0).map(Duration::from_secs),
            runtime_cap: self.hosted_runtime_cap.filter(|s| *s > 0).map(Duration::from_secs),
            dial_wait: self.hosted_dial_wait.filter(|s| *s > 0).map(Duration::from_secs),
            snapshot_every: self.hosted_snapshot_hours.map(|h| Duration::from_secs(h * 3600)),
            snapshot_hold: self.hosted_snapshot_hold_hours.map(|h| Duration::from_secs(h * 3600)),
            preview_domain: previews::domain(self.preview_domain.as_deref().unwrap_or(""))?,
            broker: broker::Config::new(
                self.inference_anthropic_key,
                &self.inference_upstream,
                self.inference_monthly_usd,
                self.inference_concurrent,
                &self.inference_platform_betas,
                &self.inference_clients,
            )?,
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
    /// [`Args::hosted_template`] and [`Args::hosted_template_large`], when
    /// set ([`template`] says which a box is made from).
    pub template: Option<String>,
    pub template_large: Option<String>,
    pub enroll: EnrollBy,
    pub accounts: Vec<String>,
    /// Seals [`crate::db::hosted`]'s secrets, when there's a key.
    pub cipher: Option<std::sync::Arc<SecretCipher>>,
    /// [`Args::hosted_idle_timeout`] and [`Args::hosted_runtime_cap`], when
    /// set: [`Config::idle_timeout`] and [`Config::runtime_cap`] say what
    /// holds.
    pub idle_timeout: Option<Duration>,
    pub runtime_cap: Option<Duration>,
    /// [`Args::hosted_dial_wait`], when set ([`Config::dial_wait`]).
    pub dial_wait: Option<Duration>,
    /// [`Args::hosted_snapshot_hours`], when set ([`Config::snapshot_every`]).
    pub snapshot_every: Option<Duration>,
    /// [`Args::hosted_snapshot_hold_hours`], when set ([`Config::snapshot_hold`]).
    pub snapshot_hold: Option<Duration>,
    /// Whose key agents run on, and the caps on the platform's (M185).
    pub inference: inference::Settings,
    /// Public previews' domain (M187), when they're on.
    pub preview_domain: Option<String>,
    /// The inference broker's key, clients and caps (M231).
    pub broker: broker::Config,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("template", &self.template)
            .field("template_large", &self.template_large)
            .field("enroll", &self.enroll)
            .field("accounts", &self.accounts)
            .field("idle_timeout", &self.idle_timeout)
            .field("runtime_cap", &self.runtime_cap)
            .field("dial_wait", &self.dial_wait)
            .field("snapshot_every", &self.snapshot_every)
            .field("snapshot_hold", &self.snapshot_hold)
            .field("inference", &self.inference)
            .field("preview_domain", &self.preview_domain)
            .field("broker", &self.broker)
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

    /// How long a woken box's daemon has to dial in.
    pub fn dial_wait(&self) -> Duration {
        self.dial_wait.unwrap_or(worker::DIAL_WAIT)
    }

    /// How long a deleted workspace waits for the provider to let go of
    /// its snapshots (M263).
    pub fn snapshot_hold(&self) -> Duration {
        self.snapshot_hold.unwrap_or(snapshot::HOLD)
    }

    /// How often a workspace's snapshot is built again (M263): `None` when
    /// snapshots are off.
    pub fn snapshot_every(&self) -> Option<Duration> {
        match self.snapshot_every {
            None => Some(snapshot::EVERY),
            Some(d) if d.is_zero() => None,
            d => d,
        }
    }
}

/// The template a new box of `size` is made from: the current release's
/// ([`crate::compute::template::current`]), unless `ARUGULA_HOSTED_TEMPLATE`
/// names another (for every size, unless `ARUGULA_HOSTED_TEMPLATE_LARGE`
/// names large's).
pub fn template(app: &Platform, size: Size) -> Option<String> {
    let named = match size {
        Size::Small => app.hosted.template.clone(),
        Size::Large => app.hosted.template_large.clone().or_else(|| app.hosted.template.clone()),
    };
    Some(named.unwrap_or_else(|| crate::compute::template::current(size)))
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
    /// The finish step (M261): a command (`just check`) a session's box
    /// runs after each turn and before it pushes. When it fails the agent
    /// gets one turn to fix it; when it fails again, nothing is pushed.
    /// Left out when empty, so a spec without one hashes as before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub finish: String,
    /// How long the finish step may run, in seconds
    /// (`arugula_control_wire::work::FINISH_TIMEOUT_S` when 0).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub finish_timeout_s: u64,
    /// Its box's size (M266): which template it's made from. Left out when
    /// small, so a spec from before sizes hashes as before; changing it
    /// needs a rebuild.
    #[serde(default, skip_serializing_if = "Size::is_small")]
    pub size: Size,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

const MAX_REPOS: usize = 10;
const MAX_PACKAGES: usize = 50;
const MAX_SETUP: usize = 64 * 1024;
const MAX_ENV: usize = 32;
const MAX_FINISH: usize = 4 * 1024;
/// The longest a finish step may be given: two hours.
const MAX_FINISH_TIMEOUT_S: u64 = 2 * 60 * 60;

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
        if self.finish.len() > MAX_FINISH {
            return Err(format!("the finish step is over {} KiB", MAX_FINISH / 1024));
        }
        if self.finish.contains('\0') {
            return Err("the finish step has a NUL in it".into());
        }
        if self.finish_timeout_s > MAX_FINISH_TIMEOUT_S {
            return Err(format!("the finish step may run at most {} hours", MAX_FINISH_TIMEOUT_S / 3600));
        }
        Ok(())
    }

    /// Its SHA-256, hex: what "rebuild needed" compares. Only what the
    /// box is built from: the finish step goes to the box with each pull
    /// of its sessions' work (`Db::session_work`), so changing it needs no
    /// rebuild.
    pub fn hash(&self) -> String {
        let built = Spec { finish: String::new(), finish_timeout_s: 0, ..self.clone() };
        hex::encode(Sha256::digest(serde_json::to_vec(&built).unwrap_or_default()))
    }

    /// Its finish step, as a session's box is sent it: `None` when it has
    /// none.
    pub fn finish_step(&self) -> Option<arugula_control_wire::work::FinishStep> {
        let command = self.finish.trim();
        (!command.is_empty()).then(|| arugula_control_wire::work::FinishStep {
            command: command.to_owned(),
            timeout_s: self.finish_timeout_s,
        })
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

/// A workspace's box changed (M258): it's a new box, with a new sandbox id
/// and a new machine (`new_machine`), as after a rebuild. Called each time
/// a box enrolls, so also for a workspace's first box, which has no
/// sessions yet.
///
/// A thread's session (M254) that ran on the workspace's earlier box can't
/// `--resume` there: that conversation's files went with the box. Each
/// live session on `workspace` moves to `new_machine`. One no box took yet
/// only follows it. One that was taken is queued again (control's move,
/// `db::sessions::rebuilt_in`), its next turn starting from a fresh
/// hydration of the thread plus its archived transcript ([`transcripts`],
/// `sessions::hydrate::after_rebuild`) as its asker's prompt: the new box
/// takes it as it takes any queued session. Its card says the box was
/// rebuilt. A session already on `new_machine` is left as it is, so this
/// may run again for the same enrollment.
pub async fn sessions_on_box_changed(app: &Platform, workspace: &str, new_machine: &str) -> anyhow::Result<()> {
    use crate::db::sessions::{NewPrompt, Source, State};
    for s in app.db.live_sessions_on(workspace)? {
        if s.machine.as_deref() == Some(new_machine) {
            continue;
        }
        let text = match s.state {
            State::Queued => String::new(),
            _ => match crate::sessions::hydrate::after_rebuild(app, &s).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(
                        session = s.id,
                        workspace,
                        error = format!("{e:#}"),
                        "can't hydrate a session's thread for its new box"
                    );
                    continue;
                }
            },
        };
        let prompt = NewPrompt { author: &s.asker, source: Source::Member, association: None, text: &text };
        let now = arugula_e2e::now_ms();
        match app.db.session_rebuilt(&s.id, s.state, new_machine, &prompt, now)? {
            Ok(_) => {
                tracing::info!(
                    session = s.id,
                    workspace,
                    machine = new_machine,
                    "a session moved to its workspace's new box"
                );
                crate::session_live::told(app, std::slice::from_ref(&s.id))?;
                app.db.nudge_session(&s.id, now)?;
            }
            // It moved meanwhile (it ended, say): its job sees to it.
            Err(e) => {
                tracing::warn!(session = s.id, workspace, refused = %e, "a session didn't move to its workspace's new box")
            }
        }
    }
    Ok(())
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
