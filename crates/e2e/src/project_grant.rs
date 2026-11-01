//! The standing project grant (M211, platform step 7): a project owner
//! says, once per project, that the platform may start runs and instruct
//! them on the project's hosted workspaces ("One standing grant per project
//! starts runs", aru-099). It's S37's standing grant
//! (`spikes/s37-team-permissions/README.md`, q3) with a project as its
//! resource: the platform stores and relays it, but can't make or widen
//! it, and hosted `arugulad` checks it against the project's delegation
//! (M178, [`crate::hosted`]) before it starts anything ([`check`]).
//!
//! What's signed is a fixed line format, as for delegations. Its own first
//! line, so a signature over it never reads as S37's machine grant (`arugula
//! exec grant v1`) or a delegation. Since DK6 (#387, aru-126 and aru-099 as
//! amended) a grant is **v2**: no device signs it, and its signature is one
//! user-verified passkey assertion whose WebAuthn challenge is SHA-256 of
//! these bytes, base64url ([`ProjectGrant::passkey_challenge`]), from a
//! passkey in the granter's published passkey log ([`crate::passkey`]):
//!
//! ```text
//! arugula project grant v2
//! id <32 hex>
//! project <prj_ id>
//! owner team:<id>|account:<id>
//! delegation <the project's delegation id>
//! resource hosted
//! to platform
//! caps <capability>        (once per capability, sorted)
//! made <ms>
//! expires <ms>
//! account <the granting owner's account>
//! ```
//!
//! Its revoke is the same (`v3`: `v2` is the device-signed revoke's):
//!
//! ```text
//! arugula project grant revoke v3
//! project <prj_ id>
//! delegation <the project's delegation id>
//! running <finish|stop>
//! at <ms>
//! account <the revoking owner's account>
//! ```
//!
//! **Before DK6** a grant was v1, signed by an owner's browser with its
//! device key: the same lines, but `expires <ms|never>` and `by <device
//! id>` in place of `account`; and a revoke was `arugula project grant
//! revoke v2`, with `by <device id>` last. The platform takes neither now.
//! A v1 grant it already holds still counts until it expires (at most 366
//! days after it was made; the page offered 30, 90 or 365), checked as it
//! was ([`Owners::browser_signed`]), and a device revoke kept or relayed
//! still counts; DK8 (#389) drops them once every v1 has expired.
//!
//! - **Who grants.** An owner of the project now, by the roster (or the
//!   account) the checker holds, with one of that account's passkeys that's
//!   in its passkey log and not removed, from an origin the checker allows
//!   for its RP, the person verified (aru-126). Which device the person
//!   used plays no part. A v1's signer was a browser of one of the
//!   project's owners, a browser all the way back to its account's root.
//! - **What it covers.** `runs.start` and `runs.instruct` ([`CAPS`]) on the
//!   hosted workspaces of the project, under the delegation it names. A
//!   new delegation needs a new grant. It never covers a deploy (aru-101).
//! - **Revoking** is S37's: a revoke by any owner, relayed by the platform.
//!   It names no grant: it takes back every grant for the project under its
//!   delegation made at or before its `at`, v1 or v2 (M211's first revoke
//!   named one grant id, which whoever relays it could choose), so a grant
//!   made after it is a new one. A hosted machine checks a revoke once,
//!   when it first hears it, and keeps it ([`Revokes`]): the platform can't
//!   bring a grant back by leaving the revoke out later, or by dropping its
//!   signer's certificate or passkey. It stops new runs; `running stop`
//!   also asks for the ones running to stop (decision 3: revoking and
//!   stopping are separate, so it says which).
//! - **Control can drop a revoke** (S37's threat model): a machine that
//!   never heard it keeps honouring the grant until it expires. Grants
//!   expire (90 days unless the owner picks), which bounds that.
//! - **A revoked delegation** stops every grant under it, on the machines
//!   enrolled under it.
//! - **What a machine checks a v2 against** is what it pinned
//!   ([`crate::deploy::Pinned`], as for a deploy's approval, DK4): its
//!   project's owners, their passkey logs as control sent them at
//!   enrollment and each refresh (each only ever extended), and the
//!   origins (only ever shrunk). Of a log the platform carries with a
//!   start, only removals after the pinned one count
//!   ([`crate::passkey::removed_since`]): an `added` event by sign-up or
//!   GitHub needs no proof, so the platform could add a key of its own.
//!   Against whoever runs control this is tamper-evident, not
//!   tamper-proof (aru-126).

use serde::{Deserialize, Serialize};

use crate::{
    cert::LegacyAppChildren,
    deploy::Pinned,
    hosted::{Delegation, Owner, Owners},
    passkey::{Assertion, Log},
    team::TeamRole,
};

/// What the grant gives, as signed: start runs, and instruct them.
pub const CAPS: &[&str] = &["runs.instruct", "runs.start"];
/// What the grant is over: the project's hosted workspaces.
pub const RESOURCE: &str = "hosted";
/// Who holds it: the platform, which starts runs under it.
pub const TO: &str = "platform";

/// M211's grant: an owner's browser's device signature over its bytes.
/// Checked until the last one expires (DK8, #389); never made now.
pub const V1: u8 = 1;
/// DK6's (#387, aru-126): a user-verified passkey assertion over its bytes
/// is the whole of its signature. The only kind the platform takes now.
pub const V2: u8 = 2;
/// M211's revoke (`arugula project grant revoke v2`), signed by an owner's
/// browser's device key: kept and relayed ones still count.
pub const REVOKE_DEVICE: u8 = 2;
/// DK6's revoke (`arugula project grant revoke v3`): a passkey assertion
/// over its bytes. It covers v1 grants as much as v2 ones.
pub const REVOKE_PASSKEY: u8 = 3;

fn v1() -> u8 {
    V1
}

fn is_v1(v: &u8) -> bool {
    *v == V1
}

fn revoke_device() -> u8 {
    REVOKE_DEVICE
}

fn is_revoke_device(v: &u8) -> bool {
    *v == REVOKE_DEVICE
}

fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn hex_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The WebAuthn challenge a passkey signs over `body`: SHA-256 of it,
/// base64url without padding, as a deploy's approval's (DK4).
fn challenge_of(body: &str) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(body.as_bytes()))
}

/// A new grant's id: 32 hex digits from 16 random bytes.
pub fn new_id() -> String {
    hex::encode(crate::random::<16>())
}

/// A project owner saying the platform may start and instruct runs on
/// `project`'s hosted workspaces, under `delegation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProjectGrant {
    pub id: String,
    pub project: String,
    /// `team:<id>` or `account:<id>`, as the granter saw it.
    pub owner: String,
    /// The project's delegation's [`Delegation::id`].
    pub delegation: String,
    /// Sorted, each once.
    pub caps: Vec<String>,
    pub made: u64,
    /// When it stops counting (ms); `None`: never (v1 only).
    #[serde(default)]
    pub expires: Option<u64>,
    /// v1: the signing device. Empty in v2, which no device signs.
    #[serde(default)]
    pub by: String,
    /// v1: its Ed25519 signature over [`ProjectGrant::body`], hex. Empty
    /// in v2.
    #[serde(default)]
    pub sig: String,
    /// v2: the granting owner's account, in the bytes. Empty in v1.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account: String,
    /// v2: the passkey's assertion over [`ProjectGrant::passkey_challenge`],
    /// its signature. Kept beside the bytes, not in them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passkey: Option<Assertion>,
    /// Its format, [`V1`] or [`V2`]: absent is v1 (every grant from before
    /// DK6).
    #[serde(default = "v1", skip_serializing_if = "is_v1")]
    pub v: u8,
}

impl ProjectGrant {
    /// A v1, unsigned: `CAPS` over the project `d` delegates, made `made`.
    /// The platform no longer takes one: for history and tests.
    pub fn new(d: &Delegation, made: u64, expires: Option<u64>) -> Self {
        Self {
            id: new_id(),
            project: d.project.clone(),
            owner: d.owner.clone(),
            delegation: d.id(),
            caps: CAPS.iter().map(|c| (*c).to_owned()).collect(),
            made,
            expires,
            by: String::new(),
            sig: String::new(),
            account: String::new(),
            passkey: None,
            v: V1,
        }
    }

    /// A v2 by `account`: `CAPS` over the project `d` delegates, made
    /// `made`, until `expires`. Nothing signs it here: its signature is a
    /// passkey's assertion over [`ProjectGrant::passkey_challenge`].
    pub fn v2(d: &Delegation, account: &str, made: u64, expires: u64) -> Self {
        Self { account: account.to_owned(), v: V2, ..Self::new(d, made, Some(expires)) }
    }

    /// The bytes it's over. Frozen once shipped: see `frozen.rs`.
    pub fn body(&self) -> String {
        let mut s = format!(
            "arugula project grant v{}\nid {}\nproject {}\nowner {}\ndelegation {}\nresource {RESOURCE}\nto {TO}\n",
            self.v, self.id, self.project, self.owner, self.delegation
        );
        for c in &self.caps {
            s.push_str(&format!("caps {c}\n"));
        }
        let expires = self.expires.map_or_else(|| "never".to_owned(), |e| e.to_string());
        let (signer, who) = if self.v == V1 { ("by", &self.by) } else { ("account", &self.account) };
        s.push_str(&format!("made {}\nexpires {expires}\n{signer} {who}\n", self.made));
        s
    }

    /// The grant `body` is: what the platform stores is the bytes, and this
    /// reads them back, a v1 with its device's `sig`, a v2 with none (its
    /// passkey is kept beside it, and the caller puts it back). `None` when
    /// they aren't exactly a grant's (another body, extra lines, a field
    /// out of place), or a v2 is handed a device signature.
    pub fn parse(body: &str, sig: &str) -> Option<Self> {
        let mut lines = body.strip_suffix('\n')?.split('\n');
        let mut field = |name: &str| lines.next()?.strip_prefix(name)?.strip_prefix(' ').map(str::to_owned);
        let v = match field("arugula project grant")?.as_str() {
            "v1" => V1,
            "v2" if sig.is_empty() => V2,
            _ => return None,
        };
        let (id, project, owner, delegation) = (field("id")?, field("project")?, field("owner")?, field("delegation")?);
        (field("resource")? == RESOURCE && field("to")? == TO).then_some(())?;
        let rest: Vec<&str> = body.lines().skip(7).collect();
        let caps_n = rest.iter().take_while(|l| l.starts_with("caps ")).count();
        let caps = rest[..caps_n].iter().map(|l| l["caps ".len()..].to_owned()).collect();
        let tail = &rest[caps_n..];
        let [made, expires, signer] = tail else { return None };
        let made = made.strip_prefix("made ")?.parse().ok()?;
        let expires = match expires.strip_prefix("expires ")? {
            "never" => None,
            e => Some(e.parse().ok()?),
        };
        let (by, account) = match v {
            V1 => (signer.strip_prefix("by ")?.to_owned(), String::new()),
            _ => (String::new(), signer.strip_prefix("account ")?.to_owned()),
        };
        let g = Self {
            id,
            project,
            owner,
            delegation,
            caps,
            made,
            expires,
            by,
            sig: sig.to_owned(),
            account,
            passkey: None,
            v,
        };
        (g.body() == body).then_some(g)
    }

    /// Well formed: its ids, its owner, and capabilities that are known,
    /// sorted and each once; a v1 names its device, a v2 its account and
    /// an expiry, and no device or signature. Says nothing about the
    /// signature.
    pub fn well_formed(&self) -> bool {
        let signer = match self.v {
            V1 => token(&self.by),
            V2 => token(&self.account) && self.by.is_empty() && self.sig.is_empty() && self.expires.is_some(),
            _ => false,
        };
        signer
            && hex_id(&self.id)
            && token(&self.project)
            && Owner::parse(&self.owner).is_some()
            && token(&self.delegation)
            && !self.caps.is_empty()
            && self.caps.windows(2).all(|w| w[0] < w[1])
            && self.caps.iter().all(|c| CAPS.contains(&c.as_str()))
            && self.expires.is_none_or(|e| e > self.made)
    }

    /// Sign a v1 with a device's keys: for history and tests only.
    pub fn sign_with(&mut self, keys: &crate::DeviceKeys) {
        self.by = keys.id();
        self.sig = hex::encode(keys.signature(self.body().as_bytes()));
    }

    /// The WebAuthn challenge a v2's passkey assertion signs: SHA-256 of
    /// [`ProjectGrant::body`], base64url without padding.
    pub fn passkey_challenge(&self) -> String {
        challenge_of(&self.body())
    }

    /// A v1 signed by a browser of one of its project's owners, a browser
    /// all the way back, as `owners` and `children` say. Never a v2.
    pub fn signed_for(&self, owners: &Owners, children: &LegacyAppChildren) -> bool {
        self.v == V1
            && self.well_formed()
            && owners.browser_signed(&self.owner, &self.by, &self.body(), &self.sig, children)
    }

    /// Signed for its project, by the format it's in: a v1 by an owner's
    /// browser ([`ProjectGrant::signed_for`]); a v2 by an owner's passkey
    /// as `signers` pinned it, at `now`, with only the removals of
    /// `carried` (the granter's log the platform sent) taken.
    pub fn signed(&self, signers: &Signers, carried: Option<&Log>, now: u64) -> Result<(), GrantRefusal> {
        match self.v {
            V1 if self.signed_for(signers.pinned.owners, signers.children) => Ok(()),
            V2 => owner_passkey(
                &self.account,
                &self.passkey_challenge(),
                self.passkey.as_ref(),
                &signers.pinned,
                carried,
                now,
            ),
            _ => Err(GrantRefusal::Unsigned),
        }
    }
}

/// What a machine pinned that grants and revokes are checked against: its
/// project's owners, their passkey logs and the origins (a deploy's
/// approval's [`Pinned`], DK4) for v2, and for a v1 the browsers the
/// desktop app's old windows approved ([`Owners::browser_signed`]).
#[derive(Clone, Copy)]
pub struct Signers<'a> {
    pub pinned: Pinned<'a>,
    pub children: &'a LegacyAppChildren,
}

/// `account` is an owner of the project as `pinned` says, and `assertion`
/// is a passkey of its, over `challenge`, that checks out against its
/// passkey log as pinned at `now` ([`crate::passkey::verify`]), from an
/// origin pinned for its RP; of `carried` only the removals after the
/// pinned log count ([`crate::passkey::removed_since`]).
fn owner_passkey(
    account: &str,
    challenge: &str,
    assertion: Option<&Assertion>,
    pinned: &Pinned,
    carried: Option<&Log>,
    now: u64,
) -> Result<(), GrantRefusal> {
    if pinned.owners.role_of(account) != Some(TeamRole::Owner) {
        return Err(GrantRefusal::NotAnOwner);
    }
    let assertion = assertion.ok_or(GrantRefusal::Unsigned)?;
    let held = pinned.passkey_logs.get(account).ok_or(GrantRefusal::PasskeyLog)?;
    crate::passkey::verify(held, account, assertion, challenge, now, pinned.origins)
        .map_err(|_| GrantRefusal::Passkey)?;
    if let Some(c) = carried {
        let removed = crate::passkey::removed_since(held, c, now).ok_or(GrantRefusal::PasskeyLog)?;
        if removed.contains(assertion.id.as_str()) {
            return Err(GrantRefusal::Passkey);
        }
    }
    Ok(())
}

/// What happens to runs already going when a grant is revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Running {
    /// They finish.
    Finish,
    /// They're stopped too.
    Stop,
}

impl Running {
    pub fn as_str(self) -> &'static str {
        match self {
            Running::Finish => "finish",
            Running::Stop => "stop",
        }
    }
}

/// The project's grants taken back, by any owner: every grant for the
/// project under `delegation` made at or before `at`, v1 or v2. It names
/// no grant, so whoever relays it can't point it at another one; a grant
/// made after it is a new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GrantRevoke {
    pub project: String,
    /// The delegation whose grants it takes back.
    pub delegation: String,
    pub running: Running,
    pub at: u64,
    /// [`REVOKE_DEVICE`]: the signing device. Empty in a passkey's.
    #[serde(default)]
    pub by: String,
    /// [`REVOKE_DEVICE`]: its signature, hex. Empty in a passkey's.
    #[serde(default)]
    pub sig: String,
    /// [`REVOKE_PASSKEY`]: the revoking owner's account, in the bytes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account: String,
    /// [`REVOKE_PASSKEY`]: the passkey's assertion over
    /// [`GrantRevoke::passkey_challenge`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passkey: Option<Assertion>,
    /// Its format, [`REVOKE_DEVICE`] or [`REVOKE_PASSKEY`]: absent is a
    /// device's (every revoke from before DK6).
    #[serde(default = "revoke_device", skip_serializing_if = "is_revoke_device")]
    pub v: u8,
}

impl GrantRevoke {
    /// Every grant for `project` under `delegation` made by `at`, revoked
    /// by `by`'s device key: what the platform took before DK6, made now
    /// only for history and tests.
    pub fn new(project: &str, delegation: &str, running: Running, at: u64, by: &crate::DeviceKeys) -> Self {
        let mut r = Self {
            project: project.to_owned(),
            delegation: delegation.to_owned(),
            running,
            at,
            by: by.id(),
            sig: String::new(),
            account: String::new(),
            passkey: None,
            v: REVOKE_DEVICE,
        };
        r.sig = hex::encode(by.signature(r.body().as_bytes()));
        r
    }

    /// Every grant for `project` under `delegation` made by `at`, revoked
    /// by `account` (DK6). Nothing signs it here: its signature is a
    /// passkey's assertion over [`GrantRevoke::passkey_challenge`].
    pub fn v3(project: &str, delegation: &str, running: Running, at: u64, account: &str) -> Self {
        Self {
            project: project.to_owned(),
            delegation: delegation.to_owned(),
            running,
            at,
            by: String::new(),
            sig: String::new(),
            account: account.to_owned(),
            passkey: None,
            v: REVOKE_PASSKEY,
        }
    }

    /// The bytes it's over. Frozen once shipped: see `frozen.rs`.
    pub fn body(&self) -> String {
        let (signer, who) = if self.v == REVOKE_DEVICE { ("by", &self.by) } else { ("account", &self.account) };
        format!(
            "arugula project grant revoke v{}\nproject {}\ndelegation {}\nrunning {}\nat {}\n{signer} {who}\n",
            self.v,
            self.project,
            self.delegation,
            self.running.as_str(),
            self.at,
        )
    }

    /// The revoke `body` is, as [`ProjectGrant::parse`]: a device's with
    /// its `sig`, a passkey's with none.
    pub fn parse(body: &str, sig: &str) -> Option<Self> {
        let mut lines = body.strip_suffix('\n')?.split('\n');
        let mut field = |name: &str| lines.next()?.strip_prefix(name)?.strip_prefix(' ').map(str::to_owned);
        let v = match field("arugula project grant revoke")?.as_str() {
            "v2" => REVOKE_DEVICE,
            "v3" if sig.is_empty() => REVOKE_PASSKEY,
            _ => return None,
        };
        let (project, delegation) = (field("project")?, field("delegation")?);
        let running = match field("running")?.as_str() {
            "finish" => Running::Finish,
            "stop" => Running::Stop,
            _ => return None,
        };
        let at = field("at")?.parse().ok()?;
        let (by, account) = match v {
            REVOKE_DEVICE => (field("by")?, String::new()),
            _ => (String::new(), field("account")?),
        };
        let r = Self { project, delegation, running, at, by, sig: sig.to_owned(), account, passkey: None, v };
        (r.body() == body).then_some(r)
    }

    /// Its ids, and its signer as its format has it: a device's names its
    /// device, a passkey's its account and no device or signature.
    pub fn well_formed(&self) -> bool {
        let signer = match self.v {
            REVOKE_DEVICE => token(&self.by),
            REVOKE_PASSKEY => token(&self.account) && self.by.is_empty() && self.sig.is_empty(),
            _ => false,
        };
        signer && token(&self.project) && token(&self.delegation)
    }

    /// The WebAuthn challenge a passkey revoke's assertion signs: SHA-256
    /// of [`GrantRevoke::body`], base64url without padding.
    pub fn passkey_challenge(&self) -> String {
        challenge_of(&self.body())
    }

    /// It's about `g`: the same project and delegation, and `g` was made at
    /// or before it, whatever format either is in. Says nothing about the
    /// signature.
    pub fn covers(&self, g: &ProjectGrant) -> bool {
        self.project == g.project && self.delegation == g.delegation && g.made <= self.at
    }

    /// Signed for the project `owner` names (by any of its owners, not
    /// only the one who granted), trusted now: a device's by a browser all
    /// the way back; a passkey's by an owner's passkey as `signers` pinned
    /// it, at `now`.
    pub fn signed_for(&self, owner: &str, signers: &Signers, now: u64) -> bool {
        if !self.well_formed() {
            return false;
        }
        match self.v {
            REVOKE_DEVICE => {
                signers.pinned.owners.browser_signed(owner, &self.by, &self.body(), &self.sig, signers.children)
            }
            _ => {
                Owner::parse(owner) == Some(signers.pinned.owners.owner())
                    && owner_passkey(
                        &self.account,
                        &self.passkey_challenge(),
                        self.passkey.as_ref(),
                        &signers.pinned,
                        None,
                        now,
                    )
                    .is_ok()
            }
        }
    }

    /// It takes back `g`: it covers it, signed by one of the project's
    /// owners now.
    pub fn revokes(&self, g: &ProjectGrant, signers: &Signers, now: u64) -> bool {
        self.covers(g) && self.signed_for(&g.owner, signers, now)
    }
}

/// Why a hosted machine won't act under a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRefusal {
    /// It isn't well formed.
    Form,
    /// It's for another project, or another owner, than the machine's.
    WrongProject,
    /// It names a delegation other than the one the machine was enrolled
    /// under.
    WrongDelegation,
    /// No owner signed it: a v1 no owner's browser signed (or that device
    /// isn't trusted now), a v2 with no passkey assertion.
    Unsigned,
    /// A v2 whose account isn't an owner of the project, by what the
    /// machine pinned.
    NotAnOwner,
    /// A v2 whose granter's passkey log the machine didn't pin, or the log
    /// the platform carried isn't the pinned one, older, or one after it.
    PasskeyLog,
    /// A v2 whose passkey doesn't check out: not in the log, or removed by
    /// now, from an origin the machine didn't pin, over other bytes, the
    /// person not verified, a bad signature.
    Passkey,
    /// It doesn't give what's asked.
    NotCovered,
    /// Its expiry has passed.
    Expired,
    /// An owner revoked the delegation it's under.
    DelegationRevoked,
    /// An owner revoked it.
    Revoked,
}

impl GrantRefusal {
    pub const ALL: [GrantRefusal; 11] = [
        GrantRefusal::Form,
        GrantRefusal::WrongProject,
        GrantRefusal::WrongDelegation,
        GrantRefusal::Unsigned,
        GrantRefusal::NotAnOwner,
        GrantRefusal::PasskeyLog,
        GrantRefusal::Passkey,
        GrantRefusal::NotCovered,
        GrantRefusal::Expired,
        GrantRefusal::DelegationRevoked,
        GrantRefusal::Revoked,
    ];

    pub fn code(self) -> &'static str {
        match self {
            GrantRefusal::Form => "grant_form",
            GrantRefusal::WrongProject => "grant_wrong_project",
            GrantRefusal::WrongDelegation => "grant_wrong_delegation",
            GrantRefusal::Unsigned => "grant_unsigned",
            GrantRefusal::NotAnOwner => "grant_not_an_owner",
            GrantRefusal::PasskeyLog => "grant_passkey_log",
            GrantRefusal::Passkey => "grant_passkey",
            GrantRefusal::NotCovered => "grant_not_covered",
            GrantRefusal::Expired => "grant_expired",
            GrantRefusal::DelegationRevoked => "delegation_revoked",
            GrantRefusal::Revoked => "grant_revoked",
        }
    }

    pub fn says(self) -> &'static str {
        match self {
            GrantRefusal::Form => "the project's standing grant isn't well formed",
            GrantRefusal::WrongProject => "the standing grant is for another project",
            GrantRefusal::WrongDelegation => {
                "the standing grant names another delegation than this machine's: an owner signs it again"
            }
            GrantRefusal::Unsigned => "no owner signed the standing grant",
            GrantRefusal::NotAnOwner => {
                "the standing grant's signer isn't an owner of this project, by what this machine pinned"
            }
            GrantRefusal::PasskeyLog => {
                "this machine pinned no passkey log for the standing grant's signer, or the one sent isn't the pinned one or one after it"
            }
            GrantRefusal::Passkey => {
                "the standing grant's passkey doesn't check out against the signer's passkey log as this machine pinned it (it takes a passkey added since its last refresh, a minute at most, at the next)"
            }
            GrantRefusal::NotCovered => "the standing grant doesn't cover this",
            GrantRefusal::Expired => "the standing grant has expired: an owner signs it again",
            GrantRefusal::DelegationRevoked => "the project's delegation for this machine was revoked",
            GrantRefusal::Revoked => "the standing grant was revoked",
        }
    }
}

/// What a hosted machine holds against a grant beside the grant itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct Revokes<'a> {
    /// Its delegation's revocations, as it pinned them.
    pub delegation: &'a [crate::hosted::DelegationRevoke],
    /// Revokes that came with the ask: they count only if one of the
    /// project's owners signed them, trusted now.
    pub sent: &'a [GrantRevoke],
    /// Revokes it checked when it first heard them and kept: they count
    /// without being checked again, so dropping their signer's certificate
    /// or passkey later doesn't bring a grant back.
    pub kept: &'a [GrantRevoke],
}

/// Whether a hosted machine enrolled under `d` may do `cap` under `g` at
/// `now`: the grant is well formed, for `d`'s project and owner, names
/// `d`, is signed by one of the project's owners as `signers` says (which
/// the machine got from its own checks, not the platform's word: a v1 by
/// an owner's browser, a v2 by an owner's passkey in the log it pinned,
/// of `carried` only removals taken), covers `cap`, hasn't expired; `d`
/// isn't revoked; and no revoke takes it back (a kept one, or a sent one
/// that checks out). Revokes that don't check out are ignored: the
/// platform may send junk, and only a refusal comes of it.
pub fn check(
    g: &ProjectGrant,
    d: &Delegation,
    revokes: Revokes,
    signers: &Signers,
    carried: Option<&Log>,
    cap: &str,
    now: u64,
) -> Result<(), GrantRefusal> {
    if !g.well_formed() {
        return Err(GrantRefusal::Form);
    }
    let owners = signers.pinned.owners;
    if g.project != d.project || g.owner != d.owner || Owner::parse(&g.owner) != Some(owners.owner()) {
        return Err(GrantRefusal::WrongProject);
    }
    if g.delegation != d.id() {
        return Err(GrantRefusal::WrongDelegation);
    }
    g.signed(signers, carried, now)?;
    if !g.caps.iter().any(|c| c == cap) {
        return Err(GrantRefusal::NotCovered);
    }
    if g.expires.is_some_and(|e| e <= now) {
        return Err(GrantRefusal::Expired);
    }
    if revokes.delegation.iter().any(|r| r.revokes(d, owners)) {
        return Err(GrantRefusal::DelegationRevoked);
    }
    if revokes.kept.iter().any(|r| r.covers(g)) || revokes.sent.iter().any(|r| r.revokes(g, signers, now)) {
        return Err(GrantRefusal::Revoked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Cert, DeviceKeys, Kind, Revocation, Trust,
        hosted::HostedKey,
        team::{AccountCerts, Member, Roster, TeamRole},
    };

    /// A machine that pinned no grandfathered app windows' browsers.
    const NONE: &LegacyAppChildren = &LegacyAppChildren::new();

    struct Person {
        account: String,
        keys: DeviceKeys,
        cert: Cert,
    }

    fn person(account: &str) -> Person {
        let keys = DeviceKeys::generate();
        let mut cert = Cert::new(&keys, account, Kind::Browser, "laptop");
        cert.sign_with(&keys);
        Person { account: account.into(), keys, cert }
    }

    fn member(p: &Person, role: TeamRole) -> Member {
        Member { account: p.account.clone(), root: p.cert.device.clone(), role, name: p.account.clone() }
    }

    fn roster(members: Vec<Member>, by: &Person) -> Roster {
        let mut r = Roster {
            v: 1,
            team: "t1".into(),
            name: "Acme".into(),
            version: 1,
            at: 1,
            members,
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        r.sign_with(&by.keys);
        r
    }

    fn certs(people: &[&Person]) -> AccountCerts {
        people.iter().map(|p| (p.account.clone(), (vec![p.cert.clone()], vec![]))).collect()
    }

    fn delegation(owner: &str, by: &DeviceKeys, at: u64) -> Delegation {
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: owner.into(),
            key: HostedKey::from_seed(&[1; 32]).public(),
            at,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(by);
        d
    }

    fn grant(d: &Delegation, by: &DeviceKeys) -> ProjectGrant {
        let mut g = ProjectGrant::new(d, 100, None);
        g.sign_with(by);
        g
    }

    /// A team with an owner, a second owner, an editor and a viewer.
    struct World {
        owner: Person,
        other_owner: Person,
        editor: Person,
        roster: Roster,
        certs: AccountCerts,
    }

    fn world() -> World {
        let (owner, other_owner, editor, viewer) = (person("own"), person("own2"), person("ed"), person("view"));
        let roster = roster(
            vec![
                member(&owner, TeamRole::Owner),
                member(&other_owner, TeamRole::Owner),
                member(&editor, TeamRole::Editor),
                member(&viewer, TeamRole::Viewer),
            ],
            &owner,
        );
        let certs = certs(&[&owner, &other_owner, &editor, &viewer]);
        World { owner, other_owner, editor, roster, certs }
    }

    #[test]
    fn an_owners_grant_starts_and_instructs() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        assert_eq!(g.caps, ["runs.instruct", "runs.start"]);
        for cap in CAPS {
            assert_eq!(check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, cap, 200), Ok(()));
        }
        // Not a deploy, or anything else.
        assert_eq!(
            check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, "deploys.apply", 200),
            Err(GrantRefusal::NotCovered)
        );
        // Another owner's device's grant counts the same.
        let g2 = grant(&d, &w.other_owner.keys);
        assert_eq!(check(&g2, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200), Ok(()));
    }

    #[test]
    fn only_owners_devices_grant() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        // An editor's device, a stranger's, control's own key: none.
        for by in [&w.editor.keys, &DeviceKeys::generate()] {
            assert_eq!(
                check(&grant(&d, by), &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
                Err(GrantRefusal::Unsigned)
            );
        }
        // An owner's machine, recovery code, CLI or desktop app doesn't
        // either, nor a browser one of those made: their keys may be a
        // program's. Nor a revoke from them.
        for kind in [Kind::Daemon, Kind::Recovery, Kind::Cli, Kind::App] {
            let k = DeviceKeys::generate();
            let mut kc = Cert::new(&k, &w.owner.account, kind, "x");
            kc.sign_with(&w.owner.keys);
            let mut certs = w.certs.clone();
            certs.get_mut(&w.owner.account).unwrap().0.push(kc);
            let owners = Owners::Team { roster: &w.roster, certs: &certs };
            assert_eq!(
                check(&grant(&d, &k), &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
                Err(GrantRefusal::Unsigned)
            );
        }
        for kind in [Kind::Cli, Kind::App] {
            let k = DeviceKeys::generate();
            let mut kc = Cert::new(&k, &w.owner.account, kind, "x");
            kc.sign_with(&w.owner.keys);
            let mut certs = w.certs.clone();
            certs.get_mut(&w.owner.account).unwrap().0.push(kc);
            let owners = Owners::Team { roster: &w.roster, certs: &certs };
            let g = grant(&d, &w.owner.keys);
            assert_eq!(check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200), Ok(()));
            let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 150, &k);
            assert!(!r.revokes(&g, &s1(&owners, NONE), 0), "a {} can't revoke it", kind.as_str());
            let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 150, &w.other_owner.keys);
            assert!(r.revokes(&g, &s1(&owners, NONE), 0), "another owner's browser can");
        }
        // A browser a CLI signed (control keeps none now) isn't one.
        let (ck, mut cc) =
            (DeviceKeys::generate(), Cert::new(&DeviceKeys::generate(), &w.owner.account, Kind::Cli, "cli"));
        cc = Cert { sign: hex::encode(ck.sign_public()), noise: hex::encode(ck.noise_public), device: ck.id(), ..cc };
        cc.sign_with(&w.owner.keys);
        let lk = DeviceKeys::generate();
        let mut laundered = Cert::new(&lk, &w.owner.account, Kind::Browser, "laundered");
        laundered.sign_with(&ck);
        let mut certs = w.certs.clone();
        certs.get_mut(&w.owner.account).unwrap().0.extend([cc, laundered]);
        let owners = Owners::Team { roster: &w.roster, certs: &certs };
        assert_eq!(
            check(&grant(&d, &lk), &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Unsigned)
        );
        // An owner who's an editor now.
        let later = roster(vec![member(&w.owner, TeamRole::Editor), member(&w.editor, TeamRole::Owner)], &w.owner);
        let owners = Owners::Team { roster: &later, certs: &w.certs };
        assert_eq!(
            check(&grant(&d, &w.owner.keys), &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Unsigned)
        );
    }

    /// The platform holds the grant: whatever it changes, the signature no
    /// longer checks out, so it can't widen one or move it.
    #[test]
    fn the_platform_cant_widen_or_move_it() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        let mut narrow = ProjectGrant { caps: vec!["runs.start".into()], ..g.clone() };
        narrow.sign_with(&w.owner.keys);
        assert_eq!(
            check(&narrow, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.instruct", 200),
            Err(GrantRefusal::NotCovered)
        );
        let wider = ProjectGrant { caps: g.caps.clone(), ..narrow.clone() };
        assert_eq!(
            check(&wider, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.instruct", 200),
            Err(GrantRefusal::Unsigned)
        );
        let mut short = ProjectGrant { expires: Some(150), ..g.clone() };
        short.sign_with(&w.owner.keys);
        assert_eq!(
            check(&short, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Expired)
        );
        assert_eq!(check(&short, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 149), Ok(()));
        let longer = ProjectGrant { expires: None, ..short.clone() };
        assert_eq!(
            check(&longer, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Unsigned)
        );
        // An unknown power, signed or not, isn't well formed.
        let mut odd = ProjectGrant { caps: vec!["deploys.apply".into(), "runs.start".into()], ..g.clone() };
        odd.sign_with(&w.owner.keys);
        assert_eq!(
            check(&odd, &d, Revokes::default(), &s1(&owners, NONE), None, "deploys.apply", 200),
            Err(GrantRefusal::Form)
        );
        // Another project, or another delegation of this one.
        let mut other = ProjectGrant { project: "prj_2".into(), ..g.clone() };
        other.sign_with(&w.owner.keys);
        assert_eq!(
            check(&other, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::WrongProject)
        );
        let d2 = delegation("team:t1", &w.owner.keys, 11);
        assert_eq!(
            check(&g, &d2, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::WrongDelegation)
        );
        // Another team's project, signed by this team's owner.
        let mut theirs = ProjectGrant { owner: "team:t2".into(), ..g.clone() };
        theirs.sign_with(&w.owner.keys);
        let dt = Delegation { owner: "team:t2".into(), ..d.clone() };
        assert_eq!(
            check(&theirs, &dt, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::WrongProject)
        );
    }

    fn sent(r: &[GrantRevoke]) -> Revokes<'_> {
        Revokes { sent: r, ..Default::default() }
    }

    #[test]
    fn any_owner_revokes_it_and_nobody_else() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        let spite = GrantRevoke::new(&g.project, &g.delegation, Running::Stop, 150, &w.editor.keys);
        assert_eq!(
            check(&g, &d, sent(std::slice::from_ref(&spite)), &s1(&owners, NONE), None, "runs.start", 200),
            Ok(())
        );
        let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 150, &w.other_owner.keys);
        assert_eq!(
            check(&g, &d, sent(&[spite, r.clone()]), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Revoked)
        );
        // It names no grant: every grant of the project under the
        // delegation made by then is revoked, and one made after isn't.
        let mut before = ProjectGrant::new(&d, 140, None);
        before.sign_with(&w.other_owner.keys);
        assert_eq!(
            check(&before, &d, sent(std::slice::from_ref(&r)), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Revoked)
        );
        let mut after = ProjectGrant::new(&d, 151, None);
        after.sign_with(&w.owner.keys);
        assert_eq!(
            check(&after, &d, sent(std::slice::from_ref(&r)), &s1(&owners, NONE), None, "runs.start", 200),
            Ok(())
        );
        // The platform can't move it to another project or delegation, or
        // turn a finish into a stop.
        let moved = GrantRevoke { delegation: "other".into(), ..r.clone() };
        assert!(!moved.revokes(&g, &s1(&owners, NONE), 0));
        let harsher = GrantRevoke { running: Running::Stop, ..r.clone() };
        assert!(!harsher.revokes(&g, &s1(&owners, NONE), 0));
    }

    /// A revoke the machine checked and kept counts after the platform
    /// drops its signer's certificate; one sent with a start only counts
    /// while its signer is trusted.
    #[test]
    fn a_kept_revoke_outlives_its_signers_certificate() {
        let w = world();
        let d = delegation("team:t1", &w.owner.keys, 10);
        // Signed by device A (the owner's), revoked by device B (the other
        // owner's).
        let g = grant(&d, &w.owner.keys);
        let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 150, &w.other_owner.keys);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        assert!(r.revokes(&g, &s1(&owners, NONE), 0), "checked when it's heard");
        // A refresh from the platform without B's certificate.
        let mut certs = w.certs.clone();
        certs.remove(&w.other_owner.account);
        let refreshed = Owners::Team { roster: &w.roster, certs: &certs };
        assert_eq!(
            check(&g, &d, sent(std::slice::from_ref(&r)), &s1(&refreshed, NONE), None, "runs.start", 200),
            Ok(()),
            "sent: no longer checks out"
        );
        let kept = Revokes { kept: std::slice::from_ref(&r), ..Default::default() };
        assert_eq!(check(&g, &d, kept, &s1(&refreshed, NONE), None, "runs.start", 200), Err(GrantRefusal::Revoked));
    }

    #[test]
    fn a_revoked_delegation_stops_its_grants() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        let gone = [crate::hosted::DelegationRevoke::new(&d, 120, &w.other_owner.keys)];
        let revokes = Revokes { delegation: &gone, ..Default::default() };
        assert_eq!(
            check(&g, &d, revokes, &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::DelegationRevoked)
        );
        // Not by an editor.
        let spite = [crate::hosted::DelegationRevoke::new(&d, 120, &w.editor.keys)];
        let revokes = Revokes { delegation: &spite, ..Default::default() };
        assert_eq!(check(&g, &d, revokes, &s1(&owners, NONE), None, "runs.start", 200), Ok(()));
    }

    #[test]
    fn a_personal_project() {
        let me = person("me");
        let d = delegation("account:me", &me.keys, 10);
        let g = grant(&d, &me.keys);
        let t = Trust { account: "me".into(), root: me.cert.device.clone() };
        let owners = Owners::Account { trust: &t, certs: std::slice::from_ref(&me.cert), revocations: &[] };
        assert_eq!(check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200), Ok(()));
        // A phone of mine, removed since: its grant stops counting.
        let pk = DeviceKeys::generate();
        let mut pc = Cert::new(&pk, "me", Kind::Browser, "phone");
        pc.sign_with(&me.keys);
        let g = grant(&d, &pk);
        let both = [me.cert.clone(), pc.clone()];
        let owners = Owners::Account { trust: &t, certs: &both, revocations: &[] };
        assert_eq!(check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200), Ok(()));
        let gone = [Revocation::new("me", &pc.device, &me.keys)];
        let owners = Owners::Account { trust: &t, certs: &both, revocations: &gone };
        assert_eq!(
            check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 200),
            Err(GrantRefusal::Unsigned)
        );
    }

    /// The desktop app's window from before it had a kind (a `browser`
    /// certificate it named) counts only as #1394 lets it (M253): as the
    /// link just above a browser it approved back then, by the set the
    /// machine pinned for that account. Never as the device that signs,
    /// nor above what it certifies now; and a machine that pinned no set
    /// counts no window at all.
    #[test]
    fn an_app_window_counts_only_just_above_a_grandfathered_browser() {
        let mk = |name: &str, account: &str, by: Option<&DeviceKeys>| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, account, Kind::Browser, name);
            c.sign_with(by.unwrap_or(&k));
            (k, c)
        };
        let (phone, root) = mk("Pixel", "me", None);
        let (chrome, chrome_cert) = mk("Chrome on Mac", "me", Some(&phone));
        let (window, window_cert) = mk("Arugula app on jake-air", "me", Some(&phone));
        let (firefox, firefox_cert) = mk("Firefox on Mac", "me", Some(&window));
        let (later, later_cert) = mk("Firefox on Linux", "me", Some(&firefox));
        let (edge, edge_cert) = mk("Edge", "me", Some(&window));
        let certs = [root, chrome_cert, window_cert, firefox_cert, later_cert, edge_cert];
        let t = Trust { account: "me".into(), root: phone.id() };
        let owners = Owners::Account { trust: &t, certs: &certs, revocations: &[] };
        let d = delegation("account:me", &phone, 10);
        let pinned = |account: &str, ks: &[&DeviceKeys]| {
            LegacyAppChildren::from([(account.to_owned(), ks.iter().map(|k| k.id()).collect())])
        };
        let ff = pinned("me", &[&firefox]);
        let start = |by: &DeviceKeys, children: &LegacyAppChildren| {
            check(&grant(&d, by), &d, Revokes::default(), &s1(&owners, children), None, "runs.start", 200)
        };
        // A browser all the way back, with or without a set pinned.
        assert_eq!(start(&chrome, NONE), Ok(()));
        assert_eq!(start(&chrome, &ff), Ok(()));
        // What the window approved back then, and what that approves now.
        assert_eq!(start(&firefox, &ff), Ok(()));
        assert_eq!(start(&later, &ff), Ok(()));
        // The window itself never signs, whatever's pinned.
        for children in [NONE, &ff, &pinned("me", &[&window])] {
            assert_eq!(start(&window, children), Err(GrantRefusal::Unsigned));
        }
        // Not through the window to what it certifies now, nor when the
        // machine pinned none (or another account's).
        assert_eq!(start(&edge, &ff), Err(GrantRefusal::Unsigned), "not a grandfathered child");
        assert_eq!(start(&firefox, NONE), Err(GrantRefusal::Unsigned), "pinned none");
        assert_eq!(start(&later, NONE), Err(GrantRefusal::Unsigned), "pinned none");
        assert_eq!(start(&firefox, &pinned("you", &[&firefox])), Err(GrantRefusal::Unsigned), "another account's");
        // Its revokes the same.
        let g = grant(&d, &chrome);
        let by = |k: &DeviceKeys| GrantRevoke::new(&g.project, &g.delegation, Running::Stop, 150, k);
        assert!(!by(&window).revokes(&g, &s1(&owners, &ff), 0));
        assert!(!by(&edge).revokes(&g, &s1(&owners, &ff), 0));
        assert!(!by(&firefox).revokes(&g, &s1(&owners, NONE), 0));
        assert!(by(&firefox).revokes(&g, &s1(&owners, &ff), 0));
        assert!(by(&chrome).revokes(&g, &s1(&owners, NONE), 0));

        // A team's owner, by the set pinned for that owner's account.
        let mut w = world();
        let (ow, ow_cert) = mk("illogical app", &w.owner.account, Some(&w.owner.keys));
        let (ob, ob_cert) = mk("Firefox", &w.owner.account, Some(&ow));
        w.certs.get_mut(&w.owner.account).unwrap().0.extend([ow_cert, ob_cert]);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let start = |by: &DeviceKeys, children: &LegacyAppChildren| {
            check(&grant(&d, by), &d, Revokes::default(), &s1(&owners, children), None, "runs.start", 200)
        };
        assert_eq!(start(&ob, &pinned(&w.owner.account, &[&ob])), Ok(()));
        assert_eq!(start(&ob, NONE), Err(GrantRefusal::Unsigned));
        assert_eq!(start(&ob, &pinned(&w.other_owner.account, &[&ob])), Err(GrantRefusal::Unsigned));
        assert_eq!(start(&ow, &pinned(&w.owner.account, &[&ob])), Err(GrantRefusal::Unsigned));
    }

    #[test]
    fn the_bytes_read_back() {
        let me = person("me");
        let d = delegation("account:me", &me.keys, 10);
        for expires in [None, Some(500)] {
            let mut g = ProjectGrant::new(&d, 100, expires);
            g.sign_with(&me.keys);
            assert_eq!(ProjectGrant::parse(&g.body(), &g.sig), Some(g.clone()));
            for running in [Running::Finish, Running::Stop] {
                let r = GrantRevoke::new(&g.project, &g.delegation, running, 300, &me.keys);
                assert_eq!(GrantRevoke::parse(&r.body(), &r.sig), Some(r));
            }
        }
        let g = grant(&d, &me.keys);
        let body = g.body();
        for bad in [
            body.replace("resource hosted", "resource machine"),
            body.replace("to platform", "to agent"),
            body.replace("arugula project grant v1", "arugula exec grant v1"),
            format!("{body}extra\n"),
            body.trim_end().to_owned(),
        ] {
            assert_eq!(ProjectGrant::parse(&bad, &g.sig), None, "{bad}");
        }
    }

    // ---- DK6: grant v2 and revoke v3, a passkey over their bytes

    use std::collections::BTreeMap;

    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    use sha2::{Digest as _, Sha256};

    use crate::passkey::{Assertion, Log, Origins, origins_of_sites};

    const NO_LOGS: &BTreeMap<String, Log> = &BTreeMap::new();
    const NO_ORIGINS: &Origins = &Origins::new();

    /// What a machine pinned when it checks v1 grants only: no passkey
    /// logs, no origins.
    pub(super) fn s1<'a>(owners: &'a Owners<'a>, children: &'a LegacyAppChildren) -> Signers<'a> {
        Signers { pinned: Pinned { owners, passkey_logs: NO_LOGS, origins: NO_ORIGINS }, children }
    }

    fn s2<'a>(owners: &'a Owners<'a>, logs: &'a BTreeMap<String, Log>, origins: &'a Origins) -> Signers<'a> {
        Signers { pinned: Pinned { owners, passkey_logs: logs, origins }, children: NONE }
    }

    const RP: &str = "control.test";
    const HERE: &str = "https://control.test";
    const UV: u8 = 0x05;

    /// A test passkey (Ed25519), its id from `n`.
    struct Pk {
        key: ed25519_dalek::SigningKey,
        id: String,
    }

    impl Pk {
        fn new(n: u8) -> Self {
            Pk { key: ed25519_dalek::SigningKey::from_bytes(&[n; 32]), id: B64.encode([n; 16]) }
        }

        /// Its `added` event in `log`, at `at`.
        fn add_to(&self, log: &mut Log, at: u64) {
            let e = crate::passkey::Draft {
                account: log.account.clone(),
                event: crate::passkey::Change::Added,
                credential: self.id.clone(),
                rp: RP.into(),
                alg: crate::passkey::EDDSA,
                key: self.key.verifying_key().to_bytes().to_vec(),
                aaguid: crate::passkey::NO_AAGUID.into(),
                at,
                by: "signup".into(),
            }
            .after(log.events.last());
            log.events.push(e);
        }

        /// Its assertion over `challenge`, from `origin`, with `flags`.
        fn over(&self, challenge: &str, origin: &str, flags: u8) -> Assertion {
            use ed25519_dalek::Signer as _;
            let cd =
                serde_json::json!({ "type": "webauthn.get", "challenge": challenge, "origin": origin }).to_string();
            let mut data = Sha256::digest(RP.as_bytes()).to_vec();
            data.push(flags);
            data.extend_from_slice(&[0, 0, 0, 0]);
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(cd.as_bytes()));
            Assertion {
                id: self.id.clone(),
                client_data_json: B64.encode(cd.as_bytes()),
                authenticator_data: B64.encode(&data),
                signature: B64.encode(self.key.sign(&msg).to_bytes()),
            }
        }
    }

    /// `log` with `pk`'s key removed at `at`.
    fn removed(log: &Log, pk: &Pk, at: u64) -> Log {
        let mut out = log.clone();
        let added = out.events.iter().find(|e| e.credential == pk.id).unwrap().clone();
        let e = added.removal(at, "reset").placed_after(out.events.last());
        out.events.push(e);
        out
    }

    /// A v2 grant by `account` under `d`, made at 100 for good, confirmed
    /// by `pk` from `origin`.
    fn passkey_grant(d: &Delegation, account: &str, pk: &Pk, origin: &str) -> ProjectGrant {
        let mut g = ProjectGrant::v2(d, account, 100, 10_000);
        g.passkey = Some(pk.over(&g.passkey_challenge(), origin, UV));
        g
    }

    /// A v3 revoke by `account` at `at`, confirmed by `pk`.
    fn passkey_revoke(g: &ProjectGrant, account: &str, pk: &Pk, at: u64) -> GrantRevoke {
        let mut r = GrantRevoke::v3(&g.project, &g.delegation, Running::Finish, at, account);
        r.passkey = Some(pk.over(&r.passkey_challenge(), HERE, UV));
        r
    }

    /// The team's people's passkeys: the owner's k1 and k2 (k2 removed at
    /// 50), the other owner's k3, the editor's k4.
    struct Keys {
        k1: Pk,
        k2: Pk,
        k3: Pk,
        k4: Pk,
        logs: BTreeMap<String, Log>,
    }

    fn keys(w: &World) -> Keys {
        let (k1, k2, k3, k4) = (Pk::new(1), Pk::new(2), Pk::new(3), Pk::new(4));
        let mut own = Log { account: w.owner.account.clone(), ..Default::default() };
        k1.add_to(&mut own, 10);
        k2.add_to(&mut own, 20);
        let own = removed(&own, &k2, 50);
        let mut own2 = Log { account: w.other_owner.account.clone(), ..Default::default() };
        k3.add_to(&mut own2, 10);
        let mut ed = Log { account: w.editor.account.clone(), ..Default::default() };
        k4.add_to(&mut ed, 10);
        let logs = [(own.account.clone(), own), (own2.account.clone(), own2), (ed.account.clone(), ed)].into();
        Keys { k1, k2, k3, k4, logs }
    }

    /// DK6: a v2 grant is one owner's passkey assertion over its bytes,
    /// checked against the passkey log the machine pinned, with no device
    /// in it. Whoever it is, and whatever device they used, only an owner
    /// of the project now, with a passkey in the log and not removed, from
    /// a pinned origin, over exactly these bytes, the person verified.
    #[test]
    fn a_passkey_grant_is_an_owners_passkey_over_its_bytes() {
        let w = world();
        let k = keys(&w);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let origins = origins_of_sites([HERE]);
        let s = s2(&owners, &k.logs, &origins);
        let d = delegation("team:t1", &w.owner.keys, 10);
        let start = |g: &ProjectGrant, s: &Signers, carried: Option<&Log>| {
            check(g, &d, Revokes::default(), s, carried, "runs.start", 200)
        };
        let g = passkey_grant(&d, &w.owner.account, &k.k1, HERE);
        assert!(g.body().starts_with("arugula project grant v2\n") && g.body().ends_with("\naccount own\n"));
        for cap in CAPS {
            assert_eq!(check(&g, &d, Revokes::default(), &s, None, cap, 200), Ok(()));
        }
        assert_eq!(start(&passkey_grant(&d, &w.other_owner.account, &k.k3, HERE), &s, None), Ok(()), "any owner");
        // An editor's passkey, a stranger's account, another account's
        // passkey, a removed one, one in no log.
        let cases = [
            (passkey_grant(&d, &w.editor.account, &k.k4, HERE), GrantRefusal::NotAnOwner),
            (passkey_grant(&d, "stranger", &k.k4, HERE), GrantRefusal::NotAnOwner),
            (passkey_grant(&d, &w.owner.account, &k.k3, HERE), GrantRefusal::Passkey),
            (passkey_grant(&d, &w.owner.account, &k.k2, HERE), GrantRefusal::Passkey),
            (passkey_grant(&d, &w.owner.account, &Pk::new(9), HERE), GrantRefusal::Passkey),
            (passkey_grant(&d, &w.owner.account, &k.k1, "https://app.test"), GrantRefusal::Passkey),
        ];
        for (g, want) in cases {
            assert_eq!(start(&g, &s, None), Err(want), "{}", g.account);
        }
        // The person not verified; no assertion; bytes changed after.
        let mut g2 = ProjectGrant::v2(&d, &w.owner.account, 100, 10_000);
        g2.passkey = Some(k.k1.over(&g2.passkey_challenge(), HERE, 0x01));
        assert_eq!(start(&g2, &s, None), Err(GrantRefusal::Passkey), "UP only");
        assert_eq!(start(&ProjectGrant { passkey: None, ..g.clone() }, &s, None), Err(GrantRefusal::Unsigned));
        assert_eq!(start(&ProjectGrant { expires: Some(20_000), ..g.clone() }, &s, None), Err(GrantRefusal::Passkey));
        let narrow = ProjectGrant { caps: vec!["runs.start".into()], ..g.clone() };
        assert_eq!(start(&narrow, &s, None), Err(GrantRefusal::Passkey));
        // A v2 names no device and holds no signature, and expires.
        for bad in [
            ProjectGrant { by: w.owner.keys.id(), ..g.clone() },
            ProjectGrant { sig: "00".into(), ..g.clone() },
            ProjectGrant { expires: None, ..g.clone() },
            ProjectGrant { account: String::new(), ..g.clone() },
            ProjectGrant { v: 3, ..g.clone() },
        ] {
            assert_eq!(start(&bad, &s, None), Err(GrantRefusal::Form), "{bad:?}");
        }
        // The platform page's origin, once the machine pinned it.
        let mut related = origins.clone();
        related.get_mut(RP).unwrap().push("https://app.test".into());
        let there = passkey_grant(&d, &w.owner.account, &k.k1, "https://app.test");
        assert_eq!(start(&there, &s2(&owners, &k.logs, &related), None), Ok(()));
        // No log pinned for the account: refused, whatever comes with it.
        let only_own: BTreeMap<String, Log> = [(w.owner.account.clone(), k.logs[&w.owner.account].clone())].into();
        let other = passkey_grant(&d, &w.other_owner.account, &k.k3, HERE);
        assert_eq!(
            start(&other, &s2(&owners, &only_own, &origins), Some(&k.logs[&w.other_owner.account])),
            Err(GrantRefusal::PasskeyLog)
        );
        // An owner who's an editor now.
        let later = roster(vec![member(&w.owner, TeamRole::Editor), member(&w.editor, TeamRole::Owner)], &w.owner);
        let now_owners = Owners::Team { roster: &later, certs: &w.certs };
        assert_eq!(start(&g, &s2(&now_owners, &k.logs, &origins), None), Err(GrantRefusal::NotAnOwner));
        // A personal project's owner is its account.
        let me = person("me");
        let dm = delegation("account:me", &me.keys, 10);
        let t = Trust { account: "me".into(), root: me.cert.device.clone() };
        let mine = Owners::Account { trust: &t, certs: std::slice::from_ref(&me.cert), revocations: &[] };
        let mut mlog = Log { account: "me".into(), ..Default::default() };
        k.k1.add_to(&mut mlog, 10);
        let mlogs: BTreeMap<String, Log> = [("me".to_owned(), mlog)].into();
        let gm = passkey_grant(&dm, "me", &k.k1, HERE);
        assert_eq!(check(&gm, &dm, Revokes::default(), &s2(&mine, &mlogs, &origins), None, "runs.start", 200), Ok(()));
        let gx = passkey_grant(&dm, &w.owner.account, &k.k1, HERE);
        assert_eq!(
            check(&gx, &dm, Revokes::default(), &s2(&mine, &k.logs, &origins), None, "runs.start", 200),
            Err(GrantRefusal::NotAnOwner)
        );
    }

    /// The log the platform carries with a start (the granter's, as it
    /// read it from the hub) may only take keys away: a removal after the
    /// pinned log counts; a key it adds never does; one that isn't the
    /// pinned log, older or after it, is refused.
    #[test]
    fn a_carried_log_only_removes() {
        let w = world();
        let k = keys(&w);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let origins = origins_of_sites([HERE]);
        let s = s2(&owners, &k.logs, &origins);
        let d = delegation("team:t1", &w.owner.keys, 10);
        let start =
            |g: &ProjectGrant, carried: Option<&Log>| check(g, &d, Revokes::default(), &s, carried, "runs.start", 200);
        let pinned = &k.logs[&w.owner.account];
        let g = passkey_grant(&d, &w.owner.account, &k.k1, HERE);
        assert_eq!(start(&g, Some(pinned)), Ok(()), "the pinned log");
        let older = Log { events: pinned.events[..1].to_vec(), ..pinned.clone() };
        assert_eq!(start(&g, Some(&older)), Ok(()), "an older one: what was pinned stands");
        // k1 removed since the machine pinned the log.
        assert_eq!(start(&g, Some(&removed(pinned, &k.k1, 150))), Err(GrantRefusal::Passkey));
        assert_eq!(start(&g, Some(&removed(pinned, &k.k1, 250))), Ok(()), "not yet removed at the machine's now");
        // A key only the carried log has (`by signup` needs no proof).
        let k9 = Pk::new(9);
        let mut added = pinned.clone();
        k9.add_to(&mut added, 150);
        let g9 = passkey_grant(&d, &w.owner.account, &k9, HERE);
        assert_eq!(start(&g9, Some(&added)), Err(GrantRefusal::Passkey), "a carried log can't add a key");
        assert_eq!(start(&g, Some(&added)), Ok(()));
        // One that dropped the removal the machine saw: refused.
        let mut rewritten = Log { events: pinned.events[..2].to_vec(), ..pinned.clone() };
        k9.add_to(&mut rewritten, 60);
        assert_eq!(start(&g, Some(&rewritten)), Err(GrantRefusal::PasskeyLog));
        // Another account's log as the granter's.
        assert_eq!(start(&g, Some(&k.logs[&w.other_owner.account])), Err(GrantRefusal::PasskeyLog));
    }

    /// DK6: a passkey revoke (v3) takes back v1 and v2 grants alike; only an
    /// owner's passkey makes one, over exactly its bytes; kept, it outlives
    /// its passkey.
    #[test]
    fn a_passkey_revoke_takes_back_v1_and_v2_grants() {
        let w = world();
        let k = keys(&w);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let origins = origins_of_sites([HERE]);
        let s = s2(&owners, &k.logs, &origins);
        let d = delegation("team:t1", &w.owner.keys, 10);
        let v1 = grant(&d, &w.owner.keys);
        let v2 = passkey_grant(&d, &w.owner.account, &k.k1, HERE);
        for g in [&v1, &v2] {
            assert_eq!(check(g, &d, Revokes::default(), &s, None, "runs.start", 200), Ok(()), "{}", g.v);
        }
        let r = passkey_revoke(&v2, &w.other_owner.account, &k.k3, 150);
        assert!(r.body().starts_with("arugula project grant revoke v3\n") && r.body().ends_with("\naccount own2\n"));
        for g in [&v1, &v2] {
            assert!(r.revokes(g, &s, 200));
            assert_eq!(
                check(g, &d, sent(std::slice::from_ref(&r)), &s, None, "runs.start", 200),
                Err(GrantRefusal::Revoked),
                "{}",
                g.v
            );
        }
        // A grant made after it isn't.
        let mut after = ProjectGrant::v2(&d, &w.owner.account, 151, 10_000);
        after.passkey = Some(k.k1.over(&after.passkey_challenge(), HERE, UV));
        assert_eq!(check(&after, &d, sent(std::slice::from_ref(&r)), &s, None, "runs.start", 200), Ok(()));
        // Not an editor's, nor one with no passkey, nor one moved to another
        // delegation or made harsher, nor a device's signature as a v3.
        let not = [
            passkey_revoke(&v2, &w.editor.account, &k.k4, 150),
            GrantRevoke { passkey: None, ..r.clone() },
            GrantRevoke { delegation: "other".into(), ..r.clone() },
            GrantRevoke { running: Running::Stop, ..r.clone() },
            GrantRevoke { at: 300, ..r.clone() },
            GrantRevoke { by: w.other_owner.keys.id(), ..r.clone() },
        ];
        for bad in not {
            assert!(!bad.revokes(&v2, &s, 200), "{bad:?}");
        }
        // Kept, it counts after the revoker's passkey is removed; sent
        // again, it doesn't check out.
        let mut later = k.logs.clone();
        let own2 = later.get_mut(&w.other_owner.account).unwrap();
        *own2 = removed(own2, &k.k3, 180);
        let s_later = s2(&owners, &later, &origins);
        assert_eq!(check(&v2, &d, sent(std::slice::from_ref(&r)), &s_later, None, "runs.start", 200), Ok(()));
        let kept = Revokes { kept: std::slice::from_ref(&r), ..Default::default() };
        assert_eq!(check(&v2, &d, kept, &s_later, None, "runs.start", 200), Err(GrantRefusal::Revoked));
        // A device's revoke (M211) still takes back a v2.
        let old = GrantRevoke::new(&v2.project, &v2.delegation, Running::Finish, 150, &w.other_owner.keys);
        assert!(old.revokes(&v2, &s, 200));
    }

    #[test]
    fn v2_and_v3_bytes_read_back() {
        let me = person("me");
        let d = delegation("account:me", &me.keys, 10);
        let g = ProjectGrant::v2(&d, "me", 100, 500);
        assert_eq!(ProjectGrant::parse(&g.body(), ""), Some(g.clone()));
        assert_eq!(ProjectGrant::parse(&g.body(), "00"), None, "a v2 holds no device signature");
        let r = GrantRevoke::v3(&g.project, &g.delegation, Running::Stop, 300, "me");
        assert_eq!(GrantRevoke::parse(&r.body(), ""), Some(r.clone()));
        assert_eq!(GrantRevoke::parse(&r.body(), "00"), None);
        // A v1's bytes are never a v2's, nor its challenge.
        let mut v1 = ProjectGrant { id: g.id.clone(), ..ProjectGrant::new(&d, 100, Some(500)) };
        v1.sign_with(&me.keys);
        assert_ne!(v1.passkey_challenge(), g.passkey_challenge());
        assert_eq!(ProjectGrant::parse(&g.body().replace("\naccount me\n", "\nby me\n"), ""), None);
        assert_eq!(ProjectGrant::parse(&v1.body().replace("grant v1", "grant v2"), ""), None);
        // As stored and sent before DK6: no `v`, `account` or `passkey`.
        let json = serde_json::to_value(&v1).unwrap();
        assert!(json.get("v").is_none() && json.get("account").is_none() && json.get("passkey").is_none());
        assert_eq!(serde_json::from_value::<ProjectGrant>(json).unwrap(), v1);
        let json = serde_json::to_value(&g).unwrap();
        assert_eq!(json["v"], 2);
        assert_eq!(serde_json::from_value::<ProjectGrant>(json).unwrap(), g);
        let old = GrantRevoke::new(&g.project, &g.delegation, Running::Stop, 300, &me.keys);
        assert!(serde_json::to_value(&old).unwrap().get("v").is_none());
        assert_eq!(serde_json::to_value(&r).unwrap()["v"], 3);
    }
}

#[cfg(test)]
mod fixed {
    use super::{tests::s1, *};
    use crate::{Cert, DeviceKeys, Kind, Trust, hosted::HostedKey};
    use ed25519_dalek::SigningKey;

    const NONE: &LegacyAppChildren = &LegacyAppChildren::new();

    fn keys(sign: u8, noise: u8) -> DeviceKeys {
        DeviceKeys { noise_private: [0; 32], noise_public: [noise; 32], sign: SigningKey::from_bytes(&[sign; 32]) }
    }

    /// The page signs the same bytes (`web/scripts/run-grant.fixture.json`,
    /// checked by `web/scripts/run-grant.test.mjs`): the same bodies and
    /// signatures, from the same keys.
    #[test]
    fn the_pages_fixture_is_these_bytes() {
        let owner = keys(2, 3);
        let mut root = Cert::new(&owner, "acct1", Kind::Browser, "laptop");
        root.created = 1000;
        root.sign_with(&owner);
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: HostedKey::from_seed(&[1; 32]).public(),
            at: 2000,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&owner);
        let mut g = ProjectGrant { id: "0123456789abcdef0123456789abcdef".into(), ..ProjectGrant::new(&d, 3000, None) };
        g.sign_with(&owner);
        let mut e =
            ProjectGrant { id: "fedcba9876543210fedcba9876543210".into(), ..ProjectGrant::new(&d, 3000, Some(9000)) };
        e.sign_with(&owner);
        let r = GrantRevoke::new(&g.project, &g.delegation, Running::Stop, 4000, &owner);
        // DK6: what the page asks a passkey to sign (the assertion is the
        // browser's, so only the bytes and the challenge are pinned).
        let v2 =
            ProjectGrant { id: "00112233445566778899aabbccddeeff".into(), ..ProjectGrant::v2(&d, "acct1", 3000, 9000) };
        let r3 = GrantRevoke::v3(&g.project, &g.delegation, Running::Stop, 4000, "acct1");
        let got = serde_json::json!({
            "root": root, "delegation": d, "grant": g, "body": g.body(), "expiring": e, "revoke": r, "revoke_body": r.body(),
            "grant_v2": v2, "grant_v2_body": v2.body(), "grant_v2_challenge": v2.passkey_challenge(),
            "revoke_v3": r3, "revoke_v3_body": r3.body(), "revoke_v3_challenge": r3.passkey_challenge(),
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/scripts/run-grant.fixture.json");
        if std::env::var_os("ARUGULA_WRITE_FIXTURE").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        }
        let want: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(got, want, "ARUGULA_WRITE_FIXTURE=1 writes it again, if the formats changed on purpose");
        let t = Trust { account: "acct1".into(), root: root.device.clone() };
        let owners = Owners::Account { trust: &t, certs: std::slice::from_ref(&root), revocations: &[] };
        assert_eq!(check(&g, &d, Revokes::default(), &s1(&owners, NONE), None, "runs.start", 3500), Ok(()));
        let sent = Revokes { sent: std::slice::from_ref(&r), ..Default::default() };
        assert_eq!(check(&g, &d, sent, &s1(&owners, NONE), None, "runs.start", 3500), Err(GrantRefusal::Revoked));
    }
}
