//! The forge mirror's tables (M162, platform step 4, #1204): linked repos'
//! PRs and issues as platform rows. They're made by `0005_forge_mirror`;
//! their queries come with the features that write them (M161 deliveries,
//! M163 links, M164 items, M167 mints, M170 drafts, M171 the inbox). Only
//! an account's deletion is here so far.

use super::sql::{Sql, params};
use crate::Deleted;

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
