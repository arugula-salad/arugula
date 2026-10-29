//! Public previews' rows (M187): `hosted_ports`' public columns, the id
//! made by migration `0024`. A port is public while `public_until` is
//! ahead; nothing needs to run when it passes.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Row, Sql, params},
};

/// How long a port stays public after it's made public or renewed.
pub const PUBLIC_FOR_MS: u64 = 7 * 86_400_000;

pub use arugula_control_wire::internal::PublicPreview;

const COLUMNS: &str =
    "p.public_id, p.workspace_id, w.project_id, w.machine, p.port, w.state, p.public_by, p.public_until";

fn row(r: &Row) -> Result<PublicPreview> {
    Ok(PublicPreview {
        id: r.get(0)?,
        workspace: r.get(1)?,
        project: r.get(2)?,
        machine: r.get(3)?,
        port: u16::try_from(r.get::<u32>(4)?)?,
        state: r.get(5)?,
        by: r.get(6)?,
        until: r.get(7)?,
    })
}

impl Db {
    /// Make a workspace's port public as `by`, until `until`, noting the
    /// port if it wasn't. A port that's public already keeps its id (a
    /// renewal); otherwise it takes `fresh`. Returns its id.
    pub fn make_port_public(
        &self,
        workspace: &str,
        port: u16,
        by: &str,
        fresh: &str,
        until: u64,
        now: u64,
    ) -> Result<String> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO hosted_ports (workspace_id, port, label, created_at) VALUES (?1, ?2, '', ?3)
             ON CONFLICT (workspace_id, port) DO NOTHING",
            params![workspace, u32::from(port), now],
        )?;
        tx.execute(
            "UPDATE hosted_ports SET
                 public_id = CASE WHEN public_id IS NOT NULL AND public_until > ?5 THEN public_id ELSE ?3 END,
                 public_by = ?4, public_until = ?6
             WHERE workspace_id = ?1 AND port = ?2",
            params![workspace, u32::from(port), fresh, by, now, until],
        )?;
        let id = tx.query_row(
            "SELECT public_id FROM hosted_ports WHERE workspace_id = ?1 AND port = ?2",
            params![workspace, u32::from(port)],
            |r| r.get::<String>(0),
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Renew a port that's public now, as `by`, until `until`: its id, or
    /// `None` when it isn't public.
    pub fn renew_port_public(
        &self,
        workspace: &str,
        port: u16,
        by: &str,
        until: u64,
        now: u64,
    ) -> Result<Option<String>> {
        self.c()
            .query_row(
                "UPDATE hosted_ports SET public_by = ?3, public_until = ?4
                 WHERE workspace_id = ?1 AND port = ?2 AND public_id IS NOT NULL AND public_until > ?5
                 RETURNING public_id",
                params![workspace, u32::from(port), by, until, now],
                |r| r.get::<String>(0),
            )
            .optional()
    }

    /// A port isn't public any more: whether it was (expired or not).
    pub fn make_port_private(&self, workspace: &str, port: u16) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE hosted_ports SET public_id = NULL, public_by = NULL, public_until = NULL
             WHERE workspace_id = ?1 AND port = ?2 AND public_id IS NOT NULL",
            params![workspace, u32::from(port)],
        )? == 1)
    }

    /// The operator takes a preview down by its id (docs/control-ops.md,
    /// "Public previews"): whether there was one.
    pub fn take_down_preview(&self, id: &str) -> Result<bool> {
        take_down(&self.c(), id)
    }

    /// The preview with this id, if it's public now and its workspace
    /// isn't deleted.
    pub fn public_preview(&self, id: &str, now: u64) -> Result<Option<PublicPreview>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM hosted_ports p JOIN hosted_workspaces w ON w.id = p.workspace_id
                     WHERE p.public_id = ?1 AND p.public_until > ?2 AND w.deleted_at IS NULL"
                ),
                params![id, now],
                row,
            )
            .optional()
    }

    /// A workspace's ports that are public now, and their ids: what its
    /// hosted machine serves.
    pub fn public_ports(&self, workspace: &str, now: u64) -> Result<Vec<(u16, String)>> {
        self.c().query_map(
            "SELECT port, public_id FROM hosted_ports
             WHERE workspace_id = ?1 AND public_id IS NOT NULL AND public_until > ?2 ORDER BY port",
            params![workspace, now],
            |r| Ok((u16::try_from(r.get::<u32>(0)?)?, r.get(1)?)),
        )
    }

    /// Every preview that's public now, for the operator.
    pub fn public_previews(&self, now: u64) -> Result<Vec<PublicPreview>> {
        public_previews(&self.c(), now)
    }
}

pub(crate) fn take_down(c: &impl Sql, id: &str) -> Result<bool> {
    Ok(c.execute(
        "UPDATE hosted_ports SET public_id = NULL, public_by = NULL, public_until = NULL WHERE public_id = ?1",
        params![id],
    )? == 1)
}

pub(crate) fn public_previews(c: &impl Sql, now: u64) -> Result<Vec<PublicPreview>> {
    c.query_map(
        &format!(
            "SELECT {COLUMNS} FROM hosted_ports p JOIN hosted_workspaces w ON w.id = p.workspace_id
             WHERE p.public_id IS NOT NULL AND p.public_until > ?1 AND w.deleted_at IS NULL
             ORDER BY p.public_until"
        ),
        params![now],
        row,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::hosted::Added, hosted::Spec};

    const DAY: u64 = 86_400_000;

    fn workspace(db: &Db) {
        assert!(matches!(
            db.add_hosted_workspace("hw_1", "prj_a", "dev", &Spec::default(), "alice", 5, 0).unwrap(),
            Added::Made(_)
        ));
    }

    /// Made public: listed for its machine and found by its id, with who
    /// did it; renewed, it keeps its id; turned off, it's gone at once.
    #[test]
    fn public_renewed_and_off() {
        let db = Db::test();
        workspace(&db);
        let id = db.make_port_public("hw_1", 5173, "alice", "aaaa", 7 * DAY, 0).unwrap();
        assert_eq!(id, "aaaa");
        assert_eq!(db.public_ports("hw_1", 1).unwrap(), [(5173, "aaaa".to_owned())]);
        let p = db.public_preview("aaaa", 1).unwrap().unwrap();
        assert_eq!((p.port, p.by.as_deref(), p.until, p.project.as_str()), (5173, Some("alice"), 7 * DAY, "prj_a"));
        assert_eq!(db.hosted_ports("hw_1").unwrap(), [(5173, String::new(), Some("alice".into()), Some(7 * DAY))]);

        // Made public again while it is: a renewal, same id.
        assert_eq!(db.make_port_public("hw_1", 5173, "bob", "bbbb", 8 * DAY, DAY).unwrap(), "aaaa");
        assert_eq!(db.renew_port_public("hw_1", 5173, "carol", 9 * DAY, 2 * DAY).unwrap().as_deref(), Some("aaaa"));
        assert_eq!(db.public_preview("aaaa", 2 * DAY).unwrap().unwrap().by.as_deref(), Some("carol"));
        assert_eq!(db.renew_port_public("hw_1", 3000, "carol", 9 * DAY, 2 * DAY).unwrap(), None);

        assert!(db.make_port_private("hw_1", 5173).unwrap());
        assert!(!db.make_port_private("hw_1", 5173).unwrap());
        assert_eq!(db.public_preview("aaaa", 2 * DAY).unwrap(), None);
        assert!(db.public_ports("hw_1", 2 * DAY).unwrap().is_empty());
        // Public again: a new id, so the old link stays dead.
        assert_eq!(db.make_port_public("hw_1", 5173, "alice", "cccc", 10 * DAY, 3 * DAY).unwrap(), "cccc");
    }

    /// Seven days on, it isn't public, with nothing run; renewing it then
    /// is refused, and making it public again gives a new id.
    #[test]
    fn it_expires() {
        let db = Db::test();
        workspace(&db);
        db.make_port_public("hw_1", 80, "alice", "aaaa", PUBLIC_FOR_MS, 0).unwrap();
        assert!(db.public_preview("aaaa", PUBLIC_FOR_MS - 1).unwrap().is_some());
        assert_eq!(db.public_preview("aaaa", PUBLIC_FOR_MS).unwrap(), None);
        assert!(db.public_ports("hw_1", PUBLIC_FOR_MS).unwrap().is_empty());
        assert!(db.public_previews(PUBLIC_FOR_MS).unwrap().is_empty());
        assert_eq!(db.renew_port_public("hw_1", 80, "alice", 2 * PUBLIC_FOR_MS, PUBLIC_FOR_MS).unwrap(), None);
        assert_eq!(db.make_port_public("hw_1", 80, "alice", "bbbb", 2 * PUBLIC_FOR_MS, PUBLIC_FOR_MS).unwrap(), "bbbb");
    }

    /// The operator takes one down by its id; a deleted workspace's
    /// previews are gone with it.
    #[test]
    fn taken_down_and_deleted() {
        let db = Db::test();
        workspace(&db);
        db.make_port_public("hw_1", 80, "alice", "aaaa", DAY, 0).unwrap();
        db.make_port_public("hw_1", 81, "alice", "bbbb", DAY, 0).unwrap();
        assert_eq!(db.public_previews(1).unwrap().len(), 2);
        assert!(db.take_down_preview("aaaa").unwrap());
        assert!(!db.take_down_preview("aaaa").unwrap());
        assert_eq!(db.public_previews(1).unwrap().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["bbbb"]);
        db.c().execute("UPDATE hosted_workspaces SET deleted_at = 1 WHERE id = 'hw_1'", params![]).unwrap();
        assert_eq!(db.public_preview("bbbb", 1).unwrap(), None);
    }
}
