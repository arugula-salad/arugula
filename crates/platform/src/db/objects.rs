//! What object threads (M196, `crate::chat::objects`) read of the rest of
//! the platform's tables: whose a project is, and what a global id names (its project, and whether more than
//! membership guards it). Storage only: who sees what is decided in
//! `crate::chat`.

use std::collections::HashMap;

use super::{
    Db,
    chat::{in_json, json_list},
    sql::{Sql, params},
};

/// Whose a project is: one account (a personal project) or one team.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holder {
    pub account: Option<String>,
    pub team: Option<String>,
}

/// A mirrored GitHub PR or issue (step 4's `forge_items`), with its repo's
/// link: what its thread needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirrored {
    pub id: String,
    /// `pr` or `issue`.
    pub kind: String,
    pub number: u64,
    pub repo_id: u64,
    pub full_name: String,
    pub private: bool,
    pub project_id: String,
    pub unlinked: bool,
}

impl Db {
    /// Whose each of these projects is, by id (one that isn't there isn't
    /// in the map). One statement.
    pub fn project_holders(&self, ids: &[String]) -> anyhow::Result<HashMap<String, Holder>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT id, account_id, team_id FROM projects WHERE {}", in_json(&c, "id", 1)),
            params![json_list(ids)?],
            |r| Ok((r.get::<String>(0)?, Holder { account: r.get(1)?, team: r.get(2)? })),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// The native issues with these ids (`iss_…`, M194): each one's id,
    /// project and number. One statement; an id that isn't there isn't in
    /// the list.
    pub fn issue_places(&self, ids: &[&str]) -> anyhow::Result<Vec<(String, String, u64)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!("SELECT id, project_id, number FROM issues WHERE {}", in_json(&c, "id", 1)),
            params![json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    /// The runs with these ids (`run_…`, M210): each one's id and project.
    /// One statement; an id that isn't there isn't in the list.
    pub fn run_places(&self, ids: &[&str]) -> anyhow::Result<Vec<(String, String)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!("SELECT id, project_id FROM runs WHERE {}", in_json(&c, "id", 1)),
            params![json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The live hosted workspaces with these ids (`hw_…`, M179): each one's
    /// id, project and name. One statement; an id that isn't there, or
    /// names a deleted workspace, isn't in the list.
    pub fn hosted_places(&self, ids: &[&str]) -> anyhow::Result<Vec<(String, String, String)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT id, project_id, name FROM hosted_workspaces WHERE deleted_at IS NULL AND {}",
                in_json(&c, "id", 1)
            ),
            params![json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    /// The mirrored items with these ids (`fi_…`), with their repos' links.
    /// One statement; an id that isn't there isn't in the list.
    pub fn mirrored_items(&self, ids: &[&str]) -> anyhow::Result<Vec<Mirrored>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT f.id, f.kind, f.number, f.repo_id, r.full_name, r.private, r.project_id, r.unlinked_at
                 FROM forge_items f JOIN linked_repos r ON r.repo_id = f.repo_id WHERE {}",
                in_json(&c, "f.id", 1)
            ),
            params![json_list(ids)?],
            |r| {
                Ok(Mirrored {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    number: r.get(2)?,
                    repo_id: r.get(3)?,
                    full_name: r.get(4)?,
                    private: r.get(5)?,
                    project_id: r.get(6)?,
                    unlinked: r.get::<Option<u64>>(7)?.is_some(),
                })
            },
        )
    }
}

/// The threads on a repo's mirrored items, and all that's in them: the
/// forge mirror's purge of the repo's rows (M163) takes them too, before
/// its items.
pub(super) fn drop_item_threads(tx: &impl Sql, repo_id: u64) -> anyhow::Result<()> {
    super::chat::drop_rooms_where(
        tx,
        "kind = 'object' AND target IN (SELECT id FROM forge_items WHERE repo_id = ?1)",
        repo_id,
    )
}

/// The thread on a hosted workspace, and all that's in it: a workspace
/// deleted for good takes it ([`Db::checkpoint_hosted`]).
pub(super) fn drop_workspace_thread(tx: &impl Sql, workspace: &str) -> anyhow::Result<()> {
    super::chat::drop_rooms_where(tx, "kind = 'object' AND target = ?1", workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project of `account`'s or `team`'s, and a linked repo in it with
    /// one PR.
    fn seed(db: &Db) {
        let c = db.c();
        c.execute(
            "INSERT INTO projects (id, account_id, team_id, name, is_default, created_at) VALUES
                 ('prj_a', 'a', NULL, 'Personal', TRUE, 1), ('prj_t', NULL, 't', 't', TRUE, 1),
                 ('prj_u', NULL, 'u', 'u', TRUE, 1)",
            params![],
        )
        .unwrap();
        c.execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_at, unlinked_at)
             VALUES (7, 'o/public', FALSE, 'prj_t', 1, 1, NULL), (8, 'o/secret', TRUE, 'prj_t', 1, 1, NULL),
                    (9, 'o/gone', FALSE, 'prj_t', 1, 1, 5)",
            params![],
        )
        .unwrap();
        c.execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES ('fi_pub', 7, 3, 'pr', 'open', 'fix it', 1, 1, 1),
                    ('fi_sec', 8, 4, 'issue', 'open', 'secret', 1, 1, 1),
                    ('fi_gone', 9, 5, 'pr', 'open', 'old', 1, 1, 1)",
            params![],
        )
        .unwrap();
    }

    #[test]
    fn projects_their_holders_issues_and_mirrored_items() {
        let db = Db::test();
        seed(&db);
        let h = db.project_holders(&["prj_a".into(), "prj_t".into(), "prj_none".into()]).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h["prj_a"], Holder { account: Some("a".into()), team: None });
        assert_eq!(h["prj_t"], Holder { account: None, team: Some("t".into()) });

        let n = super::super::issues::NewIssue { project: "prj_t", title: "x", body: "", labels: &[], assignees: &[] };
        let issue = db.create_issue(&n, super::super::issues::Actor::person("a"), 1).unwrap().unwrap();
        assert_eq!(db.issue_places(&[issue.id.as_str(), "iss_none"]).unwrap(), [(issue.id.clone(), "prj_t".into(), 1)]);
        assert!(db.issue_places(&[]).unwrap().is_empty());

        let mut items = db.mirrored_items(&["fi_pub", "fi_sec", "fi_gone", "fi_none"]).unwrap();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let got: Vec<_> =
            items.iter().map(|m| (m.id.as_str(), m.kind.as_str(), m.number, m.private, m.unlinked)).collect();
        assert_eq!(
            got,
            [("fi_gone", "pr", 5, false, true), ("fi_pub", "pr", 3, false, false), ("fi_sec", "issue", 4, true, false)]
        );
        assert_eq!(
            (items[1].full_name.as_str(), items[1].project_id.as_str(), items[1].repo_id),
            ("o/public", "prj_t", 7)
        );
    }

    /// One room per object in a project: a second room on the same target
    /// is taken, and another project (a relinked repo's) has its own.
    #[test]
    fn one_room_per_object() {
        let db = Db::test();
        let room = |id: &str| super::super::chat::Room {
            id: id.into(),
            scope: "project:prj_t".into(),
            kind: "object".into(),
            name: "PR o/public#3".into(),
            machine: None,
            target: Some("fi_pub".into()),
            created_by: "a".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        db.add_chat_room(&room("ob_1"), &[], Some("prj_t")).unwrap();
        let e = db.add_chat_room(&room("ob_2"), &[], Some("prj_t")).unwrap_err();
        assert!(e.is::<super::super::Taken>(), "{e}");
        assert_eq!(db.chat_room_on("project:prj_t", "fi_pub").unwrap().unwrap().id, "ob_1");
        assert!(db.chat_room_on("project:prj_t", "fi_none").unwrap().is_none());
        assert!(db.chat_room_on("project:prj_u", "fi_pub").unwrap().is_none());
        let moved = super::super::chat::Room { scope: "project:prj_u".into(), ..room("ob_3") };
        db.add_chat_room(&moved, &[], Some("prj_u")).unwrap();
        assert_eq!(db.chat_room_on("project:prj_u", "fi_pub").unwrap().unwrap().id, "ob_3");
        assert_eq!(db.chat_room_on("project:prj_t", "fi_pub").unwrap().unwrap().id, "ob_1");
    }

    /// A chat list's rooms take in the threads on objects in the
    /// account's own projects and its teams' (by their scopes), and no
    /// others.
    #[test]
    fn a_chat_list_has_its_projects_object_threads() {
        let db = Db::test();
        seed(&db);
        let room = |id: &str, project: &str| super::super::chat::Room {
            id: id.into(),
            scope: format!("project:{project}"),
            kind: "object".into(),
            name: id.into(),
            machine: None,
            target: Some(format!("fi_{id}")),
            created_by: "a".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        for (id, project) in [("mine", "prj_a"), ("ours", "prj_t"), ("theirs", "prj_u")] {
            db.add_chat_room(&room(id, project), &[], Some(project)).unwrap();
        }
        let seen = |account: &str, teams: &[&str]| {
            let scopes: Vec<String> = teams.iter().map(|t| format!("team:{t}")).collect();
            let mut ids: Vec<String> =
                db.chat_rooms_seen(&scopes, &[], account).unwrap().into_iter().map(|r| r.id).collect();
            ids.sort();
            ids
        };
        assert_eq!(seen("a", &["t"]), ["mine", "ours"]);
        assert_eq!(seen("b", &["t"]), ["ours"]);
        assert_eq!(seen("c", &["u"]), ["theirs"]);
        assert!(seen("d", &[]).is_empty());
    }

    /// Deleting an account takes the threads on its personal projects'
    /// objects with them, and leaves its teams'.
    #[test]
    fn an_accounts_object_threads_go_with_its_projects() {
        let db = Db::test();
        seed(&db);
        let room = |id: &str, project: &str, target: &str| super::super::chat::Room {
            id: id.into(),
            scope: format!("project:{project}"),
            kind: "object".into(),
            name: id.into(),
            machine: None,
            target: Some(target.into()),
            created_by: "a".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        db.add_chat_room(&room("ob_mine", "prj_a", "fi_x"), &[], Some("prj_a")).unwrap();
        db.add_chat_room(&room("ob_team", "prj_t", "fi_pub"), &[], Some("prj_t")).unwrap();
        let gone = crate::Deleted { account: "a".into(), machines: vec![], disbanded: vec![] };
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        super::super::projects::erase_account(&tx, &gone).unwrap();
        super::super::chat::erase_account(&tx, "a", &[], &[]).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert!(db.chat_room("ob_mine").unwrap().is_none());
        assert!(db.chat_room("ob_team").unwrap().is_some());
    }
}
