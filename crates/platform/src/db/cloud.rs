//! Durable cloud execution records. Provider identifiers never form the public
//! authorization boundary: every lookup is scoped to the owning account.
//! The tables, with notes on their columns, are made by the platform's
//! migrations (`migrations/sqlite`).
use super::{
    Db, Taken,
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, params},
    taken,
};
use anyhow::{Context as _, Result, bail};
pub use arugula_cloud_wire::{
    AgentDefinition, CloudAgent, CloudRun, CloudTab, CloudWorkspace, CloudWorkspaceChange, CloudWorkspaceNetworking,
    CloudWorkspacePackages, CloudWorkspaceRepository, CloudWorkspaceSpec, PastCloudTab, RunSpec, Skill,
};
use serde::{Deserialize, Serialize};

/// What control checks of an agent's definition before keeping it.
pub trait AgentRules {
    fn validate(&self) -> Result<()>;
}

impl AgentRules for AgentDefinition {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 120 {
            bail!("name must contain 1–120 bytes");
        }
        if self.instructions.trim().is_empty() || self.instructions.len() > 64_000 {
            bail!("instructions must contain 1–64000 bytes");
        }
        if self.mcp_servers.len() > 20 || self.mcp_servers.values().any(|v| !v.is_object()) {
            bail!("tools must contain at most 20 MCP server objects");
        }
        if self.skills.len() > 20
            || self.skills.iter().any(|s| {
                s.name.is_empty()
                    || !s.name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    || s.name.len() > 80
                    || s.content.len() > 64_000
            })
        {
            bail!("invalid skills");
        }
        if self.secret_names.len() > 32 || self.secret_names.iter().any(|s| !crate::cloud::secrets::valid_name(s)) {
            bail!("invalid or reserved secret name");
        }
        if serde_json::to_vec(self)?.len() > 256_000 {
            bail!("agent definition is too large");
        }
        Ok(())
    }
}

/// Internal provisioning journal. Persist intent before contacting the provider
/// so a restart reconciles an uncertain creation instead of duplicating it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Execution {
    pub stage: String,
    pub environment: Option<String>,
    pub agent: Option<String>,
    pub conversation: Option<String>,
    pub sandbox: Option<String>,
    pub cursor: u64,
    pub outcome: Option<String>,
    #[serde(default)]
    pub secrets_uploaded: Vec<String>,
}

/// A run that hasn't finished stopping. Runs are retired (S40, decision
/// 10): the worker only stops what's left of one, never starts one.
pub struct Job {
    pub run: CloudRun,
    pub account: String,
    pub execution: Execution,
    pub lease: String,
}

fn plain_word(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.starts_with('-') && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// What control checks of a Cloud workspace's settings, and what it makes
/// of them for Managoat.
pub trait SpecRules {
    /// Check what the user sent and fill in the default mounts, so the saved
    /// spec says where each repository is.
    fn normalize(&mut self) -> Result<()>;
    /// Every secret the vault holds: the repositories' and the others.
    fn secret_names(&self) -> Vec<String>;
    /// The environment's build fields, every one sent so the same body can
    /// replace them on an update.
    fn environment(&self) -> serde_json::Value;
    /// The build fields for a PATCH. Managoat keeps a field a PATCH leaves
    /// out, so the defaults a create leaves to it are sent too.
    fn environment_update(&self) -> serde_json::Value;
    /// One field's value as `pending` shows it. `env_vars` are names only:
    /// Managoat never says a value, and nor does this.
    fn field(&self, field: &str) -> serde_json::Value;
}

impl SpecRules for CloudWorkspaceSpec {
    fn normalize(&mut self) -> Result<()> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || self.name.len() > 120 {
            bail!("name must contain 1–120 bytes");
        }
        if self.repositories.len() > 10 {
            bail!("at most 10 repositories");
        }
        let mut mounts = std::collections::BTreeSet::new();
        for repository in &mut self.repositories {
            if !crate::cloud::https_repository(&repository.url) {
                bail!("repository must be an HTTPS URL without embedded credentials");
            }
            if repository.git_ref.starts_with('-')
                || repository.git_ref.chars().any(char::is_control)
                || repository.git_ref.len() > 256
            {
                bail!("invalid ref");
            }
            if repository.mount.is_empty() {
                let last = repository.url.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
                let name = last.strip_suffix(".git").unwrap_or(last);
                if name.is_empty()
                    || matches!(name, "." | "..")
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                {
                    bail!("give the repository a mount");
                }
                repository.mount = format!("/home/sprite/{name}");
            }
            let segments: Vec<&str> = repository.mount.split('/').skip(1).collect();
            if !repository.mount.starts_with('/')
                || repository.mount.len() > 256
                || repository.mount.chars().any(char::is_control)
                || segments.iter().any(|s| s.is_empty() || matches!(*s, "." | ".."))
            {
                bail!("a mount must be an absolute path");
            }
            if !mounts.insert(repository.mount.clone()) {
                bail!("two repositories share a mount");
            }
            if repository.secret.as_ref().is_some_and(|name| !crate::cloud::secrets::valid_name(name)) {
                bail!("invalid repository secret name");
            }
        }
        let packages = [&self.packages.apt, &self.packages.npm];
        if packages.iter().any(|list| list.len() > 100 || list.iter().any(|p| !plain_word(p, 200))) {
            bail!("invalid packages");
        }
        if self.setup_script.len() > 64_000 {
            bail!("setup script must be at most 64000 bytes");
        }
        if self.setup_timeout_seconds.is_some_and(|s| !(1..=900).contains(&s)) {
            bail!("setup timeout must be 1–900 seconds");
        }
        if self.env_vars.len() > 100
            || self.env_vars.iter().any(|(k, v)| !crate::cloud::secrets::valid_name(k) || v.len() > 32_000)
        {
            bail!("invalid or reserved environment variable");
        }
        if let Some(networking) = &self.networking
            && (!matches!(networking.kind.as_str(), "unrestricted" | "limited")
                || networking.allowed_hosts.len() > 200
                || networking.allowed_hosts.iter().any(|h| !plain_word(h, 253))
                || (networking.kind == "unrestricted" && !networking.allowed_hosts.is_empty()))
        {
            bail!("invalid networking");
        }
        if self.secrets.len() > 32 || self.secrets.iter().any(|s| !crate::cloud::secrets::valid_name(s)) {
            bail!("invalid or reserved secret name");
        }
        if serde_json::to_vec(self)?.len() > 256_000 {
            bail!("cloud workspace settings are too large");
        }
        Ok(())
    }

    fn secret_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.repositories.iter().filter_map(|r| r.secret.clone()).chain(self.secrets.iter().cloned()).collect();
        names.sort();
        names.dedup();
        names
    }

    fn environment(&self) -> serde_json::Value {
        let repositories: Vec<_> = self
            .repositories
            .iter()
            .map(|r| {
                let mut repository = serde_json::json!({"url":r.url,"mount_path":r.mount});
                if !r.git_ref.is_empty() {
                    repository["ref"] = serde_json::json!(r.git_ref);
                }
                if let Some(name) = &r.secret {
                    repository["secret_key"] = serde_json::json!(name);
                }
                repository
            })
            .collect();
        let mut packages = serde_json::Map::new();
        for (manager, list) in [("apt", &self.packages.apt), ("npm", &self.packages.npm)] {
            if !list.is_empty() {
                packages.insert(manager.into(), serde_json::json!(list));
            }
        }
        let mut body = serde_json::json!({
            "repositories": repositories, "packages": packages,
            "setup_script": self.setup_script, "env_vars": self.env_vars,
        });
        if let Some(seconds) = self.setup_timeout_seconds {
            body["setup_timeout_seconds"] = serde_json::json!(seconds);
        }
        if let Some(networking) = &self.networking {
            body["networking_type"] = serde_json::json!(networking.kind);
            if networking.kind == "limited" {
                body["networking_config"] = serde_json::json!({"allowed_hosts":networking.allowed_hosts});
            }
        }
        body
    }

    fn environment_update(&self) -> serde_json::Value {
        let mut body = self.environment();
        body["setup_timeout_seconds"] = serde_json::json!(self.setup_timeout_seconds.unwrap_or(120));
        let limited = self.networking.as_ref().filter(|n| n.kind == "limited");
        body["networking_type"] = serde_json::json!(if limited.is_some() { "limited" } else { "unrestricted" });
        body["networking_config"] = match limited {
            Some(n) => serde_json::json!({"allowed_hosts":n.allowed_hosts}),
            None => serde_json::json!({}),
        };
        body
    }

    fn field(&self, field: &str) -> serde_json::Value {
        match field {
            "repositories" => serde_json::json!(self.repositories),
            "packages" => serde_json::json!(self.packages),
            "setup_script" => serde_json::json!(self.setup_script),
            "setup_timeout_seconds" => serde_json::json!(self.setup_timeout_seconds),
            "env_vars" => serde_json::json!(self.env_vars.keys().collect::<Vec<_>>()),
            "networking" => serde_json::json!(self.networking),
            _ => serde_json::Value::Null,
        }
    }
}

/// What a rebuild would change: Managoat's `changed_fields` while it says
/// `stale`, with values from the specs. Its `networking_type` and
/// `networking_config` are the spec's one `networking`.
pub fn pending_changes(
    built: Option<&CloudWorkspaceSpec>,
    saved: &CloudWorkspaceSpec,
    stale: bool,
    changed_fields: &[String],
) -> Vec<CloudWorkspaceChange> {
    if !stale {
        return vec![];
    }
    let mut fields: Vec<&str> = vec![];
    for f in changed_fields {
        let f = if f.starts_with("networking_") { "networking" } else { f.as_str() };
        if !fields.contains(&f) {
            fields.push(f);
        }
    }
    fields
        .into_iter()
        .map(|field| CloudWorkspaceChange {
            field: field.into(),
            before: built.map(|b| b.field(field)).unwrap_or_default(),
            after: saved.field(field),
        })
        .collect()
}

/// A rebuild or retry asked for (M120), kept while Managoat has no machine
/// free for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudWorkspaceRebuild {
    /// Retry a failed setup, rather than rebuild.
    pub retry: bool,
    /// End running turns rather than be refused.
    pub interrupt: bool,
    /// When it was asked for.
    pub asked: u64,
    /// The worker has taken it to send: too late to cancel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sending: bool,
}

/// What cancelling a rebuild or retry that waits for a machine found.
#[derive(Debug, PartialEq)]
pub enum CancelledRebuild {
    /// It won't be sent.
    Cancelled,
    /// The worker has sent it, or is sending it.
    Started,
    /// Nothing was waiting.
    NotWaiting,
    NotFound,
}

/// The Cloud workspace's provisioning journal, as `Execution` is a run's.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudWorkspaceExecution {
    pub stage: String,
    pub environment: Option<String>,
    pub vault: Option<String>,
    pub workspace: Option<String>,
    #[serde(default)]
    pub secrets_uploaded: Vec<String>,
}

pub struct CloudWorkspaceJob {
    pub cloud_workspace: CloudWorkspace,
    pub account: String,
    pub execution: CloudWorkspaceExecution,
    /// Revision of the environment control made, from its create response.
    pub saved_revision: Option<String>,
    pub deleting: bool,
    /// The Cloud workspace's `rebuilds` when claimed: a rebuild taken since
    /// has newer statuses than this job read.
    pub rebuilds: u64,
    pub lease: String,
}

/// An approval push still to send.
pub struct CloudNotification {
    pub account: String,
    /// The run's or the tab's id.
    pub conversation: String,
    pub request: String,
    /// The tab's Cloud workspace; `None` for a run.
    pub cloud_workspace: Option<String>,
}

/// What a tab was opened with, kept as it was: editing the agent later
/// doesn't change a tab that's open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudTabSpec {
    pub agent_id: String,
    pub agent_version: u64,
    pub agent: AgentDefinition,
    pub runtime: String,
    pub model: String,
    pub cwd: String,
    #[serde(default)]
    pub prompt: Option<String>,
}

/// The tab's provisioning journal, as `Execution` is a run's.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudTabExecution {
    pub stage: String,
    pub agent: Option<String>,
    pub conversation: Option<String>,
    pub cursor: u64,
    /// How many times the tab's conversation failed and was replaced by a
    /// new one; each launch is labelled with it (`launch_key`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub launches: u64,
    /// The prompt the current conversation was relaunched with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl CloudTabExecution {
    /// The label and client request id of the tab's current conversation:
    /// the tab's id, then `<tab>.<n>` for the n-th relaunch, so finding one
    /// after a lost response never finds the failed one before it.
    pub fn launch_key(&self, tab: &str) -> String {
        if self.launches == 0 { tab.to_owned() } else { format!("{tab}.{}", self.launches) }
    }
}

pub struct CloudTabJob {
    pub tab: CloudTab,
    pub spec: CloudTabSpec,
    pub account: String,
    pub execution: CloudTabExecution,
    pub started: bool,
    pub closing: bool,
    /// A prompt asking for a new conversation, the current one having
    /// failed.
    pub relaunch: Option<String>,
    /// The worker took `relaunch`: the checkpoint clears it.
    pub relaunch_taken: bool,
    /// The Managoat workspace, environment and vault of its Cloud
    /// workspace, once made.
    pub workspace: Option<String>,
    pub environment: Option<String>,
    pub vault: Option<String>,
    pub lease: String,
}

const CLOUD_TAB_COLUMNS: &str = "t.id,t.cloud_workspace,t.spec,t.status,t.error,t.started,t.created,w.status";

fn cloud_tab_row(r: &Row) -> Result<(CloudTab, CloudTabSpec, bool)> {
    let spec: CloudTabSpec = serde_json::from_str(&r.get::<String>(2)?)?;
    let status: String = r.get(3)?;
    let started: bool = r.get(5)?;
    let setting_up = r.get::<String>(7)? == "provisioning";
    let waiting_for_setup = spec.prompt.is_some()
        && !started
        && setting_up
        && matches!(status.as_str(), "opening" | "pending" | "running" | "idle");
    let tab = CloudTab {
        id: r.get(0)?,
        cloud_workspace: r.get(1)?,
        agent_id: spec.agent_id.clone(),
        agent_name: spec.agent.name.clone(),
        runtime: spec.runtime.clone(),
        model: spec.model.clone(),
        cwd: spec.cwd.clone(),
        status,
        error: r.get(4)?,
        waiting_for_setup,
        created: r.get(6)?,
    };
    Ok((tab, spec, started))
}

fn past_cloud_tab_row(r: &Row) -> Result<PastCloudTab> {
    let spec: CloudTabSpec = serde_json::from_str(&r.get::<String>(4)?)?;
    Ok(PastCloudTab {
        id: r.get(0)?,
        cloud_workspace: r.get(1)?,
        cloud_workspace_name: r.get(2)?,
        cloud_workspace_deleted: r.get(3)?,
        agent_name: spec.agent.name,
        runtime: spec.runtime,
        model: spec.model,
        cwd: spec.cwd,
        created: r.get(5)?,
        closed: r.get(6)?,
    })
}

const CLOUD_WORKSPACE_COLUMNS: &str = "id,name,saved_spec,built_spec,status,sandbox_status,error,revision,built_from_revision,stale,changed_fields,created,settings_applied,rebuild,rebuild_error,setup_step";

fn cloud_workspace_row(r: &Row) -> Result<CloudWorkspace> {
    fn decode<T: serde::de::DeserializeOwned>(i: usize, s: &str) -> Result<T> {
        serde_json::from_str(s).with_context(|| format!("column {i}"))
    }
    let saved_spec: CloudWorkspaceSpec = decode(2, &r.get::<String>(2)?)?;
    let built_spec: Option<CloudWorkspaceSpec> = r.get::<Option<String>>(3)?.map(|s| decode(3, &s)).transpose()?;
    let stale: bool = r.get(9)?;
    let changed_fields: Vec<String> = decode(10, &r.get::<String>(10)?)?;
    Ok(CloudWorkspace {
        id: r.get(0)?,
        name: r.get(1)?,
        pending: pending_changes(built_spec.as_ref(), &saved_spec, stale, &changed_fields),
        saved_spec,
        built_spec,
        status: r.get(4)?,
        sandbox_status: r.get(5)?,
        error: r.get(6)?,
        revision: r.get(7)?,
        built_from_revision: r.get(8)?,
        stale,
        changed_fields,
        created: r.get(11)?,
        settings_applied: r.get(12)?,
        rebuild_queued: r.get::<Option<String>>(13)?.is_some(),
        rebuild_error: r.get(14)?,
        setup_step: r.get(15)?,
    })
}

fn row(r: &Row) -> Result<CloudRun> {
    let decode = |s: String| serde_json::from_str(&s);
    let spec: String = r.get(1)?;
    let spec = serde_json::from_str(&spec)?;
    Ok(CloudRun {
        id: r.get(0)?,
        spec,
        state: r.get(2)?,
        created: r.get(3)?,
        deadline: r.get(4)?,
        result: r.get::<Option<String>>(5)?.map(decode).transpose()?,
    })
}

/// How long a claim leases its job (`cloud::worker`): what a machine that
/// dies mid-step costs before another machine takes the job.
pub const LEASE_MS: u64 = 120_000;

/// What a claim's `SELECT` ends with (M144): on Postgres it locks the job
/// it finds and passes over any another machine's claim is locking, so two
/// never take the same job and neither waits for the other. SQLite's
/// `BEGIN IMMEDIATE` already makes claims take turns.
fn skip_locked(tx: &impl Sql) -> &'static str {
    match tx.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE OF j SKIP LOCKED",
    }
}

/// Lease the job `id` that a claim found free at `now`: a new lease, or
/// `None` if it isn't free any more.
fn lease(tx: &impl Sql, id: &str, now: u64) -> Result<Option<String>> {
    let lease = hex::encode(arugula_e2e::random::<16>());
    let changed = tx.execute(
        "UPDATE cloud_jobs SET lease=?1,lease_until=?2 WHERE id=?3 AND lease_until<=?4",
        params![lease, now + LEASE_MS, id, now],
    )?;
    Ok((changed == 1).then_some(lease))
}

/// `f`, and once more if it was refused for a row that's there already
/// ([`Taken`]): a create racing the same request key on another machine.
fn once_more<T>(f: impl Fn() -> Result<T>) -> Result<T> {
    match f() {
        Err(e) if e.is::<Taken>() => f(),
        r => r,
    }
}

impl Db {
    pub fn active_cloud_runs(&self, account: &str) -> Result<u64> {
        self.c().query_row("SELECT count(*) FROM cloud_runs WHERE account=?1 AND state NOT IN ('completed','failed','cancelled','expired')", params![account], |r| r.get(0))
    }

    pub fn cloud_secret_names(&self, account: &str) -> Result<Vec<String>> {
        self.c()
            .query_map("SELECT name FROM cloud_secrets WHERE account=?1 ORDER BY name", params![account], |r| r.get(0))
    }
    /// Keep a sealed secret in the account's personal `project` (M129:
    /// `crate::projects::personal_project`).
    pub fn put_cloud_secret(
        &self,
        account: &str,
        project: &str,
        name: &str,
        sealed: &crate::keys::Sealed,
    ) -> Result<()> {
        // Keyed by account on SQLite, by project on Postgres (M143): the
        // same row, as an account's secrets are in its personal project.
        let c = self.c();
        let key = match c.dialect() {
            Dialect::Sqlite => "account,name",
            Dialect::Postgres => "project_id,name",
        };
        c.execute(
            &format!(
                "INSERT INTO cloud_secrets(account,name,sealed,project_id,key_version) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT({key}) DO UPDATE SET sealed=excluded.sealed,key_version=excluded.key_version"
            ),
            params![account, name, sealed.bytes, project, sealed.key_version],
        )?;
        Ok(())
    }
    pub fn delete_cloud_secret(&self, account: &str, name: &str) -> Result<()> {
        self.c().execute("DELETE FROM cloud_secrets WHERE account=?1 AND name=?2", params![account, name])?;
        Ok(())
    }
    pub fn cloud_agents(&self, account: &str) -> Result<Vec<CloudAgent>> {
        self.c().query_map(
            "SELECT id,version,definition FROM cloud_agents WHERE account=?1 AND archived=FALSE ORDER BY id",
            params![account],
            |r| {
                let body: String = r.get(2)?;
                Ok(CloudAgent {
                    id: r.get(0)?,
                    version: r.get(1)?,
                    definition: serde_json::from_str(&body)?,
                    curated: false,
                })
            },
        )
    }

    pub fn create_cloud_agent(&self, account: &str, definition: AgentDefinition) -> Result<CloudAgent> {
        definition.validate()?;
        let agent = CloudAgent {
            id: format!("agent_{}", hex::encode(arugula_e2e::random::<16>())),
            version: 1,
            definition,
            curated: false,
        };
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let count: u64 = tx.query_row(
            "SELECT count(*) FROM cloud_agents WHERE account=?1 AND archived=FALSE",
            params![account],
            |r| r.get(0),
        )?;
        if count >= 100 {
            bail!("custom agent limit reached");
        }
        tx.execute(
            "INSERT INTO cloud_agents(id,account,version,definition) VALUES(?1,?2,1,?3)",
            params![agent.id, account, serde_json::to_string(&agent.definition)?],
        )?;
        tx.commit()?;
        Ok(agent)
    }

    /// Optimistic version check prevents one editor silently overwriting another.
    pub fn update_cloud_agent(
        &self,
        account: &str,
        id: &str,
        version: u64,
        definition: &AgentDefinition,
    ) -> Result<bool> {
        definition.validate()?;
        Ok(self.c().execute("UPDATE cloud_agents SET definition=?1,version=version+1 WHERE account=?2 AND id=?3 AND version=?4 AND archived=FALSE", params![serde_json::to_string(definition)?, account, id, version])? == 1)
    }

    pub fn archive_cloud_agent(&self, account: &str, id: &str) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE cloud_agents SET archived=TRUE WHERE account=?1 AND id=?2 AND archived=FALSE",
            params![account, id],
        )? == 1)
    }

    /// A run as one was made before runs were retired, with its job at
    /// `execution` and in `state`.
    #[cfg(test)]
    pub fn insert_cloud_run(
        &self,
        account: &str,
        spec: &RunSpec,
        execution: &Execution,
        state: &str,
        now: u64,
    ) -> Result<CloudRun> {
        let id = format!("run_{}", hex::encode(arugula_e2e::random::<16>()));
        let conn = self.c();
        conn.execute(
            "INSERT INTO cloud_runs(id,account,request_key,spec,state,provider_id,created,deadline) VALUES(?1,?2,?1,?3,?4,?5,?6,?7)",
            params![id, account, serde_json::to_string(spec)?, state, execution.conversation, now, now + 1_800_000],
        )?;
        conn.execute(
            "INSERT INTO cloud_jobs(id,kind,body) VALUES(?1,'run',?2)",
            params![id, serde_json::to_string(execution)?],
        )?;
        drop(conn);
        Ok(self.cloud_run(account, &id)?.expect("just inserted"))
    }

    pub fn cloud_runs(&self, account: &str) -> Result<Vec<CloudRun>> {
        self.c().query_map(
            "SELECT id,spec,state,created,deadline,result FROM cloud_runs WHERE account=?1 ORDER BY created DESC LIMIT 100",
            params![account],
            row,
        )
    }

    /// A conversation's events: a run's or a tab's.
    pub fn cloud_events(&self, account: &str, id: &str, after: u64) -> Result<Vec<serde_json::Value>> {
        self.c().query_map(
            "SELECT body FROM cloud_events WHERE account=?1 AND conversation=?2 AND cursor>?3 ORDER BY cursor LIMIT 1000",
            params![account, id, after],
            |r| Ok(serde_json::from_str(&r.get::<String>(0)?)?),
        )
    }

    pub fn cloud_requests(&self, account: &str, conversation: &str) -> Result<Vec<serde_json::Value>> {
        self.c().query_map(
            "SELECT body FROM cloud_requests WHERE account=?1 AND conversation=?2 AND pending=TRUE",
            params![account, conversation],
            |r| Ok(serde_json::from_str(&r.get::<String>(0)?)?),
        )
    }
    pub fn resolve_cloud_request(&self, account: &str, conversation: &str, request: &str) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_requests SET pending=FALSE WHERE account=?1 AND conversation=?2 AND id=?3",
            params![account, conversation, request],
        )?;
        Ok(())
    }
    /// Undelivered pushes: (account, conversation, request, and the Cloud
    /// workspace when the conversation is a tab).
    pub fn cloud_notifications(&self, now: u64) -> Result<Vec<CloudNotification>> {
        // `rowid`: in the order they were made (on Postgres, a column of
        // that name).
        self.c().query_map(
            "SELECT n.account,n.conversation,n.request,t.cloud_workspace FROM cloud_notifications n
             JOIN cloud_requests q ON q.conversation=n.conversation AND q.id=n.request
             LEFT JOIN cloud_tabs t ON t.id=n.conversation
             WHERE n.delivered=FALSE AND n.next_attempt<=?1 AND q.pending=TRUE ORDER BY n.next_attempt,n.rowid LIMIT 20",
            params![now],
            |r| {
                Ok(CloudNotification {
                    account: r.get(0)?,
                    conversation: r.get(1)?,
                    request: r.get(2)?,
                    cloud_workspace: r.get(3)?,
                })
            },
        )
    }
    /// Take one approval push to send, for `until` (M144): false when
    /// another machine took it first, or it's not due any more. Each push
    /// is sent by one machine at a time.
    pub fn claim_cloud_notification(&self, conversation: &str, request: &str, now: u64, until: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE cloud_notifications SET next_attempt=?4
             WHERE conversation=?1 AND request=?2 AND delivered=FALSE AND next_attempt<=?3",
            params![conversation, request, now, until],
        )? == 1)
    }

    pub fn cloud_notification_result(
        &self,
        conversation: &str,
        request: &str,
        delivered: bool,
        next: u64,
    ) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_notifications SET delivered=?1,next_attempt=?2 WHERE conversation=?3 AND request=?4",
            params![delivered, next, conversation, request],
        )?;
        Ok(())
    }

    /// Claim a bounded unit of work, not the lifetime of the remote run.
    pub fn claim_cloud_job(&self, now: u64) -> Result<Option<Job>> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let found = tx.query_row(&format!("SELECT r.id,r.spec,r.state,r.created,r.deadline,r.result,r.account,j.body FROM cloud_runs r JOIN cloud_jobs j ON j.id=r.id WHERE r.state NOT IN ('completed','failed','cancelled','expired') AND j.lease_until<=?1 ORDER BY j.lease_until,r.created LIMIT 1{}", skip_locked(&tx)), params![now], |r| Ok((row(r)?,r.get::<String>(6)?,r.get::<String>(7)?))).optional()?;
        let Some((run, account, body)) = found else {
            return Ok(None);
        };
        let Some(lease) = lease(&tx, &run.id, now)? else { return Ok(None) };
        tx.commit()?;
        Ok(Some(Job { run, account, execution: serde_json::from_str(&body)?, lease }))
    }

    /// Checkpoint under the claim. A stale worker cannot overwrite its successor.
    pub fn checkpoint_cloud_job(
        &self,
        job: &Job,
        state: &str,
        result: Option<&serde_json::Value>,
        release_at: Option<u64>,
    ) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let changed = tx.execute(
            "UPDATE cloud_jobs SET body=?1,lease_until=COALESCE(?2,lease_until) WHERE id=?3 AND lease=?4",
            params![serde_json::to_string(&job.execution)?, release_at, job.run.id, job.lease],
        )?;
        if changed != 1 {
            bail!("cloud job lease lost");
        }
        tx.execute(
            "UPDATE cloud_runs SET state=?1,result=COALESCE(?2,result),provider_id=?3 WHERE id=?4",
            params![state, result.map(serde_json::to_string).transpose()?, job.execution.conversation, job.run.id],
        )?;
        if matches!(state, "saving_results" | "cleaning_up" | "completed" | "failed" | "cancelled" | "expired") {
            tx.execute("UPDATE cloud_requests SET pending=FALSE WHERE conversation=?1", params![&job.run.id])?;
        }
        if matches!(state, "completed" | "failed" | "cancelled" | "expired") {
            tx.execute("DELETE FROM cloud_run_secrets WHERE run=?1", params![&job.run.id])?;
        }
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn append_cloud_events(&self, job: &Job, events: &[serde_json::Value]) -> Result<()> {
        self.append_conversation_events(&job.run.id, &job.account, &job.lease, events)
    }

    /// Take in a page of a conversation's events under its job's claim (`id`
    /// is the run's or the tab's): the approvals it asks for become requests
    /// and pushes, and the ones answered or ended stop being pending.
    pub fn append_conversation_events(
        &self,
        id: &str,
        account: &str,
        lease: &str,
        events: &[serde_json::Value],
    ) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        // The job's row, held until this commits, so it isn't claimed
        // from under this (M144).
        let owns = tx
            .query_row(
                &format!("SELECT id FROM cloud_jobs WHERE id=?1 AND lease=?2{}", tx.for_update()),
                params![id, lease],
                |r| r.get::<String>(0),
            )
            .optional()?
            .is_some();
        if !owns {
            bail!("cloud job lease lost");
        }
        for event in events {
            let cursor = event["id"].as_u64().ok_or_else(|| anyhow::anyhow!("invalid cloud event cursor"))?;
            let inserted = tx.execute(
                "INSERT INTO cloud_events(conversation,account,cursor,body) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",
                params![id, account, cursor, serde_json::to_string(event)?],
            )?;
            if inserted == 0 {
                continue;
            }
            if let Some(blocks) = event["blocks"].as_array() {
                for block in blocks {
                    if matches!(block["kind"].as_str(), Some("permission_request" | "elicitation_request"))
                        && let Some(request) = block["request_id"].as_str()
                    {
                        tx.execute(
                            "INSERT INTO cloud_requests(conversation,account,id,body) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",
                            params![id, account, request, serde_json::to_string(block)?],
                        )?;
                        tx.execute(
                            "INSERT INTO cloud_notifications(conversation,account,request) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
                            params![id, account, request],
                        )?;
                    }
                }
            }
            if event["stage"] == "request" && event["state"] == "done" {
                let data: serde_json::Value = event["data"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| event["data"].clone());
                if let Some(request) = data["request_id"].as_str().or_else(|| data["requestId"].as_str()) {
                    tx.execute(
                        "UPDATE cloud_requests SET pending=FALSE WHERE conversation=?1 AND id=?2",
                        params![id, request],
                    )?;
                }
            }
            if event["stage"] == "turn" && matches!(event["state"].as_str(), Some("done" | "failed")) {
                tx.execute("UPDATE cloud_requests SET pending=FALSE WHERE conversation=?1", params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn cloud_run(&self, account: &str, id: &str) -> Result<Option<CloudRun>> {
        self.c()
            .query_row(
                "SELECT id,spec,state,created,deadline,result FROM cloud_runs WHERE account=?1 AND id=?2",
                params![account, id],
                row,
            )
            .optional()
    }
}

impl Db {
    /// Create a Cloud workspace and its job, in the account's personal
    /// `project`. A retry with the same request key returns the first one;
    /// with other settings it's a conflict.
    pub fn create_cloud_workspace(
        &self,
        account: &str,
        project: &str,
        request_key: &str,
        spec: &CloudWorkspaceSpec,
        now: u64,
    ) -> Result<CloudWorkspace> {
        if request_key.is_empty() || request_key.len() > 128 {
            bail!("invalid request key");
        }
        // The same key at once on two machines: one inserts, and the other,
        // refused by the key's unique index, finds that one.
        once_more(|| self.create_cloud_workspace_once(account, project, request_key, spec, now))
    }

    fn create_cloud_workspace_once(
        &self,
        account: &str,
        project: &str,
        request_key: &str,
        spec: &CloudWorkspaceSpec,
        now: u64,
    ) -> Result<CloudWorkspace> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let existing = tx
            .query_row(
                &format!("SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE account=?1 AND request_key=?2"),
                params![account, request_key],
                cloud_workspace_row,
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing.saved_spec != *spec {
                bail!("request key already used with different input");
            }
            return Ok(existing);
        }
        for name in spec.secret_names() {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM cloud_secrets WHERE account=?1 AND name=?2)",
                params![account, name],
                |r| r.get(0),
            )?;
            if !exists {
                bail!("required cloud secret is missing");
            }
        }
        let id = format!("cw_{}", hex::encode(arugula_e2e::random::<16>()));
        tx.execute(
            "INSERT INTO cloud_workspaces(id,account,request_key,name,saved_spec,created,project_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id, account, request_key, spec.name, serde_json::to_string(spec)?, now, project],
        )
        .map_err(taken)?;
        tx.execute(
            "INSERT INTO cloud_jobs(id,kind,body) VALUES(?1,'cloud_workspace',?2)",
            params![id, serde_json::to_string(&CloudWorkspaceExecution::default())?],
        )?;
        let created = tx.query_row(
            &format!("SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE id=?1"),
            params![&id],
            cloud_workspace_row,
        )?;
        tx.commit()?;
        Ok(created)
    }

    /// The account's Cloud workspaces that aren't deleted yet, newest first.
    pub fn cloud_workspaces(&self, account: &str) -> Result<Vec<CloudWorkspace>> {
        self.c().query_map(
            &format!(
                "SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE account=?1 AND deleted IS NULL ORDER BY created DESC, id"
            ),
            params![account],
            cloud_workspace_row,
        )
    }

    pub fn cloud_workspace(&self, account: &str, id: &str) -> Result<Option<CloudWorkspace>> {
        self
            .c()
            .query_row(
                &format!(
                    "SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE account=?1 AND id=?2 AND deleted IS NULL"
                ),
                params![account, id],
                cloud_workspace_row,
            )
            .optional()
    }

    /// The Managoat workspace behind one of the account's Cloud workspaces,
    /// once Managoat has made it and until a delete is asked for.
    pub fn cloud_workspace_provider_id(&self, account: &str, id: &str) -> Result<Option<String>> {
        Ok(self
            .c()
            .query_row(
                "SELECT provider_id FROM cloud_workspaces
                 WHERE account=?1 AND id=?2 AND deleted IS NULL AND status!='deleting'",
                params![account, id],
                |r| r.get::<Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// Ask the worker to delete it. Saying so again is harmless.
    pub fn delete_cloud_workspace(&self, account: &str, id: &str) -> Result<bool> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let changed = tx.execute(
            "UPDATE cloud_workspaces SET status='deleting' WHERE account=?1 AND id=?2 AND deleted IS NULL",
            params![account, id],
        )?;
        tx.execute("UPDATE cloud_jobs SET cancel=TRUE WHERE id=?1 AND kind='cloud_workspace'", params![id])?;
        // Managoat ends its conversations; each tab's agent goes with its tab.
        tx.execute(
            "UPDATE cloud_tabs SET status='closing' WHERE account=?1 AND cloud_workspace=?2 AND closed IS NULL",
            params![account, id],
        )?;
        tx.execute(
            "UPDATE cloud_jobs SET cancel=TRUE WHERE kind='cloud_tab' AND id IN (SELECT id FROM cloud_tabs WHERE account=?1 AND cloud_workspace=?2)",
            params![account, id],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Account erasure: ask for every one of its Cloud workspaces to go.
    pub fn delete_cloud_workspaces(&self, account: &str) -> Result<()> {
        for w in self.cloud_workspaces(account)? {
            self.delete_cloud_workspace(account, &w.id)?;
        }
        Ok(())
    }

    /// Whether the account's Cloud workspaces are all deleted at Managoat,
    /// and every tab closed (a tab's agent is Managoat's too): what erasing
    /// the account waits for.
    pub fn cloud_workspaces_gone(&self, account: &str) -> Result<bool> {
        let live: bool = self.c().query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_workspaces WHERE account=?1 AND deleted IS NULL)
                 OR EXISTS(SELECT 1 FROM cloud_tabs WHERE account=?1 AND closed IS NULL)",
            params![account],
            |r| r.get(0),
        )?;
        Ok(!live)
    }

    /// Erase everything Cloud keeps of an account that's gone, in one
    /// transaction: what the hub's `Db::delete_account` did while Cloud
    /// lived there (M137). Answers the Cloud workspaces and tabs that
    /// weren't deleted at Managoat yet, which go all the same.
    pub fn erase_cloud_account(&self, a: &str) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let left: Vec<String> = tx.query_map(
            "SELECT id FROM cloud_workspaces WHERE account=?1 AND deleted IS NULL
             UNION ALL SELECT id FROM cloud_tabs WHERE account=?1 AND closed IS NULL",
            params![a],
            |r| r.get(0),
        )?;
        tx.execute(
            "DELETE FROM cloud_run_secrets WHERE run IN (SELECT id FROM cloud_runs WHERE account=?1)",
            params![a],
        )?;
        for table in ["cloud_requests", "cloud_notifications", "cloud_events"] {
            tx.execute(&format!("DELETE FROM {table} WHERE account=?1"), params![a])?;
        }
        tx.execute(
            "DELETE FROM cloud_jobs WHERE id IN (SELECT id FROM cloud_runs WHERE account=?1 UNION SELECT id FROM cloud_workspaces WHERE account=?1 UNION SELECT id FROM cloud_tabs WHERE account=?1)",
            params![a],
        )?;
        for table in
            ["cloud_runs", "cloud_tabs", "cloud_workspace_specs", "cloud_workspaces", "cloud_agents", "cloud_secrets"]
        {
            tx.execute(&format!("DELETE FROM {table} WHERE account=?1"), params![a])?;
        }
        tx.commit()?;
        Ok(left)
    }

    pub fn claim_cloud_workspace_job(&self, now: u64) -> Result<Option<CloudWorkspaceJob>> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let columns = CLOUD_WORKSPACE_COLUMNS.split(',').map(|c| format!("w.{c}")).collect::<Vec<_>>().join(",");
        let found = tx
            .query_row(
                &format!(
                    "SELECT {columns},w.account,w.saved_revision,j.body,j.cancel,w.rebuilds FROM cloud_workspaces w JOIN cloud_jobs j ON j.id=w.id
                     WHERE j.kind='cloud_workspace' AND w.deleted IS NULL AND j.lease_until<=?1 ORDER BY j.lease_until,w.created LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| {
                    Ok((
                        cloud_workspace_row(r)?,
                        r.get::<String>(16)?,
                        r.get::<Option<String>>(17)?,
                        r.get::<String>(18)?,
                        r.get::<bool>(19)?,
                        r.get::<u64>(20)?,
                    ))
                },
            )
            .optional()?;
        let Some((cloud_workspace, account, saved_revision, body, deleting, rebuilds)) = found else {
            return Ok(None);
        };
        let Some(lease) = lease(&tx, &cloud_workspace.id, now)? else { return Ok(None) };
        tx.commit()?;
        Ok(Some(CloudWorkspaceJob {
            cloud_workspace,
            account,
            execution: serde_json::from_str(&body)?,
            saved_revision,
            deleting,
            rebuilds,
            lease,
        }))
    }

    /// Checkpoint under the claim, as `checkpoint_cloud_job` does. A delete
    /// asked for meanwhile keeps the status `deleting`. `built_spec` becomes
    /// the spec a revision was saved as once that revision is built. What
    /// the job read is left alone when a rebuild was taken since it was
    /// claimed: the rebuild's answer is newer.
    pub fn checkpoint_cloud_workspace_job(&self, job: &CloudWorkspaceJob, release_at: Option<u64>) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let changed = tx.execute(
            "UPDATE cloud_jobs SET body=?1,lease_until=COALESCE(?2,lease_until) WHERE id=?3 AND lease=?4",
            params![serde_json::to_string(&job.execution)?, release_at, job.cloud_workspace.id, job.lease],
        )?;
        if changed != 1 {
            bail!("cloud job lease lost");
        }
        let w = &job.cloud_workspace;
        tx.execute(
            "UPDATE cloud_workspaces SET
                 status=CASE WHEN (SELECT cancel FROM cloud_jobs WHERE id=?1) THEN 'deleting' ELSE ?2 END,
                 sandbox_status=?3, error=?4, revision=?5, built_from_revision=?6, stale=?7, changed_fields=?8,
                 environment=?9, vault=?10, provider_id=?11, saved_revision=COALESCE(saved_revision,?12), setup_step=?14,
                 built_spec=CASE WHEN ?2='ready' AND CAST(?6 AS TEXT) IS NOT NULL THEN COALESCE(
                     (SELECT spec FROM cloud_workspace_specs WHERE cloud_workspace=?1 AND revision=?6),
                     CASE WHEN settings_applied=TRUE AND ?6=COALESCE(saved_revision,?12) THEN saved_spec END,
                     built_spec) ELSE built_spec END
             WHERE id=?1 AND rebuilds=?13",
            params![
                w.id,
                w.status,
                w.sandbox_status,
                w.error,
                w.revision,
                w.built_from_revision,
                w.stale,
                serde_json::to_string(&w.changed_fields)?,
                job.execution.environment,
                job.execution.vault,
                job.execution.workspace,
                job.saved_revision,
                job.rebuilds,
                w.setup_step,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Its Managoat resources are gone: keep the row as a tombstone and end
    /// the job.
    pub fn finish_cloud_workspace_delete(&self, job: &CloudWorkspaceJob, now: u64) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        if tx.execute("DELETE FROM cloud_jobs WHERE id=?1 AND lease=?2", params![job.cloud_workspace.id, job.lease])?
            != 1
        {
            bail!("cloud job lease lost");
        }
        tx.execute(
            "UPDATE cloud_workspaces SET status='deleted',deleted=?1,environment=NULL,vault=NULL,provider_id=NULL WHERE id=?2",
            params![now, job.cloud_workspace.id],
        )?;
        tx.execute("DELETE FROM cloud_workspace_specs WHERE cloud_workspace=?1", params![&job.cloud_workspace.id])?;
        tx.commit()?;
        Ok(())
    }

    /// One sealed secret of the account's and its project, read when the
    /// vault is filled ([`crate::keys::open_cloud`] opens it).
    pub fn cloud_secret(&self, account: &str, name: &str) -> Result<Option<(Option<String>, crate::keys::Sealed)>> {
        self.c()
            .query_row(
                "SELECT project_id,key_version,sealed FROM cloud_secrets WHERE account=?1 AND name=?2",
                params![account, name],
                |r| Ok((r.get(0)?, crate::keys::Sealed { key_version: r.get(1)?, bytes: r.get(2)? })),
            )
            .optional()
    }
}

/// A settings save still to send to Managoat: the saved spec, where it
/// goes, and the secrets the vault holds now.
pub struct CloudWorkspaceSettings {
    pub account: String,
    pub spec: CloudWorkspaceSpec,
    pub environment: String,
    pub vault: String,
    pub workspace: String,
    pub vault_secrets: Vec<String>,
}

/// Settings saves (M117). The route saves first and then sends them;
/// until Managoat has them, `settings_applied` is 0 and the worker sends
/// them again.
impl Db {
    /// Save new settings, to be sent. Refused while the Cloud workspace is
    /// being made or deleted, and when it names a secret the account
    /// doesn't have. False when they're the ones already sent.
    pub fn save_cloud_workspace_settings(
        &self,
        account: &str,
        id: &str,
        spec: &CloudWorkspaceSpec,
        now: u64,
    ) -> Result<bool> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        type Row = (String, Option<String>, Option<String>, String, Option<String>, bool, Option<String>);
        let found: Option<Row> = tx
            .query_row(
                &format!(
                    "SELECT status,provider_id,vault,saved_spec,saved_revision,settings_applied,vault_secrets
                     FROM cloud_workspaces WHERE account=?1 AND id=?2 AND deleted IS NULL{}",
                    tx.for_update()
                ),
                params![account, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .optional()?;
        let Some((status, provider_id, vault, saved, saved_revision, applied, vault_secrets)) = found else {
            bail!("cloud workspace not found");
        };
        if status == "deleting" {
            bail!("cloud workspace is being deleted");
        }
        if provider_id.is_none() || vault.is_none() {
            bail!("cloud workspace is not made yet");
        }
        let before: CloudWorkspaceSpec = serde_json::from_str(&saved)?;
        if before == *spec && applied {
            return Ok(false);
        }
        for name in spec.secret_names() {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM cloud_secrets WHERE account=?1 AND name=?2)",
                params![account, name],
                |r| r.get(0),
            )?;
            if !exists {
                bail!("required cloud secret is missing");
            }
        }
        // Keep the spec the environment has now, in case a setup is
        // building it.
        if let (true, Some(revision)) = (applied, &saved_revision) {
            tx.execute(
                "INSERT INTO cloud_workspace_specs(cloud_workspace,account,revision,spec,saved) VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",
                params![id, account, revision, saved, now],
            )?;
        }
        let vault_secrets = match vault_secrets {
            Some(names) => names,
            None => serde_json::to_string(&before.secret_names())?,
        };
        tx.execute(
            "UPDATE cloud_workspaces SET saved_spec=?1,name=?2,settings_applied=FALSE,vault_secrets=?3 WHERE id=?4",
            params![serde_json::to_string(spec)?, spec.name, vault_secrets, id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The settings still to send for this Cloud workspace, if any.
    pub fn unsent_cloud_workspace_settings(&self, id: &str) -> Result<Option<CloudWorkspaceSettings>> {
        let found = self
            .c()
            .query_row(
                "SELECT account,saved_spec,environment,vault,provider_id,vault_secrets FROM cloud_workspaces
                 WHERE id=?1 AND deleted IS NULL AND status!='deleting' AND settings_applied=FALSE
                   AND environment IS NOT NULL AND vault IS NOT NULL AND provider_id IS NOT NULL",
                params![id],
                |r| {
                    Ok((
                        r.get::<String>(0)?,
                        r.get::<String>(1)?,
                        r.get::<String>(2)?,
                        r.get::<String>(3)?,
                        r.get::<String>(4)?,
                        r.get::<Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((account, spec, environment, vault, workspace, vault_secrets)) = found else {
            return Ok(None);
        };
        let spec: CloudWorkspaceSpec = serde_json::from_str(&spec)?;
        let vault_secrets = match vault_secrets {
            Some(names) => serde_json::from_str(&names)?,
            None => spec.secret_names(),
        };
        Ok(Some(CloudWorkspaceSettings { account, spec, environment, vault, workspace, vault_secrets }))
    }

    /// The vault holds `names` now. Kept as each one goes, so a save cut
    /// short doesn't put or delete a secret twice.
    pub fn cloud_workspace_vault_secrets(&self, id: &str, names: &[String]) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET vault_secrets=?1 WHERE id=?2",
            params![serde_json::to_string(names)?, id],
        )?;
        Ok(())
    }

    /// Managoat has `spec`, as environment `revision`. A save made since
    /// stays unsent.
    pub fn sent_cloud_workspace_settings(
        &self,
        id: &str,
        spec: &CloudWorkspaceSpec,
        revision: Option<&str>,
        now: u64,
    ) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let saved = serde_json::to_string(spec)?;
        let changed = tx.execute(
            "UPDATE cloud_workspaces SET settings_applied=TRUE,saved_revision=COALESCE(?1,saved_revision)
             WHERE id=?2 AND saved_spec=?3 AND deleted IS NULL",
            params![revision, id, saved],
        )?;
        if let (1, Some(revision)) = (changed, revision) {
            tx.execute(
                "INSERT INTO cloud_workspace_specs(cloud_workspace,account,revision,spec,saved)
                 SELECT id,account,?2,?3,?4 FROM cloud_workspaces WHERE id=?1
                 ON CONFLICT(cloud_workspace,revision) DO UPDATE SET account=excluded.account,spec=excluded.spec,saved=excluded.saved",
                params![id, revision, saved, now],
            )?;
            tx.execute(
                "DELETE FROM cloud_workspace_specs WHERE cloud_workspace=?1
                   AND revision IS DISTINCT FROM (SELECT built_from_revision FROM cloud_workspaces WHERE id=?1)
                   AND revision NOT IN (SELECT revision FROM cloud_workspace_specs WHERE cloud_workspace=?1
                                        ORDER BY saved DESC, rowid DESC LIMIT 10)",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Managoat refused the settings: go back to the ones it has. False
    /// when those aren't known.
    pub fn refused_cloud_workspace_settings(&self, id: &str) -> Result<bool> {
        let c = self.c();
        let name = match c.dialect() {
            Dialect::Sqlite => "json_extract(spec,'$.name')",
            Dialect::Postgres => "spec::jsonb->>'name'",
        };
        let changed = c.execute(
            &format!(
                "UPDATE cloud_workspaces SET settings_applied=TRUE,
                     saved_spec=(SELECT spec FROM cloud_workspace_specs s WHERE s.cloud_workspace=?1 AND s.revision=saved_revision),
                     name=(SELECT {name} FROM cloud_workspace_specs s WHERE s.cloud_workspace=?1 AND s.revision=saved_revision)
                 WHERE id=?1 AND EXISTS(SELECT 1 FROM cloud_workspace_specs s WHERE s.cloud_workspace=?1 AND s.revision=saved_revision)"
            ),
            params![id],
        )?;
        Ok(changed == 1)
    }

    /// What Managoat said of the machine just after a save: whether it's
    /// behind, and in which fields.
    pub fn observe_cloud_workspace_staleness(
        &self,
        id: &str,
        revision: Option<&str>,
        built_from_revision: Option<&str>,
        stale: bool,
        changed_fields: &[String],
    ) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET revision=?1,built_from_revision=?2,stale=?3,changed_fields=?4
             WHERE id=?5 AND deleted IS NULL",
            params![revision, built_from_revision, stale, serde_json::to_string(changed_fields)?, id],
        )?;
        Ok(())
    }
}

/// Rebuild and Retry (M120). The route asks Managoat at once; one Managoat
/// had no machine free for waits in `rebuild` for the worker.
impl Db {
    /// The Cloud workspace and its Managoat workspace, once there is one.
    pub fn cloud_workspace_machine(&self, account: &str, id: &str) -> Result<Option<(CloudWorkspace, Option<String>)>> {
        self
            .c()
            .query_row(
                &format!(
                    "SELECT {CLOUD_WORKSPACE_COLUMNS},provider_id FROM cloud_workspaces WHERE account=?1 AND id=?2 AND deleted IS NULL"
                ),
                params![account, id],
                |r| Ok((cloud_workspace_row(r)?, r.get(16)?)),
            )
            .optional()
    }

    /// Managoat took a rebuild or a retry, and `w` is what it answered: the
    /// Cloud workspace is setting up again. One that waited is done.
    pub fn rebuilt_cloud_workspace(&self, w: &CloudWorkspace) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET status=?2,sandbox_status=?3,error=?4,revision=?5,built_from_revision=?6,stale=?7,
                 changed_fields=?8,setup_step=?9,rebuild=NULL,rebuild_error=NULL,rebuilds=rebuilds+1
             WHERE id=?1 AND deleted IS NULL AND status!='deleting'",
            params![
                w.id,
                w.status,
                w.sandbox_status,
                w.error,
                w.revision,
                w.built_from_revision,
                w.stale,
                serde_json::to_string(&w.changed_fields)?,
                w.setup_step,
            ],
        )?;
        Ok(())
    }

    /// Managoat had no machine free: the worker asks again.
    pub fn queue_cloud_workspace_rebuild(&self, id: &str, rebuild: &CloudWorkspaceRebuild) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET rebuild=?2,rebuild_error=NULL WHERE id=?1 AND deleted IS NULL",
            params![id, serde_json::to_string(rebuild)?],
        )?;
        Ok(())
    }

    pub fn queued_cloud_workspace_rebuild(&self, id: &str) -> Result<Option<CloudWorkspaceRebuild>> {
        let queued: Option<Option<String>> = self
            .c()
            .query_row("SELECT rebuild FROM cloud_workspaces WHERE id=?1 AND deleted IS NULL", params![id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(queued.flatten().map(|q| serde_json::from_str(&q)).transpose()?)
    }

    /// The worker takes the one waiting to send it, unless it was
    /// cancelled: in one transaction with `cancel_cloud_workspace_rebuild`,
    /// so a cancel answered `Cancelled` is never sent. True with it when it
    /// was taken already: control stopped while sending it.
    pub fn claim_cloud_workspace_rebuild(&self, id: &str) -> Result<Option<(CloudWorkspaceRebuild, bool)>> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let queued: Option<Option<String>> = tx
            .query_row(
                &format!("SELECT rebuild FROM cloud_workspaces WHERE id=?1 AND deleted IS NULL{}", tx.for_update()),
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(mut asked) =
            queued.flatten().map(|q| serde_json::from_str::<CloudWorkspaceRebuild>(&q)).transpose()?
        else {
            return Ok(None);
        };
        let taken = std::mem::replace(&mut asked.sending, true);
        tx.execute("UPDATE cloud_workspaces SET rebuild=?2 WHERE id=?1", params![id, serde_json::to_string(&asked)?])?;
        tx.commit()?;
        Ok(Some((asked, taken)))
    }

    /// Managoat still had no machine: it waits again, and can be cancelled.
    pub fn unclaim_cloud_workspace_rebuild(&self, id: &str, asked: &CloudWorkspaceRebuild) -> Result<()> {
        let waiting = CloudWorkspaceRebuild { sending: false, ..asked.clone() };
        self.c().execute(
            "UPDATE cloud_workspaces SET rebuild=?2 WHERE id=?1 AND deleted IS NULL AND rebuild IS NOT NULL",
            params![id, serde_json::to_string(&waiting)?],
        )?;
        Ok(())
    }

    /// The person cancels a rebuild or retry waiting for a machine. One the
    /// worker has taken has started; with none waiting, one that's setting
    /// up has too.
    pub fn cancel_cloud_workspace_rebuild(&self, account: &str, id: &str) -> Result<CancelledRebuild> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let found: Option<(Option<String>, String)> = tx
            .query_row(
                &format!(
                    "SELECT rebuild,status FROM cloud_workspaces WHERE account=?1 AND id=?2 AND deleted IS NULL{}",
                    tx.for_update()
                ),
                params![account, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((queued, status)) = found else {
            return Ok(CancelledRebuild::NotFound);
        };
        let Some(asked) = queued.map(|q| serde_json::from_str::<CloudWorkspaceRebuild>(&q)).transpose()? else {
            return Ok(if status == "provisioning" { CancelledRebuild::Started } else { CancelledRebuild::NotWaiting });
        };
        if asked.sending {
            return Ok(CancelledRebuild::Started);
        }
        tx.execute("UPDATE cloud_workspaces SET rebuild=NULL,rebuild_error=NULL WHERE id=?1", params![id])?;
        tx.commit()?;
        Ok(CancelledRebuild::Cancelled)
    }

    /// The one waiting won't be sent; `error` says why, when the person
    /// should know.
    pub fn drop_cloud_workspace_rebuild(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET rebuild=NULL,rebuild_error=?2 WHERE id=?1 AND deleted IS NULL",
            params![id, error],
        )?;
        Ok(())
    }

    /// The open tabs on these Managoat conversations, as "agent · runtime".
    pub fn cloud_tab_names(
        &self,
        account: &str,
        cloud_workspace: &str,
        conversations: &[String],
    ) -> Result<Vec<String>> {
        let rows = self.c().query_map(
            "SELECT provider_id,spec FROM cloud_tabs
             WHERE account=?1 AND cloud_workspace=?2 AND closed IS NULL AND provider_id IS NOT NULL ORDER BY created,id",
            params![account, cloud_workspace],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )?;
        let mut names = vec![];
        for (conversation, spec) in rows {
            if conversations.contains(&conversation) {
                let spec: CloudTabSpec = serde_json::from_str(&spec)?;
                names.push(format!("{} · {}", spec.agent.name, spec.runtime));
            }
        }
        Ok(names)
    }
}

impl Db {
    /// Open a tab and its job. A retry with the same request key returns the
    /// first one; with other settings it's a conflict. A Cloud workspace
    /// being deleted, or one Managoat never made, takes no tabs.
    pub fn create_cloud_tab(
        &self,
        account: &str,
        cloud_workspace: &str,
        request_key: &str,
        spec: &CloudTabSpec,
        now: u64,
    ) -> Result<CloudTab> {
        if request_key.is_empty() || request_key.len() > 128 {
            bail!("invalid request key");
        }
        once_more(|| self.create_cloud_tab_once(account, cloud_workspace, request_key, spec, now))
    }

    fn create_cloud_tab_once(
        &self,
        account: &str,
        cloud_workspace: &str,
        request_key: &str,
        spec: &CloudTabSpec,
        now: u64,
    ) -> Result<CloudTab> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let existing = tx
            .query_row(
                &format!(
                    "SELECT {CLOUD_TAB_COLUMNS} FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace
                     WHERE t.account=?1 AND t.cloud_workspace=?2 AND t.request_key=?3"
                ),
                params![account, cloud_workspace, request_key],
                cloud_tab_row,
            )
            .optional()?;
        if let Some((tab, existing, _)) = existing {
            if existing != *spec {
                bail!("request key already used with different input");
            }
            return Ok(tab);
        }
        // The Cloud workspace's row, held until the tab is in, so a delete
        // either waits for it or is seen here (M144).
        let found: Option<(String, Option<String>)> = tx
            .query_row(
                &format!(
                    "SELECT status,provider_id FROM cloud_workspaces WHERE account=?1 AND id=?2 AND deleted IS NULL{}",
                    tx.for_update()
                ),
                params![account, cloud_workspace],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match found {
            None => bail!("cloud workspace not found"),
            Some((status, _)) if status == "deleting" => bail!("cloud workspace is being deleted"),
            Some((status, None)) if status == "failed" => bail!("cloud workspace has no machine"),
            Some(_) => {}
        }
        let id = format!("cwc_{}", hex::encode(arugula_e2e::random::<16>()));
        tx.execute(
            "INSERT INTO cloud_tabs(id,account,cloud_workspace,request_key,spec,created) VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, account, cloud_workspace, request_key, serde_json::to_string(spec)?, now],
        )
        .map_err(taken)?;
        tx.execute(
            "INSERT INTO cloud_jobs(id,kind,body) VALUES(?1,'cloud_tab',?2)",
            params![id, serde_json::to_string(&CloudTabExecution::default())?],
        )?;
        let (created, _, _) = tx.query_row(
            &format!(
                "SELECT {CLOUD_TAB_COLUMNS} FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace WHERE t.id=?1"
            ),
            params![&id],
            cloud_tab_row,
        )?;
        tx.commit()?;
        Ok(created)
    }

    /// A Cloud workspace's tabs that aren't closed yet, oldest first.
    pub fn cloud_tabs(&self, account: &str, cloud_workspace: &str) -> Result<Vec<CloudTab>> {
        self.c().query_map(
            &format!(
                "SELECT {CLOUD_TAB_COLUMNS} FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace
                 WHERE t.account=?1 AND t.cloud_workspace=?2 AND t.closed IS NULL ORDER BY t.created, t.id"
            ),
            params![account, cloud_workspace],
            |r| cloud_tab_row(r).map(|(tab, _, _)| tab),
        )
    }

    pub fn cloud_tab(&self, account: &str, cloud_workspace: &str, tab: &str) -> Result<Option<CloudTab>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {CLOUD_TAB_COLUMNS} FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace
                     WHERE t.account=?1 AND t.cloud_workspace=?2 AND t.id=?3 AND t.closed IS NULL"
                ),
                params![account, cloud_workspace, tab],
                |r| cloud_tab_row(r).map(|(tab, _, _)| tab),
            )
            .optional()
    }

    /// The account's closed tabs, newest closed first: what a closed tab or
    /// a deleted Cloud workspace leaves to read.
    pub fn past_cloud_tabs(&self, account: &str) -> Result<Vec<PastCloudTab>> {
        self.c().query_map(
            "SELECT t.id,t.cloud_workspace,w.name,w.deleted IS NOT NULL,t.spec,t.created,t.closed
             FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace
             WHERE t.account=?1 AND t.closed IS NOT NULL ORDER BY t.closed DESC, t.id",
            params![account],
            past_cloud_tab_row,
        )
    }

    pub fn past_cloud_tab(&self, account: &str, tab: &str) -> Result<Option<PastCloudTab>> {
        self.c()
            .query_row(
                "SELECT t.id,t.cloud_workspace,w.name,w.deleted IS NOT NULL,t.spec,t.created,t.closed
                 FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace
                 WHERE t.account=?1 AND t.id=?2 AND t.closed IS NOT NULL",
                params![account, tab],
                past_cloud_tab_row,
            )
            .optional()
    }

    /// The Managoat conversation of a tab that's open and launched.
    pub fn cloud_tab_conversation(&self, account: &str, cloud_workspace: &str, tab: &str) -> Result<Option<String>> {
        Ok(self
            .c()
            .query_row(
                "SELECT t.provider_id FROM cloud_tabs t JOIN cloud_jobs j ON j.id=t.id
                 WHERE t.account=?1 AND t.cloud_workspace=?2 AND t.id=?3 AND t.closed IS NULL AND j.cancel=FALSE",
                params![account, cloud_workspace, tab],
                |r| r.get::<Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// A prompt to a tab whose conversation failed: the worker opens a new
    /// conversation for the tab with it (Managoat's `failed` is terminal).
    /// The tab reads `opening` until then. `gone`: Managoat refused a prompt
    /// to it as ended before control saw it fail. False when the tab isn't
    /// open, or neither failed nor gone.
    pub fn relaunch_cloud_tab(
        &self,
        account: &str,
        cloud_workspace: &str,
        tab: &str,
        prompt: &str,
        gone: bool,
    ) -> Result<bool> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let changed = tx.execute(
            "UPDATE cloud_tabs SET relaunch=?4,status='opening',error=NULL
             WHERE account=?1 AND cloud_workspace=?2 AND id=?3 AND closed IS NULL AND provider_id IS NOT NULL
               AND relaunch IS NULL AND (status='failed' OR ?5) AND status NOT IN ('closing','closed')
               AND EXISTS(SELECT 1 FROM cloud_jobs WHERE id=?3 AND cancel=FALSE)",
            params![account, cloud_workspace, tab, prompt, gone],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Ask the worker to close it. Saying so again is harmless.
    pub fn close_cloud_tab(&self, account: &str, cloud_workspace: &str, tab: &str) -> Result<bool> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let changed = tx.execute(
            "UPDATE cloud_tabs SET status='closing' WHERE account=?1 AND cloud_workspace=?2 AND id=?3 AND closed IS NULL",
            params![account, cloud_workspace, tab],
        )?;
        tx.execute("UPDATE cloud_jobs SET cancel=TRUE WHERE id=?1 AND kind='cloud_tab'", params![tab])?;
        tx.execute("UPDATE cloud_requests SET pending=FALSE WHERE conversation=?1", params![tab])?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// How many of a Cloud workspace's tabs still have a Managoat agent.
    pub fn open_cloud_tabs(&self, cloud_workspace: &str) -> Result<u64> {
        self.c().query_row(
            "SELECT count(*) FROM cloud_tabs WHERE cloud_workspace=?1 AND closed IS NULL",
            params![cloud_workspace],
            |r| r.get(0),
        )
    }

    pub fn claim_cloud_tab_job(&self, now: u64) -> Result<Option<CloudTabJob>> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let found = tx
            .query_row(
                &format!(
                    "SELECT {CLOUD_TAB_COLUMNS},t.account,j.body,j.cancel,w.provider_id,w.environment,w.vault,t.relaunch
                     FROM cloud_tabs t JOIN cloud_workspaces w ON w.id=t.cloud_workspace JOIN cloud_jobs j ON j.id=t.id
                     WHERE j.kind='cloud_tab' AND t.closed IS NULL AND j.lease_until<=?1 ORDER BY j.lease_until,t.created LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| {
                    Ok((
                        cloud_tab_row(r)?,
                        r.get::<String>(8)?,
                        r.get::<String>(9)?,
                        r.get::<bool>(10)?,
                        (r.get(11)?, r.get(12)?, r.get(13)?),
                        r.get::<Option<String>>(14)?,
                    ))
                },
            )
            .optional()?;
        let Some(((tab, spec, started), account, body, closing, (workspace, environment, vault), relaunch)) = found
        else {
            return Ok(None);
        };
        let Some(lease) = lease(&tx, &tab.id, now)? else { return Ok(None) };
        tx.commit()?;
        Ok(Some(CloudTabJob {
            tab,
            spec,
            account,
            execution: serde_json::from_str(&body)?,
            started,
            closing,
            relaunch,
            relaunch_taken: false,
            workspace,
            environment,
            vault,
            lease,
        }))
    }

    /// Checkpoint under the claim. A close asked for meanwhile keeps the
    /// status `closing`, and a tab that's closing or has ended asks nothing.
    pub fn checkpoint_cloud_tab_job(&self, job: &CloudTabJob, release_at: Option<u64>) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let changed = tx.execute(
            "UPDATE cloud_jobs SET body=?1,lease_until=COALESCE(?2,lease_until) WHERE id=?3 AND lease=?4",
            params![serde_json::to_string(&job.execution)?, release_at, job.tab.id, job.lease],
        )?;
        if changed != 1 {
            bail!("cloud job lease lost");
        }
        tx.execute(
            "UPDATE cloud_tabs SET
                 status=CASE WHEN (SELECT cancel FROM cloud_jobs WHERE id=?1) THEN 'closing'
                             WHEN relaunch IS NOT NULL AND NOT (?7 AND relaunch=?6) THEN 'opening' ELSE ?2 END,
                 error=CASE WHEN relaunch IS NOT NULL AND NOT (?7 AND relaunch=?6) THEN NULL ELSE ?3 END,
                 started=?4, provider_id=?5,
                 relaunch=CASE WHEN ?7 AND relaunch=?6 THEN NULL ELSE relaunch END
             WHERE id=?1",
            params![
                job.tab.id,
                job.tab.status,
                job.tab.error,
                job.started,
                job.execution.conversation,
                job.relaunch,
                job.relaunch_taken
            ],
        )?;
        if job.closing || matches!(job.tab.status.as_str(), "failed" | "terminated") {
            tx.execute("UPDATE cloud_requests SET pending=FALSE WHERE conversation=?1", params![&job.tab.id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Its conversation and agent are gone: keep the row (its transcript
    /// stays readable to control) and end the job.
    pub fn finish_cloud_tab_close(&self, job: &CloudTabJob, now: u64) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        if tx.execute("DELETE FROM cloud_jobs WHERE id=?1 AND lease=?2", params![job.tab.id, job.lease])? != 1 {
            bail!("cloud job lease lost");
        }
        tx.execute("UPDATE cloud_tabs SET status='closed',closed=?1 WHERE id=?2", params![now, job.tab.id])?;
        tx.execute("UPDATE cloud_requests SET pending=FALSE WHERE conversation=?1", params![&job.tab.id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_db() -> Db {
        Db::test()
    }

    fn spec() -> RunSpec {
        RunSpec {
            agent_id: "agent_test".into(),
            agent_version: 1,
            model: "anthropic/test".into(),
            runtime: "claude".into(),
            agent: AgentDefinition {
                name: "Test".into(),
                instructions: "Fix tests".into(),
                mcp_servers: Default::default(),
                skills: vec![],
                secret_names: vec![],
            },
            repository: "https://github.com/example/repo".into(),
            git_ref: "main".into(),
            repository_secret: None,
            prompt: "fix tests".into(),
        }
    }
    /// A run left running when runs were retired.
    fn run(db: &Db, account: &str, spec: &RunSpec) -> CloudRun {
        db.insert_cloud_run(account, spec, &Execution::default(), "running", 0).unwrap()
    }

    fn cloud_workspace_spec() -> CloudWorkspaceSpec {
        let mut spec: CloudWorkspaceSpec = serde_json::from_value(
            serde_json::json!({"name":"Box","repositories":[{"url":"https://github.com/example/app"}]}),
        )
        .unwrap();
        spec.normalize().unwrap();
        spec
    }

    #[test]
    fn account_erasure_waits_for_cloud_workspaces_to_be_deleted() {
        let db = test_db();
        let kept = db.create_cloud_workspace("b", "prj_b", "keep", &cloud_workspace_spec(), 0).unwrap();
        let w = db.create_cloud_workspace("a", "prj_a", "erase", &cloud_workspace_spec(), 0).unwrap();
        assert!(!db.cloud_workspaces_gone("a").unwrap());
        db.delete_cloud_workspaces("a").unwrap();
        assert!(!db.cloud_workspaces_gone("a").unwrap(), "still deleting");
        let job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        let job = if job.cloud_workspace.id == w.id { job } else { db.claim_cloud_workspace_job(0).unwrap().unwrap() };
        assert!(job.deleting);
        db.finish_cloud_workspace_delete(&job, 1).unwrap();
        assert!(db.cloud_workspaces_gone("a").unwrap());
        assert_eq!(db.erase_cloud_account("a").unwrap(), Vec::<String>::new());
        let rows: u64 = db.c().query_row("SELECT count(*) FROM cloud_workspaces", params![], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
        let jobs: u64 = db.c().query_row("SELECT count(*) FROM cloud_jobs", params![], |r| r.get(0)).unwrap();
        assert_eq!(jobs, 1);
        assert_eq!(db.cloud_workspace("b", &kept.id).unwrap().unwrap().status, "provisioning");
    }

    #[test]
    fn a_worker_that_read_before_a_rebuild_does_not_undo_it() {
        let db = test_db();
        let created = db.create_cloud_workspace("a", "prj_a", "one", &cloud_workspace_spec(), 0).unwrap();
        let mut job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        job.execution.stage = "live".into();
        job.cloud_workspace.status = "ready".into();
        job.cloud_workspace.stale = true;
        db.checkpoint_cloud_workspace_job(&job, Some(0)).unwrap();
        // The worker reads `ready` and stale; a rebuild is taken meanwhile.
        let mut job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        let mut rebuilt = job.cloud_workspace.clone();
        rebuilt.status = "provisioning".into();
        db.queue_cloud_workspace_rebuild(
            &created.id,
            &CloudWorkspaceRebuild { retry: false, interrupt: false, asked: 0, sending: false },
        )
        .unwrap();
        assert!(db.cloud_workspace("a", &created.id).unwrap().unwrap().rebuild_queued);
        db.rebuilt_cloud_workspace(&rebuilt).unwrap();
        job.cloud_workspace.status = "ready".into();
        db.checkpoint_cloud_workspace_job(&job, Some(0)).unwrap();
        let shown = db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!((shown.status.as_str(), shown.rebuild_queued), ("provisioning", false));
        // The next claim reads past it.
        let mut job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        job.cloud_workspace.status = "ready".into();
        db.checkpoint_cloud_workspace_job(&job, Some(0)).unwrap();
        assert_eq!(db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "ready");
    }

    #[test]
    fn pending_changes_follow_managoats_fields_and_never_show_a_value() {
        let built = cloud_workspace_spec();
        let mut saved = built.clone();
        saved.networking =
            Some(CloudWorkspaceNetworking { kind: "limited".into(), allowed_hosts: vec!["x.example".into()] });
        saved.env_vars.insert("TOKEN".into(), "hunter2".into());
        let fields: Vec<String> =
            ["env_vars", "networking_type", "networking_config"].into_iter().map(String::from).collect();
        let pending = pending_changes(Some(&built), &saved, true, &fields);
        assert_eq!(pending.iter().map(|c| c.field.as_str()).collect::<Vec<_>>(), ["env_vars", "networking"]);
        assert_eq!(pending[0].after, serde_json::json!(["TOKEN"]));
        assert_eq!(pending[1].before, serde_json::Value::Null);
        assert_eq!(pending[1].after["allowed_hosts"], serde_json::json!(["x.example"]));
        assert!(!serde_json::to_string(&pending).unwrap().contains("hunter2"));
        // Not stale: nothing to rebuild, whatever the names say; no built
        // spec: no before.
        assert!(pending_changes(Some(&built), &saved, false, &fields).is_empty());
        assert_eq!(pending_changes(None, &saved, true, &fields)[0].before, serde_json::Value::Null);
    }

    #[test]
    fn account_erasure_waits_for_tabs_and_takes_their_events() {
        let db = test_db();
        let w = db.create_cloud_workspace("a", "prj_a", "erase", &cloud_workspace_spec(), 0).unwrap();
        db.c()
            .execute("UPDATE cloud_workspaces SET status='ready',provider_id='ws1' WHERE id=?1", params![&w.id])
            .unwrap();
        let spec = CloudTabSpec {
            agent_id: "agent_test".into(),
            agent_version: 1,
            agent: spec().agent,
            runtime: "claude".into(),
            model: "anthropic/test".into(),
            cwd: "/home/sprite/app".into(),
            prompt: None,
        };
        assert!(db.create_cloud_tab("b", &w.id, "tab", &spec, 0).is_err(), "another account's");
        let tab = db.create_cloud_tab("a", &w.id, "tab", &spec, 0).unwrap();
        db.c()
            .execute(
                "INSERT INTO cloud_events(conversation,account,cursor,body) VALUES(?1,'a',1,'{}')",
                params![&tab.id],
            )
            .unwrap();
        // Deleting the Cloud workspace closes its tabs.
        db.delete_cloud_workspaces("a").unwrap();
        assert_eq!(db.cloud_tab("a", &w.id, &tab.id).unwrap().unwrap().status, "closing");
        assert!(db.create_cloud_tab("a", &w.id, "late", &spec, 0).is_err());
        let w_job = loop {
            let job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
            if job.cloud_workspace.id == w.id {
                break job;
            }
        };
        db.finish_cloud_workspace_delete(&w_job, 1).unwrap();
        assert!(!db.cloud_workspaces_gone("a").unwrap(), "its tab still has an agent");
        let tab_job = db.claim_cloud_tab_job(0).unwrap().unwrap();
        assert!(tab_job.closing);
        db.finish_cloud_tab_close(&tab_job, 1).unwrap();
        // Kept as a past tab until the account goes, then erased with it.
        assert_eq!(db.past_cloud_tabs("a").unwrap().len(), 1);
        assert!(db.past_cloud_tab("b", &tab.id).unwrap().is_none());
        assert!(db.cloud_workspaces_gone("a").unwrap());
        db.erase_cloud_account("a").unwrap();
        assert!(db.past_cloud_tabs("a").unwrap().is_empty());
        for table in ["cloud_tabs", "cloud_events", "cloud_jobs", "cloud_workspaces"] {
            let count: u64 =
                db.c().query_row(&format!("SELECT count(*) FROM {table}"), params![], |r| r.get(0)).unwrap();
            assert_eq!(count, 0, "{table}");
        }
    }

    #[test]
    fn built_spec_follows_the_revision_that_reached_ready() {
        let db = test_db();
        let w = db.create_cloud_workspace("a", "prj_a", "built", &cloud_workspace_spec(), 0).unwrap();
        let mut job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        job.saved_revision = Some("rev1".into());
        job.cloud_workspace.status = "ready".into();
        job.cloud_workspace.built_from_revision = Some("rev0".into());
        db.checkpoint_cloud_workspace_job(&job, Some(0)).unwrap();
        assert!(db.cloud_workspace("a", &w.id).unwrap().unwrap().built_spec.is_none());
        let mut job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        assert_eq!(job.saved_revision.as_deref(), Some("rev1"));
        job.cloud_workspace.built_from_revision = Some("rev1".into());
        job.cloud_workspace.stale = true;
        job.cloud_workspace.changed_fields = vec!["packages".into()];
        db.checkpoint_cloud_workspace_job(&job, Some(0)).unwrap();
        let saved = db.cloud_workspace("a", &w.id).unwrap().unwrap();
        assert_eq!(saved.built_spec, Some(cloud_workspace_spec()));
        assert!(saved.stale);
        assert_eq!(saved.changed_fields, ["packages"]);
        // A delete asked for under a worker's claim stays asked for.
        let job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        db.delete_cloud_workspace("a", &w.id).unwrap();
        db.checkpoint_cloud_workspace_job(&job, Some(0)).unwrap();
        assert_eq!(db.cloud_workspace("a", &w.id).unwrap().unwrap().status, "deleting");
    }

    #[test]
    fn erasing_an_account_takes_every_cloud_row_of_its() {
        let db = test_db();
        let run = run(&db, "a", &spec());
        db.create_cloud_agent("a", spec().agent).unwrap();
        db.put_cloud_secret("a", "prj_a", "TOKEN", &crate::keys::Sealed { key_version: 1, bytes: b"sealed".to_vec() })
            .unwrap();
        db.put_cloud_secret("b", "prj_b", "TOKEN", &crate::keys::Sealed { key_version: 1, bytes: b"other".to_vec() })
            .unwrap();
        assert_eq!(db.active_cloud_runs("a").unwrap(), 1);
        let job = db.claim_cloud_job(0).unwrap().unwrap();
        db.checkpoint_cloud_job(&job, "cancelled", Some(&serde_json::json!({})), None).unwrap();
        // Populate every child table so this catches incomplete erasure and
        // foreign-key ordering, including already-delivered notifications.
        for sql in [
            "INSERT INTO cloud_run_secrets(run,name,sealed) VALUES(?1,'TOKEN','x')",
            "INSERT INTO cloud_requests(conversation,account,id,body) VALUES(?1,'a','approval','{}')",
            "INSERT INTO cloud_notifications(conversation,account,request) VALUES(?1,'a','approval')",
            "INSERT INTO cloud_events(conversation,account,cursor,body) VALUES(?1,'a',1,'{}')",
        ] {
            db.c().execute(sql, params![&run.id]).unwrap();
        }
        assert_eq!(db.active_cloud_runs("a").unwrap(), 0);
        db.erase_cloud_account("a").unwrap();
        for table in [
            "cloud_runs",
            "cloud_agents",
            "cloud_run_secrets",
            "cloud_jobs",
            "cloud_requests",
            "cloud_notifications",
            "cloud_events",
        ] {
            let count: u64 =
                db.c().query_row(&format!("SELECT count(*) FROM {table}"), params![], |r| r.get(0)).unwrap();
            assert_eq!(count, 0, "{table}");
        }
        assert!(db.cloud_secret_names("a").unwrap().is_empty());
        assert_eq!(db.cloud_secret_names("b").unwrap(), ["TOKEN"]);
    }

    #[test]
    fn approval_replay_does_not_reopen_resolved_requests_or_duplicate_notifications() {
        let db = test_db();
        let run = run(&db, "a", &spec());
        let job = db.claim_cloud_job(0).unwrap().unwrap();
        let request = serde_json::json!({"id":1,"blocks":[{"kind":"permission_request","request_id":"approve-1","options":[{"optionId":"allow"}]}]});
        db.append_cloud_events(&job, std::slice::from_ref(&request)).unwrap();
        assert_eq!(db.cloud_requests("a", &run.id).unwrap().len(), 1);
        assert!(db.cloud_requests("b", &run.id).unwrap().is_empty());
        assert_eq!(db.cloud_notifications(0).unwrap().len(), 1);
        db.resolve_cloud_request("b", &run.id, "approve-1").unwrap();
        assert_eq!(db.cloud_requests("a", &run.id).unwrap().len(), 1);
        db.resolve_cloud_request("a", &run.id, "approve-1").unwrap();
        db.append_cloud_events(&job, &[request]).unwrap();
        assert!(db.cloud_requests("a", &run.id).unwrap().is_empty());
        assert!(db.cloud_notifications(0).unwrap().is_empty());
        db.append_cloud_events(
            &job,
            &[serde_json::json!({"id":2,"blocks":[{"kind":"elicitation_request","request_id":"question"}]})],
        )
        .unwrap();
        db.append_cloud_events(
            &job,
            &[serde_json::json!({"id":3,"stage":"request","state":"done","data":"{\"request_id\":\"question\"}"})],
        )
        .unwrap();
        assert!(db.cloud_requests("a", &run.id).unwrap().is_empty());
    }

    #[test]
    fn stale_worker_cannot_overwrite_reclaimed_job() {
        let db = test_db();
        let run = run(&db, "a", &spec());
        assert!(db.cloud_run("b", &run.id).unwrap().is_none());
        let first = db.claim_cloud_job(0).unwrap().unwrap();
        assert!(db.claim_cloud_job(100).unwrap().is_none());
        let second = db.claim_cloud_job(120_001).unwrap().unwrap();
        assert_eq!(first.run.id, second.run.id);
        assert!(db.checkpoint_cloud_job(&first, "running", None, Some(0)).is_err());
        assert!(db.append_cloud_events(&first, &[serde_json::json!({"id":1})]).is_err());
        assert!(db.checkpoint_cloud_job(&second, "running", None, Some(130_000)).is_ok());
        assert_eq!(db.claim_cloud_job(130_000).unwrap().unwrap().run.id, run.id);
    }

    /// Two handles on one database, as two machines have (M144): on
    /// Postgres the test's schema twice; on SQLite a file opened twice
    /// (removed when the guard drops).
    fn two_machines() -> (Db, Db, Option<std::path::PathBuf>) {
        let db = test_db();
        if let Some(other) = db.second() {
            return (db, other, None);
        }
        let path = std::env::temp_dir().join(format!("arugula-two-{}.db", hex::encode(arugula_e2e::random::<8>())));
        let (a, b) = (Db::open(&path).unwrap(), Db::open(&path).unwrap());
        // Eight threads on one file wait their turn for its lock; a loaded
        // CI runner can take longer than SQLite's usual five seconds.
        for db in [&a, &b] {
            db.sqlite().busy_timeout(std::time::Duration::from_secs(60)).unwrap();
        }
        (a, b, Some(path))
    }

    /// Two machines' workers, four each, claiming at once (M144): every
    /// run, Cloud workspace and tab job is taken exactly once, and none
    /// twice, until their leases run out.
    #[test]
    fn two_machines_never_claim_the_same_job() {
        let (a, b, file) = two_machines();
        let spec_tab = CloudTabSpec {
            agent_id: "agent_test".into(),
            agent_version: 1,
            agent: spec().agent,
            runtime: "claude".into(),
            model: "anthropic/test".into(),
            cwd: "/home/sprite/app".into(),
            prompt: None,
        };
        let mut want = std::collections::BTreeSet::new();
        for i in 0..12 {
            want.insert(run(&a, "a", &spec()).id);
            let w = a.create_cloud_workspace("a", "prj_a", &format!("k{i}"), &cloud_workspace_spec(), i).unwrap();
            a.c()
                .execute("UPDATE cloud_workspaces SET status='ready',provider_id='ws' WHERE id=?1", params![&w.id])
                .unwrap();
            want.insert(w.id.clone());
            want.insert(a.create_cloud_tab("a", &w.id, "tab", &spec_tab, i).unwrap().id);
        }
        let claimed = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for db in [&a, &b] {
                for _ in 0..4 {
                    scope.spawn(|| {
                        loop {
                            let mut got = vec![];
                            got.extend(db.claim_cloud_job(1_000).unwrap().map(|j| j.run.id));
                            got.extend(db.claim_cloud_workspace_job(1_000).unwrap().map(|j| j.cloud_workspace.id));
                            got.extend(db.claim_cloud_tab_job(1_000).unwrap().map(|j| j.tab.id));
                            if got.is_empty() {
                                return;
                            }
                            claimed.lock().unwrap().extend(got);
                        }
                    });
                }
            }
        });
        let claimed = claimed.into_inner().unwrap();
        let unique: std::collections::BTreeSet<String> = claimed.iter().cloned().collect();
        assert_eq!(unique.len(), claimed.len(), "a job was claimed twice");
        assert_eq!(unique, want, "every job, once");
        // Leased: nobody takes them again until the lease runs out.
        assert!(b.claim_cloud_workspace_job(1_000 + LEASE_MS - 1).unwrap().is_none());
        assert!(b.claim_cloud_workspace_job(1_000 + LEASE_MS).unwrap().is_some());
        drop((a, b));
        if let Some(f) = file {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", f.display()));
            }
        }
    }

    /// On Postgres a claim passes over a job another machine's claim is
    /// holding, rather than waiting for it (`SKIP LOCKED`).
    #[test]
    fn a_claim_passes_over_a_job_another_holds() {
        let (a, b, _) = two_machines();
        if a.dialect() != Dialect::Postgres {
            return;
        }
        let first = a.create_cloud_workspace("a", "prj_a", "one", &cloud_workspace_spec(), 0).unwrap();
        let second = a.create_cloud_workspace("a", "prj_a", "two", &cloud_workspace_spec(), 1).unwrap();
        let mut c = a.c();
        let held = c.transaction().unwrap();
        held.execute(&format!("SELECT id FROM cloud_jobs WHERE id=?1{}", held.for_update()), params![&first.id])
            .unwrap();
        let b = std::sync::Arc::new(b);
        let (tx, rx) = std::sync::mpsc::channel();
        let claimer = b.clone();
        std::thread::spawn(move || {
            let one = claimer.claim_cloud_workspace_job(0).unwrap().map(|j| j.cloud_workspace.id);
            let two = claimer.claim_cloud_workspace_job(0).unwrap().map(|j| j.cloud_workspace.id);
            let _ = tx.send((one, two));
        });
        let claimed = rx.recv_timeout(std::time::Duration::from_secs(5));
        drop(held);
        let (one, two) = claimed.expect("the claim waited for the held job instead of passing over it");
        assert_eq!(one.as_deref(), Some(second.id.as_str()), "the one nobody holds");
        assert_eq!(two, None, "the held one is passed over");
        assert_eq!(b.claim_cloud_workspace_job(0).unwrap().unwrap().cloud_workspace.id, first.id);
    }

    #[test]
    fn run_and_agent_survive_database_reopen() {
        let path = std::env::temp_dir().join(format!("arugula-cloud-{}.db", hex::encode(arugula_e2e::random::<16>())));
        let id;
        {
            let db = Db::open(&path).unwrap();
            db.create_cloud_agent("a", spec().agent).unwrap();
            id = run(&db, "a", &spec()).id;
        }
        {
            let db = Db::open(&path).unwrap();
            assert_eq!(db.cloud_agents("a").unwrap().len(), 1);
            assert_eq!(db.cloud_run("a", &id).unwrap().unwrap().spec, spec());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn catalog_enforces_ownership_versions_and_snapshots() {
        let db = test_db();
        let agent = db.create_cloud_agent("a", spec().agent).unwrap();
        assert!(db.cloud_agents("b").unwrap().is_empty());
        let mut launch = spec();
        launch.agent_id = agent.id.clone();
        launch.agent = agent.definition.clone();
        let run = run(&db, "a", &launch);
        let mut edited = agent.definition.clone();
        edited.instructions = "New instructions".into();
        assert!(!db.update_cloud_agent("b", &agent.id, 1, &edited).unwrap());
        assert!(db.update_cloud_agent("a", &agent.id, 1, &edited).unwrap());
        assert!(!db.update_cloud_agent("a", &agent.id, 1, &edited).unwrap());
        assert!(!db.archive_cloud_agent("b", &agent.id).unwrap());
        assert!(db.archive_cloud_agent("a", &agent.id).unwrap());
        assert!(db.cloud_agents("a").unwrap().is_empty());
        assert_eq!(db.cloud_run("a", &run.id).unwrap().unwrap().spec.agent, agent.definition);
    }

    #[test]
    fn custom_definitions_reject_reserved_secrets_and_oversize_content() {
        let mut agent = spec().agent;
        agent.secret_names = vec!["MANAGOAT_API_KEY".into()];
        assert!(agent.validate().is_err());
        agent.secret_names = vec!["GITHUB_TOKEN".into()];
        assert!(agent.validate().is_ok());
        agent.instructions = "x".repeat(64_001);
        assert!(agent.validate().is_err());
    }
}
