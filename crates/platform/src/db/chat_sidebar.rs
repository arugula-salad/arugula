//! How an account arranges its chat sidebar (#120): one JSON document per
//! account in `chat_sidebars` (migration 0043). `crate::chat::sidebar`
//! checks it on the way in; this is storage.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};

impl Db {
    /// The document `account` last saved, if it ever did.
    pub fn chat_sidebar(&self, account: &str) -> Result<Option<String>> {
        self.c()
            .query_row("SELECT doc FROM chat_sidebars WHERE account = ?1", params![account], |r| r.get(0))
            .optional()
    }

    /// Replace `account`'s document (the last write wins).
    pub fn set_chat_sidebar(&self, account: &str, doc: &str, at: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_sidebars (account, doc, at) VALUES (?1, ?2, ?3)
             ON CONFLICT (account) DO UPDATE SET doc = excluded.doc, at = excluded.at",
            params![account, doc, at],
        )?;
        Ok(())
    }
}
