//! The forge mirror's tables (M162, platform step 4, #1204): linked repos'
//! PRs and issues as platform rows. They're made by `0005_forge_mirror`;
//! their queries come with the features that write them (M161 deliveries,
//! M163 links, M164 items, M167 mints, M170 drafts, M171 the inbox). So
//! far: the webhook's deliveries (M161), the token log (M167), an
//! account's deletion, and linked repos, the App's installations and the
//! purge (M163).

use serde::Serialize;

use super::{
    Db,
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// How long an unlinked repo's rows stay before the purge takes them
/// (M163): "everything is purged within 24 hours of an uninstall", and the
/// purge runs every few minutes, so a little under a day.
pub const PURGE_GRACE_MS: u64 = 23 * 3600 * 1000;

/// A repo linked to a project, or unlinked and waiting for the purge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedRepo {
    pub repo_id: u64,
    pub full_name: String,
    pub private: bool,
    pub project_id: String,
    pub installation_id: u64,
    pub linked_by: Option<String>,
    pub linked_at: u64,
    pub unlinked_at: Option<u64>,
    pub purge_after: Option<u64>,
}

const LINK_COLUMNS: &str =
    "repo_id, full_name, private, project_id, installation_id, linked_by, linked_at, unlinked_at, purge_after";

fn link_row(r: &Row) -> anyhow::Result<LinkedRepo> {
    Ok(LinkedRepo {
        repo_id: r.get(0)?,
        full_name: r.get(1)?,
        private: r.get(2)?,
        project_id: r.get(3)?,
        installation_id: r.get(4)?,
        linked_by: r.get(5)?,
        linked_at: r.get(6)?,
        unlinked_at: r.get(7)?,
        purge_after: r.get(8)?,
    })
}

/// Where the App is installed, as GitHub says it in an `installation`
/// event or `GET /repos/{repo}/installation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub id: u64,
    pub account_id: u64,
    pub login: String,
    /// `User` or `Organization`.
    pub account_type: String,
    /// The permissions it was granted, as GitHub's JSON object.
    pub permissions: String,
}

/// What linking did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linked {
    /// Linked now. `again`: it was unlinked from this project and waiting
    /// for the purge, which it no longer is.
    Now { repo: LinkedRepo, again: bool },
    /// It's linked to another project already: a repo is in one project.
    Elsewhere,
}

/// What a claim's `SELECT` of a repo to purge ends with: on Postgres it
/// passes over one another machine is purging (`cloud_jobs`' way, M144).
fn skip_locked(c: &impl Sql) -> &'static str {
    match c.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
    }
}

/// Every row the mirror holds of a repo, but its link: its items and
/// what's on them, its drafts, and inbox rows of its items.
fn purge_rows(c: &impl Sql, repo_id: u64) -> anyhow::Result<()> {
    c.execute(
        "DELETE FROM inbox_items WHERE item IN (SELECT id FROM forge_items WHERE repo_id = ?1)",
        params![repo_id],
    )?;
    for table in ["forge_items", "forge_checks", "forge_events", "forge_content", "drafts"] {
        c.execute(&format!("DELETE FROM {table} WHERE repo_id = ?1"), params![repo_id])?;
    }
    Ok(())
}

/// Unlink every live link `filter` picks (a `WHERE` on `linked_repos`
/// whose parameters start at `?3`): how many.
fn unlink_where(c: &impl Sql, filter: &str, p: &[&dyn super::sql::Param], now: u64) -> anyhow::Result<usize> {
    let after = now + PURGE_GRACE_MS;
    let mut all: Vec<&dyn super::sql::Param> = vec![&now, &after];
    all.extend_from_slice(p);
    c.execute(
        &format!("UPDATE linked_repos SET unlinked_at = ?1, purge_after = ?2 WHERE unlinked_at IS NULL AND {filter}"),
        &all,
    )
}

/// How long a webhook delivery is remembered (M161): GitHub redelivers by
/// hand for a few days, and a deploy loses nothing.
pub const DELIVERIES_KEPT_MS: u64 = 7 * 24 * 3600 * 1000;

/// How long the token log keeps a mint (M167): a year, so a token GitHub's
/// own audit log names can be traced to who and what it was for.
pub const MINTS_KEPT_MS: u64 = 365 * 24 * 3600 * 1000;

/// Whose a minted token was: the App's installation's, or a person's user
/// token (M168).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MintKind {
    Installation,
    User,
}

/// A mint to write down: never the token.
#[derive(Debug, Clone, Copy)]
pub struct NewMint<'a> {
    pub kind: MintKind,
    pub installation_id: Option<u64>,
    pub repo_id: Option<u64>,
    /// What it was scoped to, as GitHub was asked.
    pub permissions: &'a serde_json::Value,
    /// The account it was for, or `platform`.
    pub caller: &'a str,
    /// What for (`read`, `ingest`, `send:issues`, ...).
    pub purpose: &'a str,
    pub at: u64,
}

/// A row of `github_token_mints`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenMint {
    pub id: String,
    pub kind: String,
    pub installation_id: Option<u64>,
    pub repo_id: Option<u64>,
    pub permissions: String,
    pub caller: String,
    pub purpose: String,
    pub at: u64,
}

fn mint_row(r: &Row) -> anyhow::Result<TokenMint> {
    Ok(TokenMint {
        id: r.get(0)?,
        kind: r.get(1)?,
        installation_id: r.get(2)?,
        repo_id: r.get(3)?,
        permissions: r.get(4)?,
        caller: r.get(5)?,
        purpose: r.get(6)?,
        at: r.get(7)?,
    })
}

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

    /// Write a GitHub token's mint to the log (M167): its id (`gtm_`).
    pub fn log_mint(&self, m: &NewMint<'_>) -> anyhow::Result<String> {
        let id = format!("gtm_{}", hex::encode(arugula_e2e::random::<16>()));
        let kind = match m.kind {
            MintKind::Installation => "installation",
            MintKind::User => "user",
        };
        self.c().execute(
            "INSERT INTO github_token_mints (id, kind, installation_id, repo_id, permissions, caller, purpose, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, kind, m.installation_id, m.repo_id, m.permissions.to_string(), m.caller, m.purpose, m.at],
        )?;
        Ok(id)
    }

    /// The log's mints since `since`, oldest first.
    pub fn token_mints(&self, since: u64) -> anyhow::Result<Vec<TokenMint>> {
        self.c().query_map(
            "SELECT id, kind, installation_id, repo_id, permissions, caller, purpose, at
             FROM github_token_mints WHERE at >= ?1 ORDER BY at, id",
            params![since],
            mint_row,
        )
    }

    /// Forget mints older than [`MINTS_KEPT_MS`]: how many went.
    pub fn prune_mints(&self, now: u64) -> anyhow::Result<usize> {
        let before = now.saturating_sub(MINTS_KEPT_MS);
        self.c().execute("DELETE FROM github_token_mints WHERE at < ?1", params![before])
    }

    /// A project's linked repos, and those unlinked from it that wait for
    /// the purge, by name.
    pub fn linked_repos(&self, project: &str) -> anyhow::Result<Vec<LinkedRepo>> {
        self.c().query_map(
            &format!(
                "SELECT {LINK_COLUMNS} FROM linked_repos WHERE project_id = ?1 ORDER BY lower(full_name), repo_id"
            ),
            params![project],
            link_row,
        )
    }

    /// Whose a project is: its account's, or its team's. `None` when
    /// there's no such project.
    pub fn project_holder(&self, project: &str) -> anyhow::Result<Option<(Option<String>, Option<String>)>> {
        self.c()
            .query_row("SELECT account_id, team_id FROM projects WHERE id = ?1", params![project], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
    }

    /// The link of a repo, by GitHub's id, live or waiting for the purge.
    pub fn linked_repo(&self, repo_id: u64) -> anyhow::Result<Option<LinkedRepo>> {
        self.c()
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM linked_repos WHERE repo_id = ?1"),
                params![repo_id],
                link_row,
            )
            .optional()
    }

    /// Link a repo to a project, from `installation`, as `by`. A repo
    /// unlinked from this project and waiting for the purge is linked
    /// again, and keeps its rows. One unlinked from another project loses
    /// them now (they were that project's), then is linked here. One
    /// linked to another project stays there ([`Linked::Elsewhere`]).
    pub fn link_repo(&self, l: &LinkedRepo, installation: &Installation, by: &str, now: u64) -> anyhow::Result<Linked> {
        let mut c = self.c();
        let tx = c.immediate()?;
        // Kept as it is when the App's events brought it already.
        tx.execute(
            "INSERT INTO github_installations
                 (installation_id, github_account_id, github_login, github_account_type, permissions, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (installation_id) DO NOTHING",
            params![
                installation.id,
                installation.account_id,
                installation.login,
                installation.account_type,
                installation.permissions,
                now
            ],
        )?;
        let was = tx
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM linked_repos WHERE repo_id = ?1{}", tx.for_update()),
                params![l.repo_id],
                link_row,
            )
            .optional()?;
        let again = match &was {
            Some(w) if w.project_id != l.project_id && w.unlinked_at.is_none() => return Ok(Linked::Elsewhere),
            Some(w) if w.project_id != l.project_id => {
                purge_rows(&tx, w.repo_id)?;
                tx.execute("DELETE FROM linked_repos WHERE repo_id = ?1", params![w.repo_id])?;
                false
            }
            Some(w) => w.unlinked_at.is_some(),
            None => false,
        };
        let Some(repo) = upsert_link(&tx, l, installation.id, by, now)? else { return Ok(Linked::Elsewhere) };
        tx.commit()?;
        Ok(Linked::Now { repo, again })
    }

    /// Unlink a project's repo, by name (any case): ingest stops now, and
    /// its rows go at the purge ([`PURGE_GRACE_MS`]). The link, or `None`
    /// when the project has no live link by that name.
    pub fn unlink_repo(&self, project: &str, full_name: &str, now: u64) -> anyhow::Result<Option<LinkedRepo>> {
        let c = self.c();
        let n = unlink_where(&c, "project_id = ?3 AND lower(full_name) = lower(?4)", params![project, full_name], now)?;
        if n == 0 {
            return Ok(None);
        }
        c.query_row(
            &format!(
                "SELECT {LINK_COLUMNS} FROM linked_repos WHERE project_id = ?1 AND lower(full_name) = lower(?2)
                 AND unlinked_at = ?3"
            ),
            params![project, full_name, now],
            link_row,
        )
        .optional()
    }

    /// The App is installed somewhere, or an installation changed (new
    /// permissions, unsuspended): keep it, not suspended or deleted.
    pub fn installation_seen(&self, i: &Installation, now: u64) -> anyhow::Result<()> {
        upsert_installation(&self.c(), i, now)
    }

    /// An installation was deleted (`deleted`) or suspended: noted, and
    /// every repo linked through it unlinked. How many were.
    pub fn installation_gone(&self, i: &Installation, deleted: bool, now: u64) -> anyhow::Result<usize> {
        let mut c = self.c();
        let tx = c.immediate()?;
        upsert_installation(&tx, i, now)?;
        let column = if deleted { "deleted_at" } else { "suspended_at" };
        tx.execute(
            &format!("UPDATE github_installations SET {column} = ?1, updated_at = ?1 WHERE installation_id = ?2"),
            params![now, i.id],
        )?;
        let n = unlink_where(&tx, "installation_id = ?3", params![i.id], now)?;
        tx.commit()?;
        Ok(n)
    }

    /// Repos were taken out of an installation: each one linked through it
    /// is unlinked. How many were.
    pub fn repos_removed(&self, installation: u64, repo_ids: &[u64], now: u64) -> anyhow::Result<usize> {
        let mut n = 0;
        let mut c = self.c();
        let tx = c.immediate()?;
        for id in repo_ids {
            n += unlink_where(&tx, "installation_id = ?3 AND repo_id = ?4", params![installation, id], now)?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Purge one unlinked repo whose `purge_after` has passed and whose
    /// poller holds no lease (M164): every row of it, then its link, in
    /// one transaction. Another machine purging at the same time passes
    /// over it. The repo purged, or `None` when none is due.
    pub fn purge_one(&self, now: u64) -> anyhow::Result<Option<u64>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let due = tx
            .query_row(
                &format!(
                    "SELECT repo_id FROM linked_repos
                     WHERE purge_after IS NOT NULL AND purge_after <= ?1 AND unlinked_at IS NOT NULL AND lease_until <= ?1
                     ORDER BY purge_after LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| r.get::<u64>(0),
            )
            .optional()?;
        let Some(repo_id) = due else { return Ok(None) };
        purge_rows(&tx, repo_id)?;
        tx.execute("DELETE FROM linked_repos WHERE repo_id = ?1", params![repo_id])?;
        tx.commit()?;
        Ok(Some(repo_id))
    }
}

/// Write `l`'s link, or link it again: it keeps what its poller did (M164)
/// and whether the project mirrors its content. A row of another project's
/// is left as it is, and the answer is `None`: on Postgres, another
/// transaction can link a repo nobody had between [`Db::link_repo`]'s
/// look and this write (its `FOR UPDATE` locks no row that isn't there),
/// and the repo stays with the project that linked it first.
fn upsert_link(
    c: &impl Sql,
    l: &LinkedRepo,
    installation: u64,
    by: &str,
    now: u64,
) -> anyhow::Result<Option<LinkedRepo>> {
    c.execute(
        "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_by, linked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (repo_id) DO UPDATE SET full_name = ?2, private = ?3, installation_id = ?5,
             linked_by = CASE WHEN linked_repos.unlinked_at IS NULL THEN linked_repos.linked_by ELSE ?6 END,
             linked_at = CASE WHEN linked_repos.unlinked_at IS NULL THEN linked_repos.linked_at ELSE ?7 END,
             unlinked_at = NULL, purge_after = NULL
         WHERE linked_repos.project_id = ?4",
        params![l.repo_id, l.full_name, l.private, l.project_id, installation, by, now],
    )?;
    let repo = c.query_row(
        &format!("SELECT {LINK_COLUMNS} FROM linked_repos WHERE repo_id = ?1"),
        params![l.repo_id],
        link_row,
    )?;
    Ok((repo.project_id == l.project_id).then_some(repo))
}

/// An installation as GitHub says it now, not suspended or deleted.
fn upsert_installation(c: &impl Sql, i: &Installation, now: u64) -> anyhow::Result<()> {
    c.execute(
        "INSERT INTO github_installations
             (installation_id, github_account_id, github_login, github_account_type, permissions, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (installation_id) DO UPDATE SET github_account_id = ?2, github_login = ?3,
             github_account_type = ?4, permissions = ?5, suspended_at = NULL, deleted_at = NULL, updated_at = ?6",
        params![i.id, i.account_id, i.login, i.account_type, i.permissions, now],
    )?;
    Ok(())
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

    /// A mint is a row with what it was scoped to, and a year on it goes.
    #[test]
    fn mints_are_logged_for_a_year() {
        use super::{MINTS_KEPT_MS, MintKind, NewMint};
        let db = Db::test();
        let perms = serde_json::json!({ "metadata": "read" });
        let m = NewMint {
            kind: MintKind::Installation,
            installation_id: Some(11),
            repo_id: Some(101),
            permissions: &perms,
            caller: "a1",
            purpose: "read",
            at: 1_000,
        };
        let id = db.log_mint(&m).unwrap();
        assert!(id.starts_with("gtm_"), "{id}");
        db.log_mint(&NewMint { kind: MintKind::User, installation_id: None, at: 2_000, ..m }).unwrap();
        let rows = db.token_mints(0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].kind.as_str(), rows[0].installation_id, rows[0].repo_id, rows[0].permissions.as_str()),
            ("installation", Some(11), Some(101), r#"{"metadata":"read"}"#)
        );
        assert_eq!((rows[1].kind.as_str(), rows[1].installation_id), ("user", None));
        assert_eq!(db.prune_mints(1_500 + MINTS_KEPT_MS).unwrap(), 1);
        assert_eq!(db.token_mints(0).unwrap().len(), 1);
    }

    fn link(repo: u64, project: &str) -> super::LinkedRepo {
        super::LinkedRepo {
            repo_id: repo,
            full_name: format!("acme/r{repo}"),
            private: false,
            project_id: project.into(),
            installation_id: 9,
            linked_by: None,
            linked_at: 0,
            unlinked_at: None,
            purge_after: None,
        }
    }

    fn install() -> super::Installation {
        super::Installation {
            id: 9,
            account_id: 500,
            login: "acme".into(),
            account_type: "Organization".into(),
            permissions: "{}".into(),
        }
    }

    /// The race on Postgres, as the second linker meets it: it looked and
    /// found no link, then another project's commits before its write.
    /// The write leaves that link alone, and the answer is "elsewhere".
    #[test]
    fn a_link_written_meanwhile_by_another_project_stays_its() {
        let db = Db::test();
        assert!(matches!(db.link_repo(&link(1, "prj_a"), &install(), "alice", 10).unwrap(), super::Linked::Now { .. }));
        {
            let mut c = db.c();
            let tx = c.immediate().unwrap();
            assert_eq!(super::upsert_link(&tx, &link(1, "prj_b"), 9, "bob", 20).unwrap(), None);
            tx.commit().unwrap();
        }
        let kept = db.linked_repo(1).unwrap().unwrap();
        assert_eq!((kept.project_id.as_str(), kept.linked_by.as_deref(), kept.linked_at), ("prj_a", Some("alice"), 10));
        // The same project's write still links it again.
        db.unlink_repo("prj_a", "acme/r1", 30).unwrap();
        let mut c = db.c();
        let tx = c.immediate().unwrap();
        let again = super::upsert_link(&tx, &link(1, "prj_a"), 9, "carol", 40).unwrap().unwrap();
        tx.commit().unwrap();
        assert_eq!((again.unlinked_at, again.linked_by.as_deref()), (None, Some("carol")));
    }

    /// Two projects link a repo nobody had, at once, from two machines
    /// (Postgres; SQLite's writers take turns): one links it, the other is
    /// told it's elsewhere, and the link is the first's.
    #[test]
    fn two_projects_linking_at_once_one_wins() {
        let db = Db::test();
        let Some(other) = db.second() else { return };
        for repo in 100..120u64 {
            let go = std::sync::Barrier::new(2);
            let (a, b) = std::thread::scope(|s| {
                let a = s.spawn(|| {
                    go.wait();
                    db.link_repo(&link(repo, "prj_a"), &install(), "alice", 1).unwrap()
                });
                let b = s.spawn(|| {
                    go.wait();
                    other.link_repo(&link(repo, "prj_b"), &install(), "bob", 1).unwrap()
                });
                (a.join().unwrap(), b.join().unwrap())
            });
            let stored = db.linked_repo(repo).unwrap().unwrap().project_id;
            match (a, b) {
                (super::Linked::Now { repo: r, .. }, super::Linked::Elsewhere)
                | (super::Linked::Elsewhere, super::Linked::Now { repo: r, .. }) => {
                    assert_eq!(r.project_id, stored, "repo {repo}")
                }
                (a, b) => panic!("repo {repo}: {a:?} and {b:?}"),
            }
        }
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
