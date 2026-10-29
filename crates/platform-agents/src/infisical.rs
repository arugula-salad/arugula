//! The Infisical projection (M235, platform step 8): where a project's
//! secret has to reach a cluster, the platform writes it into Infisical, so
//! Kubernetes still gets it ("The platform is the one store for users'
//! project secrets; Infisical stays the operator's", aru-107). The
//! platform's sealed copy is the truth; Infisical's is a projection of it.
//!
//! - **redone's writer identity.** It writes as redone did: the
//!   `planter-apps-secret-writer` machine identity (universal auth), into
//!   the `planter-apps` project's environment `prod`, folder `/<app>/`,
//!   which planter-apps' operator renders into the app's sandbox or pod.
//!   That identity may create a folder, create and overwrite a secret, and
//!   list names; it can't read a value or delete one. The platform asks it
//!   for nothing else: never a read of a value, never a delete. Which
//!   identity it is, is configuration ([`Args`]); taking redone's over is
//!   Jake's to say (M235's brief).
//! - **Off, dry run, or on** ([`Mode`]), `ARUGULA_INFISICAL_PROJECTION`.
//!   Off (the default) writes nothing and logs nothing. A dry run writes
//!   nothing either, and logs what it would have written; with the
//!   identity's credentials it does the read-only folder check too. On
//!   writes.
//! - **Only into a folder that's the project's** ([`project`]): one it
//!   wrote before, or one the import carried over from redone (redone's
//!   writes made it). A folder that's there already and isn't (a cluster
//!   app's, which has secrets in `planter-apps` too) is refused, as redone
//!   refused to claim such a name; so is one another project wrote. Which
//!   app names a project may project at all is the caller's to check (the
//!   project's app claims).
//! - **The log** (`secret_projections`, [`crate::db::redone`]) keeps each
//!   write's project, app, name and outcome: never the value. Infisical's
//!   answers are read for their outcome only (a created secret's answer
//!   carries its value back, and is dropped).
//!
//! **The requests are redone's** (`backends.mjs` `infisicalClient`, which
//! runs against this identity in production today), and the answers'
//! shapes are Infisical's API reference's: `POST
//! /api/v1/auth/universal-auth/login` `{clientId, clientSecret}` →
//! `{accessToken, expiresIn, accessTokenMaxTTL, tokenType}`; `GET
//! /api/v1/folders?workspaceId&environment&path=/` → `{folders: [{name,
//! …}]}`; `POST /api/v1/folders` `{workspaceId, environment, path, name}`
//! → `{folder}`, or an error whose message says it exists; `POST`, and on
//! "already exist" `PATCH`, `/api/v3/secrets/raw/<KEY>` `{workspaceId,
//! environment, secretPath, secretValue, type: shared}` → `{secret}` (or
//! `{approval}` where a change policy holds it). Errors are `{reqId,
//! statusCode, message, error}`.

use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use arugula_e2e::now_ms;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    Platform,
    db::redone::{Projection as Logged, new_projection},
};

/// How the projection is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Writes nothing, logs nothing.
    #[default]
    Off,
    /// Writes nothing; logs what it would have written.
    DryRun,
    /// Writes.
    On,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s.trim() {
            "" | "off" => Mode::Off,
            "dry-run" | "dry_run" => Mode::DryRun,
            "on" => Mode::On,
            _ => return None,
        })
    }
}

/// The projection's configuration: the platform binary's flags.
#[derive(clap::Args, Clone, Default)]
#[group(id = "infisical")]
pub struct Args {
    /// The Infisical projection (M235): `off` (the default), `dry-run`
    /// (writes nothing, logs what it would), or `on`.
    #[arg(long = "infisical-projection", env = "ARUGULA_INFISICAL_PROJECTION", default_value = "off")]
    pub projection: String,
    /// Infisical's address (redone's `INFISICAL_API`).
    #[arg(long = "infisical-url", env = "ARUGULA_INFISICAL_URL")]
    pub url: Option<String>,
    /// The project secrets are projected into (`planter-apps`' id,
    /// redone's `INFISICAL_PROJECT_ID`).
    #[arg(long = "infisical-project-id", env = "ARUGULA_INFISICAL_PROJECT_ID")]
    pub project_id: Option<String>,
    /// Its environment's slug.
    #[arg(long = "infisical-environment", env = "ARUGULA_INFISICAL_ENVIRONMENT", default_value = "prod")]
    pub environment: String,
    /// The writer identity's universal-auth client id
    /// (`planter-apps-secret-writer`'s, redone's `INFISICAL_CLIENT_ID`).
    #[arg(long = "infisical-client-id", env = "ARUGULA_INFISICAL_CLIENT_ID", hide_env_values = true)]
    pub client_id: Option<String>,
    /// Its client secret (redone's `INFISICAL_CLIENT_SECRET`).
    #[arg(long = "infisical-client-secret", env = "ARUGULA_INFISICAL_CLIENT_SECRET", hide_env_values = true)]
    pub client_secret: Option<String>,
}

impl std::fmt::Debug for Args {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Args")
            .field("projection", &self.projection)
            .field("url", &self.url)
            .field("project_id", &self.project_id)
            .field("environment", &self.environment)
            .field("client_id", &self.client_id.as_ref().map(|_| "…"))
            .field("client_secret", &self.client_secret.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Args {
    /// The projection these say. `on` needs all four of the writer's
    /// settings; a dry run takes them when they're there.
    pub fn projection(&self, http: &reqwest::Client) -> anyhow::Result<Projection> {
        let mode = Mode::parse(&self.projection).with_context(|| {
            format!("ARUGULA_INFISICAL_PROJECTION is off, dry-run or on, not {:?}", self.projection)
        })?;
        let given = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let writer =
            match (given(&self.url), given(&self.project_id), given(&self.client_id), given(&self.client_secret)) {
                (Some(url), Some(project), Some(id), Some(secret)) => Some(Writer::new(
                    http.clone(),
                    url.trim_end_matches('/'),
                    &project,
                    self.environment.trim(),
                    &id,
                    &secret,
                )),
                _ if mode == Mode::On => bail!(
                    "ARUGULA_INFISICAL_PROJECTION=on needs ARUGULA_INFISICAL_URL, ARUGULA_INFISICAL_PROJECT_ID, \
                 ARUGULA_INFISICAL_CLIENT_ID and ARUGULA_INFISICAL_CLIENT_SECRET"
                ),
                _ => None,
            };
        Ok(Projection { mode, writer })
    }
}

/// The projection: its mode, and the writer when it has its credentials.
#[derive(Default)]
pub struct Projection {
    pub mode: Mode,
    pub writer: Option<Writer>,
}

impl Projection {
    pub fn new(mode: Mode, writer: Option<Writer>) -> Self {
        Projection { mode, writer }
    }
}

/// The platform's projection: the one the platform binary put in its
/// [`slots`](Platform::slots), else off.
pub fn projection(app: &Platform) -> &Projection {
    static OFF: Projection = Projection { mode: Mode::Off, writer: None };
    app.slots.get().unwrap_or(&OFF)
}

/// Infisical as one machine identity, writing only.
pub struct Writer {
    http: reqwest::Client,
    url: String,
    project: String,
    environment: String,
    client_id: String,
    client_secret: String,
    /// The access token, and until when it's used.
    token: Mutex<Option<(String, Instant)>>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writer").field("url", &self.url).field("project", &self.project).finish_non_exhaustive()
    }
}

/// How a write went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Put {
    /// Created or overwritten.
    Written,
    /// Infisical holds it for a change approval (a policy on the
    /// environment): not in effect yet.
    Held,
}

/// What's kept and said of a refused call: Infisical's status code only.
/// Its message goes to the platform's own log (at most 200 characters,
/// with `value` taken out if it's in there), never to the projection's
/// log or an agent.
fn why(what: &str, status: reqwest::StatusCode, body: &Value, value: Option<&str>) -> String {
    let m = body.get("message").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| body.to_string());
    let m = match value.filter(|v| !v.is_empty()) {
        Some(v) => m.replace(v, "…"),
        None => m,
    };
    tracing::warn!(status = status.as_u16(), message = %m.chars().take(200).collect::<String>(), "Infisical refused {what}");
    format!("{what}: Infisical answered {}", status.as_u16())
}

/// Whether Infisical's answer says the thing is there already (redone's
/// rule: "Folder already exists", "Secret already exist").
fn exists(body: &Value) -> bool {
    body.to_string().to_ascii_lowercase().contains("exist")
}

impl Writer {
    pub fn new(
        http: reqwest::Client,
        url: &str,
        project: &str,
        environment: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Self {
        Writer {
            http,
            url: url.to_owned(),
            project: project.to_owned(),
            environment: environment.to_owned(),
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            token: Mutex::new(None),
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> anyhow::Result<(reqwest::StatusCode, Value)> {
        let r = match req.timeout(Duration::from_secs(30)).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "Infisical didn't answer");
                bail!("Infisical didn't answer");
            }
        };
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::String(text))))
    }

    /// A live access token, logging in when there's none.
    async fn token(&self) -> anyhow::Result<String> {
        let mut held = self.token.lock().await;
        if let Some((t, until)) = held.as_ref()
            && Instant::now() < *until
        {
            return Ok(t.clone());
        }
        let body = json!({ "clientId": self.client_id, "clientSecret": self.client_secret });
        let (status, v) =
            self.send(self.http.post(format!("{}/api/v1/auth/universal-auth/login", self.url)).json(&body)).await?;
        if !status.is_success() {
            bail!("{}", why("logging in", status, &v, Some(&self.client_secret)));
        }
        let t = v["accessToken"].as_str().context("Infisical's login answered no accessToken")?.to_owned();
        let life = v["expiresIn"].as_u64().unwrap_or(600).saturating_sub(60).max(60);
        *held = Some((t.clone(), Instant::now() + Duration::from_secs(life)));
        Ok(t)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<(reqwest::StatusCode, Value)> {
        let mut req = self.http.request(method, format!("{}{path}", self.url)).bearer_auth(self.token().await?);
        if let Some(b) = body {
            req = req.json(b);
        }
        self.send(req).await
    }

    /// Whether the folder `/<app>/` is there (read only: names, never a
    /// value).
    pub async fn folder_exists(&self, app: &str) -> anyhow::Result<bool> {
        let q = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("workspaceId", &self.project)
            .append_pair("environment", &self.environment)
            .append_pair("path", "/")
            .finish();
        let (status, v) = self.call(reqwest::Method::GET, &format!("/api/v1/folders?{q}"), None).await?;
        if !status.is_success() {
            bail!("{}", why("listing folders", status, &v, None));
        }
        Ok(v["folders"].as_array().into_iter().flatten().any(|f| f["name"].as_str() == Some(app)))
    }

    /// Write `key` = `value` into `/<app>/`: the folder made if it isn't
    /// there, the secret created, or overwritten if it is.
    pub async fn put(&self, app: &str, key: &str, value: &str) -> anyhow::Result<Put> {
        let folder = json!({ "workspaceId": self.project, "environment": self.environment, "path": "/", "name": app });
        let (status, v) = self.call(reqwest::Method::POST, "/api/v1/folders", Some(&folder)).await?;
        if !status.is_success() && !exists(&v) {
            bail!("{}", why(&format!("making the folder /{app}"), status, &v, None));
        }
        let body = json!({
            "workspaceId": self.project,
            "environment": self.environment,
            "secretPath": format!("/{app}"),
            "secretValue": value,
            "type": "shared",
        });
        let path = format!("/api/v3/secrets/raw/{}", urlencoding(key));
        let (mut status, mut v) = self.call(reqwest::Method::POST, &path, Some(&body)).await?;
        if !status.is_success() && exists(&v) {
            (status, v) = self.call(reqwest::Method::PATCH, &path, Some(&body)).await?;
        }
        if !status.is_success() {
            bail!("{}", why(&format!("writing /{app}/{key}"), status, &v, Some(value)));
        }
        Ok(if v.get("secret").is_some() || v.get("approval").is_none() { Put::Written } else { Put::Held })
    }
}

/// A secret's name in a path: its names are `[A-Za-z_][A-Za-z0-9_]*`, but
/// say so anyway.
fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// An app's name as planter and redone take one: Infisical's folder and
/// the app's host's first label.
pub fn valid_app(name: &str) -> bool {
    let b = name.as_bytes();
    let ok = (1..=32).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-';
    // redone's and planter's reserved names (`plans.mjs` RESERVED).
    let reserved = name == "shipped"
        || name == "app"
        || name == "apps"
        || name == "platform"
        || name.starts_with("platform-")
        || name.starts_with("planter");
    ok && !reserved
}

/// A secret's name as Infisical's folder takes it, and redone's
/// `set_secret` did: an environment variable's.
pub fn valid_key(key: &str) -> bool {
    let b = key.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// What's being projected: a project's secret for one of its apps.
pub struct Ask<'a> {
    pub project: &'a str,
    pub app: &'a str,
    pub name: &'a str,
    pub value: &'a str,
    pub by_account: Option<&'a str>,
    pub by_agent: Option<&'a str>,
}

/// Project one secret, as the projection is set, and log it: `None` when
/// it's off. The caller has sealed the value as the project's (the truth)
/// and checked that the app is the project's.
pub async fn project(app: &Platform, ask: &Ask<'_>) -> anyhow::Result<Option<Logged>> {
    let p = projection(app);
    if p.mode == Mode::Off {
        return Ok(None);
    }
    let mode = if p.mode == Mode::On { "write" } else { "dry_run" };
    let log = |outcome: &str, detail: String| Logged {
        id: new_projection(),
        project: ask.project.to_owned(),
        app: ask.app.to_owned(),
        name: ask.name.to_owned(),
        mode: mode.into(),
        outcome: outcome.into(),
        detail,
        by_account: ask.by_account.map(str::to_owned),
        by_agent: ask.by_agent.map(str::to_owned),
        at: now_ms(),
    };
    let out = decide(app, ask).await;
    let row = match out {
        Err(Decided::Refused(why)) => log("refused", why),
        Err(Decided::Failed(why)) => log("failed", why),
        Ok(checked) if p.mode == Mode::DryRun => log(
            "would_write",
            format!(
                "would write /{}/{} in Infisical{}",
                ask.app,
                ask.name,
                if checked { "" } else { " (the folder wasn't checked: no writer credentials)" }
            ),
        ),
        Ok(_) => {
            let w = p.writer.as_ref().context("the projection is on with no writer")?;
            match w.put(ask.app, ask.name, ask.value).await {
                Ok(Put::Written) => log("written", format!("wrote /{}/{} in Infisical", ask.app, ask.name)),
                Ok(Put::Held) => log("failed", "Infisical holds the change for its approval policy".into()),
                Err(e) => log("failed", format!("{e:#}").chars().take(300).collect()),
            }
        }
    };
    app.db.note_projection(&row)?;
    tracing::info!(project = ask.project, app = ask.app, name = ask.name, outcome = %row.outcome, mode, "projected a secret");
    Ok(Some(row))
}

enum Decided {
    Refused(String),
    Failed(String),
}

/// Whether the write may go: `Ok(checked)`, where `checked` says the
/// folder was asked of Infisical (or didn't need to be).
async fn decide(app: &Platform, ask: &Ask<'_>) -> Result<bool, Decided> {
    if !valid_app(ask.app) {
        return Err(Decided::Refused(format!("{:?} isn't an app's name Infisical's folders take", ask.app)));
    }
    if !valid_key(ask.name) {
        return Err(Decided::Refused(format!("{:?} isn't an environment variable's name", ask.name)));
    }
    let failed = |e: anyhow::Error| Decided::Failed(format!("{e:#}"));
    // The app's name must be this project's, and only this project's, by
    // its planter ship or promote environment (M233's claims): the folder
    // is the claimed name, never an environment's own name.
    let holders = app.db.app_claimants(ask.app).map_err(failed)?;
    if holders.iter().any(|p| p != ask.project) {
        return Err(Decided::Refused(format!("the app {} is another project's", ask.app)));
    }
    if !holders.iter().any(|p| p == ask.project) {
        return Err(Decided::Refused(format!(
            "the app {} isn't this project's: it has no ship or promote environment for it",
            ask.app
        )));
    }
    if app.db.folder_elsewhere(ask.project, ask.app).map_err(failed)? {
        return Err(Decided::Refused(format!("Infisical's /{}/ is another project's", ask.app)));
    }
    if app.db.projects_folder(ask.project, ask.app).map_err(failed)? {
        return Ok(true);
    }
    let Some(w) = &projection(app).writer else { return Ok(false) };
    match w.folder_exists(ask.app).await {
        Ok(true) => Err(Decided::Refused(format!(
            "Infisical's /{}/ is there already and isn't this project's (a cluster app's): its secrets stay the \
             operator's",
            ask.app
        ))),
        Ok(false) => Ok(true),
        Err(e) => Err(failed(e)),
    }
}

#[cfg(test)]
#[path = "infisical_tests.rs"]
pub(crate) mod tests;
