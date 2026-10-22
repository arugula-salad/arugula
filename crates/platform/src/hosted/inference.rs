//! Hosted inference (M185, #1246): whose key an agent on a hosted box
//! runs on ("Hosted inference: strangers bring their own key", aru-096).
//!
//! - **Keys never enter the box.** A box's egress rules
//!   ([`compute::Rules`]) have the provider's proxy set the key on requests
//!   to `api.anthropic.com` (`x-api-key`) and `api.openai.com`
//!   (`Authorization: Bearer`). The box's agents hold only a placeholder
//!   ([`PLACEHOLDER`]), which the proxy overwrites; plain HTTP is never
//!   filled.
//! - **A project brings its own key**, one per provider, set and cleared
//!   by its owners and shown as set or not, never read back. The platform
//!   keeps it sealed with the project's key ([`crate::keys`], bound to the
//!   project and the provider) and opens it only to write it into its boxes' rules:
//!   at create and rebuild, on a wake, at a turn's start, and on every
//!   running box when it's set or cleared. E2B's proxy holds it there, per
//!   box, so no account-wide cap on secrets applies.
//! - **Else the platform's key**, for a workspace made by an account on
//!   the beta allowlist (`ARUGULA_CLOUD_ACCOUNTS`) when the platform covers
//!   that provider (`ARUGULA_HOSTED_PLATFORM_KEYS`). Each is a dedicated key
//!   (an Anthropic workspace's, an OpenAI project's) with a hard monthly
//!   spend limit at the provider: that's what bounds spend. It's a secret
//!   in E2B's broker ([`platform_secret`]) that the operator keeps
//!   (docs/control-ops.md), named in the rules and never held here. Turns
//!   on it count against the maker's caps ([`Caps`]): at once, per UTC
//!   day, and minutes per UTC day, for fairness between accounts.
//! - **A turn asks first** ([`turn`]): the box's daemon asks the hub
//!   before each agent turn (`POST /api/hosted/turns`, signed as the
//!   hosted machine, which the hub hands on through `PlatformPort`), and is
//!   told whose key pays, or why not (no key, with how to add one; a cap,
//!   with which). The box's rules are brought up to date first.
//! - **A box's rules can hold a key**, so only their digest is kept
//!   (`hosted_workspaces.rules`), and they're never logged.
//!
//! The seam M180 changes: today the daemon asks at each agent turn and
//! says when it ended. Busy and idle from the hosted daemon (M180) can
//! end a turn the daemon never ended; until then a turn with no end counts
//! for [`STALE_MS`](crate::db::inference::STALE_MS), and a paused box's
//! turns end.
//!
//! **What caps don't bound.** The rules set the platform's key on any
//! request from the box to the provider, not only an agent block's: a
//! member who runs `claude` in a terminal on an allowlisted account's box
//! uses it outside the caps. The provider's spend limit on the dedicated
//! key still holds, and strangers never get it.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use sha2::{Digest as _, Sha256};

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use crate::db::inference::Caps;
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    compute::{self, Rules},
    db::inference::{Cap, HostedBox, Pays, ProjectKey, Started},
    forge::links::membership,
    keys,
};

/// The caps when nothing says otherwise: two turns at once, a hundred a
/// day, two hours of them a day, per account.
pub const CAPS: Caps = Caps { concurrent: 2, daily: 100, minutes: 120 };

/// What a box's agents have for a key: the proxy writes the real one over
/// it, and a request that reaches the provider unfilled fails as an
/// unknown key.
pub const PLACEHOLDER: &str = "arugula-brokered";

/// A model provider whose key the proxy fills in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Anthropic, Provider::OpenAi];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// The provider an agent's turns go to: Claude Code's, Codex's.
    pub fn of_agent(agent: &str) -> Option<Provider> {
        match agent {
            "claude" => Some(Provider::Anthropic),
            "codex" => Some(Provider::OpenAi),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
        }
    }

    /// Where its API is, and the header its key goes in, as its SDKs send
    /// it.
    fn host(self) -> &'static str {
        match self {
            Provider::Anthropic => "api.anthropic.com",
            Provider::OpenAi => "api.openai.com",
        }
    }

    /// The header its key goes in: `key` is the key itself, or a broker
    /// reference ([`compute::fill`]).
    fn header(self, key: &str) -> (String, String) {
        match self {
            Provider::Anthropic => ("x-api-key".into(), key.to_owned()),
            Provider::OpenAi => ("authorization".into(), format!("Bearer {key}")),
        }
    }
}

/// The broker's secret for the platform's key, which the operator keeps:
/// the only E2B secrets Arugula uses.
pub fn platform_secret(p: Provider) -> String {
    format!("arugula-platform-{}", p.as_str())
}

/// Whose key agents run on: the allowlist the platform pays for, the
/// providers its key covers, and the caps on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub accounts: Vec<String>,
    pub platform: Vec<Provider>,
    pub caps: Caps,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { accounts: vec![], platform: vec![], caps: CAPS }
    }
}

impl Settings {
    /// From the flags. `*` on the allowlist is no one here: the platform's
    /// key never pays for everyone.
    pub fn new(accounts: &[String], platform: &[String], caps: Caps) -> Settings {
        if accounts.iter().any(|a| a.trim() == "*") {
            warn!("ARUGULA_CLOUD_ACCOUNTS has *: the platform's inference key pays only for accounts named there");
        }
        let accounts: Vec<String> =
            accounts.iter().map(|a| a.trim().to_owned()).filter(|a| !a.is_empty() && a != "*").collect();
        let mut platform: Vec<Provider> = platform.iter().filter_map(|p| Provider::parse(p.trim())).collect();
        platform.sort();
        platform.dedup();
        Settings { accounts, platform, caps }
    }

    /// Whether the platform's key pays for `p` in workspaces `maker` made.
    fn covers(&self, maker: Option<&str>, p: Provider) -> Option<String> {
        let maker = maker?;
        (self.platform.contains(&p) && self.accounts.iter().any(|a| a == maker)).then(|| maker.to_owned())
    }
}

/// A workspace's keys as they are now: whose pays for each provider, and
/// the egress rules that set them. The rules can hold a project's key:
/// they go to the provider and nowhere else.
pub struct Plan {
    pub payers: BTreeMap<Provider, Pays>,
    pub rules: Rules,
}

/// The plan for a workspace in `project` that `maker` made: the project's
/// own key, else the platform's for its maker, else none. A project key
/// that can't be opened (no `ARUGULA_CLOUD_SECRET_KEY`, or another one) is
/// an error, never a reason to pay with the platform's key.
pub fn plan(app: &Platform, project: &str, maker: Option<&str>) -> anyhow::Result<Plan> {
    let mut own = BTreeMap::new();
    for (name, sealed) in app.db.sealed_inference_keys(project)? {
        let Some(p) = Provider::parse(&name) else { continue };
        let cipher = app.hosted.cipher.as_deref().ok_or_else(|| {
            anyhow::anyhow!("project {project} has its own {} key, and this control has no key to open it", p.as_str())
        })?;
        let key = keys::open_inference(&app.db.c(), cipher, project, p.as_str(), &sealed)
            .map_err(|e| anyhow::anyhow!("project {project}'s own {} key doesn't open: {e:#}", p.as_str()))?;
        own.insert(p, key);
    }
    let mut plan = Plan { payers: BTreeMap::new(), rules: Rules::new() };
    for p in Provider::ALL {
        let key = if let Some(key) = own.remove(&p) {
            plan.payers.insert(p, Pays::Project);
            key
        } else if let Some(account) = app.hosted.inference.covers(maker, p) {
            plan.payers.insert(p, Pays::Platform { account });
            compute::fill(&platform_secret(p))
        } else {
            continue;
        };
        plan.rules.insert(p.host().to_owned(), BTreeMap::from([p.header(&key)]));
    }
    Ok(plan)
}

/// What's kept of a box's rules: their SHA-256.
pub fn digest(rules: &Rules) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(rules).unwrap_or_default()))
}

/// The rules a new box of this workspace is made with ([`super::worker`]).
pub fn rules_for(app: &Platform, project: &str, maker: Option<&str>) -> anyhow::Result<Rules> {
    Ok(plan(app, project, maker)?.rules)
}

/// What a hosted box's turn is told ([`turn`]): its turn id and whose
/// key pays (`project` or `platform`), or why it can't run, as the hub
/// answers the box: an HTTP status, a reason (`no_key`, `cap`) and a
/// sentence for the person. It crosses `PlatformPort`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnAnswer {
    Given { turn: String, pays: String },
    Refused { status: u16, reason: Option<String>, error: String },
}

impl TurnAnswer {
    fn refused(status: StatusCode, reason: Option<&str>, error: &str) -> TurnAnswer {
        TurnAnswer::Refused { status: status.as_u16(), reason: reason.map(str::to_owned), error: error.to_owned() }
    }
}

/// The workspace `workspace` whose box enrolled as `machine`, as the
/// hub's hosted certificate names both.
fn the_box(app: &Platform, workspace: &str, machine: &str) -> anyhow::Result<Option<HostedBox>> {
    Ok(app.db.hosted_box(machine)?.filter(|b| b.workspace == workspace))
}

/// Bring a box's rules up to date, when they've changed: whether they
/// are.
async fn sync_rules(app: &Platform, b: &HostedBox, want: &Rules) -> anyhow::Result<bool> {
    let digest = digest(want);
    if digest == b.rules {
        return Ok(true);
    }
    let (Some(sub), Some(sandbox)) = (app.compute.as_ref(), b.sandbox.as_deref()) else { return Ok(false) };
    if let Err(e) = sub.set_rules(sandbox, want).await {
        warn!(workspace = %b.workspace, error = %e, "couldn't set a hosted box's egress rules");
        return Ok(false);
    }
    app.db.set_hosted_rules(&b.workspace, &digest)?;
    Ok(true)
}

/// Bring a workspace's box's rules up to date: after a wake (a key may have
/// changed while it slept), or when a key changed. Whether they are.
pub async fn refresh(app: &Platform, b: &HostedBox) -> anyhow::Result<bool> {
    let plan = plan(app, &b.project, b.created_by.as_deref())?;
    sync_rules(app, b, &plan.rules).await
}

/// A project's key changed: bring its running boxes' rules up to date. A
/// box that can't be reached now catches up at its next turn or wake.
async fn push(app: &Platform, project: &str) -> anyhow::Result<()> {
    for b in app.db.running_boxes(project)? {
        if !refresh(app, &b).await? {
            warn!(workspace = %b.workspace, "a hosted box's key will catch up at its next turn");
        }
    }
    Ok(())
}

/// What a workspace with no key for `p` is told: how to add one.
pub fn no_key(b: &HostedBox, p: Provider) -> String {
    format!(
        "No {label} key for this workspace's project, and Arugula's key doesn't cover it. \
         An owner of the project adds one with `arugula cloud key set {name} --project {project}`. \
         It's never in the box.",
        label = p.label(),
        project = b.project,
        name = p.as_str(),
    )
}

fn capped(app: &Platform, account: &str, cap: Cap, b: &HostedBox, p: Provider) -> String {
    let c = app.hosted.inference.caps;
    let which = match cap {
        Cap::Concurrent => format!("{} agent turns at once", c.concurrent),
        Cap::Daily => format!("{} agent turns today (UTC)", c.daily),
        Cap::Minutes => format!("{} minutes of agent turns today (UTC)", c.minutes),
    };
    format!(
        "{account} is at Arugula's cap on its key: {which}. Wait, or add the project's own {label} key \
         (`arugula cloud key set {name} --project {project}`), which has no cap.",
        label = p.label(),
        name = p.as_str(),
        project = b.project,
    )
}

/// The start of an agent turn of `agent` (`claude`, `codex`) on a hosted
/// box, which the hub asks for it (`PlatformPort::hosted_turn`): whose
/// key pays, or why it can't run. The box's rules are brought up to date
/// first, whatever the answer. With no key for `agent`'s provider the
/// answer is `no_key`, whether or not the rules could be set; with one,
/// rules that couldn't be set are a 503.
pub async fn turn(app: &Platform, workspace: &str, machine: &str, agent: &str) -> anyhow::Result<TurnAnswer> {
    let Some(p) = Provider::of_agent(agent) else {
        return Ok(TurnAnswer::refused(StatusCode::BAD_REQUEST, None, "agent: claude or codex"));
    };
    let Some(b) = the_box(app, workspace, machine)? else {
        return Ok(TurnAnswer::refused(StatusCode::NOT_FOUND, None, "this machine isn't a live hosted workspace"));
    };
    let Plan { payers, rules } = plan(app, &b.project, b.created_by.as_deref())?;
    let synced = sync_rules(app, &b, &rules).await?;
    let Some(pays) = payers.get(&p) else {
        info!(workspace = %b.workspace, provider = p.as_str(), "a hosted agent turn has no key");
        return Ok(TurnAnswer::refused(StatusCode::FORBIDDEN, Some("no_key"), &no_key(&b, p)));
    };
    if !synced {
        let why = "couldn't set up this workspace's key: try again in a moment";
        return Ok(TurnAnswer::refused(StatusCode::SERVICE_UNAVAILABLE, None, why));
    }
    Ok(match app.db.start_hosted_turn(&b.workspace, p.as_str(), pays, &app.hosted.inference.caps, now_ms())? {
        Started::Turn(turn) => {
            let pays = match pays {
                Pays::Project => "project",
                Pays::Platform { .. } => "platform",
            };
            TurnAnswer::Given { turn, pays: pays.into() }
        }
        Started::Capped(cap) => {
            let Pays::Platform { account } = pays else { unreachable!("a project's key has no cap") };
            info!(workspace = %b.workspace, %account, ?cap, "a hosted agent turn is at a cap");
            TurnAnswer::refused(StatusCode::TOO_MANY_REQUESTS, Some("cap"), &capped(app, account, cap, &b, p))
        }
    })
}

/// An agent turn on a hosted box ended (`PlatformPort::hosted_turn_end`):
/// whether it was that box's, open.
pub async fn turn_end(app: &Platform, workspace: &str, machine: &str, turn: &str) -> anyhow::Result<bool> {
    let Some(b) = the_box(app, workspace, machine)? else { return Ok(false) };
    app.db.end_hosted_turn(&b.workspace, turn, now_ms())
}

/// `GET /api/projects/{project}/inference-keys`: which of the project's
/// own keys are set. Never a value.
#[derive(Debug, Serialize)]
pub struct Keys {
    pub project: String,
    pub keys: Vec<KeyShown>,
}

#[derive(Debug, Serialize)]
pub struct KeyShown {
    pub provider: String,
    pub set: bool,
    pub set_by: Option<String>,
    pub updated_at: Option<u64>,
}

fn shown(project: String, kept: Vec<ProjectKey>) -> Keys {
    let keys = Provider::ALL
        .into_iter()
        .map(|p| {
            let k = kept.iter().find(|k| k.provider == p.as_str());
            KeyShown {
                provider: p.as_str().into(),
                set: k.is_some(),
                set_by: k.and_then(|k| k.set_by.clone()),
                updated_at: k.map(|k| k.updated_at),
            }
        })
        .collect();
    Keys { project, keys }
}

/// `PUT /api/projects/{project}/inference-keys/{provider}`.
#[derive(Deserialize)]
pub struct NewKey {
    pub value: String,
}

impl std::fmt::Debug for NewKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NewKey { .. }")
    }
}

fn hosted_on(app: &Platform) -> Result<(), ApiError> {
    if app.compute.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "hosted workspaces aren't set up on this control"));
    }
    Ok(())
}

/// An owner of the project: only they set or clear its keys, which spend
/// their money.
async fn owner(app: &Platform, account: &str, project: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, project, true).await?;
    if role != TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, "only a project's owners set its inference keys"));
    }
    Ok(project)
}

fn provider(name: &str) -> Result<Provider, ApiError> {
    Provider::parse(name).ok_or_else(|| err(StatusCode::NOT_FOUND, "an inference key is anthropic or openai"))
}

/// A key as a header value can carry it: printable ASCII, no spaces.
fn good_key(v: &str) -> bool {
    (8..=512).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_graphic())
}

pub async fn list_keys(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
) -> Result<Json<Keys>, ApiError> {
    hosted_on(&app)?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let kept = app.db.inference_keys(&project)?;
    Ok(Json(shown(project, kept)))
}

pub async fn set_key(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, name)): Path<(String, String)>,
    Json(b): Json<NewKey>,
) -> Result<Json<Keys>, ApiError> {
    hosted_on(&app)?;
    let p = provider(&name)?;
    let project = owner(&app, &account, &project).await?;
    let value = b.value.trim();
    if !good_key(value) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "that doesn't look like an API key"));
    }
    let Some(cipher) = app.hosted.cipher.as_deref() else {
        return Err(err(
            StatusCode::NOT_IMPLEMENTED,
            "this control can't keep inference keys (it has no key to seal them)",
        ));
    };
    let sealed = keys::seal_value(&app.db.c(), cipher, &project, &keys::inference_name(p.as_str()), value)?;
    app.db.set_inference_key(&project, p.as_str(), &sealed, &account, now_ms())?;
    info!(%account, %project, provider = p.as_str(), "set a project's inference key");
    app.analytics.capture(&account, "inference key set", serde_json::json!({ "provider": p.as_str() }));
    push(&app, &project).await?;
    let kept = app.db.inference_keys(&project)?;
    Ok(Json(shown(project, kept)))
}

pub async fn clear_key(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, name)): Path<(String, String)>,
) -> Result<Json<Keys>, ApiError> {
    hosted_on(&app)?;
    let p = provider(&name)?;
    let project = owner(&app, &account, &project).await?;
    app.db.remove_inference_key(&project, p.as_str())?;
    info!(%account, %project, provider = p.as_str(), "cleared a project's inference key");
    // Off every running box now; a paused one's rules lose it when it
    // wakes, before anything there can ask for a turn.
    push(&app, &project).await?;
    let kept = app.db.inference_keys(&project)?;
    Ok(Json(shown(project, kept)))
}

/// How often [`sweep`] runs.
const SWEEP: Duration = Duration::from_secs(60);

/// Keys whose project is gone are dropped; a paused or deleted box's open
/// turns end; old turns go.
pub async fn sweep(app: &Platform) -> anyhow::Result<()> {
    app.db.drop_orphan_inference_keys()?;
    app.db.tidy_hosted_turns(now_ms())
}

/// The sweep, once a minute, while the platform runs.
pub fn start(app: Arc<Platform>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SWEEP).await;
            if app.is_paused() || app.is_stopping() {
                continue;
            }
            if let Err(error) = sweep(&app).await {
                warn!(%error, "the hosted inference sweep failed");
            }
        }
    });
}
