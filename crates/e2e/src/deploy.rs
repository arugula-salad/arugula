//! A deploy's approval (M218, platform step 7): S37's work approval (q3)
//! over a deploy. A project owner's device signs it, and the platform stores
//! it beside the deploy and checks it, but holds no key that could make one.
//!
//! **The operation** is what's approved: the project, the environment (its
//! id and its name, so the name a person reads is in the hashed bytes), the
//! plan's digest and the source, and the hash of the deploy's `planned` event in its
//! environment's log (M217's hash chain), so the owner's device signs the
//! log up to the plan. Its SHA-256 over these exact bytes is the operation
//! hash, as S37's `arugula op v1` is for a run:
//!
//! ```text
//! arugula deploy op v1
//! project <prj id>
//! environment <env id>
//! name <the environment's name>
//! plan <the plan's SHA-256, hex>
//! source <commit|artifact> <sha or digest>
//! where <repo, registry or bucket>
//! planned <the planned event's hash, hex>
//! ```
//!
//! Changing any byte (another plan, another commit, another environment, a
//! rewritten log) changes the hash, so it needs a new approval.
//!
//! **The approval** names the deploy **and** the operation's hash, as S37's
//! names the request and the operation, so it can't move to another deploy
//! or another plan. A deny is signed too, so the record says who denied it.
//!
//! ```text
//! arugula deploy approval v1
//! deploy <dep id>
//! op <operation hash, hex>
//! account <the approver's account>
//! decision <once|deny>
//! expires <ms: by when the apply must start>
//! by <approver's device id>
//! ```
//!
//! Values here are ids, hex and one-word references (the platform refuses a
//! deploy whose source holds a space or a newline), so none is base64. Each
//! first line is its domain separator, pinned in `frozen.rs`; browsers
//! (`web/src/shell/e2e/deploy.ts`) sign the same bytes.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{cert::verify_hex, keys::DeviceKeys};

/// An approval's longest life (S37 q4: at most 7 days), from when it's
/// taken, with S37's ten minutes' slack for the approver's clock.
pub const MAX_LIFE_MS: u64 = 7 * 24 * 60 * 60 * 1000 + SLACK_MS;
/// What a page asks for unless told otherwise (S37 q4: 24 hours).
pub const DEFAULT_LIFE_MS: u64 = 24 * 60 * 60 * 1000;
/// The approver's clock may be this far off.
pub const SLACK_MS: u64 = 10 * 60 * 1000;

/// What a deploy applies, as approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub project: String,
    pub environment: String,
    /// The environment's name (`production`): a-z, 0-9 and `-`.
    pub environment_name: String,
    /// The plan's SHA-256, hex.
    pub plan: String,
    /// `commit` or `artifact`.
    pub source_kind: String,
    /// The commit's sha or the artifact's digest.
    pub source_ref: String,
    /// Where it is.
    pub source_where: String,
    /// The hash of the deploy's `planned` event, hex.
    pub planned: String,
}

impl Operation {
    pub fn body(&self) -> String {
        // Frozen once shipped: see `frozen.rs`.
        format!(
            "arugula deploy op v1\nproject {}\nenvironment {}\nname {}\nplan {}\nsource {} {}\nwhere {}\nplanned {}\n",
            self.project,
            self.environment,
            self.environment_name,
            self.plan,
            self.source_kind,
            self.source_ref,
            self.source_where,
            self.planned
        )
    }

    /// The operation hash: SHA-256 of [`Operation::body`], hex.
    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(self.body().as_bytes()))
    }
}

/// Approve it once, or deny it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Once,
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Once => "once",
            Decision::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub deploy: String,
    /// The operation's hash, hex.
    pub op: String,
    pub account: String,
    pub decision: Decision,
    /// Ms since the epoch.
    pub expires: u64,
    /// The signing device's id.
    pub by: String,
    /// Ed25519 signature by `by` over [`Approval::body`], hex.
    pub sig: String,
}

impl Approval {
    /// An approval (or a deny) of `deploy`'s `op` by `account`, signed by
    /// `signer`.
    pub fn new(
        signer: &DeviceKeys,
        deploy: &str,
        op: &Operation,
        account: &str,
        decision: Decision,
        expires: u64,
    ) -> Self {
        let mut a = Self {
            deploy: deploy.to_owned(),
            op: op.hash(),
            account: account.to_owned(),
            decision,
            expires,
            by: signer.id(),
            sig: String::new(),
        };
        a.sig = hex::encode(signer.signature(a.body().as_bytes()));
        a
    }

    pub fn body(&self) -> String {
        // Frozen once shipped: see `frozen.rs`.
        format!(
            "arugula deploy approval v1\ndeploy {}\nop {}\naccount {}\ndecision {}\nexpires {}\nby {}\n",
            self.deploy,
            self.op,
            self.account,
            self.decision.as_str(),
            self.expires,
            self.by
        )
    }

    /// No field holds a space or a newline, so it has one body only.
    pub fn well_formed(&self) -> bool {
        [&self.deploy, &self.op, &self.account, &self.by, &self.sig]
            .iter()
            .all(|f| !f.is_empty() && f.len() <= 200 && f.bytes().all(|b| b.is_ascii_graphic()))
    }

    /// The signature is `sign_hex`'s (an Ed25519 public key, hex) over
    /// [`Approval::body`].
    pub fn signed_by(&self, sign_hex: &str) -> bool {
        self.well_formed() && verify_hex(sign_hex, self.body().as_bytes(), &self.sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op() -> Operation {
        Operation {
            project: "prj_1".into(),
            environment: "env_1".into(),
            environment_name: "production".into(),
            plan: "ab".repeat(32),
            source_kind: "commit".into(),
            source_ref: "deadbeef".into(),
            source_where: "acme/app".into(),
            planned: "ef".repeat(32),
        }
    }

    #[test]
    fn the_bytes_and_the_hash_are_pinned() {
        let body = op().body();
        assert_eq!(
            body,
            format!(
                "arugula deploy op v1\nproject prj_1\nenvironment env_1\nname production\nplan {}\nsource commit \
                 deadbeef\nwhere acme/app\nplanned {}\n",
                "ab".repeat(32),
                "ef".repeat(32)
            )
        );
        assert_eq!(op().hash(), hex::encode(Sha256::digest(body.as_bytes())));
    }

    #[test]
    fn signed_by_its_device_only_and_bound_to_its_deploy_and_operation() {
        let k = DeviceKeys::generate();
        let other = DeviceKeys::generate();
        let a = Approval::new(&k, "dep_1", &op(), "acct", Decision::Once, 5);
        assert!(a.signed_by(&hex::encode(k.sign_public())));
        assert!(!a.signed_by(&hex::encode(other.sign_public())));
        assert!(a.body().starts_with("arugula deploy approval v1\ndeploy dep_1\n"));
        let moved = Approval { deploy: "dep_2".into(), ..a.clone() };
        assert!(!moved.signed_by(&hex::encode(k.sign_public())), "moved to another deploy");
        let mut changed = op();
        changed.plan = "cd".repeat(32);
        let replanned = Approval { op: changed.hash(), ..a.clone() };
        assert!(!replanned.signed_by(&hex::encode(k.sign_public())), "another plan");
        let denied = Approval { decision: Decision::Deny, ..a.clone() };
        assert!(!denied.signed_by(&hex::encode(k.sign_public())), "a once isn't a deny");
        let spaced = Approval { account: "a b".into(), ..a };
        assert!(!spaced.well_formed());
    }

    /// The page checks the same fixture (`web/scripts/deploy.fixture.json`):
    /// the operation's bytes and hash, and the approval's bytes and
    /// signature from the same key (Ed25519 signs the same bytes the same
    /// way every time).
    #[test]
    fn the_pages_fixture_is_these_bytes() {
        let k = DeviceKeys {
            noise_private: [0; 32],
            noise_public: [9; 32],
            sign: ed25519_dalek::SigningKey::from_bytes(&[7; 32]),
        };
        let a = Approval::new(
            &k,
            "dep_00112233445566778899aabbccddeeff",
            &op(),
            "acct1",
            Decision::Once,
            1_800_000_000_000,
        );
        let got = serde_json::json!({
            "seed": hex::encode([7u8; 32]),
            "sign": hex::encode(k.sign_public()),
            "operation": { "body": op().body(), "hash": op().hash() },
            "approval": a,
            "approval_body": a.body(),
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/scripts/deploy.fixture.json");
        if std::env::var_os("ARUGULA_WRITE_FIXTURE").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        }
        let want: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(got, want, "ARUGULA_WRITE_FIXTURE=1 writes it again, if the formats changed on purpose");
        assert!(a.signed_by(&hex::encode(k.sign_public())));
    }
}
