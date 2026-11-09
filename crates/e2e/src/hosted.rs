//! Hosted machines (M178): `arugulad` in a box the operator runs, enrolled
//! into one project by control under that project's delegation. Design in
//! docs/control-e2e.md, "Hosted machines: a project's delegation", and
//! "Control enrolls hosted machines under a project delegation" (aru-095).
//!
//! An unattended box has no device present to sign its join, so an owner's
//! device signs once, per project, that control may enroll hosted machines
//! into it ([`Delegation`]): it names control's hosted key. Control then
//! signs each box's certificate with that key ([`HostedCert`]), scoped to
//! the project and the workspace. Clients accept such a machine only for
//! that project ([`accept`]): it's never one of an account's devices
//! (its body is its own domain, and [`crate::Trust::evaluate`] never
//! counts it), it approves nothing, and a device outside the project
//! refuses it. Revoking the delegation ([`DelegationRevoke`]) stops
//! everything control signs under it from then on; what it signed before
//! stays, as with a revoked device's approvals.
//!
//! **A revoked delegation stays revoked** (aru-095). Control judges a revoke
//! once, as it arrives, by who's an owner then and by its own clock. From
//! then on it says the delegation ended ([`DelegationClosed`]), signed with
//! the key the delegation names: the delegate giving up what it was given,
//! which needs nobody's authority and only ever makes an answer a refusal.
//! Whoever checks keeps that word ([`HeldClosures`]).
//!
//! What's signed is a fixed line format, as for certificates:
//!
//! ```text
//! arugula project delegation v1
//! project <prj_ id>
//! owner team:<id>|account:<id>
//! key <control's hosted key, ed25519 hex>
//! at <ms>
//! by <device id>
//! ```
//!
//! ```text
//! arugula project delegation revoke v1
//! project <prj_ id>
//! delegation <delegation id>
//! at <ms>
//! by <device id>
//! ```
//!
//! ```text
//! arugula project delegation closed v1
//! project <prj_ id>
//! delegation <delegation id>
//! at <ms>
//! ```
//!
//! ```text
//! arugula hosted machine v1
//! project <prj_ id>
//! owner team:<id>|account:<id>
//! workspace <hw_ id>
//! delegation <delegation id>
//! device <id>
//! name <text>
//! noise <hex>
//! sign <hex>
//! created <ms>
//! ```

use std::collections::BTreeMap;

use anyhow::ensure;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    Cert, Kind, Revocation, Trust,
    cert::{LegacyAppChildren, legacy_app_link, verify_hex},
    keys::{device_id, hex32},
    team::{AccountCerts, Roster, TeamRole},
};

/// What a client calls such a machine, wherever it shows one: true of any
/// VM, because whoever runs it can read its memory.
pub const LABEL: &str = "Hosted · operator can read";

/// Who a project belongs to: a team, or one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Team(String),
    Account(String),
}

impl Owner {
    /// `team:<id>` or `account:<id>`, as signed.
    pub fn parse(s: &str) -> Option<Self> {
        let (kind, id) = s.split_once(':')?;
        if !token(id) {
            return None;
        }
        match kind {
            "team" => Some(Owner::Team(id.into())),
            "account" => Some(Owner::Account(id.into())),
            _ => None,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Owner::Team(t) => format!("team:{t}"),
            Owner::Account(a) => format!("account:{a}"),
        }
    }
}

fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn plain(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

/// Only people's own devices sign for a project: a browser, a CLI or the
/// desktop app; not a machine, and not a recovery code, which signs one
/// certificate and nothing else.
fn signs_for_projects(kind: Kind) -> bool {
    matches!(kind, Kind::Browser | Kind::Cli | Kind::App)
}

/// An owner's device saying control may enroll hosted machines into
/// `project` with `key`, until it's revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Delegation {
    pub project: String,
    /// `team:<id>` or `account:<id>`: the project's owner, as the signing
    /// device saw it.
    pub owner: String,
    /// Control's hosted key (Ed25519, hex): what signs the machines.
    pub key: String,
    pub at: u64,
    /// The signing device.
    pub by: String,
    pub sig: String,
}

impl Delegation {
    pub fn body(&self) -> String {
        format!(
            "arugula project delegation v1\nproject {}\nowner {}\nkey {}\nat {}\nby {}\n",
            self.project, self.owner, self.key, self.at, self.by
        )
    }

    /// What names it: the first 16 bytes of SHA-256 over its body, hex.
    pub fn id(&self) -> String {
        hex::encode(&Sha256::digest(self.body().as_bytes())[..16])
    }

    pub fn owner(&self) -> Option<Owner> {
        Owner::parse(&self.owner)
    }

    pub fn well_formed(&self) -> bool {
        token(&self.project) && self.owner().is_some() && hex32(&self.key).is_ok() && token(&self.by)
    }

    pub fn sign_with(&mut self, keys: &crate::DeviceKeys) {
        self.by = keys.id();
        self.sig = hex::encode(keys.signature(self.body().as_bytes()));
    }

    /// Signed by a device that may sign for its project, as `owners` says.
    pub fn signed_for(&self, owners: &Owners) -> bool {
        self.well_formed() && owners.may_sign(&self.owner, &self.by, &self.body(), &self.sig)
    }
}

/// A delegation taken back: control may enroll nothing more under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DelegationRevoke {
    pub project: String,
    /// The delegation's [`Delegation::id`].
    pub delegation: String,
    pub at: u64,
    pub by: String,
    pub sig: String,
}

impl DelegationRevoke {
    pub fn new(d: &Delegation, at: u64, by: &crate::DeviceKeys) -> Self {
        let mut r = Self { project: d.project.clone(), delegation: d.id(), at, by: by.id(), sig: String::new() };
        r.sig = hex::encode(by.signature(r.body().as_bytes()));
        r
    }

    pub fn body(&self) -> String {
        format!(
            "arugula project delegation revoke v1\nproject {}\ndelegation {}\nat {}\nby {}\n",
            self.project, self.delegation, self.at, self.by
        )
    }

    /// It takes back `d`, signed by a device that may sign for `d`'s
    /// project (any of its owners, not only the one who delegated).
    pub fn revokes(&self, d: &Delegation, owners: &Owners) -> bool {
        self.at <= MAX_TIME
            && self.project == d.project
            && self.delegation == d.id()
            && owners.may_sign(&d.owner, &self.by, &self.body(), &self.sig)
    }
}

/// Control's own word that a delegation ended, signed with the key the
/// delegation names. `at` is control's clock, or the earliest revoke's own
/// time when that's before it. Control sends it beside the revokes it took.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DelegationClosed {
    pub project: String,
    /// The delegation's [`Delegation::id`].
    pub delegation: String,
    pub at: u64,
    pub sig: String,
}

impl DelegationClosed {
    pub fn body(&self) -> String {
        format!(
            "arugula project delegation closed v1\nproject {}\ndelegation {}\nat {}\n",
            self.project, self.delegation, self.at
        )
    }

    /// It ends `d`, signed with the key `d` names.
    pub fn closes(&self, d: &Delegation) -> bool {
        self.at <= MAX_TIME
            && self.project == d.project
            && self.delegation == d.id()
            && verify_hex(&d.key, self.body().as_bytes(), &self.sig)
    }
}

/// The largest time these papers take, 2^53 - 1: the browser reads a JSON
/// number as a double, and signs and checks the digits it makes of it.
pub const MAX_TIME: u64 = (1 << 53) - 1;

/// A time as these papers read one, here and in the browser's copy
/// (`hosted.ts`): a JSON number whose value is a non-negative integer no larger
/// than [`MAX_TIME`], however it's written (`2500`, `2500.0`, `2.5e3`). A page
/// has only the value to go by, and the signed body says it in plain digits.
pub fn read_time(v: &serde_json::Value) -> Option<u64> {
    let t = match v.as_u64() {
        Some(t) => t,
        None => {
            let f = v.as_f64()?;
            if f.fract() != 0.0 || f < 0.0 || f > MAX_TIME as f64 {
                return None;
            }
            f as u64
        }
    };
    (t <= MAX_TIME).then_some(t)
}

/// `v` as a `T`, its `field` in plain digits if it reads as a time.
fn with_time<T: serde::de::DeserializeOwned>(v: &serde_json::Value, field: &str) -> serde_json::Result<T> {
    let mut v = v.clone();
    if let Some(t) = read_time(&v[field])
        && let Some(o) = v.as_object_mut()
    {
        o.insert(field.to_owned(), t.into());
    }
    serde_json::from_value(v)
}

/// A machine's certificate as a directory or an enrollment lists it.
pub fn read_cert(v: &serde_json::Value) -> serde_json::Result<HostedCert> {
    with_time(v, "created")
}

/// A delegation as a directory or an enrollment lists it.
pub fn read_delegation(v: &serde_json::Value) -> serde_json::Result<Delegation> {
    with_time(v, "at")
}

/// The entries of a list of revokes or closures that read, each on its own:
/// one that doesn't, or whose `at` isn't a time ([`read_time`]), is left out,
/// and takes nothing else with it. What isn't a list is none. The browser's
/// copy reads a list the same way (`hosted.ts`).
pub fn entries<T: serde::de::DeserializeOwned>(list: &serde_json::Value) -> Vec<T> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|e| read_time(&e["at"]).is_some())
        .filter_map(|e| with_time(e, "at").ok())
        .collect()
}

/// [`entries`], for a field of a type that comes over the wire.
pub fn read_entries<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(entries(&serde_json::Value::deserialize(d)?))
}

/// What a directory entry's `hosted` says of how its delegation ended: the
/// revokes control took and its own word, as a CLI and a daemon read them.
pub fn ends_listed(hosted: &serde_json::Value) -> (Vec<DelegationRevoke>, Vec<DelegationClosed>) {
    (entries(&hosted["revocations"]), entries(&hosted["closed"]))
}

/// How many delegations' ends a client keeps. Each is of a delegation an
/// owner signed, so this is far more than one client meets.
pub const HELD_CLOSURES: usize = 512;

/// Control's word that a delegation ended, as a client that once heard it
/// keeps it, by delegation: control leaving it out later changes nothing. The
/// earliest of each, and [`HELD_CLOSURES`] at most (the earliest-ended go
/// first). A client that never heard one has none to keep.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HeldClosures(BTreeMap<String, DelegationClosed>);

impl HeldClosures {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The one kept for delegation `id`.
    pub fn get(&self, id: &str) -> Option<&DelegationClosed> {
        self.0.get(id)
    }

    /// Keep those of `closed` that check out, when an owner signed `d`, as
    /// `owners` says: nothing else can fill this.
    pub fn take(&mut self, d: &Delegation, closed: &[DelegationClosed], owners: &Owners) {
        if !closed.iter().any(|c| c.closes(d)) || !d.signed_for(owners) {
            return;
        }
        for c in closed.iter().filter(|c| c.closes(d)) {
            self.keep(c.clone());
        }
    }

    /// Add what `other` holds: the earlier of each. It's checked when it's
    /// used ([`accept`]), as anything read back from a file is.
    pub fn merge(&mut self, other: &HeldClosures) {
        for c in other.0.values() {
            self.keep(c.clone());
        }
    }

    fn keep(&mut self, c: DelegationClosed) {
        match self.0.get_mut(&c.delegation) {
            Some(held) if held.at <= c.at => {}
            Some(held) => *held = c,
            None => {
                self.0.insert(c.delegation.clone(), c);
            }
        }
        while self.0.len() > HELD_CLOSURES {
            let first = self.0.values().min_by_key(|c| (c.at, c.delegation.clone())).map(|c| c.delegation.clone());
            first.map(|id| self.0.remove(&id));
        }
    }
}

/// Control's hosted key: it signs hosted machines' certificates, public
/// previews' grants (M187, [`crate::preview`]) and the end of a delegation
/// ([`DelegationClosed`]), each under a first line of its own, and nothing
/// else. A delegation names its public half.
pub struct HostedKey(SigningKey);

impl HostedKey {
    pub fn generate() -> Self {
        Self(SigningKey::from_bytes(&crate::random()))
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(seed))
    }

    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Its public half, hex: what a delegation names.
    pub fn public(&self) -> String {
        hex::encode(self.0.verifying_key().to_bytes())
    }

    pub fn sign(&self, cert: &mut HostedCert) {
        cert.sig = hex::encode(self.0.sign(cert.body().as_bytes()).to_bytes());
    }

    /// Its word that `d` ended at `at`.
    pub fn close(&self, d: &Delegation, at: u64) -> DelegationClosed {
        let mut c = DelegationClosed { project: d.project.clone(), delegation: d.id(), at, sig: String::new() };
        c.sig = hex::encode(self.0.sign(c.body().as_bytes()).to_bytes());
        c
    }

    /// An Ed25519 signature over `body`, which starts with a first line of
    /// its own (`crate::preview`'s grant).
    pub(crate) fn sign_bytes(&self, body: &[u8]) -> [u8; 64] {
        self.0.sign(body).to_bytes()
    }
}

/// A hosted machine's certificate: its keys, the project and workspace it
/// belongs to, and the delegation it was enrolled under, signed with the
/// key that delegation names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HostedCert {
    pub project: String,
    pub owner: String,
    /// The hosted workspace (`hw_…`) the box is.
    pub workspace: String,
    pub delegation: String,
    pub device: String,
    pub name: String,
    pub noise: String,
    pub sign: String,
    pub created: u64,
    #[serde(default)]
    pub sig: String,
}

impl HostedCert {
    /// Unsigned, for the machine whose public keys these are, under `d`.
    pub fn new(d: &Delegation, workspace: &str, name: &str, noise: &str, sign: &str, created: u64) -> Self {
        let device = match (hex32(noise), hex32(sign)) {
            (Ok(n), Ok(s)) => device_id(&n, &s),
            _ => String::new(),
        };
        Self {
            project: d.project.clone(),
            owner: d.owner.clone(),
            workspace: workspace.into(),
            delegation: d.id(),
            device,
            name: name.into(),
            noise: noise.into(),
            sign: sign.into(),
            created,
            sig: String::new(),
        }
    }

    pub fn body(&self) -> String {
        format!(
            "arugula hosted machine v1\nproject {}\nowner {}\nworkspace {}\ndelegation {}\ndevice {}\nname {}\nnoise {}\nsign {}\ncreated {}\n",
            self.project,
            self.owner,
            self.workspace,
            self.delegation,
            self.device,
            self.name,
            self.noise,
            self.sign,
            self.created
        )
    }

    /// Well formed, with an id that matches its keys. Says nothing about
    /// the signature.
    pub fn check_form(&self) -> anyhow::Result<()> {
        ensure!(token(&self.project) && token(&self.workspace) && token(&self.delegation), "bad ids");
        ensure!(Owner::parse(&self.owner).is_some(), "bad owner");
        ensure!(plain(&self.name, 64), "a machine name is 1 to 64 characters, none of them control characters");
        let (noise, sign) = (hex32(&self.noise)?, hex32(&self.sign)?);
        ensure!(device_id(&noise, &sign) == self.device, "device id doesn't match its keys");
        Ok(())
    }

    pub fn noise_key(&self) -> [u8; 32] {
        hex32(&self.noise).unwrap_or_default()
    }
}

/// What the machine signs with its own key when it enrolls: that it holds
/// the key, at about this time, for this enrollment (`credential` is the
/// SHA-256, hex, of the ticket or sandbox id it enrolls with, so the proof
/// can't be moved to another enrollment, and the credential itself isn't
/// in it).
pub fn enroll_proof_body(credential: &str, noise: &str, sign: &str, ms: u64) -> String {
    format!("arugula hosted enroll v1\nenrollment {credential}\nnoise {noise}\nsign {sign}\nat {ms}\n")
}

/// Who may sign for a project, as the client checking knows them: for a
/// team's, the team's roster it holds (its owners, from the roots it
/// lists) and its members' certificates; for an account's, that account's
/// pinned root and certificates.
pub enum Owners<'a> {
    Team { roster: &'a Roster, certs: &'a AccountCerts },
    Account { trust: &'a Trust, certs: &'a [Cert], revocations: &'a [Revocation] },
}

impl Owners<'_> {
    /// Which project owner this describes.
    pub fn owner(&self) -> Owner {
        match self {
            Owners::Team { roster, .. } => Owner::Team(roster.team.clone()),
            Owners::Account { trust, .. } => Owner::Account(trust.account.clone()),
        }
    }

    /// `sig` is `device`'s over `body`, and `device` is one of a project
    /// owner's own devices, trusted now: for a team, of an account the
    /// roster makes an owner; for an account, of that account. The owner
    /// named must be this one.
    pub(crate) fn may_sign(&self, owner: &str, device: &str, body: &str, sig: &str) -> bool {
        if Owner::parse(owner) != Some(self.owner()) {
            return false;
        }
        let ok = |c: &Cert| signs_for_projects(c.kind) && verify_hex(&c.sign, body.as_bytes(), sig);
        match self {
            Owners::Team { roster, certs } => roster
                .members
                .iter()
                .filter(|m| m.role == TeamRole::Owner)
                .any(|m| roster.devices(&m.account, certs).get(device).is_some_and(ok)),
            Owners::Account { trust, certs, revocations } => {
                trust.evaluate(certs, revocations).get(device).is_some_and(ok)
            }
        }
    }

    /// [`Owners::may_sign`], by a device that's a browser all the way back
    /// to its owner's root (`browser` decides each link, given the
    /// certificate just below it in the chain, `None` for `device`;
    /// recovery codes count on the way, `cert::browser_link`): what a
    /// project's standing grant and its revokes take (M211), as a team's
    /// owner or editor does. Never a CLI's or the app's, whose keys a
    /// program on their computer may hold.
    pub fn browser_signed_by(
        &self,
        owner: &str,
        device: &str,
        body: &str,
        sig: &str,
        browser: impl Fn(&Cert, Option<&Cert>) -> bool + Copy,
    ) -> bool {
        if !self.may_sign(owner, device, body, sig) {
            return false;
        }
        let chained = |trust: &Trust, certs: &[Cert]| {
            trust.chained_below(certs, device, |c, below| crate::cert::browser_link(c, device, |c| browser(c, below)))
        };
        match self {
            Owners::Team { roster, certs } => roster.members.iter().filter(|m| m.role == TeamRole::Owner).any(|m| {
                let c = certs.get(&m.account).map(|(c, _)| c.as_slice()).unwrap_or_default();
                roster.devices(&m.account, certs).get(device).is_some()
                    && chained(&Trust { account: m.account.clone(), root: m.root.clone() }, c)
            }),
            Owners::Account { trust, certs, .. } => chained(trust, certs),
        }
    }

    /// [`Owners::browser_signed_by`] with a browser's or a phone's own
    /// device on each link ([`Cert::is_browser`]), or the desktop app's
    /// window from before it had a kind just above a browser it approved
    /// back then (`children`, by account, [`legacy_app_link`]): never as
    /// the device that signs, nor above anything else. A hosted machine's
    /// check of a v1 standing grant against the set it pinned; one that
    /// pinned none counts no app window.
    pub fn browser_signed(
        &self,
        owner: &str,
        device: &str,
        body: &str,
        sig: &str,
        children: &LegacyAppChildren,
    ) -> bool {
        self.browser_signed_by(owner, device, body, sig, |c, below| {
            c.is_browser() || children.get(&c.account).is_some_and(|l| legacy_app_link(c, below, l))
        })
    }

    /// `account`'s place in the project: its team role, or owner of its
    /// own account's project. `None` when it isn't in the project.
    pub fn role_of(&self, account: &str) -> Option<TeamRole> {
        match self {
            Owners::Team { roster, .. } => roster.member(account).map(|m| m.role),
            Owners::Account { trust, .. } => (trust.account == account).then_some(TeamRole::Owner),
        }
    }
}

/// A hosted machine a client accepted: for this project only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub project: String,
    pub owner: Owner,
    pub workspace: String,
    /// The client's account's role in the project.
    pub role: TeamRole,
}

/// Why a client refused a hosted machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedRefusal {
    /// The certificate or delegation isn't well formed, or they don't name
    /// each other.
    Form,
    /// What the client knows of the project's owner isn't the owner the
    /// delegation names.
    WrongProject,
    /// The client's account isn't in the project: a device outside it
    /// never trusts its hosted machines.
    NotInProject,
    /// No owner's device signed the delegation (or that device isn't
    /// trusted now).
    Unsigned,
    /// The certificate isn't signed with the key the delegation names.
    BadSignature,
    /// Made before the delegation, or after it was revoked.
    Revoked,
}

impl HostedRefusal {
    pub fn code(self) -> &'static str {
        match self {
            HostedRefusal::Form => "form",
            HostedRefusal::WrongProject => "wrong_project",
            HostedRefusal::NotInProject => "not_in_project",
            HostedRefusal::Unsigned => "unsigned",
            HostedRefusal::BadSignature => "bad_signature",
            HostedRefusal::Revoked => "revoked",
        }
    }
}

/// Whether the client of `me` (an account) accepts `cert` as a hosted
/// machine of its project: the delegation is signed by one of the
/// project's owners (as `owners` says, which the client got from its own
/// checks, not control's word for the machine), the certificate is signed
/// with the key it names, made while it stood, and `me` is in the project.
/// It ended at the earliest of a revoke an owner signed, as `owners` says
/// now, and `closed`, control's word ([`DelegationClosed`]). Either that
/// doesn't check out is ignored, as everywhere: control may send junk, and it
/// can only make the answer a refusal.
pub fn accept(
    me: &str,
    cert: &HostedCert,
    d: &Delegation,
    revokes: &[DelegationRevoke],
    closed: &[DelegationClosed],
    owners: &Owners,
) -> Result<Accepted, HostedRefusal> {
    let owner = d.owner().ok_or(HostedRefusal::Form)?;
    if cert.check_form().is_err()
        || !d.well_formed()
        || cert.created > MAX_TIME
        || d.at > MAX_TIME
        || cert.project != d.project
        || cert.owner != d.owner
        || cert.delegation != d.id()
    {
        return Err(HostedRefusal::Form);
    }
    if owners.owner() != owner {
        return Err(HostedRefusal::WrongProject);
    }
    let role = owners.role_of(me).ok_or(HostedRefusal::NotInProject)?;
    if !d.signed_for(owners) {
        return Err(HostedRefusal::Unsigned);
    }
    if !verify_hex(&d.key, cert.body().as_bytes(), &cert.sig) {
        return Err(HostedRefusal::BadSignature);
    }
    let by_owner = revokes.iter().filter(|r| r.revokes(d, owners)).map(|r| r.at);
    let revoked = by_owner.chain(closed.iter().filter(|c| c.closes(d)).map(|c| c.at)).min();
    if cert.created < d.at || revoked.is_some_and(|at| at <= cert.created) {
        return Err(HostedRefusal::Revoked);
    }
    Ok(Accepted { project: d.project.clone(), owner, workspace: cert.workspace.clone(), role })
}

/// [`accept`], by a client that keeps what it hears: `closed` that checks out
/// goes into `held`, and what `held` has of `d` counts as though control sent
/// it again.
pub fn accept_holding(
    me: &str,
    cert: &HostedCert,
    d: &Delegation,
    revokes: &[DelegationRevoke],
    closed: &[DelegationClosed],
    owners: &Owners,
    held: &mut HeldClosures,
) -> Result<Accepted, HostedRefusal> {
    held.take(d, closed, owners);
    let all: Vec<DelegationClosed> = closed.iter().chain(held.get(&d.id())).cloned().collect();
    accept(me, cert, d, revokes, &all, owners)
}

/// Whether control may enroll under `d` now: it checks out for `owners`
/// and nothing revokes it. What control checks before it signs (clients
/// check again, with what they hold).
pub fn live(d: &Delegation, revokes: &[DelegationRevoke], owners: &Owners) -> bool {
    d.signed_for(owners) && !revokes.iter().any(|r| r.revokes(d, owners))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DeviceKeys,
        team::{Member, Roster},
    };

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

    fn delegation(owner: &str, key: &HostedKey, by: &DeviceKeys, at: u64) -> Delegation {
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: owner.into(),
            key: key.public(),
            at,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(by);
        d
    }

    fn machine(d: &Delegation, key: &HostedKey, created: u64) -> (DeviceKeys, HostedCert) {
        let k = DeviceKeys::generate();
        let mut c =
            HostedCert::new(d, "hw_1", "box", &hex::encode(k.noise_public), &hex::encode(k.sign_public()), created);
        key.sign(&mut c);
        (k, c)
    }

    /// A team with an owner, an editor and a viewer; and someone outside.
    struct World {
        owner: Person,
        editor: Person,
        viewer: Person,
        outsider: Person,
        roster: Roster,
        certs: AccountCerts,
        key: HostedKey,
    }

    fn world() -> World {
        let (owner, editor, viewer, outsider) = (person("own"), person("ed"), person("view"), person("out"));
        let roster = roster(
            vec![member(&owner, TeamRole::Owner), member(&editor, TeamRole::Editor), member(&viewer, TeamRole::Viewer)],
            &owner,
        );
        let certs = certs(&[&owner, &editor, &viewer, &outsider]);
        World { owner, editor, viewer, outsider, roster, certs, key: HostedKey::generate() }
    }

    #[test]
    fn teammates_accept_a_machine_for_their_project() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (_, c) = machine(&d, &w.key, 20);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        for (p, role) in [(&w.owner, TeamRole::Owner), (&w.editor, TeamRole::Editor), (&w.viewer, TeamRole::Viewer)] {
            let a = accept(&p.account, &c, &d, &[], &[], &owners).unwrap();
            assert_eq!(
                a,
                Accepted { project: "prj_1".into(), owner: Owner::Team("t1".into()), workspace: "hw_1".into(), role }
            );
        }
    }

    #[test]
    fn a_device_outside_the_project_refuses_it() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (_, c) = machine(&d, &w.key, 20);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        assert_eq!(accept(&w.outsider.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::NotInProject));
        // Nor as a machine of the outsider's own account's project.
        let t = Trust { account: w.outsider.account.clone(), root: w.outsider.cert.device.clone() };
        let theirs = Owners::Account { trust: &t, certs: std::slice::from_ref(&w.outsider.cert), revocations: &[] };
        assert_eq!(accept(&w.outsider.account, &c, &d, &[], &[], &theirs), Err(HostedRefusal::WrongProject));
    }

    #[test]
    fn only_owners_devices_delegate() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        // An editor, a viewer, an outsider, control's own key: none.
        for by in [&w.editor.keys, &w.viewer.keys, &w.outsider.keys] {
            let d = delegation("team:t1", &w.key, by, 10);
            let (_, c) = machine(&d, &w.key, 20);
            assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Unsigned));
        }
        // An owner's machine, or a recovery code of theirs, doesn't either.
        for kind in [Kind::Daemon, Kind::Recovery] {
            let k = DeviceKeys::generate();
            let mut kc = Cert::new(&k, &w.owner.account, kind, "x");
            kc.sign_with(&w.owner.keys);
            let mut certs = w.certs.clone();
            certs.get_mut(&w.owner.account).unwrap().0.push(kc);
            let owners = Owners::Team { roster: &w.roster, certs: &certs };
            let d = delegation("team:t1", &w.key, &k, 10);
            let (_, c) = machine(&d, &w.key, 20);
            assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Unsigned), "{kind:?}");
        }
        // A delegation for another team, signed by this team's owner,
        // isn't this team's.
        let d = delegation("team:t2", &w.key, &w.owner.keys, 10);
        let (_, c) = machine(&d, &w.key, 20);
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::WrongProject));
    }

    #[test]
    fn an_owner_who_leaves_takes_their_delegation_with_them() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (_, c) = machine(&d, &w.key, 20);
        // The owner is now an editor, and the editor the owner.
        let later = roster(vec![member(&w.owner, TeamRole::Editor), member(&w.editor, TeamRole::Owner)], &w.owner);
        let owners = Owners::Team { roster: &later, certs: &w.certs };
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Unsigned));
        // So does a device of theirs that signed it and is removed since.
        let pk = DeviceKeys::generate();
        let mut pc = Cert::new(&pk, &w.owner.account, Kind::Browser, "phone");
        pc.sign_with(&w.owner.keys);
        let d = delegation("team:t1", &w.key, &pk, 10);
        let (_, c) = machine(&d, &w.key, 20);
        let mut certs = w.certs.clone();
        certs.get_mut(&w.owner.account).unwrap().0.push(pc.clone());
        let owners = Owners::Team { roster: &w.roster, certs: &certs };
        assert!(accept(&w.editor.account, &c, &d, &[], &[], &owners).is_ok());
        certs.get_mut(&w.owner.account).unwrap().1.push(Revocation::new(&w.owner.account, &pc.device, &w.owner.keys));
        let owners = Owners::Team { roster: &w.roster, certs: &certs };
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Unsigned));
    }

    #[test]
    fn signed_with_the_delegated_key_only() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let (_, c) = machine(&d, &HostedKey::generate(), 20);
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::BadSignature));
        // Tampered after signing: another workspace, another key.
        let (_, mut c) = machine(&d, &w.key, 20);
        c.workspace = "hw_2".into();
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::BadSignature));
        let (_, mut c) = machine(&d, &w.key, 20);
        c.noise = hex::encode(DeviceKeys::generate().noise_public);
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Form));
        // Moved to another project, or under another delegation.
        let (_, mut c) = machine(&d, &w.key, 20);
        c.project = "prj_2".into();
        w.key.sign(&mut c);
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Form));
        let other = delegation("team:t1", &w.key, &w.owner.keys, 11);
        let (_, c) = machine(&other, &w.key, 20);
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[], &owners), Err(HostedRefusal::Form));
    }

    #[test]
    fn revoking_stops_what_comes_after() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let (_, before) = machine(&d, &w.key, 20);
        let (_, after) = machine(&d, &w.key, 40);
        let r = DelegationRevoke::new(&d, 30, &w.owner.keys);
        assert!(live(&d, &[], &owners));
        assert!(!live(&d, std::slice::from_ref(&r), &owners));
        assert!(accept(&w.editor.account, &before, &d, std::slice::from_ref(&r), &[], &owners).is_ok());
        assert_eq!(accept(&w.editor.account, &after, &d, &[r], &[], &owners), Err(HostedRefusal::Revoked));
        // Before the delegation was made: not under it.
        let (_, early) = machine(&d, &w.key, 5);
        assert_eq!(accept(&w.editor.account, &early, &d, &[], &[], &owners), Err(HostedRefusal::Revoked));
        // A revocation by someone who isn't an owner counts for nothing.
        let spite = DelegationRevoke::new(&d, 30, &w.editor.keys);
        assert!(live(&d, std::slice::from_ref(&spite), &owners));
        assert!(accept(&w.editor.account, &after, &d, &[spite], &[], &owners).is_ok());
    }

    /// A revoke counts by whether its signer is an owner now. Control's word
    /// counts whoever that was: an owner who left, one made an editor, a
    /// device taken off an account.
    #[test]
    fn controls_word_holds_when_a_revokes_signer_is_gone() {
        let w = world();
        let other = person("other");
        let team = |members| roster(members, &w.owner);
        let both = team(vec![
            member(&w.owner, TeamRole::Owner),
            member(&other, TeamRole::Owner),
            member(&w.editor, TeamRole::Editor),
        ]);
        let mut certs = w.certs.clone();
        certs.insert(other.account.clone(), (vec![other.cert.clone()], vec![]));
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (_, before) = machine(&d, &w.key, 20);
        let (_, after) = machine(&d, &w.key, 40);
        let r = DelegationRevoke::new(&d, 30, &other.keys);
        let closed = w.key.close(&d, 30);
        assert!(closed.closes(&d));
        let revs = std::slice::from_ref(&r);
        let word = std::slice::from_ref(&closed);

        // Still an owner: the same answer with or without control's word.
        let owners = Owners::Team { roster: &both, certs: &certs };
        for c in [&[][..], word] {
            assert!(accept(&w.editor.account, &before, &d, revs, c, &owners).is_ok());
            assert_eq!(accept(&w.editor.account, &after, &d, revs, c, &owners), Err(HostedRefusal::Revoked));
        }

        // Gone from the team, or an editor now.
        let gone = team(vec![member(&w.owner, TeamRole::Owner), member(&w.editor, TeamRole::Editor)]);
        let editor = team(vec![
            member(&w.owner, TeamRole::Owner),
            member(&other, TeamRole::Editor),
            member(&w.editor, TeamRole::Editor),
        ]);
        for later in [&gone, &editor] {
            let owners = Owners::Team { roster: later, certs: &certs };
            assert!(!r.revokes(&d, &owners));
            assert!(accept(&w.editor.account, &after, &d, revs, &[], &owners).is_ok());
            assert!(accept(&w.editor.account, &before, &d, revs, word, &owners).is_ok(), "what came before stays");
            assert_eq!(accept(&w.editor.account, &after, &d, revs, word, &owners), Err(HostedRefusal::Revoked));
        }

        // A device taken off an account.
        let me = person("me");
        let t = Trust { account: "me".into(), root: me.cert.device.clone() };
        let d = delegation("account:me", &w.key, &me.keys, 10);
        let (_, before) = machine(&d, &w.key, 20);
        let (_, after) = machine(&d, &w.key, 40);
        let pk = DeviceKeys::generate();
        let mut pc = Cert::new(&pk, "me", Kind::Browser, "phone");
        pc.sign_with(&me.keys);
        let all = [me.cert.clone(), pc.clone()];
        let removed = [Revocation::new("me", &pc.device, &me.keys)];
        let r = DelegationRevoke::new(&d, 30, &pk);
        let closed = w.key.close(&d, 30);
        let owners = Owners::Account { trust: &t, certs: &all, revocations: &removed };
        assert!(!r.revokes(&d, &owners));
        let (revs, word) = (std::slice::from_ref(&r), std::slice::from_ref(&closed));
        assert!(accept("me", &after, &d, revs, &[], &owners).is_ok());
        assert!(accept("me", &before, &d, revs, word, &owners).is_ok());
        assert_eq!(accept("me", &after, &d, revs, word, &owners), Err(HostedRefusal::Revoked));
    }

    /// Only the key the delegation names says it ended, of that delegation,
    /// for that time. Nobody else's word counts, a former owner's included.
    #[test]
    fn only_the_delegated_key_closes_it() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let other = delegation("team:t1", &w.key, &w.owner.keys, 11);
        let (_, c) = machine(&d, &w.key, 40);
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let good = w.key.close(&d, 30);
        let mut moved = good.clone();
        moved.at = 50;
        let mut elsewhere = good.clone();
        elsewhere.project = "prj_2".into();
        // A device of an owner's, or of anyone's, signing in control's place.
        let signed_by = |by: &DeviceKeys| {
            let mut x = w.key.close(&d, 30);
            x.sig = hex::encode(by.signature(x.body().as_bytes()));
            x
        };
        for no in [
            HostedKey::generate().close(&d, 30),
            w.key.close(&other, 30),
            moved,
            elsewhere,
            signed_by(&w.owner.keys),
            signed_by(&w.outsider.keys),
        ] {
            assert!(!no.closes(&d), "{no:?}");
            assert!(accept(&w.editor.account, &c, &d, &[], &[no], &owners).is_ok());
        }
        assert!(good.closes(&d));
        assert_eq!(accept(&w.editor.account, &c, &d, &[], &[good], &owners), Err(HostedRefusal::Revoked));
    }

    /// A client keeps what it heard: control leaving it out later doesn't
    /// change the answer, across a save and a load too. What doesn't check
    /// out isn't kept, nor is anything of a delegation no owner signed.
    #[test]
    fn a_client_keeps_a_delegations_end() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (_, before) = machine(&d, &w.key, 20);
        let (_, after) = machine(&d, &w.key, 40);
        let word = w.key.close(&d, 30);
        let mut held = HeldClosures::default();
        let e = &w.editor.account;

        assert!(accept_holding(e, &after, &d, &[], &[], &owners, &mut held).is_ok(), "heard nothing yet");
        assert_eq!(
            accept_holding(e, &after, &d, &[], std::slice::from_ref(&word), &owners, &mut held),
            Err(HostedRefusal::Revoked)
        );
        assert_eq!(accept_holding(e, &after, &d, &[], &[], &owners, &mut held), Err(HostedRefusal::Revoked));
        assert!(accept_holding(e, &before, &d, &[], &[], &owners, &mut held).is_ok());
        let mut again: HeldClosures = serde_json::from_str(&serde_json::to_string(&held).unwrap()).unwrap();
        let mut merged = HeldClosures::default();
        merged.merge(&again);
        assert_eq!(merged, held);
        assert_eq!(accept_holding(e, &after, &d, &[], &[], &owners, &mut again), Err(HostedRefusal::Revoked));

        // A later time changes nothing; an earlier one is kept in its place.
        held.take(&d, &[w.key.close(&d, 90)], &owners);
        assert_eq!(held.get(&d.id()).unwrap().at, 30);
        held.take(&d, &[w.key.close(&d, 25)], &owners);
        assert_eq!(held.get(&d.id()).unwrap().at, 25);

        // Nothing that doesn't check out, nor of a delegation no owner signed.
        let mut none = HeldClosures::default();
        none.take(&d, &[HostedKey::generate().close(&d, 30)], &owners);
        let unsigned = delegation("team:t1", &w.key, &w.outsider.keys, 10);
        none.take(&unsigned, &[w.key.close(&unsigned, 30)], &owners);
        assert!(none.is_empty());
    }

    #[test]
    fn what_a_client_keeps_is_bounded() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let mut held = HeldClosures::default();
        let mut first = None;
        for n in 0..HELD_CLOSURES as u64 + 3 {
            let d = delegation("team:t1", &w.key, &w.owner.keys, 10 + n);
            held.take(&d, &[w.key.close(&d, 100 + n)], &owners);
            first.get_or_insert(d.id());
        }
        assert_eq!(held.len(), HELD_CLOSURES);
        assert!(held.get(&first.unwrap()).is_none(), "the earliest-ended goes first");
    }

    /// Each entry of a list reads on its own.
    #[test]
    fn one_bad_entry_takes_nothing_else_with_it() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (word, r) = (w.key.close(&d, 30), DelegationRevoke::new(&d, 30, &w.owner.keys));
        let (revokes, closed) = ends_listed(&serde_json::json!({
            "revocations": [{}, r, null, 7], "closed": [word, {}, "x"],
        }));
        assert_eq!((revokes, closed), (vec![r], vec![word]));
        assert_eq!(ends_listed(&serde_json::json!({ "revocations": {}, "closed": "x" })), (vec![], vec![]));
    }

    /// A time is a value, however it's written, up to 2^53 - 1.
    #[test]
    fn a_time_is_a_value_up_to_the_browsers_limit() {
        for (text, want) in [
            ("2500", Some(2500)),
            ("2500.0", Some(2500)),
            ("2.5e3", Some(2500)),
            ("0", Some(0)),
            ("-0.0", Some(0)),
            ("9007199254740991", Some(MAX_TIME)),
            ("9007199254740992", None),
            ("9007199254740992.0", None),
            ("18446744073709551615", None),
            ("2500.5", None),
            ("-1", None),
            ("-1.0", None),
            ("1e300", None),
            ("\"2500\"", None),
            ("true", None),
            ("null", None),
        ] {
            assert_eq!(read_time(&serde_json::from_str(text).unwrap()), want, "{text}");
        }
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let word = w.key.close(&d, 30);
        let spelled = format!(
            r#"{{"closed":[{{"project":"{}","delegation":"{}","at":3e1,"sig":"{}"}}]}}"#,
            word.project, word.delegation, word.sig
        );
        let (_, closed) = ends_listed(&serde_json::from_str(&spelled).unwrap());
        assert_eq!(closed, std::slice::from_ref(&word));
        assert!(closed[0].closes(&d));
        assert!(!w.key.close(&d, MAX_TIME + 1).closes(&d));
    }

    /// Hex reads in either case on these papers, as everywhere in Rust. What's
    /// signed is the text as given.
    #[test]
    fn hex_reads_in_either_case() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let mut upper = w.key.close(&d, 30);
        upper.sig = upper.sig.to_uppercase();
        assert!(upper.closes(&d));
        let mut named = d.clone();
        named.key = named.key.to_uppercase();
        assert!(named.well_formed());
    }

    #[test]
    fn a_personal_project() {
        let me = person("me");
        let key = HostedKey::generate();
        let d = delegation("account:me", &key, &me.keys, 10);
        let (_, c) = machine(&d, &key, 20);
        let t = Trust { account: "me".into(), root: me.cert.device.clone() };
        let owners = Owners::Account { trust: &t, certs: std::slice::from_ref(&me.cert), revocations: &[] };
        assert_eq!(accept("me", &c, &d, &[], &[], &owners).unwrap().role, TeamRole::Owner);
        assert_eq!(accept("you", &c, &d, &[], &[], &owners), Err(HostedRefusal::NotInProject));
        // Control's own "root" for the account: the delegation's signer
        // isn't in the chain this client pinned.
        let fake = person("me");
        let d2 = delegation("account:me", &key, &fake.keys, 10);
        let (_, c2) = machine(&d2, &key, 20);
        let both = [me.cert.clone(), fake.cert.clone()];
        let owners = Owners::Account { trust: &t, certs: &both, revocations: &[] };
        assert_eq!(accept("me", &c2, &d2, &[], &[], &owners), Err(HostedRefusal::Unsigned));
    }

    /// Personal devices never trust a hosted machine for anything else:
    /// it's no device of any account, approves nothing, and its key can't
    /// stand for a daemon there.
    #[test]
    fn never_one_of_an_accounts_devices() {
        let w = world();
        let d = delegation("team:t1", &w.key, &w.owner.keys, 10);
        let (mk, c) = machine(&d, &w.key, 20);
        let t = Trust { account: w.owner.account.clone(), root: w.owner.cert.device.clone() };
        // Its keys as a daemon certificate of the owner's account, approved
        // by nobody the account trusts (control holds no device key), or
        // "signed" by the hosted key in its place.
        let mut as_daemon = Cert::new(&mk, &w.owner.account, Kind::Daemon, "box");
        as_daemon.approver = c.delegation.clone();
        as_daemon.sig = c.sig.clone();
        let mut by_itself = Cert::new(&mk, &w.owner.account, Kind::Browser, "box");
        by_itself.sign_with(&mk);
        let all = [w.owner.cert.clone(), as_daemon.clone(), by_itself.clone()];
        let trusted = t.evaluate(&all, &[]);
        assert_eq!(trusted.devices.len(), 1);
        assert!(trusted.get(&c.device).is_none());
        // What it signs (a device it "approves") doesn't chain either.
        let (_, mut sneaky) =
            (DeviceKeys::generate(), Cert::new(&DeviceKeys::generate(), &w.owner.account, Kind::Browser, "x"));
        sneaky.sign_with(&mk);
        assert_eq!(t.evaluate(&[w.owner.cert.clone(), sneaky], &[]).devices.len(), 1);
        // And its certificate's signature is no device certificate's: the
        // domains differ, so neither verifies as the other.
        assert!(!verify_hex(&w.key.public(), as_daemon.body().as_bytes(), &c.sig));
        assert!(c.body().starts_with("arugula hosted machine v1\n"));
    }
}

#[cfg(test)]
mod fixed {
    use super::*;
    use crate::DeviceKeys;

    /// Fixed keys, so the page's copy (`web/scripts/hosted.test.mjs`)
    /// checks the same bytes: an owner's root device, control's hosted
    /// key, and a box.
    pub(super) fn keys(sign: u8, noise: u8) -> DeviceKeys {
        DeviceKeys { noise_private: [0; 32], noise_public: [noise; 32], sign: SigningKey::from_bytes(&[sign; 32]) }
    }

    /// The page checks the same fixture (`web/scripts/hosted.fixture.json`):
    /// the same bodies, ids and signatures, from the same keys.
    #[test]
    fn the_pages_fixture_is_these_bytes() {
        let owner = keys(2, 3);
        let mut root = Cert::new(&owner, "acct1", Kind::Browser, "laptop");
        root.created = 1000;
        root.sign_with(&owner);
        let key = HostedKey::from_seed(&[1; 32]);
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: key.public(),
            at: 2000,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&owner);
        let boxk = keys(4, 5);
        let mut c =
            HostedCert::new(&d, "hw_1", "box", &hex::encode(boxk.noise_public), &hex::encode(boxk.sign_public()), 3000);
        key.sign(&mut c);
        let r = DelegationRevoke::new(&d, 4000, &owner);
        // Control's word that it ended, after the box was made and before.
        let (closed, closed_early) = (key.close(&d, 4000), key.close(&d, 2500));
        // The same owner's team's project.
        let mut td = Delegation { owner: "team:t1".into(), ..d.clone() };
        td.sign_with(&owner);
        let mut tc = HostedCert::new(
            &td,
            "hw_2",
            "box",
            &hex::encode(boxk.noise_public),
            &hex::encode(boxk.sign_public()),
            3000,
        );
        key.sign(&mut tc);
        let got = serde_json::json!({
            "root": root, "delegation": d, "id": d.id(), "cert": c, "revoke": r,
            "closed": closed, "closedEarly": closed_early,
            "team": { "delegation": td, "cert": tc },
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/scripts/hosted.fixture.json");
        if std::env::var_os("ARUGULA_WRITE_FIXTURE").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        }
        let want: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(got, want, "ARUGULA_WRITE_FIXTURE=1 writes it again, if the formats changed on purpose");
        let t = Trust { account: "acct1".into(), root: root.device.clone() };
        let owners = Owners::Account { trust: &t, certs: std::slice::from_ref(&root), revocations: &[] };
        assert!(accept("acct1", &c, &d, std::slice::from_ref(&r), &[], &owners).is_ok());
        assert!(accept("acct1", &c, &d, &[], &[closed], &owners).is_ok());
        assert_eq!(accept("acct1", &c, &d, &[], &[closed_early], &owners), Err(HostedRefusal::Revoked));
    }
}
