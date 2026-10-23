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

/// A project's owner letting a client read one of its own secrets, and the
/// client's reads of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretGrant {
    pub client: String,
    pub name: String,
    pub granted_by: Option<String>,
    pub granted_at: u64,
    pub reads: u64,
    pub last_read_at: Option<u64>,
}

/// One thing that happened to a client's reach into a project's secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretEvent {
    pub client: String,
    pub name: String,
    pub kind: String,
    pub app: Option<String>,
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

fn event(c: &impl Sql, project: &str, client: &str, name: &str, kind: &str, app: Option<&str>, by: Option<&str>, at: u64) -> Result<()> {
    c.execute(
        "INSERT INTO client_secret_events (project_id, client, name, kind, app, by_account, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![project, client, name, kind, app, by, at],
    )?;
    Ok(())
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
        let n = tx.execute("DELETE FROM client_consents WHERE account = ?1 AND client = ?2", params![account, client])?
            + tx.execute("DELETE FROM inference_consents WHERE account = ?1 AND client = ?2", params![account, client])?;
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
    pub fn put_client_app(&self, client: &str, app: &str, project: &str, title: &str, by: &str, now: u64) -> Result<ClientApp> {
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

    /// Which of `project`'s own secrets its owners let clients read.
    pub fn client_secret_grants(&self, project: &str) -> Result<Vec<SecretGrant>> {
        self.c().query_map(
            "SELECT client, name, granted_by, granted_at, reads, last_read_at FROM client_secret_grants
             WHERE project_id = ?1 ORDER BY client, name",
            params![project],
            |r| {
                Ok(SecretGrant {
                    client: r.get(0)?,
                    name: r.get(1)?,
                    granted_by: r.get(2)?,
                    granted_at: r.get(3)?,
                    reads: r.get(4)?,
                    last_read_at: r.get(5)?,
                })
            },
        )
    }

    /// Whether `client` may read `project`'s own secret `name`.
    pub fn client_secret_granted(&self, project: &str, client: &str, name: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row(
                "SELECT 1 FROM client_secret_grants WHERE project_id = ?1 AND client = ?2 AND name = ?3",
                params![project, client, name],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some())
    }

    /// `by` (an owner) lets `client` read `project`'s own secret `name`.
    /// Whether it's new.
    pub fn grant_client_secret(&self, project: &str, client: &str, name: &str, by: &str, now: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = tx.execute(
            "INSERT INTO client_secret_grants (project_id, client, name, granted_by, granted_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (project_id, client, name) DO NOTHING",
            params![project, client, name, by, now],
        )?;
        if n > 0 {
            event(&tx, project, client, name, "granted", None, Some(by), now)?;
        }
        tx.commit()?;
        Ok(n > 0)
    }

    /// `by` (an owner) stops `client` reading `project`'s own secret
    /// `name`. Whether it could.
    pub fn revoke_client_secret(&self, project: &str, client: &str, name: &str, by: &str, now: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = tx.execute(
            "DELETE FROM client_secret_grants WHERE project_id = ?1 AND client = ?2 AND name = ?3",
            params![project, client, name],
        )?;
        if n > 0 {
            event(&tx, project, client, name, "revoked", None, Some(by), now)?;
        }
        tx.commit()?;
        Ok(n > 0)
    }

    /// Every grant of `project`'s secret `name`, ended: the secret is gone.
    pub fn revoke_client_secret_everywhere(&self, project: &str, name: &str, by: &str, now: u64) -> Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let clients: Vec<String> = tx.query_map(
            "SELECT client FROM client_secret_grants WHERE project_id = ?1 AND name = ?2",
            params![project, name],
            |r| r.get(0),
        )?;
        for client in &clients {
            tx.execute(
                "DELETE FROM client_secret_grants WHERE project_id = ?1 AND client = ?2 AND name = ?3",
                params![project, client, name],
            )?;
            event(&tx, project, client, name, "revoked", None, Some(by), now)?;
        }
        tx.commit()?;
        Ok(clients.len())
    }

    /// `client` read `project`'s own secret `name` for its app `app`: one
    /// more read on its grant, and an event.
    pub fn note_client_secret_read(&self, project: &str, client: &str, name: &str, app: &str, now: u64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE client_secret_grants SET reads = reads + 1, last_read_at = ?4
             WHERE project_id = ?1 AND client = ?2 AND name = ?3",
            params![project, client, name, now],
        )?;
        event(&tx, project, client, name, "read", Some(app), None, now)?;
        tx.commit()?;
        Ok(())
    }

    /// The newest `limit` events of clients' reach into `project`'s secrets.
    pub fn client_secret_events(&self, project: &str, limit: u32) -> Result<Vec<SecretEvent>> {
        self.c().query_map(
            "SELECT client, name, kind, app, by_account, at FROM client_secret_events WHERE project_id = ?1
             ORDER BY at DESC, id DESC LIMIT ?2",
            params![project, i64::from(limit)],
            |r| {
                Ok(SecretEvent {
                    client: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    app: r.get(3)?,
                    by: r.get(4)?,
                    at: r.get(5)?,
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
    c.execute("UPDATE client_secret_events SET by_account = NULL WHERE by_account = ?1", params![d.account])?;
    Ok(())
}
