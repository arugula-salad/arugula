//! Moving a beta Cloud workspace off Managoat (M189): where each went, kept
//! on its `cloud_workspaces` row by migration `0025`, and the reads a move
//! makes of what it leaves behind (its tabs and their events). What a move
//! does is [`crate::cloud::moving`]'s. These go with the `cloud_*` tables,
//! after the deletion.

use anyhow::Result;
use serde_json::Value;

use super::{
    Db,
    cloud::CloudTabSpec,
    sql::{Optional as _, Sql, params},
};

/// A Cloud workspace's row, as a move reads it.
#[derive(Debug, Clone)]
pub struct MoveSource {
    /// Its project: the account's personal one (`None` only on a row from
    /// before projects, which a move refuses).
    pub project: Option<String>,
    /// Its Managoat workspace, once it was made.
    pub machine: Option<String>,
    pub moved_to: Option<String>,
    pub moved_at: Option<u64>,
}

/// One of a Cloud workspace's tabs, open or closed.
#[derive(Debug, Clone)]
pub struct MovedTab {
    pub id: String,
    pub spec: CloudTabSpec,
    pub created: u64,
    pub closed: Option<u64>,
}

impl Db {
    /// The account's Cloud workspace `id`, as a move reads it: `None` when
    /// it isn't the account's, or it's deleted.
    pub fn cloud_move_source(&self, account: &str, id: &str) -> Result<Option<MoveSource>> {
        self.c()
            .query_row(
                "SELECT project_id, provider_id, moved_to, moved_at FROM cloud_workspaces
                 WHERE account = ?1 AND id = ?2 AND deleted IS NULL",
                params![account, id],
                |r| Ok(MoveSource { project: r.get(0)?, machine: r.get(1)?, moved_to: r.get(2)?, moved_at: r.get(3)? }),
            )
            .optional()
    }

    /// Keep `hosted` as where the account's Cloud workspace `id` moves,
    /// unless it has one already: the one it has, either way.
    pub fn claim_cloud_move(&self, account: &str, id: &str, hosted: &str) -> Result<String> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.execute(
            "UPDATE cloud_workspaces SET moved_to = ?3 WHERE account = ?1 AND id = ?2 AND moved_to IS NULL",
            params![account, id, hosted],
        )?;
        let kept: Option<String> = tx.query_row(
            "SELECT moved_to FROM cloud_workspaces WHERE account = ?1 AND id = ?2",
            params![account, id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        kept.ok_or_else(|| anyhow::anyhow!("cloud workspace not found"))
    }

    /// The move to `hosted` is done.
    pub fn finish_cloud_move(&self, id: &str, hosted: &str, now: u64) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET moved_at = ?3 WHERE id = ?1 AND moved_to = ?2 AND moved_at IS NULL",
            params![id, hosted, now],
        )?;
        Ok(())
    }

    /// Forget the move to `hosted`, which made nothing or whose workspace
    /// was deleted since, so the next one starts afresh.
    pub fn drop_cloud_move(&self, id: &str, hosted: &str) -> Result<()> {
        self.c().execute(
            "UPDATE cloud_workspaces SET moved_to = NULL, moved_at = NULL WHERE id = ?1 AND moved_to = ?2",
            params![id, hosted],
        )?;
        Ok(())
    }

    /// Whether hosted workspace `id` is deleted: `None` when there's no
    /// such row at all.
    pub fn hosted_deleted(&self, id: &str) -> Result<Option<bool>> {
        self.c()
            .query_row("SELECT deleted_at IS NOT NULL FROM hosted_workspaces WHERE id = ?1", params![id], |r| r.get(0))
            .optional()
    }

    /// The transcripts a move copied into hosted workspace `id`'s archive.
    pub fn forget_moved_transcripts(&self, id: &str) -> Result<()> {
        self.c().execute(
            "DELETE FROM hosted_transcripts WHERE workspace_id = ?1 AND machine LIKE 'managoat:%'",
            params![id],
        )?;
        Ok(())
    }

    /// Every tab the account's Cloud workspace has had, oldest first.
    pub fn cloud_tabs_ever(&self, account: &str, cloud_workspace: &str) -> Result<Vec<MovedTab>> {
        self.c().query_map(
            "SELECT id, spec, created, closed FROM cloud_tabs WHERE account = ?1 AND cloud_workspace = ?2
             ORDER BY created, id",
            params![account, cloud_workspace],
            |r| {
                Ok(MovedTab {
                    id: r.get(0)?,
                    spec: serde_json::from_str(&r.get::<String>(1)?)?,
                    created: r.get(2)?,
                    closed: r.get(3)?,
                })
            },
        )
    }

    /// Every event a tab's conversation kept, in order (`cloud_events` reads
    /// a page at a time).
    pub fn cloud_events_ever(&self, account: &str, tab: &str) -> Result<Vec<Value>> {
        let mut all = Vec::new();
        let mut after = 0u64;
        loop {
            let page: Vec<(u64, String)> = self.c().query_map(
                "SELECT cursor, body FROM cloud_events WHERE account = ?1 AND conversation = ?2 AND cursor > ?3
                 ORDER BY cursor LIMIT 1000",
                params![account, tab, after],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let Some(last) = page.last().map(|p| p.0) else { break };
            for (_, body) in &page {
                all.push(serde_json::from_str(body)?);
            }
            if page.len() < 1000 {
                break;
            }
            after = last;
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A claim is kept once; a second claim gets the first; finishing and
    /// dropping only touch the claim they name.
    #[test]
    fn a_move_is_claimed_once() {
        let db = Db::test();
        let spec: arugula_cloud_wire::CloudWorkspaceSpec =
            serde_json::from_value(serde_json::json!({"name":"x"})).unwrap();
        let w = db.create_cloud_workspace("a", "prj_a", "k", &spec, 1).unwrap();
        let s = db.cloud_move_source("a", &w.id).unwrap().unwrap();
        assert_eq!((s.project.as_deref(), s.moved_to, s.moved_at), (Some("prj_a"), None, None));
        assert!(db.cloud_move_source("b", &w.id).unwrap().is_none(), "another account's");
        assert_eq!(db.claim_cloud_move("a", &w.id, "hw_1").unwrap(), "hw_1");
        assert_eq!(db.claim_cloud_move("a", &w.id, "hw_2").unwrap(), "hw_1");
        db.drop_cloud_move(&w.id, "hw_2").unwrap();
        db.finish_cloud_move(&w.id, "hw_2", 5).unwrap();
        let s = db.cloud_move_source("a", &w.id).unwrap().unwrap();
        assert_eq!((s.moved_to.as_deref(), s.moved_at), (Some("hw_1"), None));
        db.finish_cloud_move(&w.id, "hw_1", 5).unwrap();
        let s = db.cloud_move_source("a", &w.id).unwrap().unwrap();
        assert_eq!((s.moved_to.as_deref(), s.moved_at), (Some("hw_1"), Some(5)));
        assert_eq!(db.claim_cloud_move("a", &w.id, "hw_3").unwrap(), "hw_1", "a finished move stays");
        db.drop_cloud_move(&w.id, "hw_1").unwrap();
        let s = db.cloud_move_source("a", &w.id).unwrap().unwrap();
        assert_eq!((s.moved_to, s.moved_at), (None, None));
        assert!(db.claim_cloud_move("b", &w.id, "hw_3").is_err());
    }

    /// Events past a page are all read, in order.
    #[test]
    fn every_event_is_read() {
        let db = Db::test();
        for cursor in 1..=1203u64 {
            db.c()
                .execute(
                    "INSERT INTO cloud_events(conversation,account,cursor,body) VALUES('cwc_1','a',?1,?2)",
                    params![cursor, format!("{{\"id\":{cursor}}}")],
                )
                .unwrap();
        }
        let events = db.cloud_events_ever("a", "cwc_1").unwrap();
        assert_eq!(events.len(), 1203);
        assert_eq!((events[0]["id"].as_u64(), events[1202]["id"].as_u64()), (Some(1), Some(1203)));
        assert!(db.cloud_events_ever("b", "cwc_1").unwrap().is_empty());
    }
}
