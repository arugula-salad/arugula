//! Hosted agent transcripts' rows (M184), made by migration `0023`: a
//! closed agent block's transcript from a hosted workspace's box, kept read
//! only under its project. What's kept and who reads it is
//! [`crate::hosted::transcripts`]'s.

use anyhow::Result;
use serde::Serialize;

use super::{
    Db,
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// A kept transcript, without its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostedTranscript {
    pub id: String,
    pub project: String,
    pub workspace: String,
    /// The hosted machine it came from, and its block's id there.
    pub machine: String,
    pub block: u64,
    pub agent: String,
    pub title: String,
    pub cwd: Option<String>,
    pub started_at: u64,
    pub closed_at: u64,
    /// Its middle was dropped to fit.
    pub cut: bool,
    pub bytes: u64,
}

/// A new one, as [`Db::add_hosted_transcript`] keeps it.
pub struct NewTranscript<'a> {
    pub project: &'a str,
    pub workspace: &'a str,
    pub machine: &'a str,
    pub block: u64,
    pub agent: &'a str,
    pub title: &'a str,
    pub cwd: Option<&'a str>,
    pub started_at: u64,
    pub closed_at: u64,
    pub cut: bool,
    pub body: &'a str,
}

const COLUMNS: &str =
    "id, project_id, workspace_id, machine, block, agent, title, cwd, started_at, closed_at, cut, bytes";

fn row(r: &Row) -> Result<HostedTranscript> {
    Ok(HostedTranscript {
        id: r.get(0)?,
        project: r.get(1)?,
        workspace: r.get(2)?,
        machine: r.get(3)?,
        block: r.get(4)?,
        agent: r.get(5)?,
        title: r.get(6)?,
        cwd: r.get(7)?,
        started_at: r.get(8)?,
        closed_at: r.get(9)?,
        cut: r.get(10)?,
        bytes: r.get(11)?,
    })
}

impl Db {
    /// Keep a transcript as `id`, unless its machine sent that block's
    /// already (then that one's id). The project keeps its newest `max`;
    /// older ones go.
    pub fn add_hosted_transcript(&self, id: &str, t: &NewTranscript, max: usize, now: u64) -> Result<String> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.execute(
            "INSERT INTO hosted_transcripts (id, project_id, workspace_id, machine, block, agent, title, cwd,
                 started_at, closed_at, cut, bytes, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (machine, block) DO NOTHING",
            params![
                id,
                t.project,
                t.workspace,
                t.machine,
                t.block,
                t.agent,
                t.title,
                t.cwd,
                t.started_at,
                t.closed_at,
                t.cut,
                t.body.len() as u64,
                t.body,
                now
            ],
        )?;
        let kept: String = tx.query_row(
            "SELECT id FROM hosted_transcripts WHERE machine = ?1 AND block = ?2",
            params![t.machine, t.block],
            |r| r.get(0),
        )?;
        let ids: Vec<String> = tx.query_map(
            "SELECT id FROM hosted_transcripts WHERE project_id = ?1 ORDER BY created_at DESC, id DESC",
            params![t.project],
            |r| r.get(0),
        )?;
        for old in ids.iter().skip(max) {
            tx.execute("DELETE FROM hosted_transcripts WHERE id = ?1", params![old])?;
        }
        tx.commit()?;
        Ok(kept)
    }

    /// A project's transcripts, newest first: every workspace's, or one's.
    pub fn hosted_transcripts(&self, project: &str, workspace: Option<&str>) -> Result<Vec<HostedTranscript>> {
        match workspace {
            Some(w) => self.c().query_map(
                &format!(
                    "SELECT {COLUMNS} FROM hosted_transcripts WHERE project_id = ?1 AND workspace_id = ?2
                     ORDER BY closed_at DESC, id DESC"
                ),
                params![project, w],
                row,
            ),
            None => self.c().query_map(
                &format!(
                    "SELECT {COLUMNS} FROM hosted_transcripts WHERE project_id = ?1 ORDER BY closed_at DESC, id DESC"
                ),
                params![project],
                row,
            ),
        }
    }

    /// One of a project's transcripts, with its text.
    pub fn hosted_transcript(&self, project: &str, id: &str) -> Result<Option<(HostedTranscript, String)>> {
        self.c()
            .query_row(
                &format!("SELECT {COLUMNS}, body FROM hosted_transcripts WHERE project_id = ?1 AND id = ?2"),
                params![project, id],
                |r| Ok((row(r)?, r.get(12)?)),
            )
            .optional()
    }
}

/// An account is gone: the transcripts of the projects that go with it (its
/// personal one and its disbanded teams') go too. Runs before
/// `projects::erase_account`, which drops those projects.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute(
        "DELETE FROM hosted_transcripts WHERE project_id IN (SELECT id FROM projects WHERE account_id = ?1)",
        params![d.account],
    )?;
    for t in &d.disbanded {
        c.execute(
            "DELETE FROM hosted_transcripts WHERE project_id IN (SELECT id FROM projects WHERE team_id = ?1)",
            params![t],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new<'a>(project: &'a str, machine: &'a str, block: u64, body: &'a str) -> NewTranscript<'a> {
        NewTranscript {
            project,
            workspace: "hw_1",
            machine,
            block,
            agent: "claude",
            title: "fix the build",
            cwd: Some("/home/user/app"),
            started_at: 5,
            closed_at: 9,
            cut: false,
            body,
        }
    }

    /// A block sent twice is kept once; a project keeps its newest few;
    /// another project's aren't read.
    #[test]
    fn transcripts_are_kept_once_per_block_and_capped_per_project() {
        let db = Db::test();
        assert_eq!(db.add_hosted_transcript("htx_1", &new("prj_a", "m1", 7, "> hi"), 2, 10).unwrap(), "htx_1");
        assert_eq!(db.add_hosted_transcript("htx_2", &new("prj_a", "m1", 7, "again"), 2, 11).unwrap(), "htx_1");
        let (t, body) = db.hosted_transcript("prj_a", "htx_1").unwrap().unwrap();
        assert_eq!((t.bytes, t.agent.as_str(), t.cut, body.as_str()), (4, "claude", false, "> hi"));
        assert!(db.hosted_transcript("prj_b", "htx_1").unwrap().is_none(), "another project's");
        db.add_hosted_transcript("htx_3", &new("prj_a", "m1", 8, "b"), 2, 12).unwrap();
        db.add_hosted_transcript("htx_4", &new("prj_a", "m2", 7, "c"), 2, 13).unwrap();
        let ids: Vec<String> = db.hosted_transcripts("prj_a", None).unwrap().into_iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), 2, "the oldest went: {ids:?}");
        assert!(!ids.contains(&"htx_1".to_owned()));
        assert_eq!(db.hosted_transcripts("prj_a", Some("hw_9")).unwrap(), vec![]);
    }
}
