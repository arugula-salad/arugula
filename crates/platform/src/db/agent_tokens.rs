//! Agent grant tokens (M230, platform step 8): what an agent with no
//! machine of its own shows the platform's MCP endpoint
//! ([`crate::agent_mcp`]). The rules, which [`crate::agent_tokens`] keeps:
//!
//! - **A token is its owner's agent.** The account that minted it is the
//!   author of everything its agent does, labelled with the token's name
//!   ("<owner>'s agent on <name> (MCP)"), and never reaches past what that
//!   account reads now.
//! - **What it may do is grants** ("Grants, not impersonation"): one row in
//!   `grants` per power and project, `subject_kind` `agent`, given in the
//!   same transaction as the token. They're fixed: a token that should do
//!   more is a new token.
//! - **Only a hash is kept.** The token (`aru_agent_` and 64 hex
//!   characters) is shown once, when it's minted; `token_hash` is its
//!   SHA-256, which is how a call finds it.
//! - **Revoking it** revokes its grants with it, so the very next call is
//!   refused, and keeps the rows for the record.
//! - **Every call is noted** in `agent_token_calls`: the tool, the project,
//!   how it went, a line of what happened. Never a value.

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::{
    Db,
    projects::{GRANT_COLUMNS, Grant, grant_row},
    sql::{Optional as _, Row, Sql, params},
};

/// A token's id: `agt_` and 32 hex characters. Never a machine's (16 hex
/// characters, no prefix): see [`super::projects::subject_kind`].
pub const ID_PREFIX: &str = "agt_";

/// What a token itself starts with, so a leaked one is easy to find in a
/// log or a repo.
pub const TOKEN_PREFIX: &str = "aru_agent_";

/// The most tokens an account holds live at once.
pub const MAX_LIVE: usize = 10;

/// An agent grant token, without the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentToken {
    pub id: String,
    pub account: String,
    pub name: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub revoked_at: Option<u64>,
    /// The browser it was minted from (its User-Agent), or, for one
    /// carried over from another product, where from.
    pub minted_from: String,
    /// The product it was carried over from (M235: `redone`), for a token
    /// that wasn't minted on the platform page.
    pub imported_from: Option<String>,
}

/// A token to keep: its id, its minter, its name, the token's hash and
/// the browser it was minted from.
pub struct NewToken {
    pub id: String,
    pub account: String,
    pub name: String,
    pub token_hash: String,
    pub minted_from: String,
}

/// A tool call a token's agent made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCall {
    pub id: String,
    pub token: String,
    pub tool: String,
    pub project: Option<String>,
    /// `ok`, `refused` or `failed`.
    pub outcome: String,
    pub detail: String,
    pub at: u64,
}

const COLUMNS: &str = "id, account_id, name, created_at, last_used_at, revoked_at, minted_from, imported_from";

fn row(r: &Row) -> anyhow::Result<AgentToken> {
    Ok(AgentToken {
        id: r.get(0)?,
        account: r.get(1)?,
        name: r.get(2)?,
        created_at: r.get(3)?,
        last_used_at: r.get(4)?,
        revoked_at: r.get(5)?,
        minted_from: r.get(6)?,
        imported_from: r.get(7)?,
    })
}

/// A new token and its id.
pub fn new_token() -> (String, String) {
    (
        format!("{ID_PREFIX}{}", hex::encode(arugula_e2e::random::<16>())),
        format!("{TOKEN_PREFIX}{}", hex::encode(arugula_e2e::random::<32>())),
    )
}

/// What's kept of a token: its SHA-256, hex.
pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_grant() -> String {
    format!("grt_{}", hex::encode(arugula_e2e::random::<16>()))
}

impl Db {
    /// Keep a token minted by `account` (only its hash), with a live grant
    /// of each `(power, project)`, in one transaction. The caller has
    /// checked that the account reads each project now.
    pub fn mint_agent_token(&self, t: &NewToken, grants: &[(&str, &str)], now: u64) -> anyhow::Result<AgentToken> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO agent_tokens (id, account_id, name, token_hash, created_at, minted_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![t.id, t.account, t.name, t.token_hash, now, t.minted_from],
        )?;
        let (id, account) = (&t.id, &t.account);
        for (power, project) in grants {
            tx.execute(
                "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
                 VALUES (?1, 'agent', ?2, ?3, 'project', ?4, ?5, ?6)",
                params![new_grant(), id, power, project, account, now],
            )?;
        }
        let t = tx.query_row(&format!("SELECT {COLUMNS} FROM agent_tokens WHERE id = ?1"), params![id], row)?;
        tx.commit()?;
        Ok(t)
    }

    /// The live tokens `account` holds.
    pub fn live_agent_tokens(&self, account: &str) -> anyhow::Result<usize> {
        self.c().query_row(
            "SELECT count(*) FROM agent_tokens WHERE account_id = ?1 AND revoked_at IS NULL",
            params![account],
            |r| r.get(0),
        )
    }

    /// A token by its id.
    pub fn agent_token(&self, id: &str) -> anyhow::Result<Option<AgentToken>> {
        self.c().query_row(&format!("SELECT {COLUMNS} FROM agent_tokens WHERE id = ?1"), params![id], row).optional()
    }

    /// The token whose hash this is, live or revoked.
    pub fn agent_token_by_hash(&self, token_hash: &str) -> anyhow::Result<Option<AgentToken>> {
        self.c()
            .query_row(&format!("SELECT {COLUMNS} FROM agent_tokens WHERE token_hash = ?1"), params![token_hash], row)
            .optional()
    }

    /// `account`'s tokens, newest first, each with its grants (live and
    /// revoked).
    pub fn agent_tokens(&self, account: &str) -> anyhow::Result<Vec<(AgentToken, Vec<Grant>)>> {
        let c = self.c();
        let tokens = c.query_map(
            &format!("SELECT {COLUMNS} FROM agent_tokens WHERE account_id = ?1 ORDER BY created_at DESC, id"),
            params![account],
            row,
        )?;
        let mut out = Vec::with_capacity(tokens.len());
        for t in tokens {
            let grants = c.query_map(
                &format!(
                    "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'agent' AND subject_id = ?1
                     ORDER BY scope_id, power"
                ),
                params![t.id],
                grant_row,
            )?;
            out.push((t, grants));
        }
        Ok(out)
    }

    /// The live grants of token `id` that `account` gave.
    pub fn agent_token_grants(&self, id: &str, account: &str) -> anyhow::Result<Vec<Grant>> {
        self.c().query_map(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'agent' AND subject_id = ?1
                 AND granted_by = ?2 AND revoked_at IS NULL ORDER BY scope_id, power"
            ),
            params![id, account],
            grant_row,
        )
    }

    /// Revoke one of `account`'s tokens, and its grants; again changes
    /// nothing. `None`: no such token of its.
    pub fn revoke_agent_token(&self, account: &str, id: &str, now: u64) -> anyhow::Result<Option<AgentToken>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE agent_tokens SET revoked_at = ?3 WHERE id = ?1 AND account_id = ?2 AND revoked_at IS NULL",
            params![id, account, now],
        )?;
        tx.execute(
            "UPDATE grants SET revoked_at = ?3 WHERE subject_kind = 'agent' AND subject_id = ?1 AND granted_by = ?2
             AND revoked_at IS NULL",
            params![id, account, now],
        )?;
        let t = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM agent_tokens WHERE id = ?1 AND account_id = ?2"),
                params![id, account],
                row,
            )
            .optional()?;
        tx.commit()?;
        Ok(t)
    }

    /// The tokens with a live grant over `project`, anyone's, each with
    /// its live grants there: what the project's owners see.
    pub fn agent_tokens_over(&self, project: &str) -> anyhow::Result<Vec<(AgentToken, Vec<Grant>)>> {
        let c = self.c();
        let tokens = c.query_map(
            &format!(
                "SELECT {COLUMNS} FROM agent_tokens WHERE revoked_at IS NULL AND id IN (SELECT subject_id FROM grants
                 WHERE subject_kind = 'agent' AND scope_kind = 'project' AND scope_id = ?1 AND revoked_at IS NULL)
                 ORDER BY created_at DESC, id"
            ),
            params![project],
            row,
        )?;
        let mut out = Vec::with_capacity(tokens.len());
        for t in tokens {
            let grants = c.query_map(
                &format!(
                    "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'agent' AND subject_id = ?1
                     AND scope_kind = 'project' AND scope_id = ?2 AND revoked_at IS NULL ORDER BY power"
                ),
                params![t.id, project],
                grant_row,
            )?;
            out.push((t, grants));
        }
        Ok(out)
    }

    /// Revoke token `id`'s grants over `project` (a project owner's
    /// revoke): how many were live.
    pub fn revoke_agent_token_over(&self, project: &str, id: &str, now: u64) -> anyhow::Result<usize> {
        self.c().execute(
            "UPDATE grants SET revoked_at = ?3 WHERE subject_kind = 'agent' AND subject_id = ?2
             AND scope_kind = 'project' AND scope_id = ?1 AND revoked_at IS NULL",
            params![project, id, now],
        )
    }

    /// Note that the token was used now.
    pub fn touch_agent_token(&self, id: &str, now: u64) -> anyhow::Result<()> {
        self.c().execute("UPDATE agent_tokens SET last_used_at = ?2 WHERE id = ?1", params![id, now])?;
        Ok(())
    }

    /// Note a tool call a token's agent made.
    pub fn note_agent_call(&self, t: &AgentToken, call: &AgentCall) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO agent_token_calls (id, token_id, account_id, tool, project_id, outcome, detail, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![call.id, t.id, t.account, call.tool, call.project, call.outcome, call.detail, call.at],
        )?;
        Ok(())
    }

    /// The newest `limit` calls token `id` of `account`'s made, newest
    /// first.
    pub fn agent_calls(&self, account: &str, id: &str, limit: u32) -> anyhow::Result<Vec<AgentCall>> {
        self.c().query_map(
            "SELECT id, token_id, tool, project_id, outcome, detail, at FROM agent_token_calls
             WHERE token_id = ?1 AND account_id = ?2 ORDER BY at DESC, id LIMIT ?3",
            params![id, account, limit],
            |r| {
                Ok(AgentCall {
                    id: r.get(0)?,
                    token: r.get(1)?,
                    tool: r.get(2)?,
                    project: r.get(3)?,
                    outcome: r.get(4)?,
                    detail: r.get(5)?,
                    at: r.get(6)?,
                })
            },
        )
    }
}

/// Agent tokens' grants over a team's projects that a person who's no
/// longer in the team gave go (M230): run in the transaction that rewrites
/// the team's `team_members`.
pub(crate) fn revoke_departed(c: &impl Sql, team: &str, now: u64) -> anyhow::Result<usize> {
    c.execute(
        "UPDATE grants SET revoked_at = ?2 WHERE subject_kind = 'agent' AND revoked_at IS NULL
         AND scope_kind = 'project' AND scope_id IN (SELECT id FROM projects WHERE team_id = ?1)
         AND granted_by NOT IN (SELECT account_id FROM team_members WHERE team_id = ?1)",
        params![team, now],
    )
}

/// A project's own secret's name, as its owners see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectSecret {
    pub name: String,
    pub updated_at: u64,
    pub updated_by: Option<String>,
    pub updated_by_agent: Option<String>,
}

/// What a project's own secrets are kept under in `project_secrets`.
pub const PROJECT_SECRET_PREFIX: &str = "project/";

impl Db {
    /// A project's own secrets (`project/<NAME>`), by name, and its apps'
    /// (`app/<app>/<KEY>`, M235, named `<app>/<KEY>`): never a value.
    pub fn project_secrets(&self, project: &str) -> anyhow::Result<Vec<ProjectSecret>> {
        self.c().query_map(
            "SELECT name, updated_at, updated_by, updated_by_agent FROM project_secrets
             WHERE project_id = ?1 AND (name LIKE 'project/%' OR name LIKE 'app/%') ORDER BY name",
            params![project],
            |r| {
                let name: String = r.get(0)?;
                let shown = name.strip_prefix(PROJECT_SECRET_PREFIX).or_else(|| name.strip_prefix("app/"));
                Ok(ProjectSecret {
                    name: shown.unwrap_or(&name).to_owned(),
                    updated_at: r.get(1)?,
                    updated_by: r.get(2)?,
                    updated_by_agent: r.get(3)?,
                })
            },
        )
    }

    /// Note who set a project's secret `name` (its stored name).
    pub fn note_secret_writer(&self, project: &str, name: &str, by: &str, agent: Option<&str>) -> anyhow::Result<()> {
        self.c().execute(
            "UPDATE project_secrets SET updated_by = ?3, updated_by_agent = ?4 WHERE project_id = ?1 AND name = ?2",
            params![project, name, by, agent],
        )?;
        Ok(())
    }

    /// Whether a project has its own secret by this stored name.
    pub fn has_project_secret(&self, project: &str, name: &str) -> anyhow::Result<bool> {
        Ok(self
            .c()
            .query_row(
                "SELECT 1 FROM project_secrets WHERE project_id = ?1 AND name = ?2",
                params![project, name],
                |r| r.get::<i64>(0),
            )
            .optional()?
            .is_some())
    }
}

/// A new call's id.
pub fn new_call() -> String {
    format!("atc_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// Deleting an account takes its tokens and their calls (their grants go
/// with the ones it gave, [`super::projects::erase_account`]).
pub(crate) fn erase_account(c: &impl Sql, account: &str) -> anyhow::Result<()> {
    c.execute("DELETE FROM agent_token_calls WHERE account_id = ?1", params![account])?;
    c.execute("DELETE FROM agent_tokens WHERE account_id = ?1", params![account])?;
    Ok(())
}
