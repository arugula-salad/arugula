//! A deploy's approval (M218, platform step 7): S37's work approval (q3)
//! over a deploy. A project owner's passkey confirms it (DK4, aru-126), and
//! the platform stores it beside the deploy and checks it, but holds no key
//! that could make one.
//!
//! **The operation** is what's approved: the project, the environment (its
//! id and its name, so the name a person reads is in the hashed bytes), the
//! plan's digest and the source, and the hash of the deploy's `planned` event in its
//! environment's log (M217's hash chain), so the owner's approval covers the
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
//! or another plan. A deny is made the same way, so the record says who
//! denied it. Since DK4 (#385, aru-126) it's v2: no device signs it, and its
//! signature is one user-verified passkey assertion whose WebAuthn challenge
//! is SHA-256 of these bytes, base64url ([`Approval::passkey_challenge`]),
//! from a passkey in the account's published passkey log ([`crate::passkey`]):
//!
//! ```text
//! arugula deploy approval v2
//! deploy <dep id>
//! op <operation hash, hex>
//! account <the approver's account>
//! decision <once|deny>
//! expires <ms: by when the apply must start>
//! ```
//!
//! v1 (M218), which the platform no longer takes but still reads and
//! checks, named the signing device too (`by <device id>`, its last line)
//! and carried that device's Ed25519 signature beside the passkey's
//! assertion.
//!
//! Values here are ids, hex and one-word references (the platform refuses a
//! deploy whose source holds a space or a newline), so none is base64. Each
//! first line is its domain separator, pinned in `frozen.rs`; browsers
//! (`web/src/shell/e2e/deploy.ts`) make the same bytes.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[path = "deploy_apps_platform.rs"]
pub mod apps_platform;
#[path = "deploy_planter.rs"]
pub mod planter;

use std::collections::BTreeMap;

use crate::{cert::verify_hex, hosted::Owners, keys::DeviceKeys, team::TeamRole};

/// An approval's longest life (S37 q4: at most 7 days), from when it's
/// taken, with S37's ten minutes' slack for the approver's clock.
pub const MAX_LIFE_MS: u64 = 7 * 24 * 60 * 60 * 1000 + SLACK_MS;
/// What a page asks for unless told otherwise (S37 q4: 24 hours).
pub const DEFAULT_LIFE_MS: u64 = 24 * 60 * 60 * 1000;
/// The approver's clock may be this far off.
pub const SLACK_MS: u64 = 10 * 60 * 1000;

/// What a deploy applies, as approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// The plan a driver makes for a deploy (M219): the text a person reads
/// before they approve, whose SHA-256 is the deploy's `plan`. It names the
/// driver, so the operation an owner signs (which holds the plan's digest)
/// binds the driver too: a box recomputes it from the operation and the
/// driver it's asked to run ([`plan_digest`]), and refuses another.
pub fn plan_text(
    driver: &str,
    environment_name: &str,
    source_kind: &str,
    source_ref: &str,
    source_where: &str,
) -> String {
    let mut text = format!(
        "arugula deploy plan v1\ndriver {driver}\nenvironment {environment_name}\nsource {source_kind} {source_ref}\nwhere {source_where}\n"
    );
    // A driver that knows more of what it'll change says it (M222).
    if driver == apps_platform::DRIVER {
        text.push_str(&apps_platform::plan_lines(source_kind, source_ref, source_where));
    }
    text
}

/// SHA-256 of [`plan_text`], hex.
pub fn plan_digest(
    driver: &str,
    environment_name: &str,
    source_kind: &str,
    source_ref: &str,
    source_where: &str,
) -> String {
    hex::encode(Sha256::digest(plan_text(driver, environment_name, source_kind, source_ref, source_where).as_bytes()))
}

impl Operation {
    /// Whether its plan is `driver`'s plan for it ([`plan_text`]).
    pub fn planned_for(&self, driver: &str) -> bool {
        self.plan
            == plan_digest(driver, &self.environment_name, &self.source_kind, &self.source_ref, &self.source_where)
    }
}

/// Names a deploy secret can't have (M219): what the driver's shell and
/// its own environment use, and what changes how a shell or the dynamic
/// linker behaves. Checked where a secret is named, and on the box.
pub const RESERVED_SECRET_NAMES: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "LANG",
    "LANGUAGE",
    "PWD",
    "OLDPWD",
    "IFS",
    "ENV",
    "BASH_ENV",
    "CDPATH",
    "GLOBIGNORE",
    "SHELLOPTS",
    "BASHOPTS",
    "PS4",
    "PROMPT_COMMAND",
    "TMPDIR",
    "HOSTNAME",
    "MAIL",
    "POSIXLY_CORRECT",
    // What changes how node, git, ssh, curl or a language runtime behaves
    // inside the driver itself (M220's review).
    "NODE_OPTIONS",
    "NODE_EXTRA_CA_CERTS",
    "NODE_PATH",
    "SSH_ASKPASS",
    "PERL5LIB",
    "PERL5OPT",
    "RUBYOPT",
    "RUBYLIB",
];
/// Prefixes a deploy secret's name can't have: the deploy's own variables
/// (`DEPLOY_*`), the dynamic linker's, bash's, the locale's, Arugula's,
/// sudo's, git's (`GIT_CONFIG_*`, `GIT_SSH_COMMAND`), npm's, node's, TLS's
/// and curl's, Python's.
pub const RESERVED_SECRET_PREFIXES: &[&str] =
    &["DEPLOY_", "LD_", "BASH_", "LC_", "ARUGULA_", "SUDO_", "GIT_", "NPM_CONFIG_", "NODE_", "SSL_", "CURL_", "PYTHON"];
/// Suffixes a deploy secret's name can't have: proxies (`HTTPS_PROXY`,
/// `ALL_PROXY`, `NO_PROXY`).
pub const RESERVED_SECRET_SUFFIXES: &[&str] = &["_PROXY"];

/// The secret names each driver takes (M219, M220's review), the tokens
/// the platform mints for its runs among them: a driver's environment
/// holds only these, and a box hands its driver only these.
/// `None`: there's no such driver.
pub fn driver_secrets(driver: &str) -> Option<&'static [&'static str]> {
    Some(match driver {
        "fake" => &["APP_TOKEN"],
        "chant-fly" => &["FLY_API_TOKEN", "GITHUB_TOKEN"],
        // The list's write token (minted unless the environment holds its
        // own) and the app's read token (always minted, M222).
        "apps-platform" => &["APPS_PLATFORM_TOKEN", "SOURCE_TOKEN"],
        // Arugula's own release (M221): the App's tokens for the repository
        // (one writes, one follows the workflow), always minted; the
        // environment holds nothing of its own.
        "arugula-release" => &["RELEASE_TOKEN", "RELEASE_READ_TOKEN"],
        // planter's (M233): the tokens planter's App mints for the run.
        d if planter::is_planter(d) => return planter::secrets(d),
        _ => return None,
    })
}

/// Whether `driver` takes a secret called `name`: one it declares, and
/// not reserved.
pub fn driver_takes(driver: &str, name: &str) -> bool {
    secret_name_ok(name) && driver_secrets(driver).is_some_and(|n| n.contains(&name))
}

/// Whether `name` may name a deploy secret: an environment variable's name
/// (`A-Z`, `0-9`, `_`, 1 to 64, not a digit first) that isn't reserved.
pub fn secret_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
        && !RESERVED_SECRET_NAMES.contains(&name)
        && !RESERVED_SECRET_PREFIXES.iter().any(|p| name.starts_with(p))
        && !RESERVED_SECRET_SUFFIXES.iter().any(|x| name.ends_with(x))
}

/// Approve it once, or deny it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Approval {
    pub deploy: String,
    /// The operation's hash, hex.
    pub op: String,
    pub account: String,
    pub decision: Decision,
    /// Ms since the epoch.
    pub expires: u64,
    /// v1: the signing device's id. Empty in v2, which no device signs.
    #[serde(default)]
    pub by: String,
    /// v1: Ed25519 signature by `by` over [`Approval::body`], hex. Empty in
    /// v2, whose passkey assertion is its signature.
    #[serde(default)]
    pub sig: String,
    /// Its format, [`V1`] or [`V2`]: absent is v1 (every approval from
    /// before DK4).
    #[serde(default = "v1", skip_serializing_if = "is_v1")]
    pub v: u8,
}

/// M218's approval: a device's signature over its bytes, with a passkey's
/// assertion beside it. Read and checked for history only (aru-126).
pub const V1: u8 = 1;
/// DK4's (#385, aru-126): a user-verified passkey assertion over its bytes
/// is the whole of it. The only kind the platform takes now.
pub const V2: u8 = 2;

fn v1() -> u8 {
    V1
}

fn is_v1(v: &u8) -> bool {
    *v == V1
}

impl Approval {
    /// A v1 approval (or deny) of `deploy`'s `op` by `account`, signed by
    /// `signer`: what the platform took before DK4, made now only for
    /// history and its tests.
    pub fn v1(
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
            v: V1,
        };
        a.sig = hex::encode(signer.signature(a.body().as_bytes()));
        a
    }

    /// A v2 approval (or deny) of `deploy`'s `op` by `account`: nothing
    /// signs it here. Its signature is a passkey's assertion over
    /// [`Approval::passkey_challenge`], kept beside it.
    pub fn v2(deploy: &str, op: &Operation, account: &str, decision: Decision, expires: u64) -> Self {
        Self {
            deploy: deploy.to_owned(),
            op: op.hash(),
            account: account.to_owned(),
            decision,
            expires,
            by: String::new(),
            sig: String::new(),
            v: V2,
        }
    }

    /// The bytes it's over. Frozen once shipped: see `frozen.rs`.
    pub fn body(&self) -> String {
        match self.v {
            V1 => format!(
                "arugula deploy approval v1\ndeploy {}\nop {}\naccount {}\ndecision {}\nexpires {}\nby {}\n",
                self.deploy,
                self.op,
                self.account,
                self.decision.as_str(),
                self.expires,
                self.by
            ),
            _ => format!(
                "arugula deploy approval v{}\ndeploy {}\nop {}\naccount {}\ndecision {}\nexpires {}\n",
                self.v,
                self.deploy,
                self.op,
                self.account,
                self.decision.as_str(),
                self.expires
            ),
        }
    }

    /// No field holds a space or a newline, so it has one body only: a v1
    /// names its device and signature, a v2 neither. Any other version
    /// isn't one.
    pub fn well_formed(&self) -> bool {
        let word = |f: &String| !f.is_empty() && f.len() <= 200 && f.bytes().all(|b| b.is_ascii_graphic());
        match self.v {
            V1 => [&self.deploy, &self.op, &self.account, &self.by, &self.sig].into_iter().all(word),
            V2 => {
                [&self.deploy, &self.op, &self.account].into_iter().all(word)
                    && self.by.is_empty()
                    && self.sig.is_empty()
            }
            _ => false,
        }
    }

    /// A v1's signature is `sign_hex`'s (an Ed25519 public key, hex) over
    /// [`Approval::body`]. Never a v2, which no device signs.
    pub fn signed_by(&self, sign_hex: &str) -> bool {
        self.v == V1 && self.well_formed() && verify_hex(sign_hex, self.body().as_bytes(), &self.sig)
    }

    /// The WebAuthn challenge its passkey assertion signs (M218): SHA-256
    /// of [`Approval::body`], base64url without padding.
    pub fn passkey_challenge(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(self.body().as_bytes()))
    }

    /// Its passkey assertion checked against `account`'s passkey `log`
    /// (DK3) at `at`, from an origin `origins` allows: the one check that
    /// makes an approval a person's (aru-126), v1 or v2. A v1's device
    /// signature isn't checked: it adds nothing a passkey doesn't.
    pub fn passkey_checks<'a>(
        &self,
        assertion: &crate::passkey::Assertion,
        log: &'a crate::passkey::Log,
        at: u64,
        origins: &crate::passkey::Origins,
    ) -> Result<crate::passkey::Verified<'a>, crate::passkey::Refusal> {
        crate::passkey::verify(log, &self.account, assertion, &self.passkey_challenge(), at, origins)
    }
}

/// What a machine pinned that a deploy's approval is checked against
/// (M219, DK4): its project's owners, their passkey logs (by account, each
/// only ever extended: `arugula_e2e::passkey::extends`), and which origins
/// may use each RP's passkeys.
#[derive(Clone, Copy)]
pub struct Pinned<'a> {
    pub owners: &'a Owners<'a>,
    pub passkey_logs: &'a BTreeMap<String, crate::passkey::Log>,
    pub origins: &'a crate::passkey::Origins,
}

/// Why a machine won't apply a deploy (M219): what it checked itself,
/// from what it pinned, rather than take the platform's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyRefusal {
    /// The approval isn't well formed.
    Form,
    /// It's another deploy's.
    Deploy,
    /// It signs another operation than the one the run applies, or the
    /// operation is another project's.
    Operation,
    /// It's a deny.
    Denied,
    /// Its time to start has passed.
    Expired,
    /// Its account isn't an owner of the project, as the machine knows
    /// them (its team's roster, or its account's).
    NotAnOwner,
    /// The machine pinned no passkey log for its account, or the log the
    /// work carried isn't the pinned one, or one after it (one of them was
    /// rewritten).
    PasskeyLog,
    /// No passkey assertion came with it (`None`), or the account's
    /// passkey log refused it: the key not in the log, or removed by now,
    /// an origin the machine didn't pin, over other bytes, no user
    /// verification, a bad signature.
    Passkey(Option<crate::passkey::Refusal>),
}

impl ApplyRefusal {
    pub fn code(self) -> &'static str {
        match self {
            ApplyRefusal::Form => "approval_form",
            ApplyRefusal::Deploy => "approval_deploy",
            ApplyRefusal::Operation => "approval_operation",
            ApplyRefusal::Denied => "approval_denied",
            ApplyRefusal::Expired => "approval_expired",
            ApplyRefusal::NotAnOwner => "approval_not_an_owner",
            ApplyRefusal::PasskeyLog => "approval_passkey_log",
            ApplyRefusal::Passkey(_) => "approval_passkey",
        }
    }

    pub fn says(self) -> String {
        match self {
            ApplyRefusal::Form => "the deploy's approval isn't well formed".into(),
            ApplyRefusal::Deploy => "the approval is another deploy's".into(),
            ApplyRefusal::Operation => "the approval signs another operation than the one this run applies".into(),
            ApplyRefusal::Denied => "the deploy was denied, not approved".into(),
            ApplyRefusal::Expired => "the approval's time to start has passed: approve it again".into(),
            ApplyRefusal::NotAnOwner => {
                "the approver isn't an owner of this project, by what this machine pinned".into()
            }
            ApplyRefusal::PasskeyLog => {
                "the approver's passkey log isn't the one this machine pinned, or one after it".into()
            }
            ApplyRefusal::Passkey(None) => "the approval has no passkey assertion over it".into(),
            ApplyRefusal::Passkey(Some(r)) => format!("the approval's passkey doesn't check out: {r}"),
        }
    }
}

/// What a machine checks before it applies `deploy`'s operation `op` (M219;
/// DK4, aru-126), against what it pinned (`pinned`) and at `now` by its own
/// clock:
/// - the approval is well formed (v2, or a v1 made before DK4), `once`,
///   for this deploy, over exactly this operation, and not past its
///   `expires`;
/// - its account is an owner of the project, as the machine pinned them;
/// - its passkey assertion (`passkey`) checks out against the account's
///   passkey log: the newer of the one pinned and the one the work carried
///   (`carried`), when one extends the other ([`crate::passkey::newest`]);
///   the passkey in it and not removed by `now`, from an origin pinned for
///   its RP, a `webauthn.get` over exactly this approval's bytes, the
///   person verified, the signature good ([`Approval::passkey_checks`]).
///
/// No device signs it, and the devices that certified one play no part
/// (aru-126). A v1 approval (a device's signature with a passkey's
/// assertion, made before DK4, at most 7 days old) is checked by its
/// passkey alone, as a v2 is: its device's signature adds nothing a
/// passkey doesn't. Against whoever runs the platform this is
/// tamper-evident, not tamper-proof: control enrolled this machine and
/// sent what it pinned.
pub fn check_apply(
    approval: &Approval,
    op: &Operation,
    deploy: &str,
    pinned: &Pinned,
    passkey: Option<&crate::passkey::Assertion>,
    carried: Option<&crate::passkey::Log>,
    now: u64,
) -> Result<(), ApplyRefusal> {
    if !approval.well_formed() {
        return Err(ApplyRefusal::Form);
    }
    if approval.deploy != deploy {
        return Err(ApplyRefusal::Deploy);
    }
    if approval.op != op.hash() {
        return Err(ApplyRefusal::Operation);
    }
    if approval.decision != Decision::Once {
        return Err(ApplyRefusal::Denied);
    }
    if approval.expires <= now {
        return Err(ApplyRefusal::Expired);
    }
    if pinned.owners.role_of(&approval.account) != Some(TeamRole::Owner) {
        return Err(ApplyRefusal::NotAnOwner);
    }
    let assertion = passkey.ok_or(ApplyRefusal::Passkey(None))?;
    let held = pinned.passkey_logs.get(&approval.account).ok_or(ApplyRefusal::PasskeyLog)?;
    let log = match carried {
        Some(c) => crate::passkey::newest(held, c).ok_or(ApplyRefusal::PasskeyLog)?,
        None => held,
    };
    approval.passkey_checks(assertion, log, now, pinned.origins).map_err(|r| ApplyRefusal::Passkey(Some(r)))?;
    Ok(())
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
    fn a_v1_is_signed_by_its_device_only_and_bound_to_its_deploy_and_operation() {
        let k = DeviceKeys::generate();
        let other = DeviceKeys::generate();
        let a = Approval::v1(&k, "dep_1", &op(), "acct", Decision::Once, 5);
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
        let spaced = Approval { account: "a b".into(), ..a.clone() };
        assert!(!spaced.well_formed());
        // As stored before DK4: no `v`, read as v1.
        let mut json = serde_json::to_value(&a).unwrap();
        assert!(json.get("v").is_none());
        json.as_object_mut().unwrap().remove("v");
        assert_eq!(serde_json::from_value::<Approval>(json).unwrap(), a);
    }

    /// DK4: a v2 names no device and holds no signature; its bytes say v2,
    /// so no v1's bytes or challenge are ever a v2's.
    #[test]
    fn a_v2_is_its_bytes_alone() {
        let a = Approval::v2("dep_1", &op(), "acct", Decision::Once, 5);
        assert_eq!(
            a.body(),
            format!(
                "arugula deploy approval v2\ndeploy dep_1\nop {}\naccount acct\ndecision once\nexpires 5\n",
                op().hash()
            )
        );
        assert!(a.well_formed());
        assert!(!a.signed_by(&hex::encode(DeviceKeys::generate().sign_public())), "nothing signs a v2");
        let k = DeviceKeys::generate();
        let with_device = Approval { by: k.id(), ..a.clone() };
        assert!(!with_device.well_formed(), "a v2 names no device");
        let with_sig = Approval { sig: "00".into(), ..a.clone() };
        assert!(!with_sig.well_formed(), "nor holds a signature");
        assert!(!Approval { v: 3, ..a.clone() }.well_formed(), "no other version");
        let v1 = Approval::v1(&k, "dep_1", &op(), "acct", Decision::Once, 5);
        assert_ne!(a.passkey_challenge(), v1.passkey_challenge());
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["v"], 2);
        assert_eq!(serde_json::from_value::<Approval>(json).unwrap(), a);
    }

    /// The page checks the same fixture (`web/scripts/deploy.fixture.json`):
    /// the operation's bytes and hash, a v2 approval's bytes and passkey
    /// challenge, and a v1's bytes and signature from the same key (Ed25519
    /// signs the same bytes the same way every time).
    #[test]
    fn the_pages_fixture_is_these_bytes() {
        let k = DeviceKeys {
            noise_private: [0; 32],
            noise_public: [9; 32],
            sign: ed25519_dalek::SigningKey::from_bytes(&[7; 32]),
        };
        let dep = "dep_00112233445566778899aabbccddeeff";
        let a = Approval::v1(&k, dep, &op(), "acct1", Decision::Once, 1_800_000_000_000);
        let v2 = Approval::v2(dep, &op(), "acct1", Decision::Once, 1_800_000_000_000);
        let got = serde_json::json!({
            "seed": hex::encode([7u8; 32]),
            "sign": hex::encode(k.sign_public()),
            "operation": { "body": op().body(), "hash": op().hash() },
            "approval": a,
            "approval_body": a.body(),
            "approval_v2": v2,
            "approval_v2_body": v2.body(),
            "approval_v2_challenge": v2.passkey_challenge(),
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/scripts/deploy.fixture.json");
        if std::env::var_os("ARUGULA_WRITE_FIXTURE").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        }
        let want: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(got, want, "ARUGULA_WRITE_FIXTURE=1 writes it again, if the formats changed on purpose");
        assert!(a.signed_by(&hex::encode(k.sign_public())));
    }

    /// A test passkey (Ed25519) of `account`'s, its id `pk-<n>`.
    struct Pk {
        key: ed25519_dalek::SigningKey,
        id: String,
    }

    const RP: &str = "control.test";

    impl Pk {
        fn new(n: u8) -> Self {
            use base64::Engine as _;
            Pk {
                key: ed25519_dalek::SigningKey::from_bytes(&[n; 32]),
                id: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([n; 16]),
            }
        }

        /// Its `added` event in `account`'s log, at `at`, after `prev`.
        fn added(&self, account: &str, at: u64, prev: Option<&crate::passkey::Event>) -> crate::passkey::Event {
            crate::passkey::Draft {
                account: account.into(),
                event: crate::passkey::Change::Added,
                credential: self.id.clone(),
                rp: RP.into(),
                alg: crate::passkey::EDDSA,
                key: self.key.verifying_key().to_bytes().to_vec(),
                aaguid: crate::passkey::NO_AAGUID.into(),
                at,
                by: "signup".into(),
            }
            .after(prev)
        }

        /// Its assertion over `a`'s challenge, from `origin`, with `flags`.
        fn over(&self, a: &Approval, origin: &str, flags: u8) -> crate::passkey::Assertion {
            use base64::Engine as _;
            use ed25519_dalek::Signer as _;
            let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
            let cd =
                serde_json::json!({ "type": "webauthn.get", "challenge": a.passkey_challenge(), "origin": origin })
                    .to_string();
            let mut data = Sha256::digest(RP.as_bytes()).to_vec();
            data.push(flags);
            data.extend_from_slice(&[0, 0, 0, 0]);
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(cd.as_bytes()));
            crate::passkey::Assertion {
                id: self.id.clone(),
                client_data_json: b64.encode(cd.as_bytes()),
                authenticator_data: b64.encode(&data),
                signature: b64.encode(self.key.sign(&msg).to_bytes()),
            }
        }
    }

    /// M219, DK4: what a hosted machine checks before it applies a deploy,
    /// from what it pinned: the owners (a team's roster, or its account),
    /// their passkey logs and the origins. Only an owner's passkey that's in
    /// the log approves, once, over this deploy's operation, before it
    /// expires, from a pinned origin. No device plays a part: an owner whose
    /// every device is a CLI's approves with her passkey.
    #[test]
    fn a_machine_checks_the_approval_against_what_it_pinned() {
        use crate::{
            Cert, Kind, Trust,
            passkey::{Log, Origins, Refusal, origins_of_sites},
            team::{Member, Roster, TeamRole},
        };
        // a's only device is a CLI, its root: it approved nothing before DK4.
        let cli = DeviceKeys::generate();
        let mut root = Cert::new(&cli, "a", Kind::Cli, "geek");
        root.sign_with(&cli);
        let member = |a: &str, role| Member { account: a.into(), root: format!("{a}-root"), role, name: a.into() };
        let mut roster = Roster {
            v: 1,
            team: "t".into(),
            name: "t".into(),
            version: 1,
            at: 1,
            members: vec![member("a", TeamRole::Owner), member("b", TeamRole::Editor)],
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        roster.sign_with(&cli);
        let certs = crate::team::AccountCerts::new();
        let team = Owners::Team { roster: &roster, certs: &certs };
        let all = [root];
        let trust = Trust { account: "a".into(), root: cli.id() };
        let mine = Owners::Account { trust: &trust, certs: &all, revocations: &[] };
        // a's passkeys: k1 added at 100, k2 at 200, k1 removed by k2 at 300;
        // b's: kb. k9 is in no log.
        let (k1, k2, kb, k9) = (Pk::new(1), Pk::new(2), Pk::new(3), Pk::new(9));
        let mut log = Log { account: "a".into(), ..Default::default() };
        log.events.push(k1.added("a", 100, None));
        log.events.push(k2.added("a", 200, log.events.last()));
        let removal = log.events[0].removal(300, "reset").placed_after(log.events.last());
        let before_removal = log.clone();
        log.events.push(removal);
        let mut blog = Log { account: "b".into(), ..Default::default() };
        blog.events.push(kb.added("b", 100, None));
        let logs: BTreeMap<String, Log> = [("a".to_owned(), log.clone()), ("b".to_owned(), blog)].into();
        let origins = origins_of_sites(["https://control.test"]);
        let mut related = origins.clone();
        related.get_mut(RP).unwrap().push("https://app.test".into());
        let now = 1_000;
        let check = |owners: &Owners,
                     logs: &BTreeMap<String, Log>,
                     o: &Origins,
                     a: &Approval,
                     pk: Option<crate::passkey::Assertion>,
                     carried: Option<&Log>| {
            let pinned = Pinned { owners, passkey_logs: logs, origins: o };
            check_apply(a, &op(), "dep_1", &pinned, pk.as_ref(), carried, now)
        };
        let good = Approval::v2("dep_1", &op(), "a", Decision::Once, now + 1);
        let here = "https://control.test";
        for owners in [&team, &mine] {
            assert_eq!(check(owners, &logs, &origins, &good, Some(k2.over(&good, here, 0x05)), None), Ok(()));
            assert_eq!(check(owners, &logs, &origins, &good, Some(k2.over(&good, here, 0x05)), Some(&log)), Ok(()));
        }
        let pk = |p: &Pk, a: &Approval| Some(p.over(a, here, 0x05));
        // Every refusal, in order.
        let no = |r: Refusal| Err(ApplyRefusal::Passkey(Some(r)));
        let cases: Vec<(Approval, Option<crate::passkey::Assertion>, Result<(), ApplyRefusal>)> = vec![
            (Approval { by: "x".into(), ..good.clone() }, pk(&k2, &good), Err(ApplyRefusal::Form)),
            (Approval::v2("dep_2", &op(), "a", Decision::Once, now + 1), None, Err(ApplyRefusal::Deploy)),
            (Approval { op: "00".repeat(32), ..good.clone() }, None, Err(ApplyRefusal::Operation)),
            (Approval::v2("dep_1", &op(), "a", Decision::Deny, now + 1), None, Err(ApplyRefusal::Denied)),
            (Approval::v2("dep_1", &op(), "a", Decision::Once, now), None, Err(ApplyRefusal::Expired)),
            (good.clone(), None, Err(ApplyRefusal::Passkey(None))),
            (good.clone(), pk(&k9, &good), no(Refusal::NotLive)),
            (good.clone(), pk(&k1, &good), no(Refusal::NotLive)),
            (good.clone(), Some(k2.over(&good, "https://app.test", 0x05)), no(Refusal::Origin)),
            (good.clone(), Some(k2.over(&good, here, 0x01)), no(Refusal::User)),
            (
                good.clone(),
                pk(&k2, &Approval::v2("dep_1", &op(), "a", Decision::Once, now + 2)),
                no(Refusal::Challenge),
            ),
            (good.clone(), pk(&kb, &good), no(Refusal::NotLive)),
        ];
        for (a, p, want) in cases {
            for owners in [&team, &mine] {
                assert_eq!(check(owners, &logs, &origins, &a, p.clone(), None), want, "{a:?}");
            }
        }
        // The platform page's origin counts once the machine pinned it.
        let app = Some(k2.over(&good, "https://app.test", 0x05));
        assert_eq!(check(&team, &logs, &related, &good, app, None), Ok(()));
        // An editor's passkey doesn't approve, nor anyone's on another's
        // project.
        let b = Approval::v2("dep_1", &op(), "b", Decision::Once, now + 1);
        assert_eq!(check(&team, &logs, &origins, &b, pk(&kb, &b), None), Err(ApplyRefusal::NotAnOwner));
        assert_eq!(check(&mine, &logs, &origins, &b, pk(&kb, &b), None), Err(ApplyRefusal::NotAnOwner));
        // The log the work carries must be the pinned one or one after it.
        let pinned_before: BTreeMap<String, Log> = [("a".to_owned(), before_removal.clone())].into();
        let k1_good = pk(&k1, &good);
        assert_eq!(
            check(&team, &pinned_before, &origins, &good, k1_good.clone(), None),
            Ok(()),
            "pinned before the removal"
        );
        assert_eq!(
            check(&team, &pinned_before, &origins, &good, k1_good.clone(), Some(&log)),
            no(Refusal::NotLive),
            "a newer log carried: removed since"
        );
        assert_eq!(
            check(&team, &logs, &origins, &good, k1_good, Some(&before_removal)),
            no(Refusal::NotLive),
            "an older log carried: the pinned one's removal stands"
        );
        let mut rewritten = log.clone();
        rewritten.events.pop();
        rewritten.events.push(Pk::new(4).added("a", 400, rewritten.events.last()));
        assert_eq!(
            check(&team, &logs, &origins, &good, pk(&k2, &good), Some(&rewritten)),
            Err(ApplyRefusal::PasskeyLog),
            "a carried log that dropped a removal"
        );
        let none = BTreeMap::new();
        assert_eq!(
            check(&team, &none, &origins, &good, pk(&k2, &good), Some(&log)),
            Err(ApplyRefusal::PasskeyLog),
            "none pinned"
        );
        // A v1 from before DK4 applies by its passkey; its device isn't
        // looked at.
        let v1 = Approval::v1(&cli, "dep_1", &op(), "a", Decision::Once, now + 1);
        assert_eq!(check(&team, &logs, &origins, &v1, pk(&k2, &v1), None), Ok(()));
        assert_eq!(
            check(&team, &logs, &origins, &v1, pk(&k2, &good), None),
            no(Refusal::Challenge),
            "a v2's assertion"
        );
        assert_eq!(
            check(&team, &logs, &origins, &good, pk(&k2, &v1), None),
            no(Refusal::Challenge),
            "a v1's assertion"
        );
    }

    #[test]
    fn a_plan_names_its_driver_and_secrets_never_take_a_reserved_name() {
        let mut o = op();
        o.plan = plan_digest("fake", "production", "commit", "deadbeef", "acme/app");
        assert!(o.planned_for("fake"));
        assert!(!o.planned_for("chant-fly"), "another driver's plan");
        for ok in ["FLY_API_TOKEN", "GITHUB_TOKEN", "X1"] {
            assert!(secret_name_ok(ok), "{ok}");
        }
        for bad in [
            "PATH",
            "HOME",
            "IFS",
            "BASH_ENV",
            "ENV",
            "LD_PRELOAD",
            "DEPLOY_ID",
            "LC_ALL",
            "SHELL",
            "NODE_OPTIONS",
            "GIT_CONFIG_COUNT",
            "GIT_SSH_COMMAND",
            "NPM_CONFIG_USERCONFIG",
            "NODE_EXTRA_CA_CERTS",
            "SSL_CERT_FILE",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "1X",
            "a",
            "",
        ] {
            assert!(!secret_name_ok(bad), "{bad}");
        }
        // Each driver takes only the names it declares.
        assert!(driver_takes("chant-fly", "FLY_API_TOKEN") && driver_takes("chant-fly", "GITHUB_TOKEN"));
        assert!(!driver_takes("chant-fly", "APPS_PLATFORM_TOKEN") && !driver_takes("fake", "FLY_API_TOKEN"));
        assert!(driver_takes("apps-platform", "APPS_PLATFORM_TOKEN") && driver_takes("apps-platform", "SOURCE_TOKEN"));
        assert!(!driver_takes("planter", "APP_TOKEN"), "no such driver");
    }
}
