//! Per-project envelope keys for secrets (M209, platform step 7).
//!
//! Each project has a random data key, kept in `project_keys` wrapped by
//! the platform's master key (`ARUGULA_CLOUD_SECRET_KEY`,
//! [`SecretCipher`]). A project's secrets are sealed with its data key,
//! bound to the project and the secret's name ([`seal_value`]), so one
//! project's key never opens another's secrets, and a row moved to
//! another project or name doesn't open either.
//!
//! - **What's sealed this way:** hosted workspaces' sealed secrets
//!   (`hosted_workspace_secrets`, the workspace's project's), projects' own
//!   inference keys (`project_inference_keys`, M185), and secrets
//!   with no table of their own, such as an environment's (M217): those are
//!   kept in `project_secrets` by name ([`seal`], [`open`]).
//! - **Which key sealed a row:** its `key_version`. `0` ([`MASTER`]) is the
//!   master key itself, as everything was sealed before M209, bound to an
//!   owner and a name; a version is that version of the project's data key.
//!   Both open ([`open_hosted`], [`open_inference`]). A
//!   build from before M209 that writes a re-sealed row (a rolling deploy,
//!   a rollback) seals it with the master key but leaves its version, so a
//!   row the project's key doesn't open is tried with the master key too,
//!   and re-sealed.
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
//!   re-wraps the data keys and touches no secret. The new key is first
//!   only opened with (`ARUGULA_CLOUD_SECRET_KEY_NEXT`), then wraps
//!   (`ARUGULA_CLOUD_SECRET_KEY`, the old one `_PREVIOUS`), so no machine
//!   meets a key it can't open. docs/control-ops.md has the runbook.
//! - **A project that's gone** gets no new key, and its rows are neither
//!   re-sealed nor counted by [`check`].
//!
//! A build from before M209 can't open what's been re-sealed: a rollback
//! past it leaves those secrets closed until it's rolled forward again.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use chacha20poly1305::{KeyInit as _, XChaCha20Poly1305};

use crate::{
    Platform,
    db::{
        Db,
        sql::{Optional as _, Sql, params},
    },
    secrets::{SecretCipher, open_with, seal_with},
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
/// in an environment drivers know by name (`arugula-release`). Made only
/// by [`environment_secret!`](crate::environment_secret), which doesn't
/// compile for a secret `crates/control/secrets.toml` doesn't list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentSecret {
    environment: &'static str,
    name: &'static str,
}

/// The list of deploy environments and their secrets, read at compile
/// time by [`environment_secret!`](crate::environment_secret).
pub const SECRETS_TOML: &str = include_str!("../../control/secrets.toml");

impl EnvironmentSecret {
    /// The environment it's in.
    pub fn environment(&self) -> &'static str {
        self.environment
    }

    /// Its name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Where it's kept for the environment row `id` (`env_...`).
    pub fn stored_as(&self, id: &str) -> String {
        environment_name(id, self.name)
    }

    /// For [`environment_secret!`](crate::environment_secret) only, in a
    /// `const` block: it fails to compile when `secrets.toml` doesn't list
    /// the secret.
    #[doc(hidden)]
    pub const fn listed(environment: &'static str, name: &'static str) -> Self {
        if !listed_in(SECRETS_TOML.as_bytes(), environment.as_bytes(), name.as_bytes()) {
            panic!("an environment secret that crates/control/secrets.toml doesn't list");
        }
        Self { environment, name }
    }
}

/// The deploy environment `env`'s secret `NAME`, as an
/// [`EnvironmentSecret`]: it compiles only when `crates/control/secrets.toml`
/// lists it.
///
/// ```compile_fail
/// let _ = arugula_platform::environment_secret!("arugula-release", "NOT_LISTED_ANYWHERE");
/// ```
#[macro_export]
macro_rules! environment_secret {
    ($environment:literal, $name:literal) => {
        const { $crate::keys::EnvironmentSecret::listed($environment, $name) }
    };
}

/// Whether `crates/control/secrets.toml` lists `name` under the deploy
/// environment `environment`, as [`environment_secret!`](crate::environment_secret)
/// reads it.
pub fn is_listed(environment: &str, name: &str) -> bool {
    listed_in(SECRETS_TOML.as_bytes(), environment.as_bytes(), name.as_bytes())
}

/// Whether `toml` (`secrets.toml`'s shape: each `[[environment]]` has a
/// `name = "..."` line and a `secrets = [...]` line) lists `name` under
/// the environment `environment`.
pub(crate) const fn listed_in(toml: &[u8], environment: &[u8], name: &[u8]) -> bool {
    let mut at = 0;
    while let Some(start) = find(toml, b"[[environment]]", at) {
        let end = match find(toml, b"[[environment]]", start + 1) {
            Some(e) => e,
            None => toml.len(),
        };
        if let Some(n) = find(toml, b"\nname = \"", start)
            && n < end
            && starts(toml, n + 9, environment)
            && n + 9 + environment.len() < toml.len()
            && toml[n + 9 + environment.len()] == b'"'
            && let Some(list) = find(toml, b"\nsecrets = [", start)
            && list < end
        {
            let close = match find(toml, b"]", list) {
                Some(c) => c,
                None => return false,
            };
            let mut i = list;
            while let Some(q) = find(toml, b"\"", i) {
                if q >= close {
                    break;
                }
                let after = q + 1 + name.len();
                if starts(toml, q + 1, name) && after < toml.len() && toml[after] == b'"' {
                    return true;
                }
                // Past this string's closing quote.
                i = match find(toml, b"\"", q + 1) {
                    Some(c) => c + 1,
                    None => return false,
                };
            }
        }
        at = end;
    }
    false
}

const fn starts(hay: &[u8], at: usize, needle: &[u8]) -> bool {
    if at + needle.len() > hay.len() {
        return false;
    }
    let mut i = 0;
    while i < needle.len() {
        if hay[at + i] != needle[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + needle.len() <= hay.len() {
        if starts(hay, i, needle) {
            return Some(i);
        }
        i += 1;
    }
    None
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

/// Whether `project` is one the platform has (not deleted).
fn project_exists(c: &impl Sql, project: &str) -> Result<bool> {
    c.query_row("SELECT EXISTS (SELECT 1 FROM projects WHERE id = ?1)", params![project], |r| r.get(0))
}

/// The data key `project` seals with now, and its version: its latest,
/// made and wrapped with the current master key when it has none. Two
/// callers making it at once both get the one that was kept. One the
/// previous master key wraps is re-wrapped with the current one first, so
/// what's sealed after a rotation never needs the old key. A project
/// that's gone gets none.
fn current_key(c: &impl Sql, master: &SecretCipher, project: &str) -> Result<(i64, [u8; 32])> {
    if !project_exists(c, project)? {
        bail!("project {project} doesn't exist");
    }
    let latest = || -> Result<Option<(i64, String)>> {
        c.query_row(
            "SELECT version, master_id FROM project_keys WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
            params![project],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    };
    let (version, master_id) = match latest()? {
        Some(found) => found,
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
    let key = data_key(c, master, project, version)?;
    if Some(master_id.as_str()) == master.previous_id() {
        rewrap(c, master, project, version, &master_id, &key)?;
    }
    Ok((version, key))
}

/// Re-wrap version `version` of `project`'s key with the current master
/// key, if `from` still wraps it: how many rows changed.
fn rewrap(c: &impl Sql, master: &SecretCipher, project: &str, version: i64, from: &str, key: &[u8; 32]) -> Result<u64> {
    let changed = c.execute(
        "UPDATE project_keys SET wrapped = ?1, master_id = ?2, rotated_at = ?3
         WHERE project_id = ?4 AND version = ?5 AND master_id = ?6",
        params![master.wrap(project, version, key)?, master.id(), arugula_e2e::now_ms(), project, version, from],
    )?;
    Ok(changed as u64)
}

/// How many project keys the master key with id `master_id` wraps.
pub fn wrapped_by(c: &impl Sql, master_id: &str) -> Result<u64> {
    c.query_row("SELECT COUNT(*) FROM project_keys WHERE master_id = ?1", params![master_id], |r| r.get(0))
}

/// `value`, sealed with `project`'s data key and bound to the project and
/// `name` (which says what it is: [`hosted_name`], [`inference_name`],
/// [`environment_name`]). For a table that keeps its own sealed values;
/// [`seal`] keeps it in `project_secrets`.
pub fn seal_value(c: &impl Sql, master: &SecretCipher, project: &str, name: &str, value: &str) -> Result<Sealed> {
    let (key_version, key) = current_key(c, master, project)?;
    let bytes = seal_with(&cipher(&key), &aad(project, name)?, value.as_bytes())?;
    Ok(Sealed { key_version, bytes })
}

/// What [`seal_value`] sealed for `project` under `name`. One the master
/// key sealed ([`MASTER`]) is its table's to open ([`open_hosted`],
/// [`open_inference`]).
pub fn open_value(c: &impl Sql, master: &SecretCipher, project: &str, name: &str, sealed: &Sealed) -> Result<String> {
    if sealed.key_version == MASTER {
        bail!("sealed with the master key, not a project's");
    }
    let key = data_key(c, master, project, sealed.key_version)?;
    let value = open_with(&cipher(&key), &aad(project, name)?, &sealed.bytes)
        .with_context(|| format!("could not open a secret of project {project}"))?;
    String::from_utf8(value).context("a secret that isn't UTF-8")
}

/// The name a `project_secrets` row is sealed under: the table, then its
/// name, so no other table's sealed value opens as one.
fn project_secret_name(name: &str) -> String {
    format!("project_secrets/{name}")
}

/// Keep `value` as `project`'s secret `name` (in `project_secrets`),
/// sealed with the project's key.
pub fn seal(db: &Db, master: &SecretCipher, project: &str, name: &str, value: &str) -> Result<()> {
    let c = db.c();
    let s = seal_value(&c, master, project, &project_secret_name(name), value)?;
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
    found.map(|s| open_value(&c, master, project, &project_secret_name(name), &s)).transpose()
}

/// Forget `project`'s secret `name`: whether there was one.
pub fn remove(db: &Db, project: &str, name: &str) -> Result<bool> {
    Ok(db.c().execute("DELETE FROM project_secrets WHERE project_id = ?1 AND name = ?2", params![project, name])? == 1)
}

/// A value sealed with `project`'s key under `name`, else with the master
/// key itself bound to `legacy`: its `key_version` says which, but a build
/// from before M209 seals with the master key and leaves the version it
/// found, so a version the project's key doesn't open is tried with the
/// master key too. With whether it was the master key.
fn open_either(
    c: &impl Sql,
    master: &SecretCipher,
    project: Option<&str>,
    name: &str,
    legacy: (&str, &str),
    sealed: &Sealed,
) -> Result<(String, bool)> {
    let by_master = || master.open(legacy.0, legacy.1, &sealed.bytes).map(|v| (v, true));
    match (sealed.key_version, project) {
        (MASTER, _) => by_master(),
        (_, Some(project)) => match open_value(c, master, project, name, sealed) {
            Ok(v) => Ok((v, false)),
            Err(e) => by_master().map_err(|_| e),
        },
        (_, None) => by_master().map_err(|_| anyhow::anyhow!("a secret sealed with a project's key has no project")),
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
    Ok(open_either(c, master, Some(project), &hosted_name(workspace, name), (workspace, name), sealed)?.0)
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
    let legacy = format!("inference:{provider}");
    Ok(open_either(c, master, Some(project), &inference_name(provider), (project, &legacy), sealed)?.0)
}

/// A table whose rows are sealed secrets: how [`reseal`] and [`check`]
/// find its rows, which `(owner, name)` the master key bound a row to,
/// and the name its project's key binds it to.
struct Store {
    table: &'static str,
    /// Every row: its two key columns, its project, `key_version`,
    /// `sealed`, and whether its project is still there.
    rows: &'static str,
    /// Re-seal one row (`?1` sealed, `?2` key_version), by its two key
    /// columns (`?3`, `?4`), only if it's still what was read (`?5`
    /// sealed, `?6` key_version).
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
    live: bool,
}

const STORES: [Store; 2] = [
    Store {
        table: "hosted_workspace_secrets",
        rows: "SELECT s.workspace_id, s.name, w.project_id, s.key_version, s.sealed,
                   EXISTS (SELECT 1 FROM projects p WHERE p.id = w.project_id)
               FROM hosted_workspace_secrets s LEFT JOIN hosted_workspaces w ON w.id = s.workspace_id
               WHERE s.sealed IS NOT NULL ORDER BY s.key_version, s.workspace_id, s.name",
        update: "UPDATE hosted_workspace_secrets SET sealed = ?1, key_version = ?2
                 WHERE workspace_id = ?3 AND name = ?4 AND sealed = ?5 AND key_version = ?6",
        legacy: |r| (r.a.clone(), r.b.clone()),
        name: |r| hosted_name(&r.a, &r.b),
    },
    Store {
        table: "project_inference_keys",
        rows: "SELECT project_id, provider, project_id, key_version, sealed,
                   EXISTS (SELECT 1 FROM projects p WHERE p.id = k.project_id)
               FROM project_inference_keys k ORDER BY key_version, project_id, provider",
        update: "UPDATE project_inference_keys SET sealed = ?1, key_version = ?2
                 WHERE project_id = ?3 AND provider = ?4 AND sealed = ?5 AND key_version = ?6",
        legacy: |r| (r.a.clone(), format!("inference:{}", r.b)),
        name: |r| inference_name(&r.b),
    },
];

impl Store {
    fn read(&self, c: &impl Sql) -> Result<Vec<Row>> {
        c.query_map(self.rows, params![], |r| {
            Ok(Row {
                a: r.get(0)?,
                b: r.get(1)?,
                project: r.get(2)?,
                sealed: Sealed { key_version: r.get(3)?, bytes: r.get(4)? },
                live: r.get(5)?,
            })
        })
    }

    /// A row opened, and whether it was the master key ([`open_either`]).
    fn open(&self, c: &impl Sql, master: &SecretCipher, r: &Row) -> Result<(String, bool)> {
        let (owner, name) = (self.legacy)(r);
        open_either(c, master, r.project.as_deref(), &(self.name)(r), (&owner, &name), &r.sealed)
    }
}

/// What a [`reseal`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resealed {
    /// Rows now sealed with their project's key.
    pub resealed: u64,
    /// Rows still sealed with the master key, or that don't open, as
    /// `table owner name`: they don't open with it, their project is gone,
    /// they changed under it (and are done next time), or `max` was
    /// reached.
    pub left: Vec<String>,
}

/// Re-seal up to `max` rows the master key sealed with their project's
/// key, each in a transaction of its own (see the module's docs): those
/// marked [`MASTER`], and those a build from before M209 wrote over. Safe
/// to run again, from where it stopped, and on several machines at once.
pub fn reseal(db: &Db, master: &SecretCipher, max: usize) -> Result<Resealed> {
    let mut out = Resealed::default();
    for store in &STORES {
        let rows = store.read(&db.c())?;
        for r in rows {
            let what = format!("{} {} {}", store.table, r.a, r.b);
            match store.open(&db.c(), master, &r) {
                Ok((_, false)) => continue,
                Ok((_, true)) if r.live => {}
                Ok((_, true)) | Err(_) => {
                    out.left.push(what);
                    continue;
                }
            }
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
    let (value, by_master) = store.open(&tx, master, r)?;
    if !by_master {
        return Ok(false);
    }
    let s = seal_value(&tx, master, project, &(store.name)(r), &value)?;
    let changed =
        tx.execute(store.update, params![s.bytes, s.key_version, r.a, r.b, r.sealed.bytes, r.sealed.key_version])?;
    tx.commit()?;
    Ok(changed == 1)
}

/// What a [`check`] found: how many secrets open, how many of those the
/// master key sealed itself, how many belong to projects that are gone
/// (not tried), and the `(owner, name)` of each that doesn't open (a
/// project key's is `(project, "key version N")`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checked {
    pub opened: u64,
    pub by_master: u64,
    pub orphaned: u64,
    pub failed: Vec<(String, String)>,
    /// How many project keys a rotation's old key still wraps, when it
    /// was given ([`check_with`]).
    pub previous_wraps: Option<u64>,
}

/// [`check`] with the hex key `key`, and how many project keys
/// `previous` still wraps: none means it can go.
pub fn check_with(c: &impl Sql, key: &str, previous: Option<&str>) -> Result<Checked> {
    let mut out = check(c, &SecretCipher::new(key)?)?;
    if let Some(previous) = previous.filter(|p| !p.trim().is_empty()) {
        out.previous_wraps = Some(wrapped_by(c, SecretCipher::new(previous)?.id())?);
    }
    Ok(out)
}

/// Every stored secret and project key of a project that's still there,
/// tried with `master`. Reads only; no value leaves it.
pub fn check(c: &impl Sql, master: &SecretCipher) -> Result<Checked> {
    let mut out = Checked::default();
    for store in &STORES {
        for r in store.read(c)? {
            if !r.live {
                out.orphaned += 1;
                continue;
            }
            match store.open(c, master, &r) {
                Ok((_, by_master)) => {
                    out.opened += 1;
                    out.by_master += u64::from(by_master);
                }
                Err(_) => out.failed.push((r.a, r.b)),
            }
        }
    }
    let rows: Vec<(String, String, i64, Vec<u8>, bool)> = c.query_map(
        "SELECT project_id, name, key_version, sealed, EXISTS (SELECT 1 FROM projects p WHERE p.id = s.project_id)
         FROM project_secrets s ORDER BY project_id, name",
        params![],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    for (project, name, key_version, bytes, live) in rows {
        if !live {
            out.orphaned += 1;
            continue;
        }
        match open_value(c, master, &project, &project_secret_name(&name), &Sealed { key_version, bytes }) {
            Ok(_) => out.opened += 1,
            Err(_) => out.failed.push((project, name)),
        }
    }
    // A project key nothing is sealed with yet still has to open.
    let keys: Vec<(String, i64)> = c.query_map(
        "SELECT project_id, version FROM project_keys k
         WHERE EXISTS (SELECT 1 FROM projects p WHERE p.id = k.project_id) ORDER BY project_id, version",
        params![],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
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
    /// What it couldn't do, each on its own line: a row still sealed with
    /// the old key itself, a project key it couldn't re-wrap, and what
    /// doesn't open with the new key alone after. Empty when it's done.
    pub failed: Vec<String>,
}

/// Rotate the master key: `keys` is the new key with the old one as its
/// previous ([`SecretCipher::with_previous`]). Every project key the old
/// one wraps is re-wrapped with the new one, each on its own, and no
/// secret is touched. Rows the old key sealed itself are re-sealed with
/// their project's key first. What fails is said, row by row, and the rest
/// is still done ([`Rotated::failed`]). Safe to run again: what the new key
/// already wraps is left alone. Then every secret of a project that's
/// still there is opened with the new key alone.
pub fn rotate(db: &Db, keys: &SecretCipher) -> Result<Rotated> {
    let old = keys.previous_id().context("rotating needs the old master key as well as the new one")?.to_owned();
    let resealed = reseal(db, keys, usize::MAX)?;
    let mut out = Rotated { resealed: resealed.resealed, ..Default::default() };
    out.failed.extend(resealed.left.iter().map(|w| format!("still sealed with the master key itself: {w}")));
    let rows: Vec<(String, i64, String, Vec<u8>)> = db.c().query_map(
        "SELECT project_id, version, master_id, wrapped FROM project_keys ORDER BY project_id, version",
        params![],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    for (project, version, master_id, wrapped) in rows {
        if master_id == keys.id() {
            out.already += 1;
            continue;
        }
        if master_id != old {
            out.failed
                .push(format!("{project} version {version}: wrapped by master key {master_id}, neither key given"));
            continue;
        }
        let done = keys.unwrap(&master_id, &project, version, &wrapped).and_then(|key| {
            let c = db.c();
            rewrap(&c, keys, &project, version, &master_id, &key)
        });
        match done {
            Ok(n) => out.rewrapped += n,
            Err(e) => out.failed.push(format!("{project} version {version}: {e:#}")),
        }
    }
    let checked = check(&db.c(), &keys.without_previous())?;
    out.failed.extend(checked.failed.iter().map(|(a, b)| format!("doesn't open with the new key alone: {a} {b}")));
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
/// the platform is paused for a window: rows still marked [`MASTER`], and
/// rows a machine on a build from before M209 wrote over (a rolling
/// deploy, a rollback), which keep the version they had and open with the
/// master key until then. While `ARUGULA_CLOUD_SECRET_KEY_PREVIOUS` is
/// set and wraps no project key, each pass says to unset it.
pub fn start(app: &Arc<Platform>) {
    let Some(master) = app.secret_key.clone() else { return };
    tracing::info!(
        master = master.id(),
        previous = master.previous_id(),
        next = master.next_id(),
        "the master key, by id"
    );
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            if !app.is_paused() {
                let (app2, master) = (app.clone(), master.clone());
                match tokio::task::spawn_blocking(move || pass(&app2.db, &master)).await {
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

/// One of [`start`]'s passes.
fn pass(db: &Db, master: &SecretCipher) -> Result<Resealed> {
    let r = reseal(db, master, usize::MAX)?;
    if let Some(previous) = master.previous_id()
        && wrapped_by(&db.c(), previous)? == 0
    {
        tracing::warn!(
            previous,
            "ARUGULA_CLOUD_SECRET_KEY_PREVIOUS is set but wraps no project key: unset it, or the old key still opens \
             what it ever wrapped (docs/control-ops.md)"
        );
    }
    Ok(r)
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
