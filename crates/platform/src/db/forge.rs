//! The forge mirror's tables (M162, platform step 4, #1204): linked repos'
//! PRs and issues as platform rows. They're made by `0005_forge_mirror`;
//! their queries come with the features that write them (M161 deliveries,
//! M163 links, M164 items, M167 mints, M170 drafts, M171 the inbox). So
//! far: the webhook's deliveries (M161) and an account's deletion.

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};
use crate::Deleted;

/// How long a webhook delivery is remembered (M161): GitHub redelivers by
/// hand for a few days, and a deploy loses nothing.
pub const DELIVERIES_KEPT_MS: u64 = 7 * 24 * 3600 * 1000;

impl Db {
    /// Remember a webhook delivery (`X-GitHub-Delivery`): false when it
    /// was here already, a redelivery.
    pub fn first_delivery(&self, delivery: &str, event: &str, now: u64) -> anyhow::Result<bool> {
        let n = self.c().execute(
            "INSERT INTO github_webhook_deliveries (delivery, event, received_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (delivery) DO NOTHING",
            params![delivery, event, now],
        )?;
        Ok(n == 1)
    }

    /// The event a delivery was, if it was taken and is still kept.
    pub fn delivery_event(&self, delivery: &str) -> anyhow::Result<Option<String>> {
        self.c()
            .query_row("SELECT event FROM github_webhook_deliveries WHERE delivery = ?1", params![delivery], |r| {
                r.get(0)
            })
            .optional()
    }

    /// Forget one, so GitHub's redelivery of it is taken: what it asked
    /// for didn't happen.
    pub fn forget_delivery(&self, delivery: &str) -> anyhow::Result<()> {
        self.c().execute("DELETE FROM github_webhook_deliveries WHERE delivery = ?1", params![delivery])?;
        Ok(())
    }

    /// Forget deliveries older than [`DELIVERIES_KEPT_MS`]: how many went.
    pub fn prune_deliveries(&self, now: u64) -> anyhow::Result<usize> {
        let before = now.saturating_sub(DELIVERIES_KEPT_MS);
        self.c().execute("DELETE FROM github_webhook_deliveries WHERE received_at < ?1", params![before])
    }
}

/// What deleting an account takes from the mirror, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// its drafts and inbox, and its name as who linked a repo. A repo linked
/// to a project that went with it is unlinked now, and M163's purge job
/// takes its rows. The token log keeps its mints (`tables.toml`).
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted, now: u64) -> anyhow::Result<()> {
    let account = &d.account;
    c.execute("DELETE FROM drafts WHERE author = ?1 OR project_id NOT IN (SELECT id FROM projects)", params![account])?;
    c.execute("DELETE FROM inbox_items WHERE account = ?1", params![account])?;
    c.execute("UPDATE linked_repos SET linked_by = NULL WHERE linked_by = ?1", params![account])?;
    c.execute(
        "UPDATE linked_repos SET unlinked_at = ?1, purge_after = ?1
         WHERE unlinked_at IS NULL AND project_id NOT IN (SELECT id FROM projects)",
        params![now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        Db,
        sql::{Sql, params},
    };
    use crate::Deleted;

    /// A delivery is taken once, until it's forgotten or a week old.
    #[test]
    fn deliveries_once_for_a_week() {
        let db = Db::test();
        assert!(db.first_delivery("d1", "pull_request", 1_000).unwrap());
        assert!(!db.first_delivery("d1", "pull_request", 2_000).unwrap(), "a redelivery");
        db.forget_delivery("d1").unwrap();
        assert!(db.first_delivery("d1", "pull_request", 3_000).unwrap(), "forgotten: taken again");
        assert!(db.first_delivery("d2", "issues", 3_000 + super::DELIVERIES_KEPT_MS).unwrap());
        assert_eq!(db.prune_deliveries(3_001 + super::DELIVERIES_KEPT_MS).unwrap(), 1);
        assert!(db.first_delivery("d1", "pull_request", 5_000).unwrap(), "a week on, it's new");
        assert!(!db.first_delivery("d2", "issues", 5_000).unwrap());
    }

    fn count(db: &Db, sql: &str) -> u64 {
        db.c().query_row(sql, params![], |r| r.get(0)).unwrap()
    }

    /// Deleting an account takes its drafts and inbox and its name on a
    /// link, and unlinks what was linked to its personal project, for the
    /// purge. A team's repo stays linked, and others' drafts stay.
    #[test]
    fn deleting_an_account_takes_its_drafts_and_inbox() {
        let db = Db::test();
        let mine = db.personal_project("alice", 1).unwrap();
        db.c()
            .execute_batch(&format!(
                "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES ('prj_t', 't1', 't1', TRUE, 1);
                 INSERT INTO linked_repos (repo_id, full_name, project_id, installation_id, linked_by, linked_at)
                     VALUES (1, 'octo/dots', '{mine}', 9, 'alice', 1), (2, 'acme/app', 'prj_t', 9, 'alice', 1);
                 INSERT INTO drafts (id, project_id, author, repo_id, number, kind, body, created_at, updated_at)
                     VALUES ('dft_a', 'prj_t', 'alice', 2, 7, 'comment', 'lgtm', 1, 1),
                            ('dft_b', 'prj_t', 'bob', 2, 7, 'comment', 'nit', 1, 1);
                 INSERT INTO inbox_items (account, item, reason, created_at, updated_at)
                     VALUES ('alice', 'fi_1', 'review_requested', 1, 1), ('bob', 'fi_1', 'review_requested', 1, 1);"
            ))
            .unwrap();
        let gone = Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
        for _ in 0..2 {
            let mut c = db.c();
            let tx = c.transaction().unwrap();
            super::super::projects::erase_account(&tx, &gone).unwrap();
            super::erase_account(&tx, &gone, 50).unwrap();
            tx.commit().unwrap();
        }
        assert!(db.mentions("alice").is_empty(), "{:?}", db.mentions("alice"));
        assert_eq!(count(&db, "SELECT count(*) FROM drafts WHERE author = 'bob'"), 1);
        assert_eq!(count(&db, "SELECT count(*) FROM inbox_items"), 1);
        assert_eq!(count(&db, "SELECT count(*) FROM linked_repos WHERE purge_after = 50"), 1, "its own repo");
        assert_eq!(count(&db, "SELECT count(*) FROM linked_repos WHERE repo_id = 2 AND unlinked_at IS NULL"), 1);
    }
}
