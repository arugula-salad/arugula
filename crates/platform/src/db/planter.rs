//! planter's import (M232): the repos it linked (`planter_imports`), so a
//! second run changes nothing and never links again what was unlinked
//! since. [`crate::planter`] has the rules.

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};

impl Db {
    /// Where an earlier run imported `repo` (as planter named it, any case)
    /// for planter's `user`, and when.
    pub fn planter_imported(&self, user: &str, repo: &str) -> anyhow::Result<Option<(String, u64)>> {
        self.c()
            .query_row(
                "SELECT project_id, imported_at FROM planter_imports WHERE planter_user = ?1 AND repo = ?2",
                params![user, repo.to_ascii_lowercase()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    /// Where any run imported GitHub's repo `repo_id`, for whichever
    /// planter, and when: the import never links one again.
    pub fn planter_imported_repo(&self, repo_id: u64) -> anyhow::Result<Option<(String, u64)>> {
        self.c()
            .query_row(
                "SELECT project_id, imported_at FROM planter_imports WHERE repo_id = ?1 ORDER BY imported_at LIMIT 1",
                params![repo_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    /// The import linked `repo` (`outcome` `linked`) or found it linked
    /// there already (`already`): kept, once.
    #[allow(clippy::too_many_arguments)]
    pub fn record_planter_import(
        &self,
        user: &str,
        repo: &str,
        repo_id: u64,
        project: &str,
        account: &str,
        outcome: &str,
        now: u64,
    ) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO planter_imports (planter_user, repo, repo_id, project_id, account_id, outcome, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT DO NOTHING",
            params![user, repo.to_ascii_lowercase(), repo_id, project, account, outcome, now],
        )?;
        Ok(())
    }

    /// The account's personal project, if it has one yet: never made here
    /// (a dry run asks).
    pub fn personal_project_of(&self, account: &str) -> anyhow::Result<Option<String>> {
        self.c()
            .query_row("SELECT id FROM projects WHERE account_id = ?1 AND is_default = TRUE", params![account], |r| {
                r.get(0)
            })
            .optional()
    }
}

/// Deleting an account takes what was imported for it.
pub(crate) fn erase_account(c: &impl Sql, account: &str) -> anyhow::Result<()> {
    c.execute("DELETE FROM planter_imports WHERE account_id = ?1", params![account])?;
    Ok(())
}
