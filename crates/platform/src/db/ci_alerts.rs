//! CI alerts' rows (native #774), made by migration `0105`: which projects
//! have them on, whom a new red issue mentions, and each branch's open red
//! issue. [`crate::ci_alerts`] has the rules; the key is sealed in
//! `project_secrets` under [`KEY_NAME`], never here.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};

/// The name a project's CI alerts key is sealed under, beside its project.
pub const KEY_NAME: &str = "ci_alerts/key";

/// A project's CI alerts, while they're on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alerts {
    pub project: String,
    /// The people are assigned a new red issue, as well as mentioned.
    pub assign: bool,
    /// Accounts, in the order they were named.
    pub people: Vec<String>,
    /// Who made the key in use, and when. Empty once that account is
    /// deleted.
    pub key_made_by: String,
    pub key_made_at: u64,
    pub last_event_at: Option<u64>,
}

/// A branch's open red issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub issue: String,
    pub red_commit: String,
    pub opened_at: u64,
}

impl Db {
    /// A project's CI alerts, or `None` while they're off.
    pub fn ci_alerts(&self, project: &str) -> Result<Option<Alerts>> {
        let c = self.c();
        let Some((assign, key_made_by, key_made_at, last_event_at)) = c
            .query_row(
                "SELECT assign, key_made_by, key_made_at, last_event_at FROM ci_alerts WHERE project_id = ?1",
                params![project],
                |r| Ok((r.get::<i64>(0)?, r.get::<String>(1)?, r.get::<u64>(2)?, r.get::<Option<u64>>(3)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let people = c.query_map(
            "SELECT account FROM ci_alert_people WHERE project_id = ?1 ORDER BY position",
            params![project],
            |r| r.get(0),
        )?;
        Ok(Some(Alerts {
            project: project.to_owned(),
            assign: assign != 0,
            people,
            key_made_by,
            key_made_at,
            last_event_at,
        }))
    }

    /// Note a new key made for `project` by `by`: CI alerts are on from
    /// now, with their people and `assign` as they were.
    pub fn ci_alerts_key_made(&self, project: &str, by: &str, now: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO ci_alerts (project_id, key_made_by, key_made_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (project_id) DO UPDATE SET key_made_by = ?2, key_made_at = ?3",
            params![project, by, now],
        )?;
        Ok(())
    }

    /// Turn a project's CI alerts off: its row, its people and its open
    /// issues' rows go (the issues stay). Whether they were on.
    pub fn ci_alerts_off(&self, project: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = tx.execute("DELETE FROM ci_alerts WHERE project_id = ?1", params![project])?;
        tx.execute("DELETE FROM ci_alert_people WHERE project_id = ?1", params![project])?;
        tx.execute("DELETE FROM ci_alert_issues WHERE project_id = ?1", params![project])?;
        tx.commit()?;
        Ok(n == 1)
    }

    /// Set whom a project's new red issue mentions, and whether it
    /// assigns them. `false` while its alerts are off.
    pub fn set_ci_alerts(&self, project: &str, people: Option<&[String]>, assign: Option<bool>) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let on: Option<i64> =
            tx.query_row("SELECT 1 FROM ci_alerts WHERE project_id = ?1", params![project], |r| r.get(0)).optional()?;
        if on.is_none() {
            return Ok(false);
        }
        if let Some(a) = assign {
            tx.execute("UPDATE ci_alerts SET assign = ?2 WHERE project_id = ?1", params![project, i64::from(a)])?;
        }
        if let Some(people) = people {
            tx.execute("DELETE FROM ci_alert_people WHERE project_id = ?1", params![project])?;
            for (i, a) in people.iter().enumerate() {
                tx.execute(
                    "INSERT INTO ci_alert_people (project_id, account, position) VALUES (?1, ?2, ?3)
                     ON CONFLICT (project_id, account) DO NOTHING",
                    params![project, a, i as i64],
                )?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    /// A signed event arrived for `project`.
    pub fn ci_alerts_heard(&self, project: &str, now: u64) -> Result<()> {
        self.c().execute("UPDATE ci_alerts SET last_event_at = ?2 WHERE project_id = ?1", params![project, now])?;
        Ok(())
    }

    /// The red issue `branch` of `project` has open, as this table has it
    /// (its issue may have been closed by a person since).
    pub fn ci_alert_open(&self, project: &str, branch: &str) -> Result<Option<Open>> {
        self.c()
            .query_row(
                "SELECT issue_id, red_commit, opened_at FROM ci_alert_issues
                 WHERE project_id = ?1 AND branch = ?2 AND closed_at IS NULL",
                params![project, branch],
                |r| Ok(Open { issue: r.get(0)?, red_commit: r.get(1)?, opened_at: r.get(2)? }),
            )
            .optional()
    }

    /// Every branch's open red issue in `project`: (branch, issue).
    pub fn ci_alert_opens(&self, project: &str) -> Result<Vec<(String, Open)>> {
        self.c().query_map(
            "SELECT branch, issue_id, red_commit, opened_at FROM ci_alert_issues
             WHERE project_id = ?1 AND closed_at IS NULL ORDER BY opened_at",
            params![project],
            |r| Ok((r.get(0)?, Open { issue: r.get(1)?, red_commit: r.get(2)?, opened_at: r.get(3)? })),
        )
    }

    /// Open `new` as `branch`'s red issue, red at `red_commit`, in one
    /// transaction with its row here. `None` when the branch already has an
    /// open one (another machine opened it a moment ago): nothing is
    /// written then, not even a number, and the caller takes that one.
    /// The unique index on the open rows (`ci_alert_issues_open`) is what
    /// decides, on SQLite and Postgres alike; on Postgres an insert racing
    /// one not yet committed waits for it.
    pub fn create_ci_issue(
        &self,
        new: &super::issues::NewIssue,
        by: super::issues::Actor,
        branch: &str,
        red_commit: &str,
        now: u64,
    ) -> Result<Option<super::issues::Issue>> {
        self.create_issue_and(new, by, now, |tx, issue| {
            let n = tx.execute(
                "INSERT INTO ci_alert_issues (issue_id, project_id, branch, red_commit, opened_at)
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT DO NOTHING",
                params![issue.id, new.project, branch, red_commit, now],
            )?;
            Ok(n == 1)
        })
    }

    /// `issue` isn't `branch`'s open red issue any more: green closed it,
    /// or a person did.
    pub fn ci_alert_closed(&self, project: &str, branch: &str, issue: &str, now: u64) -> Result<()> {
        self.c().execute(
            "UPDATE ci_alert_issues SET closed_at = ?4
             WHERE project_id = ?1 AND branch = ?2 AND issue_id = ?3 AND closed_at IS NULL",
            params![project, branch, issue, now],
        )?;
        Ok(())
    }
}

/// Deleting an account: the rows of projects that went with it go, it's
/// named on no project's alerts any more, and a key it made stays (the
/// project's), naming nobody.
pub(crate) fn erase_account(c: &impl Sql, d: &crate::Deleted) -> Result<()> {
    for table in ["ci_alerts", "ci_alert_people", "ci_alert_issues"] {
        c.execute(&format!("DELETE FROM {table} WHERE project_id NOT IN (SELECT id FROM projects)"), params![])?;
    }
    c.execute("DELETE FROM ci_alert_people WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE ci_alerts SET key_made_by = '' WHERE key_made_by = ?1", params![d.account])?;
    Ok(())
}
