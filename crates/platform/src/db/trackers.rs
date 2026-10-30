//! Tracker connections (Arugula #356, `0071_tracker_connections`): a Plane
//! project connected to a project, the counterpart of `linked_repos`
//! ([`super::forge`]), and the work items mirrored for it.
//!
//! A connection's `mirror_id` is negative and is `forge_items.repo_id` of
//! its rows, so no GitHub repo's id is one and no query that joins
//! `linked_repos` on `repo_id` finds them. Writes run in
//! [`Db::tracker_mirror`], which checks in the same transaction that the
//! connection is live (and holds the backfill's lease), so a disconnect
//! stops ingest at the next write. Never the key: it's sealed in
//! `project_secrets` under [`key_name`] and [`webhook_name`].

use anyhow::Result;

use super::{
    Db,
    forge::{PURGE_GRACE_MS, purge_rows},
    migrate::Dialect,
    mirror::POLL_LEASE_MS,
    sql::{Optional as _, Row, Sql, Tx, params},
};
use crate::trackers::Item;

/// What a connection's API key is sealed under in `project_secrets`.
pub fn key_name(connection: &str) -> String {
    format!("tracker/plane/{connection}/key")
}

/// And its optional webhook secret.
pub fn webhook_name(connection: &str) -> String {
    format!("tracker/plane/{connection}/webhook")
}

/// A connected Plane project, or disconnected and waiting for the purge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub workspace_slug: String,
    pub source_project_id: String,
    pub identifier: String,
    pub name: String,
    pub mirror_id: i64,
    pub agent_writes: String,
    pub linked_by: Option<String>,
    pub created_at: u64,
    pub unlinked_at: Option<u64>,
    pub purge_after: Option<u64>,
    pub backfilled_at: Option<u64>,
    /// A webhook secret is sealed for it.
    pub has_webhook_secret: bool,
}

const COLUMNS: &str = "c.id, c.project_id, c.provider, c.workspace_slug, c.source_project_id, c.identifier, c.name, \
     c.mirror_id, c.agent_writes, c.linked_by, c.created_at, c.unlinked_at, c.purge_after, c.backfilled_at, \
     EXISTS (SELECT 1 FROM project_secrets s WHERE s.project_id = c.project_id \
         AND s.name = 'tracker/plane/' || c.id || '/webhook')";

fn row(r: &Row) -> Result<Connection> {
    Ok(Connection {
        id: r.get(0)?,
        project_id: r.get(1)?,
        provider: r.get(2)?,
        workspace_slug: r.get(3)?,
        source_project_id: r.get(4)?,
        identifier: r.get(5)?,
        name: r.get(6)?,
        mirror_id: r.get(7)?,
        agent_writes: r.get(8)?,
        linked_by: r.get(9)?,
        created_at: r.get(10)?,
        unlinked_at: r.get(11)?,
        purge_after: r.get(12)?,
        backfilled_at: r.get(13)?,
        has_webhook_secret: r.get(14)?,
    })
}

/// A connection to make.
#[derive(Debug, Clone)]
pub struct NewConnection {
    pub id: String,
    pub project_id: String,
    pub workspace_slug: String,
    pub source_project_id: String,
    pub identifier: String,
    pub name: String,
    pub linked_by: String,
}

/// A backfill's claim on a connection.
#[derive(Debug, Clone)]
pub struct Backfill {
    pub id: String,
    pub project_id: String,
    pub workspace_slug: String,
    pub source_project_id: String,
    pub mirror_id: i64,
    /// Where the last turn stopped, when it ran out of pages.
    pub cursor: Option<String>,
    pub failures: u64,
    pub lease: String,
}

/// How a backfill turn ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    Done,
    /// Ran out of pages: carry on from here.
    Cursor(String),
}

/// What a claim's `SELECT` ends with: on Postgres it passes over a row
/// another machine's claim holds.
fn skip_locked(c: &impl Sql) -> &'static str {
    match c.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
    }
}

impl Db {
    /// A project's connections, and those waiting for the purge.
    pub fn tracker_connections(&self, project: &str) -> Result<Vec<Connection>> {
        self.c().query_map(
            &format!("SELECT {COLUMNS} FROM tracker_connections c WHERE c.project_id = ?1 ORDER BY c.created_at, c.id"),
            params![project],
            row,
        )
    }

    pub fn tracker_connection(&self, project: &str, id: &str) -> Result<Option<Connection>> {
        self.c()
            .query_row(
                &format!("SELECT {COLUMNS} FROM tracker_connections c WHERE c.project_id = ?1 AND c.id = ?2"),
                params![project, id],
                row,
            )
            .optional()
    }

    /// How many items the platform holds for a connection.
    pub fn tracker_item_count(&self, mirror_id: i64) -> Result<u64> {
        self.c().query_row("SELECT count(*) FROM forge_items WHERE repo_id = ?1", params![mirror_id], |r| r.get(0))
    }

    /// Keep a connection: `None` when that Plane project is connected already
    /// (to this project or another).
    /// It gets the next negative `mirror_id`: one lower than the lowest.
    pub fn connect_tracker(&self, n: &NewConnection, now: u64) -> Result<Option<Connection>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.lock("tracker_connections")?;
        let live = |tx: &Tx<'_, '_>| -> Result<bool> {
            Ok(tx
                .query_row(
                    "SELECT 1 FROM tracker_connections WHERE provider = 'plane' AND workspace_slug = ?1
                     AND source_project_id = ?2 AND unlinked_at IS NULL",
                    params![n.workspace_slug, n.source_project_id],
                    |r| r.get::<i64>(0),
                )
                .optional()?
                .is_some())
        };
        if live(&tx)? {
            return Ok(None);
        }
        let mirror_id: i64 =
            tx.query_row("SELECT COALESCE(MIN(mirror_id), 0) - 1 FROM tracker_connections", params![], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO tracker_connections
                 (id, project_id, provider, workspace_slug, source_project_id, identifier, name, mirror_id,
                  linked_by, created_at)
             VALUES (?1, ?2, 'plane', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                n.id,
                n.project_id,
                n.workspace_slug,
                n.source_project_id,
                n.identifier,
                n.name,
                mirror_id,
                n.linked_by,
                now
            ],
        )?;
        let made =
            tx.query_row(&format!("SELECT {COLUMNS} FROM tracker_connections c WHERE c.id = ?1"), params![n.id], row)?;
        tx.commit()?;
        Ok(Some(made))
    }

    /// Disconnect a project's connection: ingest stops now, and its rows go
    /// at the purge ([`PURGE_GRACE_MS`]). The connection, or `None` when
    /// the project has no live one by that id.
    pub fn disconnect_tracker(&self, project: &str, id: &str, now: u64) -> Result<Option<Connection>> {
        let c = self.c();
        let n = c.execute(
            "UPDATE tracker_connections SET unlinked_at = ?1, purge_after = ?2
             WHERE project_id = ?3 AND id = ?4 AND unlinked_at IS NULL",
            params![now, now + PURGE_GRACE_MS, project, id],
        )?;
        if n == 0 {
            return Ok(None);
        }
        c.query_row(&format!("SELECT {COLUMNS} FROM tracker_connections c WHERE c.id = ?1"), params![id], row)
            .optional()
    }

    /// Whether agents' writes wait for a person (`approve`) or go
    /// (`allow`), on a live connection: whether there was one.
    pub fn set_tracker_agent_writes(&self, project: &str, id: &str, writes: &str) -> Result<bool> {
        let n = self.c().execute(
            "UPDATE tracker_connections SET agent_writes = ?1 WHERE project_id = ?2 AND id = ?3 AND unlinked_at IS NULL",
            params![writes, project, id],
        )?;
        Ok(n == 1)
    }

    /// Run `f` on a connection's mirror rows in one transaction, if it's
    /// live and (`lease`) the backfill's claim is still that one. `None`,
    /// and nothing written, otherwise.
    pub fn tracker_mirror<T>(
        &self,
        id: &str,
        lease: Option<&str>,
        f: impl FnOnce(&Tx<'_, '_>) -> Result<T>,
    ) -> Result<Option<T>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let sql = "SELECT 1 FROM tracker_connections WHERE id = ?1 AND unlinked_at IS NULL";
        let live = match lease {
            None => tx.query_row(&format!("{sql}{}", tx.for_update()), params![id], |r| r.get::<i64>(0)),
            Some(l) => {
                tx.query_row(&format!("{sql} AND lease = ?2{}", tx.for_update()), params![id, l], |r| r.get::<i64>(0))
            }
        }
        .optional()?;
        if live.is_none() {
            return Ok(None);
        }
        let out = f(&tx)?;
        tx.commit()?;
        Ok(Some(out))
    }

    /// Claim a live connection that isn't backfilled (or has a cursor to
    /// carry on from). Two machines never claim the same one (a lease of
    /// [`POLL_LEASE_MS`]).
    pub fn claim_backfill(&self, now: u64) -> Result<Option<Backfill>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let found = tx
            .query_row(
                &format!(
                    "SELECT id, project_id, workspace_slug, source_project_id, mirror_id, poll_cursor, poll_failures
                     FROM tracker_connections
                     WHERE unlinked_at IS NULL AND lease_until <= ?1 AND backfilled_at IS NULL
                     ORDER BY created_at, id LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| {
                    Ok(Backfill {
                        id: r.get(0)?,
                        project_id: r.get(1)?,
                        workspace_slug: r.get(2)?,
                        source_project_id: r.get(3)?,
                        mirror_id: r.get(4)?,
                        cursor: r.get(5)?,
                        failures: r.get(6)?,
                        lease: String::new(),
                    })
                },
            )
            .optional()?;
        let Some(mut job) = found else { return Ok(None) };
        job.lease = hex::encode(arugula_e2e::random::<16>());
        let n = tx.execute(
            "UPDATE tracker_connections SET lease = ?1, lease_until = ?2
             WHERE id = ?3 AND lease_until <= ?4 AND unlinked_at IS NULL",
            params![job.lease, now + POLL_LEASE_MS, job.id, now],
        )?;
        if n != 1 {
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(job))
    }

    /// Hold the claim for another [`POLL_LEASE_MS`]: false when it's lost,
    /// or the connection was disconnected, and the turn stops.
    pub fn renew_backfill(&self, job: &Backfill, now: u64) -> Result<bool> {
        let n = self.c().execute(
            "UPDATE tracker_connections SET lease_until = ?1 WHERE id = ?2 AND lease = ?3 AND unlinked_at IS NULL",
            params![now + POLL_LEASE_MS, job.id, job.lease],
        )?;
        Ok(n == 1)
    }

    /// A turn is done and the claim goes: backfilled, or to carry on from
    /// a cursor. False when the claim was lost.
    pub fn finish_backfill(&self, job: &Backfill, how: &Finish, now: u64) -> Result<bool> {
        let c = self.c();
        let n = match how {
            Finish::Done => c.execute(
                "UPDATE tracker_connections SET backfilled_at = ?1, poll_cursor = NULL, poll_failures = 0,
                     lease = NULL, lease_until = ?1
                 WHERE id = ?2 AND lease = ?3",
                params![now, job.id, job.lease],
            )?,
            Finish::Cursor(cursor) => c.execute(
                "UPDATE tracker_connections SET poll_cursor = ?1, poll_failures = 0, lease = NULL, lease_until = ?2
                 WHERE id = ?3 AND lease = ?4",
                params![cursor, now, job.id, job.lease],
            )?,
        };
        Ok(n == 1)
    }

    /// A turn failed: the claim goes, and the connection waits `backoff_ms`
    /// doubled for each failure in a row before it, up to an hour. How long
    /// it waits.
    pub fn fail_backfill(&self, job: &Backfill, backoff_ms: u64, now: u64) -> Result<u64> {
        let wait = backoff_ms.saturating_mul(1u64 << job.failures.min(16)).min(super::mirror::MAX_BACKOFF_MS);
        self.c().execute(
            "UPDATE tracker_connections SET poll_failures = poll_failures + 1, lease = NULL, lease_until = ?1
             WHERE id = ?2 AND lease = ?3",
            params![now + wait, job.id, job.lease],
        )?;
        Ok(wait)
    }

    /// Purge one disconnected connection whose `purge_after` has passed and
    /// whose backfill holds no lease: every row of it, its sealed secrets,
    /// then the connection, in one transaction. Another machine purging at
    /// the same time passes over it. The connection's id, or `None`.
    pub fn purge_tracker(&self, now: u64) -> Result<Option<String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let due = tx
            .query_row(
                &format!(
                    "SELECT id, project_id, mirror_id FROM tracker_connections
                     WHERE purge_after IS NOT NULL AND purge_after <= ?1 AND unlinked_at IS NOT NULL
                         AND lease_until <= ?1
                     ORDER BY purge_after LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?, r.get::<i64>(2)?)),
            )
            .optional()?;
        let Some((id, project, mirror_id)) = due else { return Ok(None) };
        purge_rows(&tx, mirror_id)?;
        for name in [key_name(&id), webhook_name(&id)] {
            tx.execute("DELETE FROM project_secrets WHERE project_id = ?1 AND name = ?2", params![project, name])?;
        }
        tx.execute("DELETE FROM tracker_connections WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(Some(id))
    }
}

/// What deleting an account takes from the connections, inside the
/// platform's `account_deleted` transaction: its name as who connected, and
/// the connections of a project that went with it are disconnected now.
pub(crate) fn erase_account(c: &impl Sql, account: &str, now: u64) -> Result<()> {
    c.execute("UPDATE tracker_connections SET linked_by = NULL WHERE linked_by = ?1", params![account])?;
    c.execute(
        "UPDATE tracker_connections SET unlinked_at = ?1, purge_after = ?1
         WHERE unlinked_at IS NULL AND project_id NOT IN (SELECT id FROM projects)",
        params![now],
    )?;
    Ok(())
}

/// Write a work item as `connection`'s row, keyed on the tracker's id for
/// it: unless the row is newer. Whether a row was written.
pub fn upsert_item(c: &impl Sql, connection: &str, mirror_id: i64, it: &Item, now: u64) -> Result<bool> {
    let id = format!("fi_{}", hex::encode(arugula_e2e::random::<16>()));
    let n = c.execute(
        "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at,
             provider, connection_id, source_item_id, item_type, state_name, state_group, parent_source_id)
         VALUES (?1, ?2, ?3, 'issue', ?4, ?5, ?6, ?7, ?8, 'plane', ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (connection_id, source_item_id) DO UPDATE SET number = excluded.number, state = excluded.state,
             title = excluded.title, created_at = excluded.created_at, updated_at = excluded.updated_at,
             synced_at = excluded.synced_at, item_type = excluded.item_type, state_name = excluded.state_name,
             state_group = excluded.state_group, parent_source_id = excluded.parent_source_id
         WHERE excluded.updated_at > forge_items.updated_at",
        params![
            id,
            mirror_id,
            it.sequence_id,
            it.state(),
            it.title,
            it.created_at,
            it.updated_at,
            now,
            connection,
            it.id,
            it.item_type,
            it.state_name,
            it.state_group,
            it.parent_id
        ],
    )?;
    Ok(n == 1)
}
