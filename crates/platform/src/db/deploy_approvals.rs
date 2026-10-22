//! Deploy approvals (M218, platform step 7, native Arugula #14): the signed
//! approvals and denies a deploy got, made by `0027_deploy_approvals`.
//!
//! An approval is S37's work approval over a deploy's operation
//! ([`arugula_e2e::deploy`]): signed by a project owner's device, which the
//! routes check ([`crate::deploys::verify`]) before they come here with a
//! [`VerifiedApproval`]. Here it's written **in the same transaction** as
//! the deploy's `approved` (or `denied`) event (M217's
//! `record_approval_in`: as a person, never an agent; the plan and the
//! `planned` event checked against the log; the stale rule).
//!
//! The event's detail holds the signed fields, so the log alone is enough to
//! check it, by anyone who has the device's public key. The row here is
//! what's quick to read. Neither is ever changed: an approval that's
//! replaced is a new deploy's.

use anyhow::{Context as _, Result, bail};
use arugula_e2e::deploy::{Approval, Decision, Operation};

use super::{
    Db,
    deploys::{
        DEPLOY_COLS, Deploy, Environment, Event, PasskeyProof, Recorded, Refusal, VerifiedApproval, deploy_and_plan,
        deploy_row, record_approval_in,
    },
    sql::{Row, Sql, params},
};

/// A new approval's id: `apv_` and 32 hex characters.
fn new_approval_id() -> String {
    format!("apv_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// An approval as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredApproval {
    pub id: String,
    pub deploy: String,
    pub environment: String,
    /// Its event's seq in the environment's log.
    pub seq: u64,
    pub decision: Decision,
    pub op: String,
    pub account: String,
    pub device: String,
    pub device_name: String,
    pub expires: u64,
    pub sig: String,
    /// The passkey assertion the hub checked over the approval's bytes.
    pub passkey: Option<PasskeyProof>,
    pub at: u64,
}

impl StoredApproval {
    /// The signed object it was made from.
    pub fn signed(&self) -> Approval {
        Approval {
            deploy: self.deploy.clone(),
            op: self.op.clone(),
            account: self.account.clone(),
            decision: self.decision,
            expires: self.expires,
            by: self.device.clone(),
            sig: self.sig.clone(),
        }
    }
}

const COLS: &str = "id, deploy_id, environment_id, seq, decision, op_hash, account, device, device_name, expires, sig, at, \
                    passkey_id, passkey_client_data, passkey_authenticator_data, passkey_signature";

fn approval_row(r: &Row) -> Result<StoredApproval> {
    let decision: String = r.get(4)?;
    Ok(StoredApproval {
        id: r.get(0)?,
        deploy: r.get(1)?,
        environment: r.get(2)?,
        seq: r.get(3)?,
        decision: match decision.as_str() {
            "once" => Decision::Once,
            "deny" => Decision::Deny,
            d => bail!("an approval's decision is {d:?}"),
        },
        op: r.get(5)?,
        account: r.get(6)?,
        device: r.get(7)?,
        device_name: r.get(8)?,
        expires: r.get(9)?,
        sig: r.get(10)?,
        at: r.get(11)?,
        passkey: Some(PasskeyProof {
            id: r.get(12)?,
            client_data_json: r.get(13)?,
            authenticator_data: r.get(14)?,
            signature: r.get(15)?,
        })
        .filter(|p| !p.id.is_empty()),
    })
}

/// What `d` asks to apply, as an owner's device signs it: its project and
/// environment (id and name), plan and source, and its `planned` event's
/// hash.
pub fn operation(d: &Deploy, env: &Environment, planned: &Event) -> Operation {
    Operation {
        project: env.project.clone(),
        environment: d.environment.clone(),
        environment_name: env.name.clone(),
        plan: d.plan_digest.clone(),
        source_kind: d.source.kind.as_str().to_owned(),
        source_ref: d.source.reference.clone(),
        source_where: d.source.place.clone(),
        planned: planned.hash.clone(),
    }
}

impl Db {
    /// A deploy's operation, as its approval signs it, read from its row
    /// and its `planned` event in the log. `None`: there's no such deploy.
    pub fn deploy_operation(&self, deploy: &str) -> Result<Option<(Deploy, Operation)>> {
        let Some((d, planned)) = deploy_and_plan(&self.c(), deploy)? else { return Ok(None) };
        let env = self.environment(&d.environment)?.context("a deploy with no environment")?;
        let op = operation(&d, &env, &planned);
        Ok(Some((d, op)))
    }

    /// Record a verified approval (or deny) of its deploy, as its account,
    /// with the signing device's name then: the deploy's event and the
    /// approval's row, in one transaction, or neither. Refused as M217's
    /// `record_approval` refuses (the deploy's state, its plan or log, the
    /// stale rule).
    pub fn record_deploy_approval(
        &self,
        v: &VerifiedApproval,
        device_name: &str,
        at: u64,
    ) -> Result<std::result::Result<(Recorded, StoredApproval), Refusal>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let recorded = match record_approval_in(&tx, v, at)? {
            Ok(r) => r,
            Err(r) => return Ok(Err(r)),
        };
        let p = v.passkey();
        let stored = tx.query_row(
            &format!(
                "INSERT INTO deploy_approvals ({COLS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 RETURNING {COLS}"
            ),
            params![
                new_approval_id(),
                v.deploy(),
                recorded.deploy.environment,
                recorded.events[0].seq,
                v.decision().as_str(),
                v.op(),
                v.account(),
                v.device(),
                device_name,
                v.expires(),
                v.signature(),
                at,
                p.map_or("", |p| p.id.as_str()),
                p.map_or("", |p| p.client_data_json.as_str()),
                p.map_or("", |p| p.authenticator_data.as_str()),
                p.map_or("", |p| p.signature.as_str())
            ],
            approval_row,
        )?;
        tx.commit()?;
        Ok(Ok((recorded, stored)))
    }

    /// A deploy's approvals and denies, oldest first.
    pub fn deploy_approvals(&self, deploy: &str) -> Result<Vec<StoredApproval>> {
        self.c().query_map(
            &format!("SELECT {COLS} FROM deploy_approvals WHERE deploy_id = ?1 ORDER BY seq"),
            params![deploy],
            approval_row,
        )
    }

    /// The environments of the projects `account` owns: its own, and those
    /// of `teams` (the teams it owns).
    pub fn owned_environments(&self, account: &str, teams: &[String]) -> Result<Vec<String>> {
        let mut args = vec![account.to_owned()];
        args.extend(teams.iter().cloned());
        let marks = vec!["?"; teams.len()].join(", ");
        let teams = if teams.is_empty() { String::new() } else { format!(" OR p.team_id IN ({marks})") };
        self.c().query_map(
            &format!(
                "SELECT e.id FROM environments e JOIN projects p ON p.id = e.project_id
                 WHERE p.account_id = ?{teams} ORDER BY e.id"
            ),
            &super::sql::list(&args),
            |r| r.get(0),
        )
    }

    /// Deploys waiting for approval in any of these environments, oldest
    /// first: what an owner's page lists.
    pub fn pending_deploys(&self, environments: &[String]) -> Result<Vec<Deploy>> {
        if environments.is_empty() {
            return Ok(vec![]);
        }
        let marks = vec!["?"; environments.len()].join(", ");
        self.c().query_map(
            &format!(
                "SELECT {DEPLOY_COLS} FROM deploys WHERE state = 'pending' AND environment_id IN ({marks})
                 ORDER BY created_at, id"
            ),
            &super::sql::list(environments),
            deploy_row,
        )
    }
}

/// What deleting an account takes from approvals: those of environments
/// that went with its projects. Elsewhere they stay as signed, as their
/// events do: rewriting one would break its signature and its chain.
pub(crate) fn erase_account(c: &impl Sql) -> Result<()> {
    c.execute("DELETE FROM deploy_approvals WHERE environment_id NOT IN (SELECT id FROM environments)", params![])?;
    Ok(())
}
