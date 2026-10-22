//! What a hosted machine pulls (M212): the runs ready for it. A run is
//! ready once its dispatch job (`platform_jobs`, kind `run`,
//! [`crate::work`]) has checked where it runs and marked it so (the job's
//! checkpoint), and until it starts. No table of its own: the job, the run
//! and the hosted workspace say it all.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};

impl Db {
    /// Its machine pulled run `run` at `now`: kept on its dispatch job's
    /// checkpoint (`pulled_at`), so the job doesn't fail it as never taken.
    /// Written only while no step holds the job, and only over the
    /// checkpoint it read, so a step's own write is never lost; a pull that
    /// meets a held job leaves it to the next pull.
    pub fn note_pulled(&self, run: &str, now: u64) -> Result<()> {
        let c = self.c();
        let Some(old) = c
            .query_row(
                "SELECT checkpoint FROM platform_jobs WHERE id = ?1 AND kind = 'run' AND state = 'pending'",
                params![run],
                |r| r.get::<Option<String>>(0),
            )
            .optional()?
            .flatten()
        else {
            return Ok(());
        };
        let mut cp: serde_json::Value = serde_json::from_str(&old)?;
        cp["pulled_at"] = now.into();
        c.execute(
            "UPDATE platform_jobs SET checkpoint = ?3, updated = ?2
             WHERE id = ?1 AND state = 'pending' AND lease_until <= ?2 AND checkpoint = ?4",
            params![run, now, cp.to_string(), old],
        )?;
        Ok(())
    }

    /// The runs ready for `machine`, the hosted machine of workspace
    /// `workspace` in `project`, that haven't started, oldest first, at
    /// most `limit`:
    /// - its dispatch job is under way and has marked it ready;
    /// - it's the workspace's, and its operation names the machine
    ///   (`daemon`), which is the one the workspace's box enrolled as;
    /// - it's `approved`, or `pending` under a standing grant.
    pub fn ready_runs(&self, project: &str, workspace: &str, machine: &str, limit: u32) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT r.id FROM platform_jobs j
             JOIN runs r ON r.id = j.id
             JOIN hosted_workspaces w ON w.id = r.workspace_id
             WHERE j.kind = 'run' AND j.state = 'pending' AND j.checkpoint IS NOT NULL
               AND r.project_id = ?1 AND r.workspace_id = ?2 AND r.daemon = ?3
               AND w.project_id = ?1 AND w.machine = ?3 AND w.deleted_at IS NULL
               AND (r.state = 'approved' OR (r.state = 'pending' AND r.grant_id IS NOT NULL))
             ORDER BY r.created_at, r.id LIMIT ?4",
            params![project, workspace, machine, limit],
            |r| r.get(0),
        )
    }
}
