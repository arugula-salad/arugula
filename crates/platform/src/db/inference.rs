//! Hosted inference's rows (M185): projects' own keys (sealed), agent turns
//! on hosted boxes and the caps they count against, and the digest of the
//! egress rules each box was last given. Made by migration `0018`; what each column
//! means is there. The logic is [`crate::hosted::inference`]'s.
//!
//! And the inference broker's (M231, migration `0032`): each call it made,
//! priced, which its monthly caps on the platform's key count and the
//! project's spend shows ([`crate::hosted::broker`]).

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};
use crate::Deleted;

/// A project's own key, as anyone may see it: which provider, who set it
/// and when. Never its value, which isn't here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProjectKey {
    pub provider: String,
    pub set_by: Option<String>,
    pub updated_at: u64,
}

/// A hosted workspace's box, as a turn's start sees it from its machine.
/// The sandbox id is the worker's and this check's alone: it's never
/// answered or logged.
#[derive(Debug, Clone)]
pub struct HostedBox {
    pub workspace: String,
    pub project: String,
    pub name: String,
    pub created_by: Option<String>,
    pub sandbox: Option<String>,
    /// The digest of its egress rules as last given.
    pub rules: String,
}

fn box_row(r: &super::sql::Row) -> Result<HostedBox> {
    Ok(HostedBox {
        workspace: r.get(0)?,
        project: r.get(1)?,
        name: r.get(2)?,
        created_by: r.get(3)?,
        sandbox: r.get(4)?,
        rules: r.get(5)?,
    })
}

/// How many platform turns an account may have: at once, in a UTC day,
/// and minutes of them in a UTC day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub concurrent: u64,
    pub daily: u64,
    pub minutes: u64,
}

/// Which cap a platform turn would pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Concurrent,
    Daily,
    Minutes,
}

/// What [`Db::start_hosted_turn`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    Turn(String),
    Capped(Cap),
}

/// Whose key a turn runs on, and whose caps a platform turn counts
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pays {
    Project,
    Platform { account: String },
}

/// A turn that never said it ended counts as ended this long after it
/// started (a daemon that died mid-turn, a box deleted under it).
pub const STALE_MS: u64 = 3 * 60 * 60 * 1000;
/// How long turns are kept.
pub const KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

impl Db {
    /// A project's own keys.
    pub fn inference_keys(&self, project: &str) -> Result<Vec<ProjectKey>> {
        self.c().query_map(
            "SELECT provider, set_by, updated_at FROM project_inference_keys WHERE project_id = ?1 ORDER BY provider",
            params![project],
            |r| Ok(ProjectKey { provider: r.get(0)?, set_by: r.get(1)?, updated_at: r.get(2)? }),
        )
    }

    /// Keep a project's own key for `provider`, sealed.
    pub fn set_inference_key(
        &self,
        project: &str,
        provider: &str,
        sealed: &crate::keys::Sealed,
        by: &str,
        now: u64,
    ) -> Result<()> {
        self.c().execute(
            "INSERT INTO project_inference_keys (project_id, provider, sealed, key_version, set_by, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (project_id, provider) DO UPDATE SET sealed = excluded.sealed,
                 key_version = excluded.key_version, set_by = excluded.set_by, updated_at = excluded.updated_at",
            params![project, provider, sealed.bytes, sealed.key_version, by, now],
        )?;
        Ok(())
    }

    /// A project's own keys, sealed: each provider and its sealed key, for
    /// a box's egress rules and nothing else ([`crate::keys::open_inference`]
    /// opens one).
    pub fn sealed_inference_keys(&self, project: &str) -> Result<Vec<(String, crate::keys::Sealed)>> {
        self.c().query_map(
            "SELECT provider, key_version, sealed FROM project_inference_keys WHERE project_id = ?1 ORDER BY provider",
            params![project],
            |r| Ok((r.get(0)?, crate::keys::Sealed { key_version: r.get(1)?, bytes: r.get(2)? })),
        )
    }

    pub fn remove_inference_key(&self, project: &str, provider: &str) -> Result<bool> {
        Ok(self.c().execute(
            "DELETE FROM project_inference_keys WHERE project_id = ?1 AND provider = ?2",
            params![project, provider],
        )? == 1)
    }

    /// Drop the keys whose project is gone: how many.
    pub fn drop_orphan_inference_keys(&self) -> Result<usize> {
        self.c()
            .execute("DELETE FROM project_inference_keys WHERE project_id NOT IN (SELECT id FROM projects)", params![])
    }

    /// A project's running boxes, for a key that changed.
    pub fn running_boxes(&self, project: &str) -> Result<Vec<HostedBox>> {
        self.c().query_map(
            "SELECT id, project_id, name, created_by, sandbox_id, rules FROM hosted_workspaces
             WHERE project_id = ?1 AND deleted_at IS NULL AND state = 'ready' AND sandbox_id IS NOT NULL
             ORDER BY id",
            params![project],
            box_row,
        )
    }

    /// The live workspace `workspace`, for a box just woken.
    pub fn hosted_box_of(&self, workspace: &str) -> Result<Option<HostedBox>> {
        self.c()
            .query_row(
                "SELECT id, project_id, name, created_by, sandbox_id, rules FROM hosted_workspaces
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![workspace],
                box_row,
            )
            .optional()
    }

    /// The live workspace whose box enrolled as `machine`.
    pub fn hosted_box(&self, machine: &str) -> Result<Option<HostedBox>> {
        self.c()
            .query_row(
                "SELECT id, project_id, name, created_by, sandbox_id, rules FROM hosted_workspaces
                 WHERE machine = ?1 AND deleted_at IS NULL AND state NOT IN ('deleting', 'deleted')
                 ORDER BY created_at DESC LIMIT 1",
                params![machine],
                box_row,
            )
            .optional()
    }

    /// Note the digest of the egress rules a workspace's box was given.
    pub fn set_hosted_rules(&self, workspace: &str, digest: &str) -> Result<()> {
        self.c().execute("UPDATE hosted_workspaces SET rules = ?2 WHERE id = ?1", params![workspace, digest])?;
        Ok(())
    }

    /// Start a turn on `workspace`'s box, unless it runs on the platform's
    /// key and its account is at a cap. Checked and kept in one
    /// transaction, so two boxes starting at once can't both pass the last
    /// slot. A day is a UTC day.
    pub fn start_hosted_turn(
        &self,
        workspace: &str,
        provider: &str,
        pays: &Pays,
        caps: &Caps,
        now: u64,
    ) -> Result<Started> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let account = match pays {
            Pays::Project => None,
            Pays::Platform { account } => Some(account.as_str()),
        };
        if let Some(account) = account {
            let day = now / DAY_MS * DAY_MS;
            let (open, today, ms): (u64, u64, i64) = tx.query_row(
                "SELECT
                     CAST(COALESCE(SUM(CASE WHEN ended_at IS NULL AND started_at > ?3 THEN 1 ELSE 0 END), 0) AS BIGINT),
                     CAST(COALESCE(SUM(CASE WHEN started_at >= ?2 THEN 1 ELSE 0 END), 0) AS BIGINT),
                     CAST(COALESCE(SUM(CASE WHEN started_at >= ?2 THEN
                         (CASE WHEN COALESCE(ended_at, ?4) < started_at + ?5 THEN COALESCE(ended_at, ?4)
                               ELSE started_at + ?5 END) - started_at
                     ELSE 0 END), 0) AS BIGINT)
                 FROM hosted_turns WHERE pays = 'platform' AND account = ?1 AND started_at >= ?6",
                params![
                    account,
                    day,
                    now.saturating_sub(STALE_MS),
                    now,
                    STALE_MS,
                    day.min(now.saturating_sub(STALE_MS))
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let capped = if open >= caps.concurrent {
                Some(Cap::Concurrent)
            } else if today >= caps.daily {
                Some(Cap::Daily)
            } else if u64::try_from(ms.max(0)).unwrap_or(0) >= caps.minutes * 60_000 {
                Some(Cap::Minutes)
            } else {
                None
            };
            if let Some(cap) = capped {
                return Ok(Started::Capped(cap));
            }
        }
        let id = format!("ht_{}", hex::encode(arugula_e2e::random::<16>()));
        let paid = if account.is_some() { "platform" } else { "project" };
        tx.execute(
            "INSERT INTO hosted_turns (id, workspace_id, provider, pays, account, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, workspace, provider, paid, account, now],
        )?;
        tx.commit()?;
        Ok(Started::Turn(id))
    }

    /// A turn on `workspace`'s box ended: whether it was one of its, open.
    pub fn end_hosted_turn(&self, workspace: &str, turn: &str, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE hosted_turns SET ended_at = ?3 WHERE id = ?1 AND workspace_id = ?2 AND ended_at IS NULL",
            params![turn, workspace, now],
        )? == 1)
    }

    /// End the open turns of workspaces whose box is paused or gone (no
    /// turn runs there), and forget turns older than [`KEEP_MS`].
    pub fn tidy_hosted_turns(&self, now: u64) -> Result<()> {
        let c = self.c();
        c.execute(
            "UPDATE hosted_turns SET ended_at = ?1 WHERE ended_at IS NULL AND workspace_id IN
                 (SELECT id FROM hosted_workspaces WHERE deleted_at IS NOT NULL OR state IN ('paused', 'failed', 'deleting', 'deleted'))",
            params![now],
        )?;
        c.execute("DELETE FROM hosted_turns WHERE started_at < ?1", params![now.saturating_sub(KEEP_MS)])?;
        Ok(())
    }
}

/// A call the inference broker made (M231), as [`Db::record_call`] keeps
/// it: what migration `0032` says each column is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub id: String,
    pub project: String,
    pub account: Option<String>,
    pub client: Option<String>,
    pub provider: String,
    /// `project`, `account` or `platform`.
    pub pays: String,
    /// Whose own key paid, for `account`.
    pub payer: Option<String>,
    pub model: Option<String>,
    pub status: u16,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub cost_micros: u64,
    pub priced_as: Option<String>,
    pub started_at: u64,
    pub ended_at: u64,
}

/// One line of a project's spend ([`Db::project_spend`]): an account's
/// calls through one client (or none) on one kind of key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Spent {
    pub account: Option<String>,
    pub client: Option<String>,
    pub pays: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micros: u64,
}

/// A project's agent turns on its hosted boxes ([`Db::project_turns`]):
/// how many, how many on the platform's key, and their minutes.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Turns {
    pub turns: u64,
    pub platform_turns: u64,
    pub minutes: u64,
}

/// How long the broker's calls are kept: the month a cap counts and the
/// two before it.
pub const CALLS_KEEP_MS: u64 = 90 * DAY_MS;

impl Db {
    /// Keep a call the broker made.
    pub fn record_call(&self, c: &Call) -> Result<()> {
        self.c().execute(
            "INSERT INTO inference_calls (id, project_id, account, client, provider, pays, payer, model, status,
                 input_tokens, output_tokens, cache_write_tokens, cache_write_1h_tokens, cache_read_tokens,
                 cost_micros, priced_as, started_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                c.id,
                c.project,
                c.account,
                c.client,
                c.provider,
                c.pays,
                c.payer,
                c.model,
                u32::from(c.status),
                c.input,
                c.output,
                c.cache_write,
                c.cache_write_1h,
                c.cache_read,
                c.cost_micros,
                c.priced_as,
                c.started_at,
                c.ended_at
            ],
        )?;
        Ok(())
    }

    /// What `account`'s calls through `client` (`None`: its own, with no
    /// client) cost on the platform's key since `since`, in millionths of
    /// a dollar: what its monthly cap counts.
    pub fn platform_spend(&self, account: &str, client: Option<&str>, since: u64) -> Result<u64> {
        self.c().query_row(
            "SELECT CAST(COALESCE(SUM(cost_micros), 0) AS BIGINT) FROM inference_calls
             WHERE pays = 'platform' AND account = ?1 AND COALESCE(client, '') = ?2 AND started_at >= ?3",
            params![account, client.unwrap_or(""), since],
            |r| r.get(0),
        )
    }

    /// What a project's calls cost in `[since, until)`, by account, client
    /// and whose key, dearest first.
    pub fn project_spend(&self, project: &str, since: u64, until: u64) -> Result<Vec<Spent>> {
        self.c().query_map(
            "SELECT account, client, pays, CAST(count(*) AS BIGINT),
                 CAST(COALESCE(SUM(input_tokens + cache_write_tokens + cache_write_1h_tokens + cache_read_tokens), 0) AS BIGINT),
                 CAST(COALESCE(SUM(output_tokens), 0) AS BIGINT),
                 CAST(COALESCE(SUM(cost_micros), 0) AS BIGINT)
             FROM inference_calls WHERE project_id = ?1 AND started_at >= ?2 AND started_at < ?3
             GROUP BY account, client, pays ORDER BY 7 DESC, 1, 2, 3",
            params![project, since, until],
            |r| {
                Ok(Spent {
                    account: r.get(0)?,
                    client: r.get(1)?,
                    pays: r.get(2)?,
                    calls: r.get(3)?,
                    input_tokens: r.get(4)?,
                    output_tokens: r.get(5)?,
                    cost_micros: r.get(6)?,
                })
            },
        )
    }

    /// A project's agent turns on its hosted boxes that started in
    /// `[since, until)` (step 5's `hosted_turns`), a turn with no end
    /// counting until it's stale, as the caps count it.
    pub fn project_turns(&self, project: &str, since: u64, until: u64, now: u64) -> Result<Turns> {
        let (turns, platform, ms): (u64, u64, i64) = self.c().query_row(
            "SELECT CAST(count(*) AS BIGINT),
                 CAST(COALESCE(SUM(CASE WHEN t.pays = 'platform' THEN 1 ELSE 0 END), 0) AS BIGINT),
                 CAST(COALESCE(SUM(
                     (CASE WHEN COALESCE(t.ended_at, ?4) < t.started_at + ?5 THEN COALESCE(t.ended_at, ?4)
                           ELSE t.started_at + ?5 END) - t.started_at), 0) AS BIGINT)
             FROM hosted_turns t JOIN hosted_workspaces w ON w.id = t.workspace_id
             WHERE w.project_id = ?1 AND t.started_at >= ?2 AND t.started_at < ?3",
            params![project, since, until, now, STALE_MS],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(Turns { turns, platform_turns: platform, minutes: u64::try_from(ms.max(0)).unwrap_or(0) / 60_000 })
    }

    /// Forget the broker's calls older than [`CALLS_KEEP_MS`].
    pub fn tidy_inference_calls(&self, now: u64) -> Result<usize> {
        self.c()
            .execute("DELETE FROM inference_calls WHERE started_at < ?1", params![now.saturating_sub(CALLS_KEEP_MS)])
    }
}

/// An account is gone: no key or turn names it any more. Its projects'
/// keys go when the sweep finds their project gone.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("UPDATE project_inference_keys SET set_by = NULL WHERE set_by = ?1", params![d.account])?;
    c.execute("UPDATE hosted_turns SET account = NULL WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE inference_calls SET account = NULL WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE inference_calls SET payer = NULL WHERE payer = ?1", params![d.account])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::hosted::Added;

    const CAPS: Caps = Caps { concurrent: 2, daily: 3, minutes: 10 };

    fn platform(a: &str) -> Pays {
        Pays::Platform { account: a.into() }
    }

    fn turn(s: Started) -> String {
        match s {
            Started::Turn(id) => id,
            other => panic!("{other:?}"),
        }
    }

    /// Platform turns count against their account: at once, per UTC day,
    /// and minutes per day (a turn with no end counts until it's stale);
    /// a project's key is never capped, and another account's turns don't
    /// count.
    #[test]
    fn platform_turns_are_capped_per_account() {
        let db = Db::test();
        let day = 100 * DAY_MS;
        let t1 = turn(db.start_hosted_turn("hw_1", "anthropic", &platform("a"), &CAPS, day + 1_000).unwrap());
        turn(db.start_hosted_turn("hw_2", "anthropic", &platform("a"), &CAPS, day + 2_000).unwrap());
        assert_eq!(
            db.start_hosted_turn("hw_1", "anthropic", &platform("a"), &CAPS, day + 3_000).unwrap(),
            Started::Capped(Cap::Concurrent)
        );
        turn(db.start_hosted_turn("hw_1", "anthropic", &platform("b"), &CAPS, day + 3_000).unwrap());
        turn(db.start_hosted_turn("hw_1", "anthropic", &Pays::Project, &CAPS, day + 3_000).unwrap());
        assert!(!db.end_hosted_turn("hw_2", &t1, day + 4_000).unwrap(), "only its own box ends a turn");
        assert!(db.end_hosted_turn("hw_1", &t1, day + 4_000).unwrap());
        assert!(!db.end_hosted_turn("hw_1", &t1, day + 5_000).unwrap(), "once");
        let t3 = turn(db.start_hosted_turn("hw_1", "openai", &platform("a"), &CAPS, day + 5_000).unwrap());
        db.end_hosted_turn("hw_1", &t3, day + 6_000).unwrap();
        assert_eq!(
            db.start_hosted_turn("hw_1", "anthropic", &platform("a"), &CAPS, day + 7_000).unwrap(),
            Started::Capped(Cap::Daily)
        );
        // A new UTC day; the one turn left open from yesterday went stale.
        let next = day + DAY_MS + STALE_MS;
        let long = turn(db.start_hosted_turn("hw_1", "anthropic", &platform("a"), &CAPS, next).unwrap());
        db.end_hosted_turn("hw_1", &long, next + 10 * 60_000).unwrap();
        assert_eq!(
            db.start_hosted_turn("hw_1", "anthropic", &platform("a"), &CAPS, next + 11 * 60_000).unwrap(),
            Started::Capped(Cap::Minutes)
        );
    }

    /// Keys are sealed values per project and provider; a key whose project
    /// is gone is dropped; an account that goes is named nowhere.
    #[test]
    fn keys_boxes_and_erasing() {
        let db = Db::test();
        let p = db.personal_project("alice", 0).unwrap();
        let sealed = |bytes: &[u8]| crate::keys::Sealed { key_version: 1, bytes: bytes.to_vec() };
        db.set_inference_key(&p, "anthropic", &sealed(&[1]), "alice", 1).unwrap();
        db.set_inference_key(&p, "anthropic", &sealed(&[2, 3]), "alice", 2).unwrap();
        db.set_inference_key("prj_gone", "openai", &sealed(&[4]), "alice", 2).unwrap();
        assert_eq!(
            db.inference_keys(&p).unwrap(),
            [ProjectKey { provider: "anthropic".into(), set_by: Some("alice".into()), updated_at: 2 }]
        );
        assert_eq!(db.sealed_inference_keys(&p).unwrap(), [("anthropic".to_owned(), sealed(&[2, 3]))]);
        assert_eq!(db.drop_orphan_inference_keys().unwrap(), 1, "prj_gone's");
        assert_eq!(db.drop_orphan_inference_keys().unwrap(), 0);
        assert!(db.remove_inference_key(&p, "anthropic").unwrap());
        assert!(!db.remove_inference_key(&p, "anthropic").unwrap());
        db.set_inference_key(&p, "anthropic", &sealed(&[5]), "alice", 3).unwrap();

        let spec = crate::hosted::Spec::default();
        assert!(matches!(db.add_hosted_workspace("hw_1", &p, "dev", &spec, "alice", 5, 0).unwrap(), Added::Made(_)));
        assert!(db.hosted_box("m1").unwrap().is_none(), "not enrolled");
        db.c()
            .execute("UPDATE hosted_workspaces SET machine = 'm1', sandbox_id = 'sbx1', state = 'ready'", params![])
            .unwrap();
        let b = db.hosted_box("m1").unwrap().unwrap();
        assert_eq!((b.workspace.as_str(), b.rules.as_str(), b.sandbox.as_deref()), ("hw_1", "", Some("sbx1")));
        db.set_hosted_rules("hw_1", "d1").unwrap();
        assert_eq!(db.hosted_box("m1").unwrap().unwrap().rules, "d1");
        assert_eq!(db.hosted_box_of("hw_1").unwrap().unwrap().rules, "d1");
        assert_eq!(db.running_boxes(&p).unwrap().len(), 1);

        let open = turn(db.start_hosted_turn("hw_1", "anthropic", &platform("alice"), &CAPS, 10).unwrap());
        db.c().execute("UPDATE hosted_workspaces SET state = 'paused'", params![]).unwrap();
        db.tidy_hosted_turns(20).unwrap();
        assert!(!db.end_hosted_turn("hw_1", &open, 30).unwrap(), "a paused box's turns ended");
        db.tidy_hosted_turns(KEEP_MS + 11).unwrap();
        let n: u64 = db.c().query_row("SELECT count(*) FROM hosted_turns", params![], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "kept 30 days");

        turn(db.start_hosted_turn("hw_1", "anthropic", &platform("alice"), &CAPS, KEEP_MS + 20).unwrap());
        let d = Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        erase_account(&tx, &d).unwrap();
        crate::db::hosted::erase_account(&tx, &d, 9).unwrap();
        crate::db::projects::erase_account(&tx, &d).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert!(db.mentions("alice").is_empty(), "{:?}", db.mentions("alice"));
        assert_eq!(db.drop_orphan_inference_keys().unwrap(), 1, "its personal project's key");
    }
}
