//! A hosted workspace's snapshots (M263), on its row (migration `0054`
//! says what each column is): its current one, the latest build's status,
//! and the earlier ones still to delete. Its builder is a row of its own,
//! of purpose `snapshot`, whose `parent_id` is the workspace.
//! [`crate::hosted::snapshot`] has the rules.
//!
//! None of this is written by a checkpoint: a builder's step writes its
//! workspace's columns while the workspace's own row may be leased, and a
//! workspace's step reads them fresh.

use anyhow::Result;

use super::{
    Db, Taken,
    hosted::{HostedWorkspace, SNAPSHOT, Snapshots},
    sql::{Optional as _, Sql, params},
    taken,
};

const SNAPSHOT_COLUMNS: &str = "snapshot_id, snapshot_hash, snapshot_template, snapshot_built_at, snapshot_status, \
     snapshot_error, snapshot_try_hash, snapshot_try_at, snapshot_retired, box_snapshot";

fn retired(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn read(c: &impl Sql, id: &str) -> Result<Option<Snapshots>> {
    c.query_row(&format!("SELECT {SNAPSHOT_COLUMNS} FROM hosted_workspaces WHERE id = ?1"), params![id], |r| {
        Ok(Snapshots {
            id: r.get(0)?,
            hash: r.get(1)?,
            template: r.get(2)?,
            built_at: r.get(3)?,
            status: r.get(4)?,
            error: r.get(5)?,
            try_hash: r.get(6)?,
            try_at: r.get(7)?,
            retired: retired(&r.get::<String>(8)?),
            box_snapshot: r.get(9)?,
        })
    })
    .optional()
}

/// A builder's name: one live builder per workspace, by the names' unique
/// index, and never a people's workspace's name (`:` isn't in one).
pub fn builder_name(parent: &str) -> String {
    format!("snapshot:{parent}")
}

impl Db {
    /// Workspace `id`'s snapshots: all `None` before its first build.
    pub fn hosted_snapshots(&self, id: &str) -> Result<Snapshots> {
        Ok(read(&self.c(), id)?.unwrap_or_default())
    }

    /// Start building a snapshot for workspace `w` with a builder of id
    /// `builder`, `creating`, its spec and maker the workspace's: whether it
    /// started (`false`: the workspace has a live builder, or is gone).
    pub fn start_snapshot(&self, w: &HostedWorkspace, builder: &str, now: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let hash = w.spec.hash();
        let marked = tx.execute(
            "UPDATE hosted_workspaces SET snapshot_status = 'building', snapshot_try_hash = ?2, snapshot_try_at = ?3
             WHERE id = ?1 AND deleted_at IS NULL AND purpose = 'workspace' AND COALESCE(asked, '') <> 'delete'",
            params![w.id, hash, now],
        )?;
        if marked != 1 {
            return Ok(false);
        }
        let made = tx.execute(
            "INSERT INTO hosted_workspaces
                 (id, project_id, name, spec, spec_hash, state, created_by, created_at, updated_at, purpose, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'creating', ?6, ?7, ?7, ?8, ?9)",
            params![
                builder,
                w.project,
                builder_name(&w.id),
                serde_json::to_string(&w.spec)?,
                hash,
                w.created_by,
                now,
                SNAPSHOT,
                w.id
            ],
        );
        match made.map_err(taken) {
            Err(e) if e.is::<Taken>() => return Ok(false),
            r => r?,
        };
        tx.commit()?;
        Ok(true)
    }

    /// Workspace `parent`'s live builder, if it has one.
    pub fn snapshot_builder(&self, parent: &str) -> Result<Option<HostedWorkspace>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {} FROM hosted_workspaces
                     WHERE parent_id = ?1 AND purpose = 'snapshot' AND deleted_at IS NULL
                     ORDER BY created_at DESC LIMIT 1",
                    super::hosted::COLUMNS
                ),
                params![parent],
                super::hosted::row,
            )
            .optional()
    }

    /// A build for `parent` made snapshot `id` from the spec `hash`, on
    /// `template`: it's the current one, and the one before is retired (to
    /// be deleted once no box is made from it). `false` when the workspace
    /// is gone or being deleted: nobody keeps the new one, so the caller
    /// deletes it.
    pub fn snapshot_built(&self, parent: &str, id: &str, hash: &str, template: &str, now: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let live: Option<String> = tx
            .query_row(
                "SELECT COALESCE(asked, '') FROM hosted_workspaces WHERE id = ?1 AND deleted_at IS NULL",
                params![parent],
                |r| r.get(0),
            )
            .optional()?;
        if live.is_none_or(|asked| asked == "delete") {
            return Ok(false);
        }
        let Some(s) = read(&tx, parent)? else { return Ok(false) };
        let mut retired = s.retired;
        if let Some(old) = s.id.filter(|old| old != id)
            && !retired.contains(&old)
        {
            retired.push(old);
        }
        tx.execute(
            "UPDATE hosted_workspaces SET snapshot_id = ?2, snapshot_hash = ?3, snapshot_template = ?4,
                 snapshot_built_at = ?5, snapshot_status = 'ready', snapshot_error = NULL, snapshot_retired = ?6
             WHERE id = ?1",
            params![parent, id, hash, template, now, serde_json::to_string(&retired)?],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// A build for `parent` failed: it says why, and the current snapshot
    /// (if any) stays.
    pub fn snapshot_failed(&self, parent: &str, error: &str) -> Result<()> {
        self.c().execute(
            "UPDATE hosted_workspaces SET snapshot_status = 'failed', snapshot_error = ?2 WHERE id = ?1",
            params![parent, error],
        )?;
        Ok(())
    }

    /// The provider takes no snapshots: noted as a try of the spec `hash`,
    /// so it isn't asked again until the next is due, with no status.
    pub fn snapshot_skipped(&self, parent: &str, hash: &str, now: u64) -> Result<()> {
        self.c().execute(
            "UPDATE hosted_workspaces SET snapshot_try_hash = ?2, snapshot_try_at = ?3 WHERE id = ?1",
            params![parent, hash, now],
        )?;
        Ok(())
    }

    /// Snapshot `id`, `parent`'s current one, is gone at the provider: it's
    /// forgotten, and the workspace says why.
    pub fn snapshot_gone(&self, parent: &str, id: &str, error: &str) -> Result<()> {
        self.c().execute(
            "UPDATE hosted_workspaces SET snapshot_id = NULL, snapshot_hash = NULL, snapshot_template = NULL,
                 snapshot_built_at = NULL, snapshot_status = 'failed', snapshot_error = ?3
             WHERE id = ?1 AND snapshot_id = ?2",
            params![parent, id, error],
        )?;
        Ok(())
    }

    /// An earlier snapshot of `parent`'s was deleted at the provider.
    pub fn snapshot_deleted(&self, parent: &str, id: &str) -> Result<()> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(s) = read(&tx, parent)? else { return Ok(()) };
        let left: Vec<&String> = s.retired.iter().filter(|r| r.as_str() != id).collect();
        tx.execute(
            "UPDATE hosted_workspaces SET snapshot_retired = ?2 WHERE id = ?1",
            params![parent, serde_json::to_string(&left)?],
        )?;
        tx.commit()
    }

    /// The workspace is going: its current snapshot is retired with the
    /// earlier ones, and its box (deleted) is made from none.
    pub fn snapshots_retire_all(&self, parent: &str) -> Result<()> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(s) = read(&tx, parent)? else { return Ok(()) };
        let mut retired = s.retired;
        if let Some(id) = s.id.filter(|id| !retired.contains(id)) {
            retired.push(id);
        }
        tx.execute(
            "UPDATE hosted_workspaces SET snapshot_id = NULL, snapshot_hash = NULL, snapshot_template = NULL,
                 snapshot_built_at = NULL, snapshot_retired = ?2, box_snapshot = NULL
             WHERE id = ?1",
            params![parent, serde_json::to_string(&retired)?],
        )?;
        tx.commit()
    }

    /// Retire snapshot `id` of `parent`'s, a workspace being deleted (a
    /// build that ended after it was asked): whether there's a row to keep
    /// it on until it's deleted.
    pub fn snapshot_retire(&self, parent: &str, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let live: Option<u64> = tx
            .query_row("SELECT 1 FROM hosted_workspaces WHERE id = ?1 AND deleted_at IS NULL", params![parent], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(s) = live.and(read(&tx, parent)?) else { return Ok(false) };
        let mut retired = s.retired;
        if !retired.iter().any(|r| r == id) {
            retired.push(id.to_owned());
        }
        tx.execute(
            "UPDATE hosted_workspaces SET snapshot_retired = ?2 WHERE id = ?1",
            params![parent, serde_json::to_string(&retired)?],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The snapshot workspace `id`'s box is made from now, or none (the
    /// template). Written before the create is sent.
    pub fn set_box_snapshot(&self, id: &str, snapshot: Option<&str>) -> Result<()> {
        self.c().execute("UPDATE hosted_workspaces SET box_snapshot = ?2 WHERE id = ?1", params![id, snapshot])?;
        Ok(())
    }

    /// A deleted workspace's delete found a snapshot still held: since when
    /// that's so (the first time, `now`).
    pub fn snapshot_held(&self, parent: &str, now: u64) -> Result<u64> {
        let c = self.c();
        c.execute(
            "UPDATE hosted_workspaces SET snapshot_held_since = ?2 WHERE id = ?1 AND snapshot_held_since IS NULL",
            params![parent, now],
        )?;
        Ok(c.query_row("SELECT snapshot_held_since FROM hosted_workspaces WHERE id = ?1", params![parent], |r| {
            r.get::<Option<u64>>(0)
        })
        .optional()?
        .flatten()
        .unwrap_or(now))
    }

    /// A workspace deleted for good leaves its retired snapshots behind, on
    /// the platform's list of orphans: the ids moved.
    pub fn orphan_snapshots(&self, parent: &str, project: &str, now: u64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(s) = read(&tx, parent)? else { return Ok(vec![]) };
        for id in &s.retired {
            tx.execute(
                "INSERT INTO hosted_snapshot_orphans (snapshot_id, workspace_id, project_id, since, tried_at)
                 VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT (snapshot_id) DO NOTHING",
                params![id, parent, project, now],
            )?;
        }
        tx.execute("UPDATE hosted_workspaces SET snapshot_retired = '[]' WHERE id = ?1", params![parent])?;
        tx.commit()?;
        Ok(s.retired)
    }

    /// An orphan due to be asked again (last tried at or before `before`),
    /// taken: its `tried_at` is `now`, so no other worker takes it. Its id
    /// and workspace.
    pub fn take_orphan_snapshot(&self, before: u64, now: u64) -> Result<Option<(String, String)>> {
        let c = self.c();
        let Some((id, ws, tried)) = c
            .query_row(
                "SELECT snapshot_id, workspace_id, tried_at FROM hosted_snapshot_orphans
                 WHERE tried_at <= ?1 ORDER BY tried_at, snapshot_id LIMIT 1",
                params![before],
                |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?, r.get::<u64>(2)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let took = c.execute(
            "UPDATE hosted_snapshot_orphans SET tried_at = ?3 WHERE snapshot_id = ?1 AND tried_at = ?2",
            params![id, tried, now],
        )?;
        Ok((took == 1).then_some((id, ws)))
    }

    /// The orphan `id` was deleted at the provider.
    pub fn orphan_snapshot_gone(&self, id: &str) -> Result<()> {
        self.c().execute("DELETE FROM hosted_snapshot_orphans WHERE snapshot_id = ?1", params![id])?;
        Ok(())
    }

    /// The orphans, oldest first: id and workspace.
    pub fn orphan_snapshot_ids(&self) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT snapshot_id, workspace_id FROM hosted_snapshot_orphans ORDER BY since, snapshot_id",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// Workspace `id` is due now, unless a worker holds it.
    pub fn nudge_hosted(&self, id: &str) -> Result<()> {
        self.c().execute(
            "UPDATE hosted_workspaces SET lease_until = 0 WHERE id = ?1 AND lease IS NULL AND deleted_at IS NULL",
            params![id],
        )?;
        Ok(())
    }
}
