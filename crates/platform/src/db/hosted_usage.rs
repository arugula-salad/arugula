//! Hosted workspaces' usage rows (M188), made by migration `0014`: each
//! box's running intervals, metered in the worker's checkpoint, and what
//! the provider says each box ran. What they mean is
//! [`crate::hosted::usage`]'s.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};
use crate::{
    compute::Run,
    hosted::usage::{self, Change, Checked, Class, Interval, Observed, Open},
};

fn class(cpus: Option<u32>, memory_mb: Option<u32>) -> Option<Class> {
    Some(Class { cpus: cpus?, memory_mb: memory_mb? })
}

/// Keep what a checkpoint saw of a workspace's box: inside the
/// checkpoint's transaction, under its lease.
pub(crate) fn meter(c: &impl Sql, workspace: &str, project: &str, o: &Observed) -> Result<()> {
    let open = c
        .query_row(
            &format!(
                "SELECT sandbox_id, started_at, cpus, memory_mb FROM hosted_usage
                 WHERE workspace_id = ?1 AND ended_at IS NULL{}",
                c.for_update()
            ),
            params![workspace],
            |r| Ok(Open { sandbox: r.get(0)?, started_at: r.get(1)?, class: class(r.get(2)?, r.get(3)?) }),
        )
        .optional()?;
    let last_end: Option<u64> =
        c.query_row("SELECT MAX(ended_at) FROM hosted_usage WHERE workspace_id = ?1", params![workspace], |r| {
            r.get(0)
        })?;
    for change in usage::changes(open.as_ref(), last_end.unwrap_or(0), o) {
        match change {
            Change::Close { started_at, ended_at } => c.execute(
                "UPDATE hosted_usage SET ended_at = ?3 WHERE workspace_id = ?1 AND started_at = ?2 AND ended_at IS NULL",
                params![workspace, started_at, ended_at],
            )?,
            Change::Drop { started_at } => c.execute(
                "DELETE FROM hosted_usage WHERE workspace_id = ?1 AND started_at = ?2 AND ended_at IS NULL",
                params![workspace, started_at],
            )?,
            Change::Open { sandbox, started_at, class } => c.execute(
                "INSERT INTO hosted_usage (workspace_id, project_id, sandbox_id, cpus, memory_mb, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    workspace,
                    project,
                    sandbox,
                    class.map(|c| c.cpus),
                    class.map(|c| c.memory_mb),
                    started_at
                ],
            )?,
            Change::Size { started_at, class } => c.execute(
                "UPDATE hosted_usage SET cpus = ?3, memory_mb = ?4 WHERE workspace_id = ?1 AND started_at = ?2",
                params![workspace, started_at, class.cpus, class.memory_mb],
            )?,
        };
    }
    Ok(())
}

/// Every interval that overlaps `[from, to)`, oldest first: on a
/// read-only connection too (`arugula-platform usage`).
pub fn intervals(c: &impl Sql, from: u64, to: u64) -> Result<Vec<Interval>> {
    c.query_map(
        "SELECT workspace_id, project_id, cpus, memory_mb, started_at, ended_at FROM hosted_usage
         WHERE started_at < ?2 AND (ended_at IS NULL OR ended_at > ?1)
         ORDER BY started_at, workspace_id",
        params![from, to],
        |r| {
            Ok(Interval {
                workspace: r.get(0)?,
                project: r.get(1)?,
                class: class(r.get(2)?, r.get(3)?),
                started_at: r.get(4)?,
                ended_at: r.get(5)?,
            })
        },
    )
}

/// The boxes that ran in `[from, to)` and that the provider has a
/// record of, each with what the platform metered over its whole life
/// (a box still running isn't one: it's read once it stops).
pub fn checked(c: &impl Sql, from: u64, to: u64) -> Result<Vec<Checked>> {
    c.query_map(
        "SELECT p.workspace_id, p.ms, p.cpu_ms, p.mib_ms, CAST(SUM(u.ended_at - u.started_at) AS BIGINT)
         FROM hosted_usage_provider p JOIN hosted_usage u ON u.sandbox_id = p.sandbox_id
         WHERE p.runs IS NOT NULL AND p.sandbox_id IN (
             SELECT sandbox_id FROM hosted_usage WHERE started_at < ?2 AND (ended_at IS NULL OR ended_at > ?1))
         GROUP BY p.sandbox_id, p.workspace_id, p.ms, p.cpu_ms, p.mib_ms
         HAVING COUNT(*) = COUNT(u.ended_at)
         ORDER BY p.workspace_id, p.sandbox_id",
        params![from, to],
        |r| {
            Ok(Checked {
                workspace: r.get(0)?,
                provider_ms: r.get(1)?,
                provider_cpu_ms: r.get(2)?,
                provider_mib_ms: r.get(3)?,
                metered_ms: r.get::<Option<u64>>(4)?.unwrap_or(0),
            })
        },
    )
}

impl Db {
    /// Every interval that overlaps `[from, to)`, oldest first.
    pub fn hosted_usage(&self, from: u64, to: u64) -> Result<Vec<Interval>> {
        intervals(&self.c(), from, to)
    }

    /// [`checked`]'s boxes.
    pub fn hosted_usage_checked(&self, from: u64, to: u64) -> Result<Vec<Checked>> {
        checked(&self.c(), from, to)
    }

    /// Boxes whose provider record is due (`hosted::usage::reconcile`):
    /// stopped at least `settle` ms ago, and not read since they last
    /// stopped and settled. Each as its sandbox, workspace and project.
    pub fn hosted_usage_due(&self, now: u64, settle: u64, limit: usize) -> Result<Vec<(String, String, String)>> {
        self.c().query_map(
            "SELECT u.sandbox_id, MIN(u.workspace_id), MIN(u.project_id) FROM hosted_usage u
             LEFT JOIN hosted_usage_provider p ON p.sandbox_id = u.sandbox_id
             GROUP BY u.sandbox_id, p.checked_at
             HAVING COUNT(*) = COUNT(u.ended_at) AND MAX(u.ended_at) + ?2 <= ?1
                 AND (p.checked_at IS NULL OR p.checked_at < MAX(u.ended_at) + ?2)
             ORDER BY MAX(u.ended_at) LIMIT ?3",
            params![now, settle, limit as u64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    /// Keep what the provider says a box ran: `None` when it keeps no
    /// record of it.
    pub fn set_hosted_usage_provider(
        &self,
        sandbox: &str,
        workspace: &str,
        project: &str,
        runs: Option<&[Run]>,
        now: u64,
    ) -> Result<()> {
        let sum = |f: fn(&Run) -> u64| runs.map_or(0, |r| r.iter().map(f).sum::<u64>());
        self.c().execute(
            "INSERT INTO hosted_usage_provider (sandbox_id, workspace_id, project_id, runs, ms, cpu_ms, mib_ms, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (sandbox_id) DO UPDATE SET runs = excluded.runs, ms = excluded.ms,
                 cpu_ms = excluded.cpu_ms, mib_ms = excluded.mib_ms, checked_at = excluded.checked_at",
            params![
                sandbox,
                workspace,
                project,
                runs.map(|r| r.len() as u64),
                sum(|r| r.ms),
                sum(|r| r.ms * u64::from(r.cpus)),
                sum(|r| r.ms * u64::from(r.memory_mb)),
                now
            ],
        )?;
        Ok(())
    }
}
