//! Absorbing redone (M235, platform step 8): what the import carried over
//! (`redone_imports`), the redone tokens that became agent grant tokens
//! (`agent_tokens.imported_from`), and the projection's log of each write
//! of a project's secret into Infisical (`secret_projections`). The rules
//! are [`crate::redone`]'s and [`crate::infisical`]'s.
//!
//! - **An import record is never changed or deleted by the import:** a
//!   second run finds it and changes nothing, and a token the person
//!   revoked since is never carried over again.
//! - **A carried-over token keeps redone's hash:** redone kept each `rdn_`
//!   token's SHA-256 (hex), which is how the platform keeps its own
//!   ([`super::agent_tokens::hash`]), so the token works on the platform's
//!   MCP endpoint without anyone seeing it again.
//! - **The projection's log holds names, never values.**

use serde::Serialize;

use super::{
    Db,
    agent_tokens::{AgentToken, NewToken},
    sql::{Optional as _, Row, Sql, params},
};
use crate::port::Deleted;

/// What an imported token's `imported_from` says.
pub const FROM_REDONE: &str = "redone";

/// One thing the import carried over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Imported {
    /// `tenant`, `token` or `app`.
    pub kind: String,
    /// Its id in redone: a tenant's id, a token's (`tok_…`), an app's name.
    pub redone_id: String,
    pub tenant: String,
    pub project: String,
    /// A token's owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// What it became: an agent token (`agt_…`), an environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_id: Option<String>,
    /// An app's Infisical folder the import vouches is the project's: one
    /// redone had applied a deploy of. `None` for any other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    pub at: u64,
}

/// A write (or, dry, a would-be write) of a project's secret into
/// Infisical, as its log keeps it: never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Projection {
    pub id: String,
    pub project: String,
    pub app: String,
    pub name: String,
    /// `dry_run` or `write`.
    pub mode: String,
    /// `written`, `would_write`, `refused` or `failed`.
    pub outcome: String,
    pub detail: String,
    pub by_account: Option<String>,
    pub by_agent: Option<String>,
    pub at: u64,
}

const IMPORT_COLUMNS: &str = "kind, redone_id, tenant, project_id, account_id, platform_id, folder, at";

fn imported(r: &Row) -> anyhow::Result<Imported> {
    Ok(Imported {
        kind: r.get(0)?,
        redone_id: r.get(1)?,
        tenant: r.get(2)?,
        project: r.get(3)?,
        account: r.get(4)?,
        platform_id: r.get(5)?,
        folder: r.get(6)?,
        at: r.get(7)?,
    })
}

const PROJECTION_COLUMNS: &str = "id, project_id, app, name, mode, outcome, detail, by_account, by_agent, at";

fn projection(r: &Row) -> anyhow::Result<Projection> {
    Ok(Projection {
        id: r.get(0)?,
        project: r.get(1)?,
        app: r.get(2)?,
        name: r.get(3)?,
        mode: r.get(4)?,
        outcome: r.get(5)?,
        detail: r.get(6)?,
        by_account: r.get(7)?,
        by_agent: r.get(8)?,
        at: r.get(9)?,
    })
}

/// A new projection's id.
pub fn new_projection() -> String {
    format!("spj_{}", hex::encode(arugula_e2e::random::<16>()))
}

impl Db {
    /// What the import carried over of redone's `kind` `redone_id`, if it did.
    pub fn redone_imported(&self, kind: &str, redone_id: &str) -> anyhow::Result<Option<Imported>> {
        self.c()
            .query_row(
                &format!("SELECT {IMPORT_COLUMNS} FROM redone_imports WHERE kind = ?1 AND redone_id = ?2"),
                params![kind, redone_id],
                imported,
            )
            .optional()
    }

    /// Everything the import carried over into `project`, oldest first.
    pub fn redone_imports_in(&self, project: &str) -> anyhow::Result<Vec<Imported>> {
        self.c().query_map(
            &format!("SELECT {IMPORT_COLUMNS} FROM redone_imports WHERE project_id = ?1 ORDER BY at, kind, redone_id"),
            params![project],
            imported,
        )
    }

    /// Record that a tenant became `project` (or an app became what
    /// `platform_id` names). Again changes nothing.
    pub fn record_redone_import(&self, i: &Imported) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO redone_imports (kind, redone_id, tenant, project_id, account_id, platform_id, folder, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT (kind, redone_id) DO NOTHING",
            params![i.kind, i.redone_id, i.tenant, i.project, i.account, i.platform_id, i.folder, i.at],
        )?;
        Ok(())
    }

    /// Carry a redone token over: an agent grant token with redone's hash
    /// (`t.token_hash`), owned by `t.account`, `imported_from` redone, with
    /// a live grant of each `(power, project)`, and its import record, in
    /// one transaction. `None` when a token with that hash is here already
    /// (it was carried over before, or it's somehow one of the platform's).
    pub fn import_redone_token(
        &self,
        redone_id: &str,
        tenant: &str,
        t: &NewToken,
        grants: &[(&str, &str)],
        now: u64,
    ) -> anyhow::Result<Option<AgentToken>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let there: Option<String> = tx
            .query_row("SELECT id FROM agent_tokens WHERE token_hash = ?1", params![t.token_hash], |r| r.get(0))
            .optional()?;
        if there.is_some() {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO agent_tokens (id, account_id, name, token_hash, created_at, minted_from, imported_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![t.id, t.account, t.name, t.token_hash, now, t.minted_from, FROM_REDONE],
        )?;
        let mut project = String::new();
        for (power, p) in grants {
            tx.execute(
                "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
                 VALUES (?1, 'agent', ?2, ?3, 'project', ?4, ?5, ?6)",
                params![format!("grt_{}", hex::encode(arugula_e2e::random::<16>())), t.id, power, p, t.account, now],
            )?;
            project = (*p).to_owned();
        }
        tx.execute(
            "INSERT INTO redone_imports (kind, redone_id, tenant, project_id, account_id, platform_id, at)
             VALUES ('token', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![redone_id, tenant, project, t.account, t.id, now],
        )?;
        tx.commit()?;
        // One connection at a time: this one goes before the next is asked.
        drop(c);
        self.agent_token(&t.id)
    }

    /// Log a projection (or, dry, a would-be one).
    pub fn note_projection(&self, p: &Projection) -> anyhow::Result<()> {
        self.c().execute(
            &format!(
                "INSERT INTO secret_projections ({PROJECTION_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ),
            params![p.id, p.project, p.app, p.name, p.mode, p.outcome, p.detail, p.by_account, p.by_agent, p.at],
        )?;
        Ok(())
    }

    /// `project`'s projections, newest first, at most `limit`.
    pub fn projections(&self, project: &str, limit: u32) -> anyhow::Result<Vec<Projection>> {
        self.c().query_map(
            &format!(
                "SELECT {PROJECTION_COLUMNS} FROM secret_projections WHERE project_id = ?1 ORDER BY at DESC, id LIMIT ?2"
            ),
            params![project, limit],
            projection,
        )
    }

    /// Whether `project` may write Infisical's folder `/<app>/` on its own
    /// word: it wrote there before, or the import carried the app over to
    /// it (redone's writes made that folder). Any other folder that's
    /// there already is someone else's (a cluster app's).
    pub fn projects_folder(&self, project: &str, app: &str) -> anyhow::Result<bool> {
        let c = self.c();
        let wrote = c
            .query_row(
                "SELECT 1 FROM secret_projections WHERE project_id = ?1 AND app = ?2 AND outcome = 'written' LIMIT 1",
                params![project, app],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some();
        let imported = c
            .query_row(
                "SELECT 1 FROM redone_imports WHERE kind = 'app' AND folder = ?1 AND project_id = ?2",
                params![app, project],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some();
        Ok(wrote || imported)
    }

    /// Whether another project than `project` wrote Infisical's folder
    /// `/<app>/`, or had the app carried over to it.
    pub fn folder_elsewhere(&self, project: &str, app: &str) -> anyhow::Result<bool> {
        let c = self.c();
        let wrote = c
            .query_row(
                "SELECT 1 FROM secret_projections WHERE project_id != ?1 AND app = ?2 AND outcome = 'written' LIMIT 1",
                params![project, app],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some();
        let imported = c
            .query_row(
                "SELECT 1 FROM redone_imports WHERE kind = 'app' AND folder = ?1 AND project_id != ?2",
                params![app, project],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some();
        Ok(wrote || imported)
    }

    /// The environments of the apps the import carried over with the
    /// tenant agent token `token` came from (M235): all a carried token's
    /// agent may plan to or set secrets of.
    pub fn redone_token_environments(&self, token: &str) -> anyhow::Result<Vec<String>> {
        self.c().query_map(
            "SELECT a.platform_id FROM redone_imports a JOIN redone_imports t
               ON t.kind = 'token' AND t.platform_id = ?1 AND a.kind = 'app' AND a.tenant = t.tenant
               AND a.project_id = t.project_id
             WHERE a.platform_id IS NOT NULL ORDER BY a.platform_id",
            params![token],
            |r| r.get(0),
        )
    }

    /// The projects whose planter ship or promote environment holds the
    /// app's name `app` (M233's claims: its `app` setting), sorted.
    pub fn app_claimants(&self, app: &str) -> anyhow::Result<Vec<String>> {
        use arugula_e2e::deploy::planter as p;
        let mut out = vec![];
        for driver in [p::SHIP, p::PROMOTE] {
            for e in self.environments_driven_by(driver)? {
                if e.settings.get(p::APP).map(String::as_str) == Some(app) {
                    out.push(e.project);
                }
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// The tenants the import made into `project`.
    pub fn redone_tenants_in(&self, project: &str) -> anyhow::Result<Vec<String>> {
        self.c().query_map(
            "SELECT redone_id FROM redone_imports WHERE kind = 'tenant' AND project_id = ?1 ORDER BY redone_id",
            params![project],
            |r| r.get(0),
        )
    }

    /// The stored names of `project`'s secrets that start with `prefix`
    /// (an app's, `app/<app>/`), sorted: never a value.
    pub fn project_secret_names(&self, project: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
        self.c().query_map(
            "SELECT name FROM project_secrets WHERE project_id = ?1 AND substr(name, 1, length(?2)) = ?2 ORDER BY name",
            params![project, prefix],
            |r| r.get(0),
        )
    }

    /// The account's personal project, if it has one yet (never made here).
    pub fn redone_personal_project(&self, account: &str) -> anyhow::Result<Option<String>> {
        self.c()
            .query_row("SELECT id FROM projects WHERE account_id = ?1 AND is_default = TRUE", params![account], |r| {
                r.get(0)
            })
            .optional()
    }
}

/// Deleting an account: the import's rows naming it lose its id but stay
/// (a tombstone: a token carried over before is never carried over again,
/// even once its account and its agent token are gone); its name comes off
/// the projections it set, and the projections of projects that are gone
/// go. Run in the `account_deleted` transaction, after
/// [`super::projects::erase_account`].
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> anyhow::Result<()> {
    c.execute("UPDATE redone_imports SET account_id = NULL WHERE account_id = ?1", params![d.account])?;
    c.execute(
        "UPDATE secret_projections SET by_account = NULL, by_agent = NULL WHERE by_account = ?1",
        params![d.account],
    )?;
    c.execute("DELETE FROM secret_projections WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    Ok(())
}
