//! What registered clients keep on the platform beyond the broker's calls
//! (M236, "studio is a full client"): what an account lets a client see
//! (`client_consents`), the client's apps by project (`client_apps`), and
//! which of a project's own secrets its owners let a client read, with
//! every read (`client_secret_grants`, `client_secret_events`). Migration
//! `0033` says what each column is; `crate::clients` is who may do what.

use anyhow::Result;
use serde::Serialize;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};
use crate::Deleted;

/// How long a client's secret events are kept.
pub const EVENTS_KEEP_MS: u64 = 90 * 86_400_000;

/// What an account may let a client do beyond calling Claude for it
/// (M231's `inference_consents`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// See the account's projects and its role in each, and keep the
    /// client's apps in the projects it edits.
    Projects,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Projects => "projects",
        }
    }
}

/// A client's app, and the project it's in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientApp {
    pub client: String,
    pub app: String,
    pub project: String,
    pub title: String,
    pub added_by: Option<String>,
    pub added_at: u64,
}

/// One of a project's own secrets a client may send for one of its apps,
/// to one host: asked for by the client, granted by an owner, and how the
/// secret has changed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretGrant {
    pub client: String,
    pub name: String,
    pub app: String,
    pub host: String,
    pub requested_by: Option<String>,
    pub requested_at: u64,
    /// Who let it, and when; none while it's only asked for.
    pub granted_by: Option<String>,
    pub granted_at: Option<u64>,
    pub reads: u64,
    pub last_read_at: Option<u64>,
    /// When the secret's value was last set, and by whom (an agent's token
    /// when an agent did): changed since it was granted shows here.
    pub value_set_at: Option<u64>,
    pub value_set_by: Option<String>,
    pub value_set_by_agent: Option<String>,
}

impl SecretGrant {
    /// Granted, for this host, and not stale: what a read needs.
    pub fn holds(&self, host: &str) -> bool {
        self.granted_at.is_some() && self.host == host && !self.stale()
    }

    /// An agent set the value after the grant: an owner grants it again
    /// before the client gets it.
    pub fn stale(&self) -> bool {
        match (self.granted_at, self.value_set_at) {
            (Some(g), Some(v)) => v > g && self.value_set_by_agent.is_some(),
            _ => false,
        }
    }

    /// The value was set (by anyone) after the grant.
    pub fn changed_since(&self) -> bool {
        matches!((self.granted_at, self.value_set_at), (Some(g), Some(v)) if v > g)
    }
}

/// One thing that happened to a client's reach into a project's secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretEvent {
    pub client: String,
    pub name: String,
    pub kind: String,
    pub app: Option<String>,
    pub host: Option<String>,
    pub by: Option<String>,
    pub at: u64,
}

const APP: &str = "SELECT client, app, project_id, title, added_by, added_at FROM client_apps";

fn app_row(r: &super::sql::Row) -> Result<ClientApp> {
    Ok(ClientApp {
        client: r.get(0)?,
        app: r.get(1)?,
        project: r.get(2)?,
        title: r.get(3)?,
        added_by: r.get(4)?,
        added_at: r.get(5)?,
    })
}

/// One event of a grant's, as each of its writers knows it.
struct Ev<'a> {
    project: &'a str,
    client: &'a str,
    name: &'a str,
    app: Option<&'a str>,
    host: Option<&'a str>,
}

fn event(c: &impl Sql, e: &Ev<'_>, kind: &str, by: Option<&str>, at: u64) -> Result<()> {
    c.execute(
        "INSERT INTO client_secret_events (project_id, client, name, kind, app, host, by_account, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![e.project, e.client, e.name, kind, e.app, e.host, by, at],
    )?;
    Ok(())
}

const GRANT: &str = "SELECT g.client, g.name, g.app, g.host, g.requested_by, g.requested_at, g.granted_by, g.granted_at,
    g.reads, g.last_read_at, s.updated_at, s.updated_by, s.updated_by_agent
    FROM client_secret_grants g LEFT JOIN project_secrets s ON s.project_id = g.project_id AND s.name = 'project/' || g.name";

fn grant_row(r: &super::sql::Row) -> Result<SecretGrant> {
    Ok(SecretGrant {
        client: r.get(0)?,
        name: r.get(1)?,
        app: r.get(2)?,
        host: r.get(3)?,
        requested_by: r.get(4)?,
        requested_at: r.get(5)?,
        granted_by: r.get(6)?,
        granted_at: r.get(7)?,
        reads: r.get(8)?,
        last_read_at: r.get(9)?,
        value_set_at: r.get(10)?,
        value_set_by: r.get(11)?,
        value_set_by_agent: r.get(12)?,
    })
}

impl Db {
    /// Whether `account` lets `client` do what `scope` says.
    pub fn client_consented(&self, account: &str, client: &str, scope: Scope) -> Result<bool> {
        Ok(self
            .c()
            .query_row(
                "SELECT 1 FROM client_consents WHERE account = ?1 AND client = ?2 AND scope = ?3",
                params![account, client, scope.as_str()],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some())
    }

    /// The clients `account` lets do what `scope` says.
    pub fn client_consents(&self, account: &str, scope: Scope) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT client FROM client_consents WHERE account = ?1 AND scope = ?2 ORDER BY client",
            params![account, scope.as_str()],
            |r| r.get(0),
        )
    }

    /// Let `client` do what `scope` says for `account`, or stop it.
    pub fn set_client_consent(&self, account: &str, client: &str, scope: Scope, on: bool, now: u64) -> Result<()> {
        if on {
            self.c().execute(
                "INSERT INTO client_consents (account, client, scope, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (account, client, scope) DO NOTHING",
                params![account, client, scope.as_str(), now],
            )?;
        } else {
            self.c().execute(
                "DELETE FROM client_consents WHERE account = ?1 AND client = ?2 AND scope = ?3",
                params![account, client, scope.as_str()],
            )?;
        }
        Ok(())
    }

    /// Everything `account` let `client` do on the platform, ended at once:
    /// what unlinking the product on the platform page does here. Whether
    /// there was any.
    pub fn end_client_consents(&self, account: &str, client: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = tx
            .execute("DELETE FROM client_consents WHERE account = ?1 AND client = ?2", params![account, client])?
            + tx.execute(
                "DELETE FROM inference_consents WHERE account = ?1 AND client = ?2",
                params![account, client],
            )?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// `client`'s app `app`, wherever it is.
    pub fn client_app(&self, client: &str, app: &str) -> Result<Option<ClientApp>> {
        self.c().query_row(&format!("{APP} WHERE client = ?1 AND app = ?2"), params![client, app], app_row).optional()
    }

    /// `client`'s apps in `projects`, newest first.
    pub fn client_apps_in(&self, client: &str, projects: &[String]) -> Result<Vec<ClientApp>> {
        let mut out = vec![];
        for p in projects {
            out.extend(self.c().query_map(
                &format!("{APP} WHERE client = ?1 AND project_id = ?2"),
                params![client, p],
                app_row,
            )?);
        }
        out.sort_by(|a, b| b.added_at.cmp(&a.added_at).then_with(|| a.app.cmp(&b.app)));
        Ok(out)
    }

    /// Every client's apps in `project`, newest first.
    pub fn project_client_apps(&self, project: &str) -> Result<Vec<ClientApp>> {
        self.c().query_map(
            &format!("{APP} WHERE project_id = ?1 ORDER BY added_at DESC, client, app"),
            params![project],
            app_row,
        )
    }

    /// Keep `client`'s app `app` in `project`, titled `title`, put there by
    /// `by`: a new app, a new title, or a move. The app as it is now.
    pub fn put_client_app(
        &self,
        client: &str,
        app: &str,
        project: &str,
        title: &str,
        by: &str,
        now: u64,
    ) -> Result<ClientApp> {
        self.c().execute(
            "INSERT INTO client_apps (client, app, project_id, title, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (client, app) DO UPDATE SET title = excluded.title,
               added_by = CASE WHEN client_apps.project_id = excluded.project_id THEN client_apps.added_by ELSE excluded.added_by END,
               added_at = CASE WHEN client_apps.project_id = excluded.project_id THEN client_apps.added_at ELSE excluded.added_at END,
               project_id = excluded.project_id",
            params![client, app, project, title, by, now],
        )?;
        self.client_app(client, app)?.ok_or_else(|| anyhow::anyhow!("{client}'s app {app} wasn't kept"))
    }

    /// Forget `client`'s app `app`. Whether it was there.
    pub fn remove_client_app(&self, client: &str, app: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM client_apps WHERE client = ?1 AND app = ?2", params![client, app])? > 0)
    }

    /// The secrets clients asked to send, and those owners let them, in
    /// `project`.
    pub fn client_secret_grants(&self, project: &str) -> Result<Vec<SecretGrant>> {
        self.c().query_map(
            &format!("{GRANT} WHERE g.project_id = ?1 ORDER BY g.name, g.client, g.app"),
            params![project],
            grant_row,
        )
    }

    /// `client`'s grant of `project`'s secret `name` for its app `app`.
    pub fn client_secret_grant(
        &self,
        project: &str,
        client: &str,
        name: &str,
        app: &str,
    ) -> Result<Option<SecretGrant>> {
        self.c()
            .query_row(
                &format!("{GRANT} WHERE g.project_id = ?1 AND g.client = ?2 AND g.name = ?3 AND g.app = ?4"),
                params![project, client, name, app],
                grant_row,
            )
            .optional()
    }

    /// `client` asks to send `project`'s secret `name` for its app `app` to
    /// `host`, for `by` (an owner). The same host again keeps a grant; a new
    /// one asks afresh.
    #[allow(clippy::too_many_arguments)]
    pub fn request_client_secret(
        &self,
        project: &str,
        client: &str,
        name: &str,
        app: &str,
        host: &str,
        by: &str,
        now: u64,
    ) -> Result<SecretGrant> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let same: Option<String> = tx
            .query_row(
                "SELECT host FROM client_secret_grants WHERE project_id = ?1 AND client = ?2 AND name = ?3 AND app = ?4",
                params![project, client, name, app],
                |r| r.get(0),
            )
            .optional()?;
        if same.as_deref() != Some(host) {
            tx.execute(
                "INSERT INTO client_secret_grants (project_id, client, name, app, host, requested_by, requested_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (project_id, client, name, app) DO UPDATE SET host = excluded.host,
                   requested_by = excluded.requested_by, requested_at = excluded.requested_at,
                   granted_by = NULL, granted_at = NULL",
                params![project, client, name, app, host, by, now],
            )?;
            let e = Ev { project, client, name, app: Some(app), host: Some(host) };
            event(&tx, &e, "requested", Some(by), now)?;
        }
        tx.commit()?;
        // The connection goes back before the read takes it again.
        drop(c);
        self.client_secret_grant(project, client, name, app)?
            .ok_or_else(|| anyhow::anyhow!("{client}'s ask for {name} wasn't kept"))
    }

    /// `by` (an owner) lets `client` send `project`'s secret `name` for its
    /// app `app` to the host it asked for. Whether there was an ask.
    pub fn grant_client_secret(
        &self,
        project: &str,
        client: &str,
        name: &str,
        app: &str,
        by: &str,
        now: u64,
    ) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let host: Option<String> = tx
            .query_row(
                "SELECT host FROM client_secret_grants WHERE project_id = ?1 AND client = ?2 AND name = ?3 AND app = ?4",
                params![project, client, name, app],
                |r| r.get(0),
            )
            .optional()?;
        let Some(host) = host else { return Ok(false) };
        tx.execute(
            "UPDATE client_secret_grants SET granted_by = ?5, granted_at = ?6
             WHERE project_id = ?1 AND client = ?2 AND name = ?3 AND app = ?4",
            params![project, client, name, app, by, now],
        )?;
        event(&tx, &Ev { project, client, name, app: Some(app), host: Some(&host) }, "granted", Some(by), now)?;
        tx.commit()?;
        Ok(true)
    }

    /// `by` (an owner) ends `client`'s grant, or ask, of `project`'s secret
    /// `name` for its app `app`. Whether there was one.
    pub fn revoke_client_secret(
        &self,
        project: &str,
        client: &str,
        name: &str,
        app: &str,
        by: &str,
        now: u64,
    ) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = tx.execute(
            "DELETE FROM client_secret_grants WHERE project_id = ?1 AND client = ?2 AND name = ?3 AND app = ?4",
            params![project, client, name, app],
        )?;
        if n > 0 {
            event(&tx, &Ev { project, client, name, app: Some(app), host: None }, "revoked", Some(by), now)?;
        }
        tx.commit()?;
        Ok(n > 0)
    }

    /// Every grant of `project`'s secret `name` ends: the secret is gone.
    pub fn revoke_client_secret_everywhere(&self, project: &str, name: &str, by: &str, now: u64) -> Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let rows: Vec<(String, String)> = tx.query_map(
            "SELECT client, app FROM client_secret_grants WHERE project_id = ?1 AND name = ?2",
            params![project, name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute("DELETE FROM client_secret_grants WHERE project_id = ?1 AND name = ?2", params![project, name])?;
        for (client, app) in &rows {
            event(&tx, &Ev { project, client, name, app: Some(app), host: None }, "revoked", Some(by), now)?;
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// Every grant and ask for `client`'s app `app` ends: it moved to
    /// another project, or went. How many.
    pub fn end_app_grants(&self, client: &str, app: &str, now: u64) -> Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let rows: Vec<(String, String)> = tx.query_map(
            "SELECT project_id, name FROM client_secret_grants WHERE client = ?1 AND app = ?2",
            params![client, app],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute("DELETE FROM client_secret_grants WHERE client = ?1 AND app = ?2", params![client, app])?;
        for (project, name) in &rows {
            event(&tx, &Ev { project, client, name, app: Some(app), host: None }, "ended", None, now)?;
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// `client` read `project`'s own secret `name` for its app `app`, to
    /// send to `host`: one more read on its grant, and an event.
    pub fn note_client_secret_read(
        &self,
        project: &str,
        client: &str,
        name: &str,
        app: &str,
        host: &str,
        now: u64,
    ) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE client_secret_grants SET reads = reads + 1, last_read_at = ?5
             WHERE project_id = ?1 AND client = ?2 AND name = ?3 AND app = ?4",
            params![project, client, name, app, now],
        )?;
        event(&tx, &Ev { project, client, name, app: Some(app), host: Some(host) }, "read", None, now)?;
        tx.commit()?;
        Ok(())
    }

    /// How many times clients read `project`'s secret `name` since `since`.
    pub fn client_secret_reads_since(&self, project: &str, name: &str, since: u64) -> Result<u64> {
        self.c().query_row(
            "SELECT COUNT(*) FROM client_secret_events WHERE project_id = ?1 AND name = ?2 AND kind = 'read' AND at >= ?3",
            params![project, name, since],
            |r| r.get(0),
        )
    }

    /// The newest `limit` events of clients' reach into `project`'s secrets.
    pub fn client_secret_events(&self, project: &str, limit: u32) -> Result<Vec<SecretEvent>> {
        self.c().query_map(
            "SELECT client, name, kind, app, host, by_account, at FROM client_secret_events WHERE project_id = ?1
             ORDER BY at DESC, id DESC LIMIT ?2",
            params![project, i64::from(limit)],
            |r| {
                Ok(SecretEvent {
                    client: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    app: r.get(3)?,
                    host: r.get(4)?,
                    by: r.get(5)?,
                    at: r.get(6)?,
                })
            },
        )
    }

    /// Forget events older than [`EVENTS_KEEP_MS`].
    pub fn tidy_client_secret_events(&self, now: u64) -> Result<()> {
        self.c()
            .execute("DELETE FROM client_secret_events WHERE at < ?1", params![now.saturating_sub(EVENTS_KEEP_MS)])?;
        Ok(())
    }
}

/// What deleting projects takes from here, inside the same transaction:
/// clients' apps in them, and their secret grants and events. `which` is a
/// `SELECT id FROM projects ...` with `?1` for `arg`, as
/// [`crate::keys::erase_projects`] takes.
pub(crate) fn erase_projects(c: &impl Sql, which: &str, arg: &str) -> Result<()> {
    for t in ["client_apps", "client_secret_grants", "client_secret_events"] {
        c.execute(&format!("DELETE FROM {t} WHERE project_id IN ({which})"), params![arg])?;
    }
    Ok(())
}

/// An account is gone: what it let clients do goes, and nothing here names
/// it any more.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("DELETE FROM client_consents WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE client_apps SET added_by = NULL WHERE added_by = ?1", params![d.account])?;
    c.execute("UPDATE client_secret_grants SET granted_by = NULL WHERE granted_by = ?1", params![d.account])?;
    c.execute("UPDATE client_secret_grants SET requested_by = NULL WHERE requested_by = ?1", params![d.account])?;
    c.execute("UPDATE client_secret_events SET by_account = NULL WHERE by_account = ?1", params![d.account])?;
    Ok(())
}
