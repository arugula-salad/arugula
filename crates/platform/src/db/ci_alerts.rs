//! CI alerts' rows (native #774), made by migration `0105`: which projects
//! have them on, whom a new red issue mentions, each branch's red issues
//! and its newest green. [`crate::ci_alerts`] has the rules; the key is
//! sealed in `project_secrets` under [`KEY_NAME`], never here.
//!
//! Each write an event makes notes the event (its delivery, in the GitHub
//! webhook's table) in the same transaction, so an event is applied once:
//! a delivery that finds it noted already writes nothing.

use std::cell::Cell;

use anyhow::Result;

use super::{
    Db,
    issues::{Actor, Issue, NewIssue},
    sql::{Optional as _, Sql, Tx, params},
};

/// The name a project's CI alerts key is sealed under, beside its project.
pub const KEY_NAME: &str = "ci_alerts/key";

/// A project's CI alerts, made when their first key was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alerts {
    pub project: String,
    /// A key is in use: events are taken.
    pub on: bool,
    /// The people are assigned a new red issue, as well as mentioned.
    pub assign: bool,
    /// Accounts, in the order they were named.
    pub people: Vec<String>,
    /// Who made the key in use, and when. Empty while off, or once that
    /// account is deleted.
    pub key_made_by: String,
    pub key_made_at: Option<u64>,
    pub last_event_at: Option<u64>,
}

/// A branch's open red issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub issue: String,
    pub red_commit: String,
    /// Its first event's `at`.
    pub red_at: u64,
    pub opened_at: u64,
}

/// A branch's newest green.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Green {
    pub commit: String,
    /// Its event's `at`.
    pub at: u64,
}

/// An event, as noted with the write it makes: its delivery key and name.
#[derive(Debug, Clone, Copy)]
pub struct Noted<'a> {
    pub delivery: &'a str,
    pub event: &'a str,
    pub now: u64,
}

impl Noted<'_> {
    /// Note it in `tx`: false when it was noted already.
    pub fn in_tx(&self, tx: &impl Sql) -> Result<bool> {
        Db::first_delivery_in(tx, self.delivery, self.event, self.now)
    }
}

/// What [`Db::create_ci_issue`] did.
#[derive(Debug)]
pub enum Created {
    Opened(Box<Issue>),
    /// The branch has an open issue already: nothing was written.
    Taken,
    /// The event was applied already: nothing was written.
    Applied,
}

impl Db {
    /// A project's CI alerts, or `None` when they were never turned on.
    pub fn ci_alerts(&self, project: &str) -> Result<Option<Alerts>> {
        let c = self.c();
        let Some((assign, key_made_by, key_made_at, last_event_at)) = c
            .query_row(
                "SELECT assign, key_made_by, key_made_at, last_event_at FROM ci_alerts WHERE project_id = ?1",
                params![project],
                |r| Ok((r.get::<i64>(0)?, r.get::<String>(1)?, r.get::<Option<u64>>(2)?, r.get::<Option<u64>>(3)?)),
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
            on: key_made_at.is_some(),
            assign: assign != 0,
            people,
            key_made_by,
            key_made_at,
            last_event_at,
        }))
    }

    /// Note a new key made for `project` by `by`: CI alerts are on from
    /// now, with their people, `assign` and open issues as they were.
    pub fn ci_alerts_key_made(&self, project: &str, by: &str, now: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO ci_alerts (project_id, key_made_by, key_made_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (project_id) DO UPDATE SET key_made_by = ?2, key_made_at = ?3",
            params![project, by, now],
        )?;
        Ok(())
    }

    /// Turn a project's CI alerts off: they keep their people and open
    /// issues for when a new key is made. Whether they were on.
    pub fn ci_alerts_off(&self, project: &str) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE ci_alerts SET key_made_at = NULL, key_made_by = '' WHERE project_id = ?1 AND key_made_at IS NOT NULL",
            params![project],
        )? == 1)
    }

    /// Set whom a project's new red issue mentions, and whether it
    /// assigns them. `false` when alerts were never turned on.
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

    /// The projects of `team` whose alerts name someone, for pruning those
    /// who left it.
    pub fn ci_alert_team_projects(&self, team: &str) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT DISTINCT p.project_id FROM ci_alert_people p JOIN projects pr ON pr.id = p.project_id
             WHERE pr.team_id = ?1",
            params![team],
            |r| r.get(0),
        )
    }

    /// Take everyone but `members` off a project's alerts: how many went.
    pub fn ci_alert_keep_people(&self, project: &str, members: &[String]) -> Result<usize> {
        let people: Vec<String> =
            self.c().query_map("SELECT account FROM ci_alert_people WHERE project_id = ?1", params![project], |r| {
                r.get(0)
            })?;
        let mut gone = 0;
        for a in people.iter().filter(|a| !members.contains(a)) {
            gone += self
                .c()
                .execute("DELETE FROM ci_alert_people WHERE project_id = ?1 AND account = ?2", params![project, a])?;
        }
        Ok(gone)
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
                "SELECT issue_id, red_commit, red_at, opened_at FROM ci_alert_issues
                 WHERE project_id = ?1 AND branch = ?2 AND closed_at IS NULL",
                params![project, branch],
                |r| Ok(Open { issue: r.get(0)?, red_commit: r.get(1)?, red_at: r.get(2)?, opened_at: r.get(3)? }),
            )
            .optional()
    }

    /// Every branch's open red issue in `project`: (branch, issue).
    pub fn ci_alert_opens(&self, project: &str) -> Result<Vec<(String, Open)>> {
        self.c().query_map(
            "SELECT branch, issue_id, red_commit, red_at, opened_at FROM ci_alert_issues
             WHERE project_id = ?1 AND closed_at IS NULL ORDER BY opened_at",
            params![project],
            |r| {
                Ok((
                    r.get(0)?,
                    Open { issue: r.get(1)?, red_commit: r.get(2)?, red_at: r.get(3)?, opened_at: r.get(4)? },
                ))
            },
        )
    }

    /// The newest green `branch` of `project` was told.
    pub fn ci_alert_green(&self, project: &str, branch: &str) -> Result<Option<Green>> {
        self.c()
            .query_row(
                "SELECT green_commit, green_at FROM ci_alert_greens WHERE project_id = ?1 AND branch = ?2",
                params![project, branch],
                |r| Ok(Green { commit: r.get(0)?, at: r.get(1)? }),
            )
            .optional()
    }

    /// Open `new` as `branch`'s red issue, red at `red_commit` by an event
    /// at `red_at`, in one transaction with its row here and the event
    /// noted. Nothing is written, not even a number, when the branch has an
    /// open issue already (another machine opened it a moment ago) or the
    /// event was applied already. The unique index on the open rows
    /// (`ci_alert_issues_open`) is what decides, on SQLite and Postgres
    /// alike; on Postgres an insert racing one not yet committed waits for
    /// it.
    pub fn create_ci_issue(
        &self,
        new: &NewIssue,
        by: Actor,
        branch: &str,
        (red_commit, red_at): (&str, u64),
        noted: Noted,
    ) -> Result<Created> {
        let why = Cell::new(Created::Taken);
        let made = self.create_issue_and(new, by, noted.now, |tx, issue| {
            if !noted.in_tx(tx)? {
                why.set(Created::Applied);
                return Ok(false);
            }
            let n = tx.execute(
                "INSERT INTO ci_alert_issues (issue_id, project_id, branch, red_commit, red_at, opened_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO NOTHING",
                params![issue.id, new.project, branch, red_commit, red_at, noted.now],
            )?;
            Ok(n == 1)
        })?;
        Ok(match made {
            Some(issue) => Created::Opened(Box::new(issue)),
            None => why.into_inner(),
        })
    }

    /// Note a green for `branch`, unless a newer one is noted, and the
    /// event, in one transaction: false when the event was applied
    /// already.
    pub fn ci_green_noted(&self, project: &str, branch: &str, green: &Green, noted: Noted) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        if !noted.in_tx(&tx)? {
            return Ok(false);
        }
        green_in(&tx, project, branch, green)?;
        tx.commit()?;
        Ok(true)
    }

    /// `issue` isn't `branch`'s open red issue any more: a person closed
    /// it.
    pub fn ci_alert_closed(&self, project: &str, branch: &str, issue: &str, now: u64) -> Result<()> {
        closed_in(&self.c(), project, branch, issue, now)
    }
}

/// [`Db::ci_alert_closed`] in the caller's transaction.
pub fn closed_in(c: &impl Sql, project: &str, branch: &str, issue: &str, now: u64) -> Result<()> {
    c.execute(
        "UPDATE ci_alert_issues SET closed_at = ?4
         WHERE project_id = ?1 AND branch = ?2 AND issue_id = ?3 AND closed_at IS NULL",
        params![project, branch, issue, now],
    )?;
    Ok(())
}

/// Note `green` as `branch`'s newest, unless a newer one is.
pub fn green_in(c: &impl Sql, project: &str, branch: &str, green: &Green) -> Result<()> {
    c.execute(
        "INSERT INTO ci_alert_greens (project_id, branch, green_commit, green_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (project_id, branch) DO UPDATE SET green_commit = ?3, green_at = ?4
         WHERE ci_alert_greens.green_at < ?4",
        params![project, branch, green.commit, green.at],
    )?;
    Ok(())
}

/// What a green that closes an issue writes beside the close, in its
/// transaction: the issue's row closed, the green, and the event noted.
/// False when the event was applied already.
pub fn green_closes(
    tx: &Tx<'_, '_>,
    project: &str,
    branch: &str,
    issue: &str,
    green: &Green,
    noted: Noted,
) -> Result<bool> {
    if !noted.in_tx(tx)? {
        return Ok(false);
    }
    closed_in(tx, project, branch, issue, noted.now)?;
    green_in(tx, project, branch, green)?;
    Ok(true)
}

/// Deleting an account: the rows of projects that went with it go, it's
/// named on no project's alerts any more, and a key it made stays (the
/// project's), naming nobody.
pub(crate) fn erase_account(c: &impl Sql, d: &crate::Deleted) -> Result<()> {
    for table in ["ci_alerts", "ci_alert_people", "ci_alert_issues", "ci_alert_greens"] {
        c.execute(&format!("DELETE FROM {table} WHERE project_id NOT IN (SELECT id FROM projects)"), params![])?;
    }
    c.execute("DELETE FROM ci_alert_people WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE ci_alerts SET key_made_by = '' WHERE key_made_by = ?1", params![d.account])?;
    Ok(())
}
