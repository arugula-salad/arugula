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
    /// slot. A day is a UTC day. `asker` is the account a session's turn
    /// is for (aru-115): a turn on the project's key is kept as theirs.
    pub fn start_hosted_turn(
        &self,
        workspace: &str,
        provider: &str,
        pays: &Pays,
        asker: Option<&str>,
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
            params![id, workspace, provider, paid, account.or(asker), now],
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

/// A call the inference broker made (M231), as migration `0032` keeps it:
/// what each column is is there.
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
    /// `open`, `settled` or `kept`.
    pub state: String,
    pub model: Option<String>,
    pub status: u16,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub web_searches: u64,
    pub reserved_micros: u64,
    pub cost_micros: u64,
    pub priced_as: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
}

/// What a call came to when it ended ([`Db::end_call`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ended {
    pub status: u16,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub web_searches: u64,
    /// The provider's final usage arrived: the call costs this (its list
    /// price, at `priced_as`). `None`: its reservation stands.
    pub settled: Option<(u64, String)>,
}

/// The cap a call on the platform's key is reserved under
/// ([`Db::reserve_call`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthCap {
    /// What the account's month (through this client) may cost.
    pub micros: u64,
    /// The start of the month counted.
    pub since: u64,
    /// How many of its calls on the platform's key may run at once.
    pub concurrent: u64,
}

/// What [`Db::reserve_call`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reserved {
    Yes,
    /// The month's calls already cost this much.
    Capped(u64),
    /// This many of its calls are running.
    Busy(u64),
}

/// A call still `open` this long after it started is counted as running no
/// more (a machine that died mid-call): the broker's read timeout, and
/// some.
pub const OPEN_MS: u64 = 15 * 60 * 1000;

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

/// One line of what others spent on an account's own key
/// ([`Db::paid_by`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PaidBy {
    pub project: String,
    pub account: Option<String>,
    pub client: Option<String>,
    pub calls: u64,
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

const PLATFORM_SPEND: &str = "SELECT CAST(COALESCE(SUM(cost_micros), 0) AS BIGINT) FROM inference_calls
     WHERE pays = 'platform' AND account = ?1 AND COALESCE(client, '') = ?2 AND started_at >= ?3";

impl Db {
    /// Keep a call before it goes out, at its reservation (`call.cost_micros`
    /// is `call.reserved_micros`, its state `open`). On the platform's key
    /// (`cap`), only if the account's month through this client is under
    /// the cap and fewer than `cap.concurrent` of its calls run, counting
    /// every reservation: checked and kept under the account's lock, so
    /// two calls at once can't both take the last of it.
    pub fn reserve_call(&self, call: &Call, cap: Option<&MonthCap>) -> Result<Reserved> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if let (Some(cap), Some(account)) = (cap, call.account.as_deref()) {
            tx.lock(&format!("inference:{account}"))?;
            let client = call.client.as_deref().unwrap_or("");
            let spent: u64 = tx.query_row(PLATFORM_SPEND, params![account, client, cap.since], |r| r.get(0))?;
            if spent >= cap.micros {
                return Ok(Reserved::Capped(spent));
            }
            let running: u64 = tx.query_row(
                "SELECT CAST(count(*) AS BIGINT) FROM inference_calls
                 WHERE pays = 'platform' AND account = ?1 AND state = 'open' AND started_at > ?2",
                params![account, call.started_at.saturating_sub(OPEN_MS)],
                |r| r.get(0),
            )?;
            if running >= cap.concurrent {
                return Ok(Reserved::Busy(running));
            }
        }
        tx.execute(
            "INSERT INTO inference_calls (id, project_id, account, client, provider, pays, payer, state, model,
                 reserved_micros, cost_micros, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?9, ?9, ?10)",
            params![
                call.id,
                call.project,
                call.account,
                call.client,
                call.provider,
                call.pays,
                call.payer,
                call.model,
                call.reserved_micros,
                call.started_at
            ],
        )?;
        tx.commit()?;
        Ok(Reserved::Yes)
    }

    /// A call ended: what it used, and its cost once the provider's final
    /// usage arrived (`settled`); without it, its reservation stands
    /// (`kept`). Only an open call ends, once.
    pub fn end_call(&self, id: &str, e: &Ended, now: u64) -> Result<bool> {
        let (state, cost, priced) = match &e.settled {
            Some((cost, row)) => ("settled", Some(*cost), Some(row.as_str()).filter(|r| !r.is_empty())),
            None => ("kept", None, None),
        };
        Ok(self.c().execute(
            "UPDATE inference_calls SET state = ?2, status = ?3, model = COALESCE(?4, model), input_tokens = ?5,
                 output_tokens = ?6, cache_write_tokens = ?7, cache_write_1h_tokens = ?8, cache_read_tokens = ?9,
                 web_search_requests = ?10, cost_micros = COALESCE(?11, cost_micros), priced_as = ?12, ended_at = ?13
             WHERE id = ?1 AND state = 'open'",
            params![
                id,
                state,
                u32::from(e.status),
                e.model,
                e.input,
                e.output,
                e.cache_write,
                e.cache_write_1h,
                e.cache_read,
                e.web_searches,
                cost,
                priced,
                now
            ],
        )? == 1)
    }

    /// A call, by id.
    pub fn call(&self, id: &str) -> Result<Option<Call>> {
        self.c()
            .query_row(
                "SELECT id, project_id, account, client, provider, pays, payer, state, model, status, input_tokens,
                     output_tokens, cache_write_tokens, cache_write_1h_tokens, cache_read_tokens, web_search_requests,
                     reserved_micros, cost_micros, priced_as, started_at, ended_at
                 FROM inference_calls WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Call {
                        id: r.get(0)?,
                        project: r.get(1)?,
                        account: r.get(2)?,
                        client: r.get(3)?,
                        provider: r.get(4)?,
                        pays: r.get(5)?,
                        payer: r.get(6)?,
                        state: r.get(7)?,
                        model: r.get(8)?,
                        status: u16::try_from(r.get::<u32>(9)?).unwrap_or(0),
                        input: r.get(10)?,
                        output: r.get(11)?,
                        cache_write: r.get(12)?,
                        cache_write_1h: r.get(13)?,
                        cache_read: r.get(14)?,
                        web_searches: r.get(15)?,
                        reserved_micros: r.get(16)?,
                        cost_micros: r.get(17)?,
                        priced_as: r.get(18)?,
                        started_at: r.get(19)?,
                        ended_at: r.get(20)?,
                    })
                },
            )
            .optional()
    }

    /// The ids of every call, oldest first: for tests.
    #[cfg(test)]
    pub fn call_ids(&self) -> Result<Vec<String>> {
        self.c().query_map("SELECT id FROM inference_calls ORDER BY started_at, id", params![], |r| r.get(0))
    }

    /// What `account`'s calls through `client` (`None`: its own, with no
    /// client) cost on the platform's key since `since`, reservations
    /// included, in millionths of a dollar: what its monthly cap counts.
    pub fn platform_spend(&self, account: &str, client: Option<&str>, since: u64) -> Result<u64> {
        self.c().query_row(PLATFORM_SPEND, params![account, client.unwrap_or(""), since], |r| r.get(0))
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

    /// What calls on `payer`'s own key cost in `[since, until)`, by project,
    /// account and client: its own and others'.
    pub fn paid_by(&self, payer: &str, since: u64, until: u64) -> Result<Vec<PaidBy>> {
        self.c().query_map(
            "SELECT project_id, account, client, CAST(count(*) AS BIGINT), CAST(COALESCE(SUM(cost_micros), 0) AS BIGINT)
             FROM inference_calls WHERE pays = 'account' AND payer = ?1 AND started_at >= ?2 AND started_at < ?3
             GROUP BY project_id, account, client ORDER BY 5 DESC, 1, 2, 3",
            params![payer, since, until],
            |r| {
                Ok(PaidBy {
                    project: r.get(0)?,
                    account: r.get(1)?,
                    client: r.get(2)?,
                    calls: r.get(3)?,
                    cost_micros: r.get(4)?,
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

    /// Forget the broker's calls older than [`CALLS_KEEP_MS`]; a call still
    /// open past [`OPEN_MS`] (its machine died) keeps its reservation.
    pub fn tidy_inference_calls(&self, now: u64) -> Result<()> {
        let c = self.c();
        c.execute(
            "UPDATE inference_calls SET state = 'kept', ended_at = ?1 WHERE state = 'open' AND started_at < ?2",
            params![now, now.saturating_sub(OPEN_MS)],
        )?;
        c.execute("DELETE FROM inference_calls WHERE started_at < ?1", params![now.saturating_sub(CALLS_KEEP_MS)])?;
        Ok(())
    }

    /// Whether `account` lets `client` act for it.
    pub fn consented(&self, account: &str, client: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row(
                "SELECT 1 FROM inference_consents WHERE account = ?1 AND client = ?2",
                params![account, client],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some())
    }

    /// The clients `account` lets act for it.
    pub fn consents(&self, account: &str) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT client FROM inference_consents WHERE account = ?1 ORDER BY client",
            params![account],
            |r| r.get(0),
        )
    }

    /// Let `client` act for `account`, or stop it.
    pub fn set_consent(&self, account: &str, client: &str, on: bool, now: u64) -> Result<()> {
        if on {
            self.c().execute(
                "INSERT INTO inference_consents (account, client, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (account, client) DO NOTHING",
                params![account, client, now],
            )?;
        } else {
            self.c().execute(
                "DELETE FROM inference_consents WHERE account = ?1 AND client = ?2",
                params![account, client],
            )?;
        }
        Ok(())
    }
}

/// An account is gone: no key or turn names it any more. Its projects'
/// keys go when the sweep finds their project gone.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("UPDATE project_inference_keys SET set_by = NULL WHERE set_by = ?1", params![d.account])?;
    c.execute("UPDATE hosted_turns SET account = NULL WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE inference_calls SET account = NULL WHERE account = ?1", params![d.account])?;
    c.execute("UPDATE inference_calls SET payer = NULL WHERE payer = ?1", params![d.account])?;
    c.execute("DELETE FROM inference_consents WHERE account = ?1", params![d.account])?;
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
        let t1 = turn(db.start_hosted_turn("hw_1", "anthropic", &platform("a"), None, &CAPS, day + 1_000).unwrap());
        turn(db.start_hosted_turn("hw_2", "anthropic", &platform("a"), None, &CAPS, day + 2_000).unwrap());
        assert_eq!(
            db.start_hosted_turn("hw_1", "anthropic", &platform("a"), None, &CAPS, day + 3_000).unwrap(),
            Started::Capped(Cap::Concurrent)
        );
        turn(db.start_hosted_turn("hw_1", "anthropic", &platform("b"), None, &CAPS, day + 3_000).unwrap());
        turn(db.start_hosted_turn("hw_1", "anthropic", &Pays::Project, None, &CAPS, day + 3_000).unwrap());
        assert!(!db.end_hosted_turn("hw_2", &t1, day + 4_000).unwrap(), "only its own box ends a turn");
        assert!(db.end_hosted_turn("hw_1", &t1, day + 4_000).unwrap());
        assert!(!db.end_hosted_turn("hw_1", &t1, day + 5_000).unwrap(), "once");
        let t3 = turn(db.start_hosted_turn("hw_1", "openai", &platform("a"), None, &CAPS, day + 5_000).unwrap());
        db.end_hosted_turn("hw_1", &t3, day + 6_000).unwrap();
        assert_eq!(
            db.start_hosted_turn("hw_1", "anthropic", &platform("a"), None, &CAPS, day + 7_000).unwrap(),
            Started::Capped(Cap::Daily)
        );
        // A new UTC day; the one turn left open from yesterday went stale.
        let next = day + DAY_MS + STALE_MS;
        let long = turn(db.start_hosted_turn("hw_1", "anthropic", &platform("a"), None, &CAPS, next).unwrap());
        db.end_hosted_turn("hw_1", &long, next + 10 * 60_000).unwrap();
        assert_eq!(
            db.start_hosted_turn("hw_1", "anthropic", &platform("a"), None, &CAPS, next + 11 * 60_000).unwrap(),
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

        let open = turn(db.start_hosted_turn("hw_1", "anthropic", &platform("alice"), None, &CAPS, 10).unwrap());
        db.c().execute("UPDATE hosted_workspaces SET state = 'paused'", params![]).unwrap();
        db.tidy_hosted_turns(20).unwrap();
        assert!(!db.end_hosted_turn("hw_1", &open, 30).unwrap(), "a paused box's turns ended");
        db.tidy_hosted_turns(KEEP_MS + 11).unwrap();
        let n: u64 = db.c().query_row("SELECT count(*) FROM hosted_turns", params![], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "kept 30 days");

        turn(db.start_hosted_turn("hw_1", "anthropic", &platform("alice"), None, &CAPS, KEEP_MS + 20).unwrap());
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
