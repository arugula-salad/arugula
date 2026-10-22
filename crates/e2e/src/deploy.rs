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

use crate::{Cert, Trust, cert::verify_hex, hosted::Owners, keys::DeviceKeys, team::TeamRole};

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
    format!(
        "arugula deploy plan v1\ndriver {driver}\nenvironment {environment_name}\nsource {source_kind} {source_ref}\nwhere {source_where}\n"
    )
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

/// The secret names each driver takes (M219, M220's review): a driver's
/// environment holds only these, and a box hands its driver only these.
/// `None`: there's no such driver.
pub fn driver_secrets(driver: &str) -> Option<&'static [&'static str]> {
    Some(match driver {
        "fake" => &["APP_TOKEN"],
        "chant-fly" => &["FLY_API_TOKEN", "GITHUB_TOKEN"],
        "apps-platform" => &["APPS_PLATFORM_TOKEN"],
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

    /// The WebAuthn challenge its passkey assertion signs (M218): SHA-256
    /// of [`Approval::body`], base64url without padding.
    pub fn passkey_challenge(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(self.body().as_bytes()))
    }
}

/// Whether `device` may approve a deploy for `account` (M218): it, and
/// every device that certified it back to the account's `root`, is a
/// browser's or a phone's own (never a CLI's or the desktop app's, whose
/// keys a program on its computer can read), by [`Trust::chained`] over
/// every certificate kept on the way. The platform asks it before it takes
/// an approval, and a hosted machine before it applies one (M219).
pub fn approver_chained(account: &str, root: &str, certs: &[Cert], device: &str) -> bool {
    let trust = Trust { account: account.to_owned(), root: root.to_owned() };
    trust.chained(certs, device, |c| {
        crate::cert::browser_link(c, device, Cert::is_browser) && (c.device != root || c.is_browser())
    })
}

/// The parts of a passkey's assertion a machine can check without the
/// passkey's public key (which only the hub keeps): each base64url, as the
/// approval's record keeps them.
#[derive(Debug, Clone, Copy)]
pub struct PasskeyParts<'a> {
    pub client_data_json: &'a str,
    pub authenticator_data: &'a str,
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
    /// Its device isn't one of the owner's that counts: trusted now, and a
    /// browser certified by browsers to their root.
    Device,
    /// The signature isn't that device's over the approval.
    Signature,
    /// No passkey assertion over this approval, with user verification.
    Passkey,
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
            ApplyRefusal::Device => "approval_device",
            ApplyRefusal::Signature => "approval_signature",
            ApplyRefusal::Passkey => "approval_passkey",
        }
    }

    pub fn says(self) -> &'static str {
        match self {
            ApplyRefusal::Form => "the deploy's approval isn't well formed",
            ApplyRefusal::Deploy => "the approval is another deploy's",
            ApplyRefusal::Operation => "the approval signs another operation than the one this run applies",
            ApplyRefusal::Denied => "the deploy was denied, not approved",
            ApplyRefusal::Expired => "the approval's time to start has passed: approve it again",
            ApplyRefusal::NotAnOwner => "the approver isn't an owner of this project, by what this machine pinned",
            ApplyRefusal::Device => {
                "the approving device isn't one of the owner's browsers (trusted now, certified by browsers to their root)"
            }
            ApplyRefusal::Signature => "the approval isn't signed by its device",
            ApplyRefusal::Passkey => "the approval has no passkey assertion over it, with user verification",
        }
    }
}

/// What a machine checks before it applies `deploy`'s operation `op` (M219),
/// with the owners it pinned at enrollment (`owners`: its team's roster
/// and members' certificates, or its account's chain), at `now` by its own
/// clock:
/// - the approval is `once`, for this deploy, over exactly this operation,
///   and not past its `expires`;
/// - its account is an owner of the project, and its device one of that
///   owner's, trusted now, a browser certified by browsers to their root
///   ([`approver_chained`]), whose signature over the approval checks;
/// - a passkey assertion came with it whose ceremony is `webauthn.get`
///   over exactly this approval's challenge, with user presence and
///   verification. Its signature is the hub's to check (it keeps passkeys'
///   public keys); until the hub publishes them, the machine checks the
///   rest.
pub fn check_apply(
    approval: &Approval,
    op: &Operation,
    deploy: &str,
    owners: &Owners,
    passkey: Option<PasskeyParts>,
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
    if owners.role_of(&approval.account) != Some(TeamRole::Owner) {
        return Err(ApplyRefusal::NotAnOwner);
    }
    let (trusted, root, all) = match owners {
        Owners::Team { roster, certs } => {
            let root = roster.member(&approval.account).map(|m| m.root.clone()).unwrap_or_default();
            let all = certs.get(&approval.account).map(|(c, _)| c.as_slice()).unwrap_or_default();
            (roster.devices(&approval.account, certs), root, all)
        }
        Owners::Account { trust, certs, revocations } => {
            let t = Trust { account: approval.account.clone(), root: trust.root.clone() };
            (t.evaluate(certs, revocations), trust.root.clone(), *certs)
        }
    };
    let Some(cert) = trusted.get(&approval.by) else { return Err(ApplyRefusal::Device) };
    if !approver_chained(&approval.account, &root, all, &approval.by) {
        return Err(ApplyRefusal::Device);
    }
    if !approval.signed_by(&cert.sign) {
        return Err(ApplyRefusal::Signature);
    }
    let pk = passkey.ok_or(ApplyRefusal::Passkey)?;
    if !passkey_binds(approval, pk) {
        return Err(ApplyRefusal::Passkey);
    }
    Ok(())
}

/// Whether a passkey assertion's client data and authenticator data are
/// over `approval`: a `webauthn.get` whose challenge is its
/// [`Approval::passkey_challenge`], user present and verified.
pub fn passkey_binds(approval: &Approval, pk: PasskeyParts) -> bool {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let Ok(raw) = b64.decode(pk.client_data_json.trim_end_matches('=')) else { return false };
    let Ok(cd) = serde_json::from_slice::<serde_json::Value>(&raw) else { return false };
    let Ok(data) = b64.decode(pk.authenticator_data.trim_end_matches('=')) else { return false };
    cd["type"] == "webauthn.get"
        && cd["challenge"].as_str() == Some(approval.passkey_challenge().as_str())
        && data.len() >= 37
        && data[32] & 0x05 == 0x05
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

    /// A passkey assertion's parts over `a`, as a browser gives them:
    /// `webauthn.get` over its challenge, with `flags`.
    fn passkey_over(a: &Approval, kind: &str, flags: u8) -> (String, String) {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let cd = serde_json::json!({ "type": kind, "challenge": a.passkey_challenge(), "origin": "https://c.test" });
        let mut data = Sha256::digest(b"c.test").to_vec();
        data.push(flags);
        data.extend_from_slice(&[0, 0, 0, 0]);
        (b64.encode(cd.to_string()), b64.encode(data))
    }

    /// M219: what a hosted machine checks before it applies a deploy, from
    /// the owners it pinned: a team's roster and certificates, or its
    /// account's chain. Only an owner's browser approves, once, over this
    /// deploy's operation, before it expires, confirmed with a passkey.
    #[test]
    fn a_machine_checks_the_approval_against_what_it_pinned() {
        use crate::{
            Cert, Kind, Revocation,
            team::{Member, Roster, TeamRole},
        };
        let phone = DeviceKeys::generate();
        let mut root = Cert::new(&phone, "a", Kind::Browser, "Pixel");
        root.sign_with(&phone);
        let cli = DeviceKeys::generate();
        let mut cli_cert = Cert::new(&cli, "a", Kind::Cli, "geek");
        cli_cert.sign_with(&phone);
        let gone = DeviceKeys::generate();
        let mut gone_cert = Cert::new(&gone, "a", Kind::Browser, "old laptop");
        gone_cert.sign_with(&phone);
        let revoked = Revocation::new("a", &gone.id(), &phone);
        let bphone = DeviceKeys::generate();
        let mut broot = Cert::new(&bphone, "b", Kind::Browser, "b's phone");
        broot.sign_with(&bphone);
        let member = |a: &str, root: &str, role| Member { account: a.into(), root: root.into(), role, name: a.into() };
        let mut roster = Roster {
            v: 1,
            team: "t".into(),
            name: "t".into(),
            version: 1,
            at: 1,
            members: vec![member("a", &phone.id(), TeamRole::Owner), member("b", &bphone.id(), TeamRole::Editor)],
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        roster.sign_with(&phone);
        let certs: crate::team::AccountCerts = [
            ("a".to_owned(), (vec![root.clone(), cli_cert.clone(), gone_cert.clone()], vec![revoked.clone()])),
            ("b".to_owned(), (vec![broot], vec![])),
        ]
        .into();
        let team = Owners::Team { roster: &roster, certs: &certs };
        let all = [root, cli_cert, gone_cert];
        let trust = Trust { account: "a".into(), root: phone.id() };
        let mine = Owners::Account { trust: &trust, certs: &all, revocations: std::slice::from_ref(&revoked) };
        let now = 1_000;
        let check = |owners: &Owners, a: &Approval, pk: Option<(String, String)>| {
            let parts = pk.as_ref().map(|(c, d)| PasskeyParts { client_data_json: c, authenticator_data: d });
            check_apply(a, &op(), "dep_1", owners, parts, now)
        };
        let good = Approval::new(&phone, "dep_1", &op(), "a", Decision::Once, now + 1);
        for owners in [&team, &mine] {
            assert_eq!(check(owners, &good, Some(passkey_over(&good, "webauthn.get", 0x05))), Ok(()));
        }
        let ok_pk = |a: &Approval| Some(passkey_over(a, "webauthn.get", 0x05));
        assert_eq!(check(&team, &good, None), Err(ApplyRefusal::Passkey), "no passkey");
        type Case = (Approval, Option<(String, String)>, ApplyRefusal);
        let cases: Vec<Case> = vec![
            (good.clone(), Some(passkey_over(&good, "webauthn.get", 0x01)), ApplyRefusal::Passkey),
            (good.clone(), Some(passkey_over(&good, "webauthn.create", 0x05)), ApplyRefusal::Passkey),
            (
                good.clone(),
                ok_pk(&Approval::new(&phone, "dep_1", &op(), "a", Decision::Once, now + 2)),
                ApplyRefusal::Passkey,
            ),
            (Approval::new(&phone, "dep_2", &op(), "a", Decision::Once, now + 1), None, ApplyRefusal::Deploy),
            (Approval { op: "00".repeat(32), ..good.clone() }, None, ApplyRefusal::Operation),
            (Approval::new(&phone, "dep_1", &op(), "a", Decision::Deny, now + 1), None, ApplyRefusal::Denied),
            (Approval::new(&phone, "dep_1", &op(), "a", Decision::Once, now), None, ApplyRefusal::Expired),
            (Approval::new(&cli, "dep_1", &op(), "a", Decision::Once, now + 1), None, ApplyRefusal::Device),
            (Approval::new(&gone, "dep_1", &op(), "a", Decision::Once, now + 1), None, ApplyRefusal::Device),
            (Approval { sig: "00".repeat(64), ..good.clone() }, None, ApplyRefusal::Signature),
        ];
        for (a, pk, want) in cases {
            let pk = pk.or_else(|| ok_pk(&a));
            assert_eq!(check(&team, &a, pk.clone()), Err(want), "{a:?}");
            assert_eq!(check(&mine, &a, pk), Err(want), "{a:?}");
        }
        // An editor's own browser doesn't approve.
        let b = Approval::new(&bphone, "dep_1", &op(), "b", Decision::Once, now + 1);
        assert_eq!(check(&team, &b, ok_pk(&b)), Err(ApplyRefusal::NotAnOwner));
        assert_eq!(check(&mine, &b, ok_pk(&b)), Err(ApplyRefusal::NotAnOwner), "not this account");
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
        assert!(!driver_takes("planter", "APP_TOKEN"), "no such driver");
    }
}
