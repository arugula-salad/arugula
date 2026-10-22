//! Managed cloud execution, separate from the opaque terminal relay.
//! The cloud service processes prompts and results; terminal relay encryption
//! and device authorization remain unchanged.
//!
//! Cloud workspaces (aru-046): workspaces, tabs, the leased worker, secrets
//! and Managoat. They moved here from control (M137), and reach the hub only
//! through [`HubPort`](crate::HubPort): who's asking (the gate's principal),
//! a machine's `cloud.drive` grant, the account's personal project, and push
//! for approvals. Their routes are in [`router`](crate::router()), on the
//! same paths as ever (`/api/cloud/…`).
pub mod account;
pub mod files;
pub mod migration;
pub mod notifications;
pub mod provider;
pub mod rebuild;
pub mod secrets;
pub mod tabs;
pub mod worker;
pub mod workspaces;

use crate::{
    Platform,
    auth::{Caller, Person},
    db::cloud::{AgentDefinition, AgentRules, CloudAgent},
};
use anyhow::{Result, bail};
pub use arugula_cloud_wire::CloudModel;
use arugula_control_common::{ApiError, Principal, err};
use axum::{
    Json,
    extract::{FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// Cloud's flags and their env vars. They moved here from control's with
/// the same names (M137), so a deploy's secrets don't change: control takes
/// them (`#[command(flatten)]`), and so will the platform's own binary
/// (M140).
#[derive(clap::Args, Debug, Clone)]
#[group(id = "cloud")]
pub struct Args {
    /// Managed cloud agents. Credentials are held only by control.
    #[arg(long, env = "ARUGULA_MANAGOAT_URL", default_value = "https://managoat.com")]
    pub cloud_provider_url: String,
    #[arg(long, env = "ARUGULA_MANAGOAT_API_KEY", hide_env_values = true)]
    pub cloud_provider_key: Option<String>,
    /// Separate 32-byte hex encryption key for user cloud secrets. Back up with deployment secrets.
    #[arg(long, env = "ARUGULA_CLOUD_SECRET_KEY", hide_env_values = true)]
    pub cloud_secret_key: Option<String>,
    /// The master key before this one, only while it's rotated
    /// (docs/control-ops.md): it still opens what it wrapped.
    #[arg(long, env = "ARUGULA_CLOUD_SECRET_KEY_PREVIOUS", hide_env_values = true)]
    pub cloud_secret_key_previous: Option<String>,
    /// The master key after this one, only while it's rotated, before it
    /// seals anything: it opens what a machine that has it as
    /// ARUGULA_CLOUD_SECRET_KEY wraps.
    #[arg(long, env = "ARUGULA_CLOUD_SECRET_KEY_NEXT", hide_env_values = true)]
    pub cloud_secret_key_next: Option<String>,
    /// Limited beta allowlist; empty disables access for all accounts. Hosted
    /// workspaces these accounts make run on the platform's inference key
    /// (M185, `hosted::inference`).
    #[arg(long, env = "ARUGULA_CLOUD_ACCOUNTS", value_delimiter = ',')]
    pub cloud_accounts: Vec<String>,
    /// Platform-funded model, required when cloud execution is configured
    /// and ARUGULA_CLOUD_MODELS is unset.
    #[arg(long, env = "ARUGULA_CLOUD_MODEL")]
    pub cloud_model: Option<String>,
    #[arg(long, env = "ARUGULA_CLOUD_RUNTIME", default_value = "claude", value_parser = ["claude", "codex", "gemini"])]
    pub cloud_runtime: String,
    /// What a Cloud workspace's tabs may run on: `runtime:model` pairs,
    /// comma-separated, the first the default. Unset, it's
    /// ARUGULA_CLOUD_RUNTIME:ARUGULA_CLOUD_MODEL.
    #[arg(long, env = "ARUGULA_CLOUD_MODELS", value_delimiter = ',')]
    pub cloud_models: Vec<String>,
    /// Retired with Cloud runs (S40, decision 10): still parsed, so an old
    /// deploy starts, and do nothing but say so. Hosted agents' caps on the
    /// platform's key are `hosted::Args`' (M185).
    #[arg(long, env = "ARUGULA_CLOUD_DAILY_RUNS", hide = true)]
    pub cloud_daily_runs: Option<String>,
    #[arg(long, env = "ARUGULA_CLOUD_CONCURRENT_RUNS", hide = true)]
    pub cloud_concurrent_runs: Option<String>,
    #[arg(long, env = "ARUGULA_CLOUD_RUN_MINUTES", hide = true)]
    pub cloud_run_minutes: Option<String>,
}

impl Args {
    /// The master key these flags give (M209: it wraps each project's data
    /// key, [`crate::keys`]), with the previous or next one while it's
    /// rotated. Unset is none; set but empty is refused, as any key that
    /// isn't one is.
    pub fn master_key(&self) -> Result<Option<secrets::SecretCipher>> {
        self.cloud_secret_key
            .as_deref()
            .map(|k| {
                anyhow::ensure!(
                    !k.trim().is_empty(),
                    "ARUGULA_CLOUD_SECRET_KEY is set but empty: unset it, or give a key"
                );
                secrets::SecretCipher::new(k)?
                    .with_previous(self.cloud_secret_key_previous.as_deref())?
                    .with_next(self.cloud_secret_key_next.as_deref())
            })
            .transpose()
    }

    /// The Cloud these flags set up: none without a Managoat key.
    pub fn cloud(self) -> Result<Option<Cloud>> {
        for (name, set) in [
            ("ARUGULA_CLOUD_DAILY_RUNS", self.cloud_daily_runs.is_some()),
            ("ARUGULA_CLOUD_CONCURRENT_RUNS", self.cloud_concurrent_runs.is_some()),
            ("ARUGULA_CLOUD_RUN_MINUTES", self.cloud_run_minutes.is_some()),
        ] {
            if set {
                tracing::warn!(
                    "{name} does nothing: Cloud runs were retired. Hosted agents' caps are ARUGULA_HOSTED_CONCURRENT_TURNS, ARUGULA_HOSTED_DAILY_TURNS and ARUGULA_HOSTED_TURN_MINUTES"
                );
            }
        }
        let secret_cipher = self.master_key()?;
        Ok(match self.cloud_provider_key.filter(|k| !k.is_empty()) {
            Some(key) => Some(Cloud {
                secret_cipher,
                provider: provider::Provider::new(&self.cloud_provider_url, key)?,
                accounts: self.cloud_accounts,
                models: configured_models(self.cloud_model, self.cloud_runtime, &self.cloud_models)?,
            }),
            None => None,
        })
    }
}

/// Cloud's background work, when it's configured: the leased worker and
/// approval pushes.
pub fn start(app: &Arc<Platform>) {
    if app.cloud.is_some() {
        worker::start(app.clone());
        notifications::start(app.clone());
    }
}

/// Someone driving the account's Cloud workspaces: a signed-in person, a
/// CLI device, or (#1094) one of the account's daemons, for the agents on
/// that machine. A daemon acts as its account (its owner), so anyone who
/// may run things on that machine drives the owner's Cloud workspaces
/// through it; that was decided, not overlooked. Since M130 (#1126) the
/// daemon needs a live `cloud.drive` grant over its account ("Grants, not
/// impersonation"): every joined machine has one until its owner revokes
/// it, kept by the platform ([`crate::grants::can_drive_cloud`]). Who's asking is
/// the hub's to say (the gate), with a person's same-origin check. Only the
/// Cloud workspace and tab routes take one: settings, secrets, deleting and
/// the agent library stay a person's ([`Person`]).
pub struct CloudDriver(pub Person);

/// What a machine without a live `cloud.drive` grant is told: which
/// machine, and how its owner gives the power back.
pub fn no_cloud_drive(name: &str) -> String {
    format!(
        "{name} can't drive this account's Cloud workspaces: its cloud.drive grant was revoked. \
         To restore it, run `arugula grants restore {name}` on a device logged in to the account (`arugula login`)"
    )
}

impl FromRequestParts<Arc<Platform>> for CloudDriver {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<Platform>) -> Result<Self, ApiError> {
        let Caller(who) = Caller::from_request_parts(parts, app).await?;
        let (account, daemon) = match who {
            Principal::Person { account } | Principal::Cli { account, .. } => {
                return Ok(CloudDriver(Person { account }));
            }
            Principal::Daemon { account, daemon } => (account, daemon),
        };
        if !crate::grants::can_drive_cloud(app, &daemon, &account).await? {
            let name = app
                .cache
                .daemons(&*app.hub, std::slice::from_ref(&daemon))
                .await?
                .remove(&daemon)
                .map_or(daemon.clone(), |d| d.name);
            tracing::warn!(what = "cloud_drive", %account, %daemon, why = "no live cloud.drive grant", "refused");
            return Err(err(StatusCode::FORBIDDEN, &no_cloud_drive(&name)));
        }
        Ok(CloudDriver(Person { account }))
    }
}

/// The account's personal project, which its Cloud workspaces and cloud
/// secrets belong to (M129), made the first time. An account that's gone
/// is refused as it always was (a 500: nobody should be asking).
async fn personal_project(app: &Platform, account: &str) -> Result<String, ApiError> {
    Ok(crate::projects::personal_project(app, account)
        .await?
        .ok_or_else(|| anyhow::anyhow!("account no longer exists"))?)
}

/// That the account is still there, before keeping anything of its: as
/// the cache has it, and asked again before refusing.
async fn require_account(app: &Platform, account: &str) -> Result<(), ApiError> {
    if !app.cache.account_there(&*app.hub, account).await? {
        return Err(anyhow::anyhow!("account no longer exists").into());
    }
    Ok(())
}

/// A clone URL Managoat may fetch: HTTPS, with no credentials in it.
pub fn https_repository(repository: &str) -> bool {
    url::Url::parse(repository).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

/// Whether `runtime` drives `model`, a canonical `provider/model_id`.
pub fn model_fits_runtime(runtime: &str, model: &str) -> bool {
    let prefix = match runtime {
        "claude" => "anthropic/",
        "codex" => "openai/",
        "gemini" => "google/",
        _ => return false,
    };
    model.starts_with(prefix) && model.len() > prefix.len() && !model.chars().any(|c| c.is_whitespace())
}

/// `ARUGULA_CLOUD_MODELS`: `runtime:model` pairs, the first the default.
/// Each pair's platform inference must be set up on the Managoat account.
pub fn parse_models(pairs: &[String]) -> Result<Vec<CloudModel>> {
    let mut models: Vec<CloudModel> = vec![];
    for pair in pairs.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let Some((runtime, model)) = pair.split_once(':') else {
            bail!("ARUGULA_CLOUD_MODELS takes runtime:model pairs, not {pair:?}");
        };
        let model = CloudModel { runtime: runtime.trim().into(), model: model.trim().into() };
        if !model_fits_runtime(&model.runtime, &model.model) {
            bail!("cloud model {:?} must match its runtime {:?}", model.model, model.runtime);
        }
        if models.contains(&model) {
            bail!("ARUGULA_CLOUD_MODELS lists {pair:?} twice");
        }
        models.push(model);
    }
    Ok(models)
}

/// What tabs may run on: `ARUGULA_CLOUD_MODELS`, or else the one pair
/// `ARUGULA_CLOUD_RUNTIME`:`ARUGULA_CLOUD_MODEL`, so a deploy from before
/// the list keeps working. With the list set, the other two are unused.
pub fn configured_models(model: Option<String>, runtime: String, pairs: &[String]) -> Result<Vec<CloudModel>> {
    let models = parse_models(pairs)?;
    if !models.is_empty() {
        return Ok(models);
    }
    let Some(model) = model.filter(|m| !m.trim().is_empty()) else {
        bail!("ARUGULA_CLOUD_MODEL or ARUGULA_CLOUD_MODELS is required for cloud agents");
    };
    if !model_fits_runtime(&runtime, &model) {
        bail!("cloud model must match the configured runtime");
    }
    Ok(vec![CloudModel { runtime, model }])
}

pub struct Cloud {
    pub provider: provider::Provider,
    pub secret_cipher: Option<secrets::SecretCipher>,
    pub accounts: Vec<String>,
    /// What a tab may run on; never empty.
    pub models: Vec<CloudModel>,
}

fn enabled<'a>(app: &'a Platform, account: &str) -> Result<&'a Cloud, ApiError> {
    let cloud = app.cloud.as_ref().ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud agents are not configured"))?;
    if !cloud.accounts.iter().any(|a| a == account || a == "*") {
        return Err(err(StatusCode::FORBIDDEN, "cloud agents are in a limited beta"));
    }
    Ok(cloud)
}

fn curated_agents() -> Vec<CloudAgent> {
    [
        ("coding", "Coding agent", "Implement the requested change in the checked-out repository. Read its instructions first, run relevant checks, and explain the changes and verification."),
        ("review", "Code reviewer", "Review the checked-out repository for the requested concerns. Explain concrete findings with file references, severity, and suggested fixes. Do not modify files unless asked."),
    ].into_iter().map(|(id, name, instructions)| CloudAgent {
        id: format!("curated_{id}"), version: 1, curated: true,
        definition: AgentDefinition { name: name.into(), instructions: instructions.into(), mcp_servers: Default::default(), skills: vec![], secret_names: vec![] },
    }).collect()
}

pub async fn catalog(
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    let mut agents = curated_agents();
    agents.extend(app.db.cloud_agents(&session.account)?);
    Ok(Json(json!({ "agents": agents })))
}

/// The runtimes and models **+ Agent** offers, the first the default.
pub async fn models(
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    Ok(Json(json!({ "models": cloud.models })))
}

pub async fn create_agent(
    State(app): State<Arc<Platform>>,
    session: Person,
    Json(definition): Json<AgentDefinition>,
) -> Result<Json<CloudAgent>, ApiError> {
    enabled(&app, &session.account)?;
    definition.validate().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    let secrets = definition.secret_names.len();
    require_account(&app, &session.account).await?;
    let agent = app.db.create_cloud_agent(&session.account, definition)?;
    app.analytics.capture(&session.account, "cloud agent created", json!({ "secrets": secrets }));
    Ok(Json(agent))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUpdate {
    version: u64,
    definition: AgentDefinition,
}

pub async fn update_agent(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
    Json(update): Json<AgentUpdate>,
) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    update.definition.validate().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    if !app.db.cloud_agents(&session.account)?.iter().any(|a| a.id == id) {
        return Err(err(StatusCode::NOT_FOUND, "agent not found"));
    }
    if !app.db.update_cloud_agent(&session.account, &id, update.version, &update.definition)? {
        return Err(err(StatusCode::CONFLICT, "agent changed; reload before saving"));
    }
    app.analytics.capture(&session.account, "cloud agent updated", json!({ "version": update.version + 1 }));
    Ok(Json(json!({"version": update.version + 1})))
}

pub async fn delete_agent(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    if !app.db.archive_cloud_agent(&session.account, &id)? {
        return Err(err(StatusCode::NOT_FOUND, "agent not found"));
    }
    app.analytics.capture(&session.account, "cloud agent deleted", json!({}));
    Ok(Json(json!({})))
}

/// A curated agent or one of the account's.
fn find_agent(app: &Platform, account: &str, id: &str) -> Result<CloudAgent, ApiError> {
    let mut agents = curated_agents();
    agents.extend(app.db.cloud_agents(account)?);
    agents.into_iter().find(|a| a.id == id).ok_or_else(|| err(StatusCode::NOT_FOUND, "agent not found"))
}

pub async fn runs(State(app): State<Arc<Platform>>, session: Person) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({"runs":app.db.cloud_runs(&session.account)?})))
}

pub async fn run(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Json<crate::db::cloud::CloudRun>, ApiError> {
    Ok(Json(app.db.cloud_run(&session.account, &id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "run not found"))?))
}

#[derive(Deserialize)]
pub struct EventQuery {
    #[serde(default)]
    after: u64,
}
pub async fn events(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<EventQuery>,
) -> Result<Json<Value>, ApiError> {
    if app.db.cloud_run(&session.account, &id)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "run not found"));
    }
    Ok(Json(json!({"events":app.db.cloud_events(&session.account,&id,query.after)?})))
}

/// A past run's pending requests: none once it's stopped, but the
/// transcript view still reads them.
pub async fn requests(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if app.db.cloud_run(&session.account, &id)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "run not found"));
    }
    Ok(Json(json!({"requests":app.db.cloud_requests(&session.account,&id)?})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    option_id: Option<String>,
    action: Option<String>,
    content: Option<Value>,
}
/// Answer a request in a tab's conversation (`id`), whose Managoat
/// conversation is `conversation`.
async fn deliver_answer(
    app: &Platform,
    account: &str,
    id: &str,
    conversation: &str,
    request: &str,
    answer: Answer,
) -> Result<Json<Value>, ApiError> {
    let cloud =
        app.cloud.as_ref().ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "cloud execution is unavailable"))?;
    let kind = if answer.option_id.is_some() { "option" } else { answer.action.as_deref().unwrap_or("") };
    let kind = kind.to_owned();
    let body = match (answer.option_id, answer.action) {
        (Some(option), None) if !option.is_empty() && option.len() <= 256 && answer.content.is_none() => {
            json!({"option_id":option})
        }
        (None, Some(action)) if matches!(action.as_str(), "accept" | "decline" | "cancel") => {
            json!({"action":action,"content":answer.content})
        }
        _ => return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid answer")),
    };
    cloud.provider.answer(conversation, request, body).await.map_err(|e| {
        if let Some(provider::HttpFailure(code, _)) = e.downcast_ref::<provider::HttpFailure>() {
            if *code == 409 {
                let _ = app.db.resolve_cloud_request(account, id, request);
                return err(StatusCode::CONFLICT, "request already answered or expired");
            }
            if *code == 422 {
                return err(StatusCode::UNPROCESSABLE_ENTITY, "answer does not match the request");
            }
        }
        err(StatusCode::BAD_GATEWAY, "could not deliver the answer; refresh before retrying")
    })?;
    app.db.resolve_cloud_request(account, id, request)?;
    app.analytics.capture(account, "cloud request answered", json!({ "answer": kind }));
    Ok(Json(json!({})))
}

pub async fn list_secrets(State(app): State<Arc<Platform>>, session: Person) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    Ok(Json(json!({"names":app.db.cloud_secret_names(&session.account)?,"available":cloud.secret_cipher.is_some()})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretInput {
    value: String,
}
pub async fn put_secret(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(name): Path<String>,
    Json(input): Json<SecretInput>,
) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    let cipher = cloud
        .secret_cipher
        .as_ref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "cloud secrets are not configured"))?;
    if !secrets::valid_name(&name) || input.value.is_empty() || input.value.len() > 64_000 {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid secret name or value"));
    }
    if app.db.cloud_secret_names(&session.account)?.len() >= 100
        && !app.db.cloud_secret_names(&session.account)?.contains(&name)
    {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "secret limit reached"));
    }
    let project = personal_project(&app, &session.account).await?;
    let sealed = crate::keys::seal_value(
        &app.db.c(),
        cipher,
        &project,
        &crate::keys::cloud_name(&session.account, &name),
        &input.value,
    )?;
    app.db.put_cloud_secret(&session.account, &project, &name, &sealed)?;
    app.analytics.capture(&session.account, "cloud secret saved", json!({}));
    Ok(Json(json!({"name":name})))
}
pub async fn delete_secret(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    app.db.delete_cloud_secret(&session.account, &name)?;
    app.analytics.capture(&session.account, "cloud secret deleted", json!({}));
    Ok(Json(json!({})))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::auth::{Asked, Credential};
    use crate::hub::{
        Account, ChangePage, Daemon, Flags, HubPort, HubResult, Meter, PushOptions, Pushed, Resolved, Team,
    };
    use futures_util::future::BoxFuture;
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Mutex,
    };

    /// The hub, as far as Cloud asks it: accounts `a`, `b` and `c`, the
    /// machines joined to them (whose `cloud.drive` grants are the
    /// platform's own), and the pushes it was asked for. Control's own port
    /// keeps the same contract (`contract`, run in `hub_port`), and the
    /// tests of the whole path through it are control's (`grants_wire`).
    pub(crate) struct TestHub {
        pub accounts: Mutex<BTreeSet<String>>,
        /// Machine id → (account, name).
        pub daemons: Mutex<BTreeMap<String, (String, String)>>,
        /// What each push said, to whom, and how it was to go.
        pub pushes: Mutex<Vec<(String, Value)>>,
        pub options: Mutex<Vec<PushOptions>>,
        /// How every push goes.
        pub pushed: Mutex<Pushed>,
    }

    impl Default for TestHub {
        fn default() -> Self {
            TestHub {
                accounts: Mutex::new(["a", "b", "c"].map(String::from).into()),
                daemons: Default::default(),
                pushes: Default::default(),
                options: Default::default(),
                pushed: Mutex::new(Pushed { sent: 1, gone: 0, failed: 0 }),
            }
        }
    }

    impl TestHub {
        /// A machine joined to `account`: its id.
        pub(crate) fn daemon(&self, account: &str, name: &str) -> String {
            let id = format!("dmn-{}", hex::encode(arugula_e2e::random::<8>()));
            self.daemons.lock().unwrap().insert(id.clone(), (account.into(), name.into()));
            id
        }
    }

    fn done<'a, T: Send + 'a>(t: T) -> BoxFuture<'a, HubResult<T>> {
        Box::pin(async move { Ok(t) })
    }

    impl HubPort for TestHub {
        fn resolve<'a>(&'a self, _: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
            done(None)
        }
        fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
            let known = self.accounts.lock().unwrap();
            let found = ids
                .iter()
                .filter(|id| known.contains(*id))
                .map(|id| Account { id: id.clone(), login: id.clone(), name: id.clone() })
                .collect();
            done(found)
        }
        fn accounts_called<'a>(&'a self, _: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
            done(vec![])
        }
        fn teams_of<'a>(&'a self, _: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
            done(vec![])
        }
        fn teams<'a>(&'a self, _: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
            done(vec![])
        }
        fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
            let known = self.daemons.lock().unwrap();
            let found = ids
                .iter()
                .filter_map(|id| {
                    let (account, name) = known.get(id)?;
                    Some(Daemon { id: id.clone(), account: account.clone(), team: None, name: name.clone() })
                })
                .collect();
            done(found)
        }
        fn machines_for<'a>(&'a self, _: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
            done(vec![])
        }
        fn may_reach<'a>(&'a self, _: &'a str, _: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
            done(vec![])
        }
        fn push<'a>(&'a self, account: &'a str, message: &'a Value) -> BoxFuture<'a, HubResult<Pushed>> {
            self.push_with(account, message, PushOptions::default())
        }
        fn push_with<'a>(
            &'a self,
            account: &'a str,
            message: &'a Value,
            options: PushOptions,
        ) -> BoxFuture<'a, HubResult<Pushed>> {
            self.pushes.lock().unwrap().push((account.into(), message.clone()));
            self.options.lock().unwrap().push(options);
            done(*self.pushed.lock().unwrap())
        }
        fn labs_flags<'a>(&'a self, _: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>> {
            done(BTreeMap::new())
        }
        fn meters<'a>(&'a self, _: &'a [String], _: u64, _: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
            done(vec![])
        }
        fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
            done(vec![])
        }
        fn changes(&self, _: u64, _: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
            done(ChangePage { changes: vec![], last: 0, gap: false })
        }
        fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
            let known = self.accounts.lock().unwrap().contains(account);
            done(known.then(|| format!("prj_{account}")))
        }
        fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>> {
            let mine = self.daemons.lock().unwrap().get(daemon).is_some_and(|(a, _)| a == account);
            done(mine)
        }
        fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
            done(Some(format!("prj_{team}")))
        }
    }

    impl Platform {
        /// In its `Arc`, as control holds it.
        pub(crate) fn start(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    pub(crate) fn test_app() -> Arc<Platform> {
        unstarted_app().start()
    }

    /// [`test_app`] before it's started, to change first.
    pub(crate) fn unstarted_app() -> Platform {
        unstarted_app_with(Arc::default())
    }

    /// [`unstarted_app`], with a hub the test looks at.
    pub(crate) fn unstarted_app_with(hub: Arc<TestHub>) -> Platform {
        let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
        let mut app = Platform::new(cfg, crate::db::Db::test(), hub, reqwest::Client::new(), Default::default());
        app.cloud = Some(Cloud {
            secret_cipher: Some(secrets::SecretCipher::new(&"01".repeat(32)).unwrap()),
            provider: provider::Provider::new("https://provider.example", "platform-key".into()).unwrap(),
            accounts: vec!["a".into(), "b".into()],
            models: vec![
                CloudModel { runtime: "claude".into(), model: "anthropic/test-model".into() },
                CloudModel { runtime: "codex".into(), model: "openai/test-model".into() },
            ],
        });
        app
    }

    /// The platform's routes behind a gate that says who's asking as the
    /// hub's would: `ilg_session=session-<account>` is that account's
    /// session (with the same-origin check), and `x-test-daemon: <account>
    /// <machine>` that machine's checked signature.
    pub(crate) fn router(app: Arc<Platform>) -> axum::Router {
        crate::router(app).layer(axum::middleware::from_fn(gate))
    }

    async fn gate(mut req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
        let headers = req.headers();
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let session = header("cookie").and_then(|c| {
            c.split(';').find_map(|kv| kv.trim().strip_prefix("ilg_session=session-").map(str::to_owned))
        });
        let asked = if let Some(d) = header("x-test-daemon") {
            let (account, daemon) = d.split_once(' ').unwrap();
            Some(Asked::Principal(Principal::Daemon { account: account.into(), daemon: daemon.into() }))
        } else if let Some(account) = session {
            let foreign = header("origin").is_some_and(|o| o != "https://control.example");
            Some(if req.method() != axum::http::Method::GET && foreign {
                Asked::Refused {
                    status: StatusCode::FORBIDDEN,
                    error: "cross-origin request refused".into(),
                    by: Credential::Session,
                }
            } else {
                Asked::Principal(Principal::Person { account })
            })
        } else {
            None
        };
        if let Some(a) = asked {
            req.extensions_mut().insert(a);
        }
        next.run(req).await
    }

    #[test]
    fn models_fall_back_to_the_one_configured_pair() {
        let pair = |runtime: &str, model: &str| CloudModel { runtime: runtime.into(), model: model.into() };
        let models = configured_models(Some("anthropic/one".into()), "claude".into(), &[]).unwrap();
        assert_eq!(models, [pair("claude", "anthropic/one")]);
        let listed = ["claude:anthropic/a".to_owned(), " codex:openai/b ".into(), "gemini:google/c".into()];
        let models = configured_models(None, "claude".into(), &listed).unwrap();
        assert_eq!(models, [pair("claude", "anthropic/a"), pair("codex", "openai/b"), pair("gemini", "google/c")]);
        // The list wins when both are set.
        let models = configured_models(Some("openai/old".into()), "codex".into(), &listed).unwrap();
        assert_eq!(models[0], pair("claude", "anthropic/a"));
        for (model, runtime, pairs) in [
            (None, "claude", vec![]),
            (Some("openai/x"), "claude", vec![]),
            (None, "claude", vec!["codex:anthropic/x".to_owned()]),
            (None, "claude", vec!["anthropic/x".to_owned()]),
            (None, "claude", vec!["acp:anthropic/x".to_owned()]),
            (None, "claude", vec!["claude:anthropic/x".to_owned(), "claude:anthropic/x".into()]),
        ] {
            assert!(
                configured_models(model.map(str::to_owned), runtime.into(), &pairs).is_err(),
                "{model:?} {pairs:?}"
            );
        }
    }

    #[tokio::test]
    async fn catalog_http_auth_isolation_and_edit_conflicts() {
        let app = test_app();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/cloud/agents", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router(app)).await.unwrap() });
        let client = reqwest::Client::new();
        assert_eq!(client.get(&base).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            client.get(&base).header("cookie", "ilg_session=session-c").send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let definition =
            json!({"name":"My agent", "instructions":"Review code", "mcp_servers":{}, "skills":[], "secret_names":[]});
        assert_eq!(
            client
                .post(&base)
                .header("cookie", "ilg_session=session-a")
                .header("origin", "https://other.example")
                .json(&definition)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response =
            client.post(&base).header("cookie", "ilg_session=session-a").json(&definition).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = response.json().await.unwrap();
        let item = format!("{base}/{}", created["id"].as_str().unwrap());
        let b: Value =
            client.get(&base).header("cookie", "ilg_session=session-b").send().await.unwrap().json().await.unwrap();
        assert_eq!(b["agents"].as_array().unwrap().len(), 2);
        assert!(b["agents"].as_array().unwrap().iter().all(|a| a["curated"] == true));
        assert!(!b.to_string().contains("platform-key"));
        assert_eq!(
            client.delete(&item).header("cookie", "ilg_session=session-b").send().await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        let update = json!({"version":1, "definition":definition});
        assert_eq!(
            client.put(&item).header("cookie", "ilg_session=session-a").json(&update).send().await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(
            client.put(&item).header("cookie", "ilg_session=session-a").json(&update).send().await.unwrap().status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            client.delete(&item).header("cookie", "ilg_session=session-a").send().await.unwrap().status(),
            StatusCode::OK
        );
        let a: Value =
            client.get(&base).header("cookie", "ilg_session=session-a").send().await.unwrap().json().await.unwrap();
        assert_eq!(a["agents"].as_array().unwrap().len(), 2);
        server.abort();
    }
}
