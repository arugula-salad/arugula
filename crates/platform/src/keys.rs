//! Per-project envelope keys for secrets (M209, platform step 7).
//!
//! Each project has a random data key, kept in `project_keys` wrapped by
//! the platform's master key (`ARUGULA_CLOUD_SECRET_KEY`,
//! [`SecretCipher`]). A project's secrets are sealed with its data key,
//! bound to the project and the secret's name ([`seal_value`]), so one
//! project's key never opens another's secrets, and a row moved to
//! another project or name doesn't open either.
//!
//! - **What's sealed this way:** cloud secrets (`cloud_secrets`, the
//!   owner's personal project's), hosted workspaces' sealed secrets
//!   (`hosted_workspace_secrets`, the workspace's project's), projects' own
//!   inference keys (`project_inference_keys`, M185), and secrets
//!   with no table of their own, such as an environment's (M217): those are
//!   kept in `project_secrets` by name ([`seal`], [`open`]).
//! - **Which key sealed a row:** its `key_version`. `0` ([`MASTER`]) is the
//!   master key itself, as everything was sealed before M209, bound to an
//!   owner and a name; a version is that version of the project's data key.
//!   Both open ([`open_cloud`], [`open_hosted`], [`open_inference`]).
//! - **Re-sealing what the master key sealed** ([`reseal`]) runs at start
//!   and hourly ([`start`]), one row per transaction: each row is opened
//!   with the master key and written back sealed with its project's key
//!   only if it's still as it was read. So a crash halfway leaves rows of
//!   both kinds, each of which opens, and running it again (or on two
//!   machines at once) finishes the job and does nothing after. It isn't a
//!   migration: migrations are SQL that every opener runs, including the
//!   window's read-only tools, and the master key is kept apart from the
//!   database on purpose.
//! - **Rotating the master key** ([`rotate`], `arugula-platform rotate-key`)
//!   re-wraps the data keys and touches no secret. docs/control-ops.md has
//!   the runbook.
//!
//! A build from before M209 can't open what's been re-sealed: a rollback
//! past it leaves cloud secrets closed until it's rolled forward again.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use chacha20poly1305::{KeyInit as _, XChaCha20Poly1305};

use crate::{
    Platform,
    cloud::secrets::{SecretCipher, open_with, seal_with},
    db::{
        Db,
        sql::{Optional as _, Sql, params},
    },
};

/// The `key_version` of a secret the master key sealed itself (before
/// M209).
pub const MASTER: i64 = 0;

/// A sealed value and which key sealed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    /// [`MASTER`], or the version of the project's data key.
    pub key_version: i64,
    pub bytes: Vec<u8>,
}

/// The name a cloud secret is sealed under, beside its project.
pub fn cloud_name(account: &str, name: &str) -> String {
    format!("cloud_secrets/{account}/{name}")
}

/// The name a hosted workspace's secret is sealed under, beside its
/// project.
pub fn hosted_name(workspace: &str, name: &str) -> String {
    format!("hosted_workspace_secrets/{workspace}/{name}")
}

/// The name a project's own inference key for `provider` (M185) is sealed
/// under, beside its project.
pub fn inference_name(provider: &str) -> String {
    format!("project_inference_keys/{provider}")
}

/// The name an environment's secret is kept under in `project_secrets`
/// (M217): `environment` is its id (`env_...`).
pub fn environment_name(environment: &str, name: &str) -> String {
    format!("environment/{environment}/{name}")
}

/// A deploy environment's secret that code names: a driver's credential
/// in the environment drivers know by name (`arugula-release`). Every one
/// is listed in `crates/control/secrets.toml`, and a test fails when code
/// names one that isn't ([`environment_secret`] with literals only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentSecret {
    pub environment: &'static str,
    pub name: &'static str,
}

/// The secret `name` of the deploy environment `environment`. Call it with
/// string literals: `secrets.toml`'s test reads the calls.
pub const fn environment_secret(environment: &'static str, name: &'static str) -> EnvironmentSecret {
    EnvironmentSecret { environment, name }
}

impl EnvironmentSecret {
    /// Where it's kept for the environment row `id` (`env_...`).
    pub fn stored_as(&self, id: &str) -> String {
        environment_name(id, self.name)
    }
}

fn aad(project: &str, name: &str) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&("arugula-project-secret-v1", project, name))?)
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

/// Version `version` of `project`'s data key, opened.
fn data_key(c: &impl Sql, master: &SecretCipher, project: &str, version: i64) -> Result<[u8; 32]> {
    let (master_id, wrapped): (String, Vec<u8>) = c
        .query_row(
            "SELECT master_id, wrapped FROM project_keys WHERE project_id = ?1 AND version = ?2",
            params![project, version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .with_context(|| format!("project {project} has no key version {version}"))?;
    master.unwrap(&master_id, project, version, &wrapped)
}

/// The data key `project` seals with now, and its version: its latest,
/// made and wrapped with the current master key when it has none. Two
/// callers making it at once both get the one that was kept.
fn current_key(c: &impl Sql, master: &SecretCipher, project: &str) -> Result<(i64, [u8; 32])> {
    let latest = || -> Result<Option<i64>> {
        c.query_row(
            "SELECT version FROM project_keys WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
            params![project],
            |r| r.get(0),
        )
        .optional()
    };
    let version = match latest()? {
        Some(v) => v,
        None => {
            let key = arugula_e2e::random::<32>();
            c.execute(
                "INSERT INTO project_keys (project_id, version, wrapped, master_id, created_at)
                 VALUES (?1, 1, ?2, ?3, ?4) ON CONFLICT (project_id, version) DO NOTHING",
                params![project, master.wrap(project, 1, &key)?, master.id(), arugula_e2e::now_ms()],
            )?;
            latest()?.context("a project's key that was just made")?
        }
    };
    Ok((version, data_key(c, master, project, version)?))
}

/// `value`, sealed with `project`'s data key and bound to the project and
/// `name` (which says what it is: [`cloud_name`], [`hosted_name`],
/// [`environment_name`]). For a table that keeps its own sealed values;
/// [`seal`] keeps it in `project_secrets`.
pub fn seal_value(c: &impl Sql, master: &SecretCipher, project: &str, name: &str, value: &str) -> Result<Sealed> {
    let (key_version, key) = current_key(c, master, project)?;
    let bytes = seal_with(&cipher(&key), &aad(project, name)?, value.as_bytes())?;
    Ok(Sealed { key_version, bytes })
}

/// What [`seal_value`] sealed for `project` under `name`. One the master
/// key sealed ([`MASTER`]) is its table's to open ([`open_cloud`],
/// [`open_hosted`]).
pub fn open_value(c: &impl Sql, master: &SecretCipher, project: &str, name: &str, sealed: &Sealed) -> Result<String> {
    if sealed.key_version == MASTER {
        bail!("sealed with the master key, not a project's");
    }
    let key = data_key(c, master, project, sealed.key_version)?;
    let value = open_with(&cipher(&key), &aad(project, name)?, &sealed.bytes)
        .with_context(|| format!("could not open a secret of project {project}"))?;
    String::from_utf8(value).context("a secret that isn't UTF-8")
}

/// Keep `value` as `project`'s secret `name` (in `project_secrets`),
/// sealed with the project's key.
pub fn seal(db: &Db, master: &SecretCipher, project: &str, name: &str, value: &str) -> Result<()> {
    let c = db.c();
    let s = seal_value(&c, master, project, name, value)?;
    c.execute(
        "INSERT INTO project_secrets (project_id, name, key_version, sealed, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (project_id, name) DO UPDATE SET key_version = excluded.key_version, sealed = excluded.sealed,
             updated_at = excluded.updated_at",
        params![project, name, s.key_version, s.bytes, arugula_e2e::now_ms()],
    )?;
    Ok(())
}

/// `project`'s secret `name`, opened; `None` when there's none.
pub fn open(db: &Db, master: &SecretCipher, project: &str, name: &str) -> Result<Option<String>> {
    let c = db.c();
    let found = c
        .query_row(
            "SELECT key_version, sealed FROM project_secrets WHERE project_id = ?1 AND name = ?2",
            params![project, name],
            |r| Ok(Sealed { key_version: r.get(0)?, bytes: r.get(1)? }),
        )
        .optional()?;
    found.map(|s| open_value(&c, master, project, name, &s)).transpose()
}

/// Forget `project`'s secret `name`: whether there was one.
pub fn remove(db: &Db, project: &str, name: &str) -> Result<bool> {
    Ok(db.c().execute("DELETE FROM project_secrets WHERE project_id = ?1 AND name = ?2", params![project, name])? == 1)
}

/// A cloud secret of `account`'s, opened. `project` is its row's (the
/// account's personal project; `None` only on a SQLite row from before
/// M129, which the master key sealed).
pub fn open_cloud(
    c: &impl Sql,
    master: &SecretCipher,
    account: &str,
    name: &str,
    project: Option<&str>,
    sealed: &Sealed,
) -> Result<String> {
    match (sealed.key_version, project) {
        (MASTER, _) => master.open(account, name, &sealed.bytes),
        (_, Some(project)) => open_value(c, master, project, &cloud_name(account, name), sealed),
        (_, None) => bail!("a cloud secret sealed with a project's key has no project"),
    }
}

/// A hosted workspace's sealed secret, opened: `project` is the
/// workspace's.
pub fn open_hosted(
    c: &impl Sql,
    master: &SecretCipher,
    project: &str,
    workspace: &str,
    name: &str,
    sealed: &Sealed,
) -> Result<String> {
    match sealed.key_version {
        MASTER => master.open(workspace, name, &sealed.bytes),
        _ => open_value(c, master, project, &hosted_name(workspace, name), sealed),
    }
}

/// A project's own inference key for `provider` (M185), opened. One the
/// master key sealed is bound to the project and `inference:<provider>`.
pub fn open_inference(
    c: &impl Sql,
    master: &SecretCipher,
    project: &str,
    provider: &str,
    sealed: &Sealed,
) -> Result<String> {
    match sealed.key_version {
        MASTER => master.open(project, &format!("inference:{provider}"), &sealed.bytes),
        _ => open_value(c, master, project, &inference_name(provider), sealed),
    }
}

/// A table whose rows are sealed secrets: how [`reseal`] and [`check`]
/// find its rows, which `(owner, name)` the master key bound a row to,
/// and the name its project's key binds it to.
struct Store {
    table: &'static str,
    /// Every row: its two key columns, its project, `key_version` and
    /// `sealed`, the master key's first.
    rows: &'static str,
    /// Re-seal one row (`?1` sealed, `?2` key_version), by its two key
    /// columns (`?3`, `?4`), only if it's still what was read (`?5`).
    update: &'static str,
    legacy: fn(&Row) -> (String, String),
    name: fn(&Row) -> String,
}

/// One row of a [`Store`].
struct Row {
    a: String,
    b: String,
    project: Option<String>,
    sealed: Sealed,
}

const STORES: [Store; 3] = [
    Store {
        table: "cloud_secrets",
        rows: "SELECT account, name, project_id, key_version, sealed FROM cloud_secrets
               ORDER BY key_version, account, name",
        update: "UPDATE cloud_secrets SET sealed = ?1, key_version = ?2
                 WHERE account = ?3 AND name = ?4 AND key_version = 0 AND sealed = ?5",
        legacy: |r| (r.a.clone(), r.b.clone()),
        name: |r| cloud_name(&r.a, &r.b),
    },
    Store {
        table: "hosted_workspace_secrets",
        rows: "SELECT s.workspace_id, s.name, w.project_id, s.key_version, s.sealed
               FROM hosted_workspace_secrets s LEFT JOIN hosted_workspaces w ON w.id = s.workspace_id
               WHERE s.sealed IS NOT NULL ORDER BY s.key_version, s.workspace_id, s.name",
        update: "UPDATE hosted_workspace_secrets SET sealed = ?1, key_version = ?2
                 WHERE workspace_id = ?3 AND name = ?4 AND key_version = 0 AND sealed = ?5",
        legacy: |r| (r.a.clone(), r.b.clone()),
        name: |r| hosted_name(&r.a, &r.b),
    },
    Store {
        table: "project_inference_keys",
        rows: "SELECT project_id, provider, project_id, key_version, sealed FROM project_inference_keys
               ORDER BY key_version, project_id, provider",
        update: "UPDATE project_inference_keys SET sealed = ?1, key_version = ?2
                 WHERE project_id = ?3 AND provider = ?4 AND key_version = 0 AND sealed = ?5",
        legacy: |r| (r.a.clone(), format!("inference:{}", r.b)),
        name: |r| inference_name(&r.b),
    },
];

impl Store {
    fn read(&self, c: &impl Sql, only_master: bool) -> Result<Vec<Row>> {
        let rows = c.query_map(self.rows, params![], |r| {
            Ok(Row {
                a: r.get(0)?,
                b: r.get(1)?,
                project: r.get(2)?,
                sealed: Sealed { key_version: r.get(3)?, bytes: r.get(4)? },
            })
        })?;
        Ok(rows.into_iter().filter(|r| !only_master || r.sealed.key_version == MASTER).collect())
    }

    fn open(&self, c: &impl Sql, master: &SecretCipher, r: &Row) -> Result<String> {
        if r.sealed.key_version == MASTER {
            let (owner, name) = (self.legacy)(r);
            return master.open(&owner, &name, &r.sealed.bytes);
        }
        let project = r.project.as_deref().context("no project")?;
        open_value(c, master, project, &(self.name)(r), &r.sealed)
    }
}

/// What a [`reseal`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resealed {
    /// Rows now sealed with their project's key.
    pub resealed: u64,
    /// Rows still sealed with the master key, as `table owner name`: they
    /// don't open with it, have no project, or changed under it (and are
    /// done next time).
    pub left: Vec<String>,
}

/// Re-seal up to `max` rows the master key sealed with their project's
/// key, each in a transaction of its own (see the module's docs). Safe to
/// run again, from where it stopped, and on several machines at once.
pub fn reseal(db: &Db, master: &SecretCipher, max: usize) -> Result<Resealed> {
    let mut out = Resealed::default();
    for store in &STORES {
        let rows = store.read(&db.c(), true)?;
        for r in rows {
            let what = format!("{} {} {}", store.table, r.a, r.b);
            if out.resealed as usize >= max {
                out.left.push(what);
                continue;
            }
            match reseal_row(db, master, store, &r) {
                Ok(true) => out.resealed += 1,
                Ok(false) => out.left.push(what),
                Err(error) => {
                    tracing::warn!(table = store.table, %error, "a secret the master key sealed can't be re-sealed");
                    out.left.push(what);
                }
            }
        }
    }
    Ok(out)
}

/// One row: whether it was re-sealed.
fn reseal_row(db: &Db, master: &SecretCipher, store: &Store, r: &Row) -> Result<bool> {
    let Some(project) = r.project.as_deref() else { bail!("it has no project") };
    let mut c = db.c();
    let tx = c.transaction()?;
    let value = store.open(&tx, master, r)?;
    let s = seal_value(&tx, master, project, &(store.name)(r), &value)?;
    let changed = tx.execute(store.update, params![s.bytes, s.key_version, r.a, r.b, r.sealed.bytes])?;
    tx.commit()?;
    Ok(changed == 1)
}

/// What a [`check`] found: how many secrets open, how many of those the
/// master key sealed itself, and the `(owner, name)` of each that doesn't
/// open (a project key's is `(project, "key version N")`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checked {
    pub opened: u64,
    pub by_master: u64,
    pub failed: Vec<(String, String)>,
}

/// Every stored secret and project key, tried with `master`. Reads only;
/// no value leaves it.
pub fn check(c: &impl Sql, master: &SecretCipher) -> Result<Checked> {
    let mut out = Checked::default();
    for store in &STORES {
        for r in store.read(c, false)? {
            match store.open(c, master, &r) {
                Ok(_) => {
                    out.opened += 1;
                    out.by_master += u64::from(r.sealed.key_version == MASTER);
                }
                Err(_) => out.failed.push((r.a, r.b)),
            }
        }
    }
    let rows: Vec<(String, String, i64, Vec<u8>)> = c.query_map(
        "SELECT project_id, name, key_version, sealed FROM project_secrets ORDER BY project_id, name",
        params![],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    for (project, name, key_version, bytes) in rows {
        match open_value(c, master, &project, &name, &Sealed { key_version, bytes }) {
            Ok(_) => out.opened += 1,
            Err(_) => out.failed.push((project, name)),
        }
    }
    // A project key nothing is sealed with yet still has to open.
    let keys: Vec<(String, i64)> =
        c.query_map("SELECT project_id, version FROM project_keys ORDER BY project_id, version", params![], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    for (project, version) in keys {
        if data_key(c, master, &project, version).is_err() {
            out.failed.push((project, format!("key version {version}")));
        }
    }
    Ok(out)
}

/// What a [`rotate`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rotated {
    /// Rows the master key had sealed itself, re-sealed with their
    /// project's key first.
    pub resealed: u64,
    /// Project keys re-wrapped with the new master key.
    pub rewrapped: u64,
    /// Project keys it already wrapped (a rotation run again).
    pub already: u64,
}

/// Rotate the master key: `keys` is the new key with the old one as its
/// previous ([`SecretCipher::with_previous`]). Every project key the old
/// one wrapped is re-wrapped with the new one, each on its own, and no
/// secret is touched. Rows the old key sealed itself are re-sealed with
/// their project's key first, as they couldn't be opened after; if any is
/// left, nothing is re-wrapped. Safe to run again: what the new key
/// already wraps is left alone. Then every secret is opened with the new
/// key alone, and any that doesn't fails it.
pub fn rotate(db: &Db, keys: &SecretCipher) -> Result<Rotated> {
    let old = keys.previous_id().context("rotating needs the old master key as well as the new one")?.to_owned();
    let resealed = reseal(db, keys, usize::MAX)?;
    if !resealed.left.is_empty() {
        bail!(
            "these are still sealed with the master key itself, so no project key was re-wrapped:\n  {}",
            resealed.left.join("\n  ")
        );
    }
    let mut out = Rotated { resealed: resealed.resealed, ..Default::default() };
    let rows: Vec<(String, i64, String, Vec<u8>)> = db.c().query_map(
        "SELECT project_id, version, master_id, wrapped FROM project_keys ORDER BY project_id, version",
        params![],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let mut others = Vec::new();
    for (project, version, master_id, wrapped) in rows {
        if master_id == keys.id() {
            out.already += 1;
            continue;
        }
        if master_id != old {
            others.push(format!("{project} version {version} (master key {master_id})"));
            continue;
        }
        let key = keys.unwrap(&master_id, &project, version, &wrapped)?;
        let changed = db.c().execute(
            "UPDATE project_keys SET wrapped = ?1, master_id = ?2, rotated_at = ?3
             WHERE project_id = ?4 AND version = ?5 AND master_id = ?6",
            params![keys.wrap(&project, version, &key)?, keys.id(), arugula_e2e::now_ms(), project, version, master_id],
        )?;
        out.rewrapped += changed as u64;
    }
    if !others.is_empty() {
        bail!("these project keys are wrapped by neither key given:\n  {}", others.join("\n  "));
    }
    let new_only = keys.without_previous();
    let checked = check(&db.c(), &new_only)?;
    if !checked.failed.is_empty() {
        let failed: Vec<String> = checked.failed.iter().map(|(a, b)| format!("{a} {b}")).collect();
        bail!("after re-wrapping, these don't open with the new key:\n  {}", failed.join("\n  "));
    }
    Ok(out)
}

/// What deleting projects takes from here: their keys and the secrets
/// kept under them, inside the same transaction. `which` is a `SELECT id
/// FROM projects ...` with `?1` for `arg`.
pub(crate) fn erase_projects(c: &impl Sql, which: &str, arg: &str) -> Result<()> {
    for t in ["project_keys", "project_secrets"] {
        c.execute(&format!("DELETE FROM {t} WHERE project_id IN ({which})"), params![arg])?;
    }
    Ok(())
}

/// Re-seal what the master key sealed, at start and then hourly, unless
/// the platform is paused for a window. Rows a machine on an older build
/// writes during a rolling deploy are done the next hour.
pub fn start(app: &Arc<Platform>) {
    let Some(master) = app.secret_key.clone() else { return };
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            if !app.is_paused() {
                let (app2, master) = (app.clone(), master.clone());
                match tokio::task::spawn_blocking(move || reseal(&app2.db, &master, usize::MAX)).await {
                    Ok(Ok(r)) if r.resealed > 0 || !r.left.is_empty() => {
                        tracing::info!(
                            resealed = r.resealed,
                            left = r.left.len(),
                            "secrets re-sealed with their project's key"
                        );
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => tracing::warn!(%error, "re-sealing secrets"),
                    Err(error) => tracing::warn!(%error, "re-sealing secrets"),
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
