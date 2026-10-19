//! Durable cloud execution records. Provider identifiers never form the public
//! authorization boundary: every lookup is scoped to the owning account.
use super::Db;
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS cloud_agents (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    version INTEGER NOT NULL,
    definition TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS cloud_agents_owner ON cloud_agents(account);
CREATE TABLE IF NOT EXISTS cloud_runs (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    request_key TEXT NOT NULL,
    spec TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued',
    provider_id TEXT,
    created INTEGER NOT NULL,
    deadline INTEGER NOT NULL,
    result TEXT,
    UNIQUE(account, request_key)
);
CREATE INDEX IF NOT EXISTS cloud_runs_owner ON cloud_runs(account, created);
CREATE TABLE IF NOT EXISTS cloud_secrets (
    account TEXT NOT NULL,
    name TEXT NOT NULL,
    sealed BLOB NOT NULL,
    PRIMARY KEY(account,name)
);
CREATE TABLE IF NOT EXISTS cloud_run_secrets (
    run TEXT NOT NULL REFERENCES cloud_runs(id),
    name TEXT NOT NULL,
    sealed BLOB NOT NULL,
    PRIMARY KEY(run,name)
);
-- One per run, and one per Cloud workspace for its whole life: `id` is the
-- run's or the Cloud workspace's, and `kind` says which ('run' or
-- 'cloud_workspace'). `cancel` on a Cloud workspace's job is its delete.
CREATE TABLE IF NOT EXISTS cloud_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'run',
    body TEXT NOT NULL,
    cancel INTEGER NOT NULL DEFAULT 0,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS cloud_requests (
    run TEXT NOT NULL REFERENCES cloud_runs(id),
    id TEXT NOT NULL,
    body TEXT NOT NULL,
    pending INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(run,id)
);
CREATE TABLE IF NOT EXISTS cloud_notifications (
    run TEXT NOT NULL REFERENCES cloud_runs(id),
    request TEXT NOT NULL,
    delivered INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(run,request)
);
CREATE TABLE IF NOT EXISTS cloud_events (
    run TEXT NOT NULL REFERENCES cloud_runs(id),
    cursor INTEGER NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY(run,cursor)
);
-- `saved_spec` is what the user saved and `saved_revision` the environment
-- revision it became; `built_spec` is the saved spec as of the revision that
-- last reached `ready`. The rest mirrors the Managoat workspace's last poll.
-- `deleted` is set once its Managoat resources are gone.
CREATE TABLE IF NOT EXISTS cloud_workspaces (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    request_key TEXT NOT NULL,
    name TEXT NOT NULL,
    saved_spec TEXT NOT NULL,
    saved_revision TEXT,
    built_spec TEXT,
    status TEXT NOT NULL DEFAULT 'provisioning',
    sandbox_status TEXT,
    error TEXT,
    revision TEXT,
    built_from_revision TEXT,
    stale INTEGER NOT NULL DEFAULT 0,
    changed_fields TEXT NOT NULL DEFAULT '[]',
    environment TEXT,
    vault TEXT,
    provider_id TEXT,
    created INTEGER NOT NULL,
    deleted INTEGER,
    UNIQUE(account, request_key)
);
CREATE INDEX IF NOT EXISTS cloud_workspaces_owner ON cloud_workspaces(account, created);
";

/// `cloud_jobs` keyed on a run (`run`, a foreign key to `cloud_runs`) before
/// Cloud workspaces (M114): rebuilt keyed on `id`, with a `kind`.
pub(super) fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let keyed_on_run: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('cloud_jobs') WHERE name='run')", [], |r| {
            r.get(0)
        })?;
    if keyed_on_run {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE cloud_jobs_v2 (
                 id TEXT PRIMARY KEY,
                 kind TEXT NOT NULL DEFAULT 'run',
                 body TEXT NOT NULL,
                 cancel INTEGER NOT NULL DEFAULT 0,
                 lease TEXT,
                 lease_until INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO cloud_jobs_v2(id,kind,body,cancel,lease,lease_until)
                 SELECT run,'run',body,cancel,lease,lease_until FROM cloud_jobs;
             DROP TABLE cloud_jobs;
             ALTER TABLE cloud_jobs_v2 RENAME TO cloud_jobs;
             COMMIT;",
        )?;
    }
    Ok(())
}

/// User-editable agent definition. Model funding and execution credentials are
/// supplied by control, not accepted as user-selected provider configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub name: String,
    pub instructions: String,
    #[serde(default)]
    pub mcp_servers: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub secret_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub name: String,
    pub content: String,
}

impl AgentDefinition {
    pub fn validate(&self) -> Result<()> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAgent {
    pub id: String,
    pub version: u64,
    pub definition: AgentDefinition,
    pub curated: bool,
}

/// Immutable launch snapshot. Changing an agent later cannot change a queued run.
/// Credentials belong in the secret store, never in this record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSpec {
    pub agent_id: String,
    pub agent_version: u64,
    pub agent: AgentDefinition,
    pub model: String,
    pub runtime: String,
    pub repository: String,
    pub git_ref: String,
    #[serde(default)]
    pub repository_secret: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudRun {
    pub id: String,
    pub spec: RunSpec,
    pub state: String,
    pub created: u64,
    pub deadline: u64,
    pub result: Option<serde_json::Value>,
}

pub struct RunLimits {
    pub daily_runs: u64,
    pub concurrent_runs: u64,
    pub duration_ms: u64,
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

pub struct Job {
    pub run: CloudRun,
    pub account: String,
    pub execution: Execution,
    pub cancelled: bool,
    pub lease: String,
}

/// What a Cloud workspace's machine is built from, as the user saved it: it
/// becomes the environment control keeps for the Cloud workspace. Secret
/// values stay in the account's secret store; this only names them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspaceSpec {
    pub name: String,
    #[serde(default)]
    pub repositories: Vec<CloudWorkspaceRepository>,
    #[serde(default)]
    pub packages: CloudWorkspacePackages,
    #[serde(default)]
    pub setup_script: String,
    #[serde(default)]
    pub setup_timeout_seconds: Option<u32>,
    #[serde(default)]
    pub env_vars: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub networking: Option<CloudWorkspaceNetworking>,
    /// The account's secrets the vault holds besides the repositories' own.
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspaceRepository {
    pub url: String,
    #[serde(default, rename = "ref")]
    pub git_ref: String,
    /// `/home/sprite/<repository name>` when left out.
    #[serde(default)]
    pub mount: String,
    /// The secret the clone authenticates with, kept in the vault.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Managoat installs `apt` and `npm` packages; other toolchains go in the
/// setup script.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspacePackages {
    #[serde(default)]
    pub apt: Vec<String>,
    #[serde(default)]
    pub npm: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudWorkspaceNetworking {
    /// `unrestricted` or `limited`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

fn plain_word(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.starts_with('-') && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl CloudWorkspaceSpec {
    /// Check what the user sent and fill in the default mounts, so the saved
    /// spec says where each repository is.
    pub fn normalize(&mut self) -> Result<()> {
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

    /// Every secret the vault holds: the repositories' and the others.
    pub fn secret_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.repositories.iter().filter_map(|r| r.secret.clone()).chain(self.secrets.iter().cloned()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The environment's build fields, every one sent so the same body can
    /// replace them on an update.
    pub fn environment(&self) -> serde_json::Value {
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
}

/// A Cloud workspace as the account sees it. Its Managoat ids stay in
/// control.
#[derive(Debug, Clone, Serialize)]
pub struct CloudWorkspace {
    pub id: String,
    pub name: String,
    pub saved_spec: CloudWorkspaceSpec,
    pub built_spec: Option<CloudWorkspaceSpec>,
    /// Managoat's `provisioning`, `ready` or `failed`, or `deleting`.
    pub status: String,
    pub sandbox_status: Option<String>,
    pub error: Option<String>,
    pub revision: Option<String>,
    pub built_from_revision: Option<String>,
    pub stale: bool,
    pub changed_fields: Vec<String>,
    pub created: u64,
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
    pub lease: String,
}

const CLOUD_WORKSPACE_COLUMNS: &str = "id,name,saved_spec,built_spec,status,sandbox_status,error,revision,built_from_revision,stale,changed_fields,created";

fn cloud_workspace_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CloudWorkspace> {
    fn decode<T: serde::de::DeserializeOwned>(i: usize, s: &str) -> rusqlite::Result<T> {
        serde_json::from_str(s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e)))
    }
    Ok(CloudWorkspace {
        id: r.get(0)?,
        name: r.get(1)?,
        saved_spec: decode(2, &r.get::<_, String>(2)?)?,
        built_spec: r.get::<_, Option<String>>(3)?.map(|s| decode(3, &s)).transpose()?,
        status: r.get(4)?,
        sandbox_status: r.get(5)?,
        error: r.get(6)?,
        revision: r.get(7)?,
        built_from_revision: r.get(8)?,
        stale: r.get(9)?,
        changed_fields: decode(10, &r.get::<_, String>(10)?)?,
        created: r.get(11)?,
    })
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CloudRun> {
    let decode = |s: String| {
        serde_json::from_str(&s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))
    };
    let spec: String = r.get(1)?;
    let spec = serde_json::from_str(&spec)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?;
    Ok(CloudRun {
        id: r.get(0)?,
        spec,
        state: r.get(2)?,
        created: r.get(3)?,
        deadline: r.get(4)?,
        result: r.get::<_, Option<String>>(5)?.map(decode).transpose()?,
    })
}

fn require_account(conn: &rusqlite::Connection, account: &str) -> Result<()> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)", [account], |r| r.get(0))?;
    if !exists {
        bail!("account no longer exists");
    }
    Ok(())
}

impl Db {
    pub fn active_cloud_runs(&self, account: &str) -> Result<u64> {
        Ok(self.c().query_row("SELECT count(*) FROM cloud_runs WHERE account=?1 AND state NOT IN ('completed','failed','cancelled','expired')", [account], |r| r.get(0))?)
    }

    pub fn cloud_secret_names(&self, account: &str) -> Result<Vec<String>> {
        let conn = self.c();
        let mut q = conn.prepare("SELECT name FROM cloud_secrets WHERE account=?1 ORDER BY name")?;
        Ok(q.query_map([account], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn put_cloud_secret(&self, account: &str, name: &str, sealed: &[u8]) -> Result<()> {
        let conn = self.c();
        require_account(&conn, account)?;
        conn.execute("INSERT INTO cloud_secrets(account,name,sealed) VALUES(?1,?2,?3) ON CONFLICT(account,name) DO UPDATE SET sealed=excluded.sealed",params![account,name,sealed])?;
        Ok(())
    }
    pub fn delete_cloud_secret(&self, account: &str, name: &str) -> Result<()> {
        self.c().execute("DELETE FROM cloud_secrets WHERE account=?1 AND name=?2", params![account, name])?;
        Ok(())
    }
    pub fn cloud_run_secrets(&self, account: &str, id: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let conn = self.c();
        let mut q = conn.prepare("SELECT s.name,s.sealed FROM cloud_run_secrets s JOIN cloud_runs r ON r.id=s.run WHERE r.account=?1 AND r.id=?2 ORDER BY s.name")?;
        Ok(q.query_map(params![account, id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn cloud_agents(&self, account: &str) -> Result<Vec<CloudAgent>> {
        let conn = self.c();
        let mut q =
            conn.prepare("SELECT id,version,definition FROM cloud_agents WHERE account=?1 AND archived=0 ORDER BY id")?;
        let rows =
            q.query_map([account], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?, r.get::<_, String>(2)?)))?;
        rows.map(|r| {
            let (id, version, body) = r?;
            Ok(CloudAgent { id, version, definition: serde_json::from_str(&body)?, curated: false })
        })
        .collect()
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
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require_account(&tx, account)?;
        let count: u64 =
            tx.query_row("SELECT count(*) FROM cloud_agents WHERE account=?1 AND archived=0", [account], |r| r.get(0))?;
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
        Ok(self.c().execute("UPDATE cloud_agents SET definition=?1,version=version+1 WHERE account=?2 AND id=?3 AND version=?4 AND archived=0", params![serde_json::to_string(definition)?, account, id, version])? == 1)
    }

    pub fn archive_cloud_agent(&self, account: &str, id: &str) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE cloud_agents SET archived=1 WHERE account=?1 AND id=?2 AND archived=0",
            params![account, id],
        )? == 1)
    }

    /// Reserve capacity and insert the run in one write transaction. Retries
    /// return the original run, including after it finishes; they never execute
    /// another prompt or consume another slot. A reused key with different input
    /// is a conflict rather than silently accepting the wrong work.
    pub fn reserve_cloud_run(
        &self,
        account: &str,
        request_key: &str,
        spec: &RunSpec,
        limits: &RunLimits,
        now: u64,
    ) -> Result<CloudRun> {
        if request_key.is_empty() || request_key.len() > 128 {
            bail!("invalid request key");
        }
        if limits.duration_ms == 0 {
            bail!("cloud run duration must be positive");
        }
        let deadline = now.checked_add(limits.duration_ms).ok_or_else(|| anyhow::anyhow!("invalid deadline"))?;
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require_account(&tx, account)?;
        let existing = tx
            .query_row(
                "SELECT id,spec,state,created,deadline,result FROM cloud_runs WHERE account=?1 AND request_key=?2",
                params![account, request_key],
                row,
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing.spec != *spec {
                bail!("request key already used with different input");
            }
            return Ok(existing);
        }
        let daily: u64 = tx.query_row(
            "SELECT count(*) FROM cloud_runs WHERE account=?1 AND created>=?2",
            params![account, now / 86_400_000 * 86_400_000],
            |r| r.get(0),
        )?;
        // Expired executions continue to occupy capacity until the worker has
        // confirmed termination and preserved their results.
        let active: u64 = tx.query_row(
            "SELECT count(*) FROM cloud_runs WHERE account=?1 AND state NOT IN ('completed','failed','cancelled','expired')",
            [account], |r| r.get(0),
        )?;
        if daily >= limits.daily_runs {
            bail!("daily cloud run limit reached");
        }
        if active >= limits.concurrent_runs {
            bail!("concurrent cloud run limit reached");
        }
        let run = CloudRun {
            id: format!("run_{}", hex::encode(arugula_e2e::random::<16>())),
            spec: spec.clone(),
            state: "queued".into(),
            created: now,
            deadline,
            result: None,
        };
        tx.execute(
            "INSERT INTO cloud_runs(id,account,request_key,spec,created,deadline) VALUES(?1,?2,?3,?4,?5,?6)",
            params![run.id, account, request_key, serde_json::to_string(spec)?, now, deadline],
        )?;
        let mut names = spec.agent.secret_names.clone();
        if let Some(name) = &spec.repository_secret {
            names.push(name.clone());
        }
        names.sort();
        names.dedup();
        for name in names {
            let sealed: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT sealed FROM cloud_secrets WHERE account=?1 AND name=?2",
                    params![account, name],
                    |r| r.get(0),
                )
                .optional()?;
            let sealed = sealed.ok_or_else(|| anyhow::anyhow!("required cloud secret is missing"))?;
            tx.execute(
                "INSERT INTO cloud_run_secrets(run,name,sealed) VALUES(?1,?2,?3)",
                params![run.id, name, sealed],
            )?;
        }
        tx.execute(
            "INSERT INTO cloud_jobs(id,kind,body) VALUES(?1,'run',?2)",
            params![run.id, serde_json::to_string(&Execution::default())?],
        )?;
        tx.commit()?;
        Ok(run)
    }

    pub fn cloud_runs(&self, account: &str) -> Result<Vec<CloudRun>> {
        let conn = self.c();
        let mut q = conn.prepare("SELECT id,spec,state,created,deadline,result FROM cloud_runs WHERE account=?1 ORDER BY created DESC LIMIT 100")?;
        Ok(q.query_map([account], row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn cloud_conversation(&self, account: &str, id: &str, now: u64) -> Result<Option<String>> {
        Ok(self.c().query_row("SELECT r.provider_id FROM cloud_runs r JOIN cloud_jobs j ON j.id=r.id WHERE r.account=?1 AND r.id=?2 AND r.state IN ('running','needs_input','recovering') AND j.cancel=0 AND r.deadline>?3", params![account,id,now], |r| r.get::<_,Option<String>>(0)).optional()?.flatten())
    }

    pub fn cancel_cloud_run(&self, account: &str, id: &str) -> Result<bool> {
        Ok(self.c().execute("UPDATE cloud_jobs SET cancel=1 WHERE id IN (SELECT id FROM cloud_runs WHERE account=?1 AND id=?2 AND state NOT IN ('completed','failed','cancelled','expired'))", params![account,id])? == 1)
    }

    pub fn cloud_events(&self, account: &str, id: &str, after: u64) -> Result<Vec<serde_json::Value>> {
        let conn = self.c();
        let mut q = conn.prepare("SELECT e.body FROM cloud_events e JOIN cloud_runs r ON r.id=e.run WHERE r.account=?1 AND r.id=?2 AND e.cursor>?3 ORDER BY e.cursor LIMIT 1000")?;
        let rows = q.query_map(params![account, id, after], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    pub fn cloud_requests(&self, account: &str, run: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.c();
        let mut q = conn.prepare("SELECT q.body FROM cloud_requests q JOIN cloud_runs r ON r.id=q.run WHERE r.account=?1 AND r.id=?2 AND q.pending=1")?;
        let rows = q.query_map(params![account, run], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn resolve_cloud_request(&self, account: &str, run: &str, request: &str) -> Result<()> {
        self.c().execute("UPDATE cloud_requests SET pending=0 WHERE run=?1 AND id=?2 AND EXISTS(SELECT 1 FROM cloud_runs WHERE id=?1 AND account=?3)",params![run,request,account])?;
        Ok(())
    }
    pub fn cloud_notifications(&self, now: u64) -> Result<Vec<(String, String, String)>> {
        let conn = self.c();
        let mut q = conn.prepare("SELECT r.account,n.run,n.request FROM cloud_notifications n JOIN cloud_runs r ON r.id=n.run JOIN cloud_requests q ON q.run=n.run AND q.id=n.request WHERE n.delivered=0 AND n.next_attempt<=?1 AND q.pending=1 ORDER BY r.created LIMIT 20")?;
        Ok(q.query_map([now], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn cloud_notification_result(&self, run: &str, request: &str, delivered: bool, next: u64) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_notifications SET delivered=?1,next_attempt=?2 WHERE run=?3 AND request=?4",
            params![delivered, next, run, request],
        )?;
        Ok(())
    }

    /// Claim a bounded unit of work, not the lifetime of the remote run.
    pub fn claim_cloud_job(&self, now: u64) -> Result<Option<Job>> {
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let found = tx.query_row("SELECT r.id,r.spec,r.state,r.created,r.deadline,r.result,r.account,j.body,j.cancel FROM cloud_runs r JOIN cloud_jobs j ON j.id=r.id WHERE r.state NOT IN ('completed','failed','cancelled','expired') AND j.lease_until<=?1 ORDER BY j.lease_until,r.created LIMIT 1", [now], |r| Ok((row(r)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,bool>(8)?))).optional()?;
        let Some((run, account, body, cancelled)) = found else {
            return Ok(None);
        };
        let lease = hex::encode(arugula_e2e::random::<16>());
        tx.execute("UPDATE cloud_jobs SET lease=?1,lease_until=?2 WHERE id=?3", params![lease, now + 120_000, run.id])?;
        tx.commit()?;
        Ok(Some(Job { run, account, execution: serde_json::from_str(&body)?, cancelled, lease }))
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
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
            tx.execute("UPDATE cloud_requests SET pending=0 WHERE run=?1", [&job.run.id])?;
        }
        if matches!(state, "completed" | "failed" | "cancelled" | "expired") {
            tx.execute("DELETE FROM cloud_run_secrets WHERE run=?1", [&job.run.id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn append_cloud_events(&self, job: &Job, events: &[serde_json::Value]) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owns: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_jobs WHERE id=?1 AND lease=?2)",
            params![job.run.id, job.lease],
            |r| r.get(0),
        )?;
        if !owns {
            bail!("cloud job lease lost");
        }
        for event in events {
            let cursor = event["id"].as_u64().ok_or_else(|| anyhow::anyhow!("invalid cloud event cursor"))?;
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO cloud_events(run,cursor,body) VALUES(?1,?2,?3)",
                params![job.run.id, cursor, serde_json::to_string(event)?],
            )?;
            if inserted == 0 {
                continue;
            }
            if let Some(blocks) = event["blocks"].as_array() {
                for block in blocks {
                    if matches!(block["kind"].as_str(), Some("permission_request" | "elicitation_request"))
                        && let Some(id) = block["request_id"].as_str()
                    {
                        tx.execute(
                            "INSERT OR IGNORE INTO cloud_requests(run,id,body) VALUES(?1,?2,?3)",
                            params![job.run.id, id, serde_json::to_string(block)?],
                        )?;
                        tx.execute(
                            "INSERT OR IGNORE INTO cloud_notifications(run,request) VALUES(?1,?2)",
                            params![job.run.id, id],
                        )?;
                    }
                }
            }
            if event["stage"] == "request" && event["state"] == "done" {
                let data: serde_json::Value = event["data"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| event["data"].clone());
                if let Some(id) = data["request_id"].as_str().or_else(|| data["requestId"].as_str()) {
                    tx.execute("UPDATE cloud_requests SET pending=0 WHERE run=?1 AND id=?2", params![job.run.id, id])?;
                }
            }
            if event["stage"] == "turn" && matches!(event["state"].as_str(), Some("done" | "failed")) {
                tx.execute("UPDATE cloud_requests SET pending=0 WHERE run=?1", [&job.run.id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn cloud_run(&self, account: &str, id: &str) -> Result<Option<CloudRun>> {
        Ok(self
            .c()
            .query_row(
                "SELECT id,spec,state,created,deadline,result FROM cloud_runs WHERE account=?1 AND id=?2",
                params![account, id],
                row,
            )
            .optional()?)
    }
}

impl Db {
    /// Create a Cloud workspace and its job. A retry with the same request
    /// key returns the first one; with other settings it's a conflict.
    pub fn create_cloud_workspace(
        &self,
        account: &str,
        request_key: &str,
        spec: &CloudWorkspaceSpec,
        now: u64,
    ) -> Result<CloudWorkspace> {
        if request_key.is_empty() || request_key.len() > 128 {
            bail!("invalid request key");
        }
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        require_account(&tx, account)?;
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
            "INSERT INTO cloud_workspaces(id,account,request_key,name,saved_spec,created) VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, account, request_key, spec.name, serde_json::to_string(spec)?, now],
        )?;
        tx.execute(
            "INSERT INTO cloud_jobs(id,kind,body) VALUES(?1,'cloud_workspace',?2)",
            params![id, serde_json::to_string(&CloudWorkspaceExecution::default())?],
        )?;
        let created = tx.query_row(
            &format!("SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE id=?1"),
            [&id],
            cloud_workspace_row,
        )?;
        tx.commit()?;
        Ok(created)
    }

    /// The account's Cloud workspaces that aren't deleted yet, newest first.
    pub fn cloud_workspaces(&self, account: &str) -> Result<Vec<CloudWorkspace>> {
        let conn = self.c();
        let mut q = conn.prepare(&format!(
            "SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE account=?1 AND deleted IS NULL ORDER BY created DESC, id"
        ))?;
        Ok(q.query_map([account], cloud_workspace_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn cloud_workspace(&self, account: &str, id: &str) -> Result<Option<CloudWorkspace>> {
        Ok(self
            .c()
            .query_row(
                &format!(
                    "SELECT {CLOUD_WORKSPACE_COLUMNS} FROM cloud_workspaces WHERE account=?1 AND id=?2 AND deleted IS NULL"
                ),
                params![account, id],
                cloud_workspace_row,
            )
            .optional()?)
    }

    /// Ask the worker to delete it. Saying so again is harmless.
    pub fn delete_cloud_workspace(&self, account: &str, id: &str) -> Result<bool> {
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE cloud_workspaces SET status='deleting' WHERE account=?1 AND id=?2 AND deleted IS NULL",
            params![account, id],
        )?;
        tx.execute("UPDATE cloud_jobs SET cancel=1 WHERE id=?1 AND kind='cloud_workspace'", [id])?;
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

    pub fn claim_cloud_workspace_job(&self, now: u64) -> Result<Option<CloudWorkspaceJob>> {
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let columns = CLOUD_WORKSPACE_COLUMNS.split(',').map(|c| format!("w.{c}")).collect::<Vec<_>>().join(",");
        let found = tx
            .query_row(
                &format!(
                    "SELECT {columns},w.account,w.saved_revision,j.body,j.cancel FROM cloud_workspaces w JOIN cloud_jobs j ON j.id=w.id
                     WHERE j.kind='cloud_workspace' AND w.deleted IS NULL AND j.lease_until<=?1 ORDER BY j.lease_until,w.created LIMIT 1"
                ),
                [now],
                |r| {
                    Ok((
                        cloud_workspace_row(r)?,
                        r.get::<_, String>(12)?,
                        r.get::<_, Option<String>>(13)?,
                        r.get::<_, String>(14)?,
                        r.get::<_, bool>(15)?,
                    ))
                },
            )
            .optional()?;
        let Some((cloud_workspace, account, saved_revision, body, deleting)) = found else {
            return Ok(None);
        };
        let lease = hex::encode(arugula_e2e::random::<16>());
        tx.execute(
            "UPDATE cloud_jobs SET lease=?1,lease_until=?2 WHERE id=?3",
            params![lease, now + 120_000, cloud_workspace.id],
        )?;
        tx.commit()?;
        Ok(Some(CloudWorkspaceJob {
            cloud_workspace,
            account,
            execution: serde_json::from_str(&body)?,
            saved_revision,
            deleting,
            lease,
        }))
    }

    /// Checkpoint under the claim, as `checkpoint_cloud_job` does. A delete
    /// asked for meanwhile keeps the status `deleting`. `built_spec` becomes
    /// the saved spec once the revision that spec was saved as is built.
    pub fn checkpoint_cloud_workspace_job(&self, job: &CloudWorkspaceJob, release_at: Option<u64>) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
                 status=CASE WHEN (SELECT cancel FROM cloud_jobs WHERE id=?1)=1 THEN 'deleting' ELSE ?2 END,
                 sandbox_status=?3, error=?4, revision=?5, built_from_revision=?6, stale=?7, changed_fields=?8,
                 environment=?9, vault=?10, provider_id=?11, saved_revision=COALESCE(saved_revision,?12),
                 built_spec=CASE WHEN ?2='ready' AND ?6 IS NOT NULL AND ?6=COALESCE(saved_revision,?12) THEN saved_spec ELSE built_spec END
             WHERE id=?1",
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
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Its Managoat resources are gone: keep the row as a tombstone and end
    /// the job.
    pub fn finish_cloud_workspace_delete(&self, job: &CloudWorkspaceJob, now: u64) -> Result<()> {
        let mut conn = self.c();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if tx.execute("DELETE FROM cloud_jobs WHERE id=?1 AND lease=?2", params![job.cloud_workspace.id, job.lease])?
            != 1
        {
            bail!("cloud job lease lost");
        }
        tx.execute(
            "UPDATE cloud_workspaces SET status='deleted',deleted=?1,environment=NULL,vault=NULL,provider_id=NULL WHERE id=?2",
            params![now, job.cloud_workspace.id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// One sealed secret of the account's, read when the vault is filled.
    pub fn cloud_secret(&self, account: &str, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .c()
            .query_row("SELECT sealed FROM cloud_secrets WHERE account=?1 AND name=?2", params![account, name], |r| {
                r.get(0)
            })
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_db() -> Db {
        let db = Db::memory();
        for id in ["a", "b"] {
            db.account_for("test", id, id, id, 0).unwrap();
        }
        db
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
    fn limits() -> RunLimits {
        RunLimits { daily_runs: 2, concurrent_runs: 1, duration_ms: 60000 }
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
        let kept = db.create_cloud_workspace("b", "keep", &cloud_workspace_spec(), 0).unwrap();
        let w = db.create_cloud_workspace("a", "erase", &cloud_workspace_spec(), 0).unwrap();
        let erase = super::super::Erase { account: "a", disband: &[], retain_for: &[] };
        assert!(db.delete_account(&erase).is_err());
        db.delete_cloud_workspaces("a").unwrap();
        assert!(db.delete_account(&erase).is_err(), "still deleting");
        let job = db.claim_cloud_workspace_job(0).unwrap().unwrap();
        let job = if job.cloud_workspace.id == w.id { job } else { db.claim_cloud_workspace_job(0).unwrap().unwrap() };
        assert!(job.deleting);
        db.finish_cloud_workspace_delete(&job, 1).unwrap();
        db.delete_account(&erase).unwrap();
        let rows: u64 = db.c().query_row("SELECT count(*) FROM cloud_workspaces", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
        let jobs: u64 = db.c().query_row("SELECT count(*) FROM cloud_jobs", [], |r| r.get(0)).unwrap();
        assert_eq!(jobs, 1);
        assert_eq!(db.cloud_workspace("b", &kept.id).unwrap().unwrap().status, "provisioning");
    }

    #[test]
    fn cloud_jobs_keyed_on_run_are_rekeyed_with_their_kind() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cloud_runs (id TEXT PRIMARY KEY);
             CREATE TABLE cloud_jobs (
                 run TEXT PRIMARY KEY REFERENCES cloud_runs(id),
                 body TEXT NOT NULL,
                 cancel INTEGER NOT NULL DEFAULT 0,
                 lease TEXT,
                 lease_until INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO cloud_runs VALUES ('run_1');
             INSERT INTO cloud_jobs(run,body,cancel,lease,lease_until) VALUES ('run_1','{}',1,'held',42);",
        )
        .unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let row: (String, String, String, bool, String, u64) = conn
            .query_row("SELECT id,kind,body,cancel,lease,lease_until FROM cloud_jobs", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .unwrap();
        assert_eq!(row, ("run_1".into(), "run".into(), "{}".into(), true, "held".into(), 42));
        conn.execute("INSERT INTO cloud_jobs(id,kind,body) VALUES('cw_1','cloud_workspace','{}')", []).unwrap();
    }

    #[test]
    fn built_spec_follows_the_revision_that_reached_ready() {
        let db = test_db();
        let w = db.create_cloud_workspace("a", "built", &cloud_workspace_spec(), 0).unwrap();
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
    fn account_deletion_waits_for_cleanup_then_erases_cloud_data() {
        let db = test_db();
        let run = db.reserve_cloud_run("a", "delete", &spec(), &limits(), 0).unwrap();
        db.create_cloud_agent("a", spec().agent).unwrap();
        db.put_cloud_secret("a", "TOKEN", b"sealed").unwrap();
        db.put_cloud_secret("b", "TOKEN", b"other-owner").unwrap();
        let erase = super::super::Erase { account: "a", disband: &[], retain_for: &[] };
        assert!(db.delete_account(&erase).is_err());
        assert!(db.account("a").unwrap().is_some());
        assert_eq!(db.active_cloud_runs("a").unwrap(), 1);
        let job = db.claim_cloud_job(0).unwrap().unwrap();
        db.checkpoint_cloud_job(&job, "cancelled", Some(&serde_json::json!({})), None).unwrap();
        // Populate every child table so this catches incomplete erasure and
        // foreign-key ordering, including already-delivered notifications.
        for sql in [
            "INSERT INTO cloud_run_secrets(run,name,sealed) VALUES(?1,'TOKEN',X'01')",
            "INSERT INTO cloud_requests(run,id,body) VALUES(?1,'approval','{}')",
            "INSERT INTO cloud_notifications(run,request) VALUES(?1,'approval')",
            "INSERT INTO cloud_events(run,cursor,body) VALUES(?1,1,'{}')",
        ] {
            db.c().execute(sql, [&run.id]).unwrap();
        }
        db.delete_account(&erase).unwrap();
        assert!(db.account("a").unwrap().is_none());
        for table in [
            "cloud_runs",
            "cloud_agents",
            "cloud_run_secrets",
            "cloud_jobs",
            "cloud_requests",
            "cloud_notifications",
            "cloud_events",
        ] {
            let count: u64 = db.c().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(count, 0, "{table}");
        }
        assert!(db.cloud_secret_names("a").unwrap().is_empty());
        assert_eq!(db.cloud_secret_names("b").unwrap(), ["TOKEN"]);
        assert!(db.reserve_cloud_run("a", "raced", &spec(), &limits(), 0).is_err());
        assert!(db.create_cloud_agent("a", spec().agent).is_err());
        assert!(db.put_cloud_secret("a", "TOKEN", b"raced").is_err());
    }

    #[test]
    fn cancelled_and_expired_runs_cannot_receive_answers() {
        let db = test_db();
        let run = db.reserve_cloud_run("a", "approval", &spec(), &limits(), 0).unwrap();
        let mut job = db.claim_cloud_job(0).unwrap().unwrap();
        job.execution.conversation = Some("conversation".into());
        db.checkpoint_cloud_job(&job, "needs_input", None, None).unwrap();
        assert_eq!(db.cloud_conversation("a", &run.id, 1).unwrap().as_deref(), Some("conversation"));
        assert!(db.cloud_conversation("b", &run.id, 1).unwrap().is_none());
        assert!(db.cloud_conversation("a", &run.id, run.deadline).unwrap().is_none());
        db.cancel_cloud_run("a", &run.id).unwrap();
        assert!(db.cloud_conversation("a", &run.id, 1).unwrap().is_none());
    }

    #[test]
    fn approval_replay_does_not_reopen_resolved_requests_or_duplicate_notifications() {
        let db = test_db();
        let run = db.reserve_cloud_run("a", "approval", &spec(), &limits(), 0).unwrap();
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
    fn run_secrets_are_encrypted_snapshots_and_missing_secrets_roll_back() {
        let db = test_db();
        let cipher = crate::cloud::secrets::SecretCipher::new(&"01".repeat(32)).unwrap();
        let mut spec = spec();
        spec.repository_secret = Some("GITHUB_TOKEN".into());
        assert!(db.reserve_cloud_run("a", "missing", &spec, &limits(), 0).is_err());
        assert!(db.cloud_runs("a").unwrap().is_empty());
        let original = cipher.seal("a", "GITHUB_TOKEN", "original").unwrap();
        db.put_cloud_secret("a", "GITHUB_TOKEN", &original).unwrap();
        let run = db.reserve_cloud_run("a", "ready", &spec, &limits(), 0).unwrap();
        db.put_cloud_secret("a", "GITHUB_TOKEN", &cipher.seal("a", "GITHUB_TOKEN", "rotated").unwrap()).unwrap();
        db.delete_cloud_secret("a", "GITHUB_TOKEN").unwrap();
        let snapshot = db.cloud_run_secrets("a", &run.id).unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(cipher.open("a", "GITHUB_TOKEN", &snapshot[0].1).unwrap(), "original");
        assert!(db.cloud_run_secrets("b", &run.id).unwrap().is_empty());
        assert!(!serde_json::to_string(&run).unwrap().contains("original"));
    }

    #[test]
    fn stale_worker_cannot_overwrite_reclaimed_job() {
        let db = test_db();
        let run = db.reserve_cloud_run("a", "claim", &spec(), &limits(), 0).unwrap();
        let first = db.claim_cloud_job(0).unwrap().unwrap();
        assert!(db.claim_cloud_job(100).unwrap().is_none());
        let second = db.claim_cloud_job(120_001).unwrap().unwrap();
        assert_eq!(first.run.id, second.run.id);
        assert!(db.checkpoint_cloud_job(&first, "running", None, Some(0)).is_err());
        assert!(db.append_cloud_events(&first, &[serde_json::json!({"id":1})]).is_err());
        assert!(db.checkpoint_cloud_job(&second, "running", None, Some(130_000)).is_ok());
        assert!(!db.cancel_cloud_run("b", &run.id).unwrap());
        assert!(db.cancel_cloud_run("a", &run.id).unwrap());
        assert!(db.claim_cloud_job(130_000).unwrap().unwrap().cancelled);
    }

    #[test]
    fn run_and_agent_survive_database_reopen() {
        let path = std::env::temp_dir().join(format!("arugula-cloud-{}.db", hex::encode(arugula_e2e::random::<16>())));
        let id;
        {
            let db = Db::open(&path).unwrap();
            db.account_for("test", "a", "a", "a", 0).unwrap();
            db.create_cloud_agent("a", spec().agent).unwrap();
            id = db.reserve_cloud_run("a", "request", &spec(), &limits(), 100).unwrap().id;
        }
        {
            let db = Db::open(&path).unwrap();
            assert_eq!(db.cloud_agents("a").unwrap().len(), 1);
            assert_eq!(db.cloud_run("a", &id).unwrap().unwrap().spec, spec());
            assert_eq!(db.reserve_cloud_run("a", "request", &spec(), &limits(), 200).unwrap().id, id);
            assert!(db.reserve_cloud_run("a", "another", &spec(), &limits(), 200).is_err());
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
        let run = db.reserve_cloud_run("a", "launch", &launch, &limits(), 0).unwrap();
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

    #[test]
    fn retries_are_idempotent_and_account_scoped() {
        let db = test_db();
        let first = db.reserve_cloud_run("a", "request", &spec(), &limits(), 100).unwrap();
        let retry = db.reserve_cloud_run("a", "request", &spec(), &limits(), 200).unwrap();
        assert_eq!(first.id, retry.id);
        assert_eq!(first.deadline, retry.deadline);
        assert!(db.cloud_run("b", &first.id).unwrap().is_none());
        assert!(db.cloud_run("a", &first.id).unwrap().is_some());
        let other = db.reserve_cloud_run("b", "request", &spec(), &limits(), 200).unwrap();
        assert_ne!(first.id, other.id);
        let mut changed = spec();
        changed.prompt = "different work".into();
        assert!(db.reserve_cloud_run("a", "request", &changed, &limits(), 200).is_err());
    }

    #[test]
    fn expired_but_unconfirmed_work_still_occupies_capacity() {
        let db = test_db();
        db.reserve_cloud_run("a", "one", &spec(), &limits(), 0).unwrap();
        assert!(db.reserve_cloud_run("a", "two", &spec(), &limits(), 70000).is_err());
    }

    #[test]
    fn daily_limit_survives_completion() {
        let db = test_db();
        for key in ["one", "two"] {
            db.reserve_cloud_run("a", key, &spec(), &limits(), 100).unwrap();
            db.c().execute("UPDATE cloud_runs SET state='completed'", []).unwrap();
        }
        assert!(db.reserve_cloud_run("a", "three", &spec(), &limits(), 200).is_err());
        assert!(db.reserve_cloud_run("a", "four", &spec(), &limits(), 86_400_000).is_ok());
    }

    #[test]
    fn concurrent_submissions_cannot_overbook() {
        let db = std::sync::Arc::new(test_db());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let db = db.clone();
                std::thread::spawn(move || db.reserve_cloud_run("a", &i.to_string(), &spec(), &limits(), 0).is_ok())
            })
            .collect();
        assert_eq!(handles.into_iter().map(|h| usize::from(h.join().unwrap())).sum::<usize>(), 1);
    }
}
