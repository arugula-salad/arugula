//! An account's passkey log (DK3, #384; aru-126): the hub publishes every
//! passkey it added to or removed from an account as an append-only,
//! hash-chained log, so the platform, a box or anyone else can check a
//! stored passkey assertion (a deploy's approval) without asking the hub.
//! [`verify`] is that check, the one the hub's own sign-ins use too. The
//! page's copy is `web/src/shell/e2e/passkey.ts`; the example files in
//! `contract/examples/logic/passkey/` pin both (#220).
//!
//! Each event is text, like the deploy bodies:
//!
//! ```text
//! arugula passkey log v1
//! account <account id>
//! seq <0, 1, 2, ...>
//! prev <hex SHA-256 of the event before, or 64 zeros for seq 0>
//! event added | removed
//! credential <base64url credential id>
//! rp <the RP ID it was made for>
//! alg <COSE alg: -7, -8 or -257>
//! key <sec1 | raw | spki> <base64url>
//! aaguid <32 hex, zeros when the authenticator didn't say>
//! at <ms since the epoch, by the hub's clock>
//! by <what let it happen>
//! ```
//!
//! An event's hash is SHA-256 (hex) of its body and its proof line
//! (`proof <credential> <clientDataJSON> <authenticatorData> <signature>`,
//! each base64url, only on a `by passkey` event), so the next event's
//! `prev` covers both, as `deploy_events` do (aru-103).
//!
//! `by` says what let it happen:
//! - `passkey`: another of the account's passkeys, live then, signed it:
//!   the proof is its assertion, with user verification, whose challenge
//!   is SHA-256 of the body ([`Event::challenge`]). The proof names the
//!   signer, since the body is made before the person picks a passkey.
//! - the rest the hub vouches for, and the log says which: `signup` (a
//!   passkey that made its account, #102), `github` (a first passkey after
//!   a fresh GitHub sign-in to an account that never had one, #1386),
//!   `device <id>` (a first passkey one of the account's browsers signed
//!   for), `session` (removed by a signed-in page, before DK10 #393 made
//!   removal take another passkey: no longer made), `reset` (aru-119, after
//!   its wait: every passkey removed, or a first one added again from a
//!   fresh GitHub sign-in), `deleted` (the account was deleted) and
//!   `backfill <ms>` (a passkey from before the log, written at `<ms>`).
//!
//! **Backfilled history is partial.** A log whose events say `backfill <ms>`
//! began at `<ms>`: the passkeys the account had then are there, added at
//! the time they were made, but any removed before `<ms>` aren't, so an
//! account's log says nothing of what it had and lost before. The hub
//! didn't keep a passkey's RP before the log either, so a backfilled
//! event's `rp` may name each of control's sites, space-separated: the one
//! whose hash the authenticator data carries is the passkey's.
//!
//! What a verifier pins rather than reads from the log: which origins may
//! use each RP ([`Origins`]), and the time to check at (`at`), which must be
//! the verifier's own record of when the assertion arrived (the deploy's
//! event log), never a time the approval states itself: a removed passkey
//! could backdate that.
//!
//! No provider policy: every passkey counts (aru-126). The log keeps each
//! passkey's AAGUID so one can be added.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Each event's first line.
pub const DOMAIN: &str = "arugula passkey log v1";
/// `prev` of the first event.
pub const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// The AAGUID of an authenticator that didn't say.
pub const NO_AAGUID: &str = "00000000000000000000000000000000";
/// COSE algorithms: ES256 (P-256), EdDSA (Ed25519) and RS256 (RSA PKCS#1
/// v1.5 with SHA-256), what control registers.
pub const ES256: i64 = -7;
pub const EDDSA: i64 = -8;
pub const RS256: i64 = -257;
/// Authenticator data's flags: user present, user verified.
pub const UP: u8 = 0x01;
pub const UV: u8 = 0x04;

/// Which origins may use each RP ID: `{"control.arugula.io":
/// ["https://control.arugula.io"]}`. The verifier's configuration, never
/// the log's.
pub type Origins = BTreeMap<String, Vec<String>>;

/// The origins of `sites` (control's URLs), each for its own host as RP:
/// what control's passkeys have been made and used at, before related
/// origins (DK4).
pub fn origins_of_sites<'a>(sites: impl IntoIterator<Item = &'a str>) -> Origins {
    let mut o = Origins::new();
    for s in sites {
        let s = s.trim_end_matches('/');
        let Some((scheme, rest)) = s.split_once("://") else { continue };
        let authority = rest.split('/').next().unwrap_or_default().to_ascii_lowercase();
        let host = match authority.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => h.to_owned(),
            _ => authority.clone(),
        };
        let origin = format!("{}://{authority}", scheme.to_ascii_lowercase());
        let list = o.entry(host).or_default();
        if !list.contains(&origin) {
            list.push(origin);
        }
    }
    o
}

/// What happened to a passkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Removed,
}

impl Change {
    pub fn as_str(self) -> &'static str {
        match self {
            Change::Added => "added",
            Change::Removed => "removed",
        }
    }
}

/// A passkey's assertion, as the page got it from
/// `navigator.credentials.get`: each base64url.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    /// The credential's id (`rawId`).
    pub id: String,
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    #[serde(rename = "authenticatorData")]
    pub authenticator_data: String,
    pub signature: String,
}

/// One event of an account's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub account: String,
    pub seq: u64,
    /// The hash of the event before, or [`ZERO`].
    pub prev: String,
    pub event: Change,
    /// The passkey's credential id, base64url.
    pub credential: String,
    /// The RP ID it was made for; for a backfilled event, possibly each of
    /// control's sites, space-separated.
    pub rp: String,
    pub alg: i64,
    /// How `key` is encoded: `sec1` (P-256, uncompressed), `raw` (Ed25519)
    /// or `spki` (RSA).
    #[serde(rename = "keyFormat")]
    pub key_format: String,
    /// Its public key, base64url.
    pub key: String,
    /// The authenticator's AAGUID, 32 hex, [`NO_AAGUID`] when unknown.
    pub aaguid: String,
    pub at: u64,
    /// What let it happen ([`By`]).
    pub by: String,
    /// On a `by passkey` event, the signer's assertion over [`Event::challenge`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<Assertion>,
    /// SHA-256 of the body and proof line, hex.
    pub hash: String,
}

/// An account's log, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Log {
    pub account: String,
    pub events: Vec<Event>,
    /// When this hub began keeping logs (ms): removals before then are in
    /// no log. Said by the hub, not hashed; `None` when it doesn't say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub began: Option<u64>,
}

/// What let an event happen, as its `by` line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By<'a> {
    Passkey,
    Signup,
    Github,
    Device(&'a str),
    Session,
    Reset,
    Deleted,
    Backfill(u64),
}

impl<'a> By<'a> {
    pub fn parse(s: &'a str) -> Option<Self> {
        Some(match s.split_once(' ') {
            None => match s {
                "passkey" => By::Passkey,
                "signup" => By::Signup,
                "github" => By::Github,
                "session" => By::Session,
                "reset" => By::Reset,
                "deleted" => By::Deleted,
                _ => return None,
            },
            Some(("device", d)) if word(d) => By::Device(d),
            Some(("backfill", ms)) if ms.bytes().all(|b| b.is_ascii_digit()) && !ms.starts_with('0') => {
                By::Backfill(ms.parse().ok()?)
            }
            _ => return None,
        })
    }

    pub fn text(&self) -> String {
        match self {
            By::Passkey => "passkey".into(),
            By::Signup => "signup".into(),
            By::Github => "github".into(),
            By::Device(d) => format!("device {d}"),
            By::Session => "session".into(),
            By::Reset => "reset".into(),
            By::Deleted => "deleted".into(),
            By::Backfill(ms) => format!("backfill {ms}"),
        }
    }
}

/// A token that can sit on a line: not empty, no space or control
/// character.
fn word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_graphic())
}

fn b64_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'=')
}

fn hex_of_len(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Base64url, with or without padding.
fn unb64(s: &str) -> Option<Vec<u8>> {
    B64.decode(s.trim_end_matches('=')).ok()
}

/// How a key of `alg` is kept: `sec1`, `raw` or `spki`.
pub fn key_format(alg: i64) -> Option<&'static str> {
    match alg {
        ES256 => Some("sec1"),
        EDDSA => Some("raw"),
        RS256 => Some("spki"),
        _ => None,
    }
}

impl Event {
    /// The event's text, hashed and (on a `by passkey` event) signed.
    pub fn body(&self) -> String {
        format!(
            "{DOMAIN}\naccount {}\nseq {}\nprev {}\nevent {}\ncredential {}\nrp {}\nalg {}\nkey {} {}\naaguid {}\nat {}\nby {}\n",
            self.account,
            self.seq,
            self.prev,
            self.event.as_str(),
            self.credential,
            self.rp,
            self.alg,
            self.key_format,
            self.key,
            self.aaguid,
            self.at,
            self.by
        )
    }

    /// The proof's line, or nothing.
    pub fn proof_line(&self) -> String {
        match &self.proof {
            Some(p) => format!("proof {} {} {} {}\n", p.id, p.client_data_json, p.authenticator_data, p.signature),
            None => String::new(),
        }
    }

    /// SHA-256 of the body and the proof line, hex.
    pub fn compute_hash(&self) -> String {
        hex::encode(Sha256::digest(format!("{}{}", self.body(), self.proof_line()).as_bytes()))
    }

    /// What a `by passkey` proof signs: SHA-256 of the body, base64url,
    /// as a WebAuthn challenge.
    pub fn challenge(&self) -> String {
        B64.encode(Sha256::digest(self.body().as_bytes()))
    }

    /// With its proof (`by passkey`), hashed again.
    pub fn with_proof(mut self, proof: Assertion) -> Self {
        self.proof = Some(proof);
        self.hash = self.compute_hash();
        self
    }
}

/// An event before it has a place in the log.
#[derive(Debug, Clone)]
pub struct Draft {
    pub account: String,
    pub event: Change,
    pub credential: String,
    pub rp: String,
    pub alg: i64,
    /// The public key's bytes, as [`key_format`] says for `alg`.
    pub key: Vec<u8>,
    pub aaguid: String,
    pub at: u64,
    pub by: String,
}

impl Draft {
    /// The event after `prev` (none: the first), with no proof yet.
    pub fn after(self, prev: Option<&Event>) -> Event {
        let mut e = Event {
            account: self.account,
            seq: prev.map_or(0, |p| p.seq + 1),
            prev: prev.map_or_else(|| ZERO.to_owned(), |p| p.hash.clone()),
            event: self.event,
            credential: self.credential,
            rp: self.rp,
            alg: self.alg,
            key_format: key_format(self.alg).unwrap_or("unknown").to_owned(),
            key: B64.encode(&self.key),
            aaguid: self.aaguid,
            at: self.at,
            by: self.by,
            proof: None,
            hash: String::new(),
        };
        e.hash = e.compute_hash();
        e
    }
}

impl Event {
    /// The same passkey removed at `at`, `by` this: its key as this, its
    /// `added` event, has it. Not yet placed ([`Event::placed_after`]).
    pub fn removal(&self, at: u64, by: &str) -> Event {
        Event {
            account: self.account.clone(),
            seq: 0,
            prev: String::new(),
            event: Change::Removed,
            credential: self.credential.clone(),
            rp: self.rp.clone(),
            alg: self.alg,
            key_format: self.key_format.clone(),
            key: self.key.clone(),
            aaguid: self.aaguid.clone(),
            at,
            by: by.to_owned(),
            proof: None,
            hash: String::new(),
        }
    }

    /// This event placed after `prev` (none: the first), hashed again.
    pub fn placed_after(mut self, prev: Option<&Event>) -> Self {
        self.seq = prev.map_or(0, |p| p.seq + 1);
        self.prev = prev.map_or_else(|| ZERO.to_owned(), |p| p.hash.clone());
        self.hash = self.compute_hash();
        self
    }
}

/// Why an assertion doesn't check out against a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The log isn't the account's the verifier asked about.
    Account,
    /// The log doesn't hold together at this event (its index): a hash,
    /// seq, prev, account, time, field, `by` or proof that's wrong.
    Chain(u64),
    /// The passkey wasn't in the account's log at that time.
    NotLive,
    /// clientDataJSON isn't base64url JSON with a type, challenge and origin.
    ClientData,
    /// Not a `webauthn.get`.
    Ceremony,
    /// Over another challenge.
    Challenge,
    /// From inside another site's frame (`crossOrigin`, `topOrigin`).
    Framed,
    /// Authenticator data that isn't base64url, or is too short.
    AuthData,
    /// Its rpIdHash isn't the passkey's RP's.
    Rp,
    /// From an origin the verifier doesn't allow for that RP.
    Origin,
    /// The person wasn't present and verified (UP and UV).
    User,
    /// The key in the log can't be read.
    Key,
    /// The signature doesn't check out.
    Signature,
}

impl Refusal {
    /// The code both implementations give (the example files'
    /// `refused`).
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Account => "account",
            Refusal::Chain(_) => "chain",
            Refusal::NotLive => "not_live",
            Refusal::ClientData => "client_data",
            Refusal::Ceremony => "ceremony",
            Refusal::Challenge => "challenge",
            Refusal::Framed => "framed",
            Refusal::AuthData => "auth_data",
            Refusal::Rp => "rp",
            Refusal::Origin => "origin",
            Refusal::User => "user",
            Refusal::Key => "key",
            Refusal::Signature => "signature",
        }
    }

    /// In words, for a person.
    pub fn says(self) -> String {
        match self {
            Refusal::Account => "that's another account's passkey log".into(),
            Refusal::Chain(i) => format!("the account's passkey log doesn't hold together at event {i}"),
            Refusal::NotLive => "that passkey wasn't one of the account's then".into(),
            Refusal::ClientData => "bad clientDataJSON".into(),
            Refusal::Ceremony => "wrong WebAuthn ceremony".into(),
            Refusal::Challenge => "the passkey signed something else".into(),
            Refusal::Framed => "a passkey asked for from inside another site's frame".into(),
            Refusal::AuthData => "bad authenticator data".into(),
            Refusal::Rp => "a passkey for another site".into(),
            Refusal::Origin => "a passkey from another site".into(),
            Refusal::User => "the passkey must verify you (PIN, fingerprint or face)".into(),
            Refusal::Key => "the passkey's key in the log can't be read".into(),
            Refusal::Signature => "the passkey's signature doesn't check out".into(),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.says())
    }
}

/// What checked out: the passkey's `added` event, the RP the assertion was
/// for, and its flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified<'a> {
    pub key: &'a Event,
    pub rp: String,
    pub flags: u8,
    /// The authenticator's signature counter (the hub keeps it; nobody
    /// else needs it: synced passkeys say 0).
    pub count: u32,
}

/// Check `assertion` against `log`, as `account`'s, trusting nothing else:
/// - the log is `account`'s;
/// - the whole chain holds ([`check_chain`]);
/// - its passkey was live in the log at `at` (added at or before, not
///   removed at or before);
/// - its client data is a `webauthn.get` over exactly `challenge`, not
///   from inside another site's frame;
/// - its authenticator data is for the passkey's RP, from an origin
///   `origins` allows for that RP, with UP and UV;
/// - the signature checks out with the key in the log.
///
/// `origins` is only for this assertion: the log's own proofs are checked
/// against what the log says ([`check_chain`]), so which sites a verifier
/// knows never changes whether a log holds together.
pub fn verify<'a>(
    log: &'a Log,
    account: &str,
    assertion: &Assertion,
    challenge: &str,
    at: u64,
    origins: &Origins,
) -> Result<Verified<'a>, Refusal> {
    if log.account != account {
        return Err(Refusal::Account);
    }
    let then = live_at(log, at)?;
    let key = then.get(assertion.id.as_str()).ok_or(Refusal::NotLive)?;
    check(key, assertion, challenge, Allowed::Pinned(origins))
}

/// Whether the whole log holds together: each event is the account's, in
/// order (seq, prev, its hash, times that don't go back), well formed, and
/// each `by passkey` event's proof is a passkey live just before it,
/// signing its body ([`Event::challenge`]) with user verification, for the
/// signer's own RP (one of them, for a backfilled one) from that RP's own
/// origin (`https://<rp>`, or `http://` and a port: control's page, where
/// passkeys are added and removed). That's the log's own word, not any
/// verifier's configuration, so a log holds together for everyone or for
/// nobody, whichever sites a verifier knows today. A passkey is added only
/// when it isn't live, and removed only when it is, with the key it was
/// added with.
pub fn check_chain(log: &Log) -> Result<(), Refusal> {
    live_at(log, u64::MAX).map(|_| ())
}

/// The passkeys live at `at`, by credential id, having checked the whole
/// chain.
fn live_at(log: &Log, at: u64) -> Result<BTreeMap<&str, &Event>, Refusal> {
    let mut live: BTreeMap<&str, &Event> = BTreeMap::new();
    let mut then = None;
    for (i, e) in log.events.iter().enumerate() {
        let bad = Refusal::Chain(i as u64);
        let prev = if i == 0 { ZERO } else { log.events[i - 1].hash.as_str() };
        if e.account != log.account || e.seq != i as u64 || e.prev != prev || e.hash != e.compute_hash() {
            return Err(bad);
        }
        if i > 0 && e.at < log.events[i - 1].at {
            return Err(bad);
        }
        if !well_formed(e) {
            return Err(bad);
        }
        let by = By::parse(&e.by).ok_or(bad)?;
        match (by, &e.proof) {
            (By::Passkey, Some(p)) => {
                let signer = live.get(p.id.as_str()).ok_or(bad)?;
                check(signer, p, &e.challenge(), Allowed::OwnRp).map_err(|_| bad)?;
            }
            (By::Passkey, None) | (_, Some(_)) => return Err(bad),
            _ => {}
        }
        // Only a backfilled event may leave its RP open.
        if e.rp.contains(' ') && !matches!(by, By::Backfill(_)) {
            return Err(bad);
        }
        if e.at > at && then.is_none() {
            then = Some(live.clone());
        }
        match e.event {
            Change::Added => {
                if live.insert(&e.credential, e).is_some() {
                    return Err(bad);
                }
            }
            Change::Removed => match live.remove(e.credential.as_str()) {
                Some(k) if k.alg == e.alg && k.key_format == e.key_format && k.key == e.key => {}
                _ => return Err(bad),
            },
        }
    }
    Ok(then.unwrap_or(live))
}

/// Each field can sit on its line, and the key's format is its alg's.
fn well_formed(e: &Event) -> bool {
    word(&e.account)
        && b64_word(&e.credential)
        && !e.rp.is_empty()
        && e.rp.split(' ').all(word)
        && key_format(e.alg) == Some(e.key_format.as_str())
        && b64_word(&e.key)
        && hex_of_len(&e.aaguid, 32)
        && hex_of_len(&e.prev, 64)
        && e.proof.as_ref().is_none_or(|p| {
            b64_word(&p.id)
                && b64_word(&p.client_data_json)
                && b64_word(&p.authenticator_data)
                && b64_word(&p.signature)
        })
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(rename = "crossOrigin", default)]
    cross_origin: Option<bool>,
    #[serde(rename = "topOrigin", default)]
    top_origin: Option<String>,
}

/// Which origins an assertion may come from.
#[derive(Clone, Copy)]
enum Allowed<'a> {
    /// Those the verifier pinned for the RP.
    Pinned(&'a Origins),
    /// The RP's own: a proof in the log ([`check_chain`]).
    OwnRp,
}

/// Whether `origin` is `rp`'s own: `http(s)://<rp>`, with or without a port.
fn own_origin(origin: &str, rp: &str) -> bool {
    let Some(rest) = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")) else {
        return false;
    };
    let host = match rest.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => rest,
    };
    host == rp
}

/// `a` checked against `key` (the passkey's `added` event).
fn check<'a>(key: &'a Event, a: &Assertion, challenge: &str, allowed: Allowed) -> Result<Verified<'a>, Refusal> {
    let raw = unb64(&a.client_data_json).ok_or(Refusal::ClientData)?;
    let cd: ClientData = serde_json::from_slice(&raw).map_err(|_| Refusal::ClientData)?;
    if cd.kind != "webauthn.get" {
        return Err(Refusal::Ceremony);
    }
    if cd.challenge != challenge {
        return Err(Refusal::Challenge);
    }
    if cd.cross_origin == Some(true) || cd.top_origin.as_deref().is_some_and(|t| t != cd.origin) {
        return Err(Refusal::Framed);
    }
    let data = unb64(&a.authenticator_data).ok_or(Refusal::AuthData)?;
    if data.len() < 37 {
        return Err(Refusal::AuthData);
    }
    let rp = key.rp.split(' ').find(|h| Sha256::digest(h.as_bytes())[..] == data[..32]).ok_or(Refusal::Rp)?;
    let origin_ok = match allowed {
        Allowed::Pinned(origins) => origins.get(rp).is_some_and(|o| o.contains(&cd.origin)),
        Allowed::OwnRp => own_origin(&cd.origin, rp),
    };
    if !origin_ok {
        return Err(Refusal::Origin);
    }
    let flags = data[32];
    if flags & UP == 0 || flags & UV == 0 {
        return Err(Refusal::User);
    }
    let sig = unb64(&a.signature).ok_or(Refusal::Signature)?;
    let public = unb64(&key.key).ok_or(Refusal::Key)?;
    let mut msg = data.clone();
    msg.extend_from_slice(&Sha256::digest(&raw));
    if !signed(key.alg, &public, &msg, &sig)? {
        return Err(Refusal::Signature);
    }
    let count = u32::from_be_bytes(data[33..37].try_into().unwrap());
    Ok(Verified { key, rp: rp.to_owned(), flags, count })
}

/// Whether `sig` is `public`'s over `msg`, by `alg`.
fn signed(alg: i64, public: &[u8], msg: &[u8], sig: &[u8]) -> Result<bool, Refusal> {
    Ok(match alg {
        ES256 => {
            use p256::ecdsa::signature::Verifier as _;
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(public).map_err(|_| Refusal::Key)?;
            let Ok(s) = p256::ecdsa::Signature::from_der(sig) else { return Ok(false) };
            key.verify(msg, &s).is_ok()
        }
        EDDSA => {
            let key: [u8; 32] = public.try_into().map_err(|_| Refusal::Key)?;
            let key = ed25519_dalek::VerifyingKey::from_bytes(&key).map_err(|_| Refusal::Key)?;
            let Ok(sig) = <[u8; 64]>::try_from(sig) else { return Ok(false) };
            key.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
        }
        RS256 => {
            use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
            UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public).verify(msg, sig).is_ok()
        }
        _ => return Err(Refusal::Key),
    })
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::Signer as _;
    use serde_json::json;

    use super::*;

    const RP: &str = "control.test";

    fn origins() -> Origins {
        origins_of_sites(["https://control.test"])
    }

    struct Pk {
        key: ed25519_dalek::SigningKey,
        id: String,
    }

    impl Pk {
        fn new(n: u8) -> Self {
            Pk { key: ed25519_dalek::SigningKey::from_bytes(&[n; 32]), id: B64.encode([n; 16]) }
        }

        fn draft(&self, at: u64, by: &str) -> Draft {
            Draft {
                account: "acct".into(),
                event: Change::Added,
                credential: self.id.clone(),
                rp: RP.into(),
                alg: EDDSA,
                key: self.key.verifying_key().to_bytes().to_vec(),
                aaguid: NO_AAGUID.into(),
                at,
                by: by.into(),
            }
        }

        fn sign(&self, challenge: &str, flags: u8, rp: &str, extra: serde_json::Value) -> Assertion {
            let mut cd = json!({ "type": "webauthn.get", "challenge": challenge, "origin": "https://control.test" });
            cd.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let cd = serde_json::to_vec(&cd).unwrap();
            let mut data = Sha256::digest(rp.as_bytes()).to_vec();
            data.push(flags);
            data.extend_from_slice(&0u32.to_be_bytes());
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(&cd));
            Assertion {
                id: self.id.clone(),
                client_data_json: B64.encode(&cd),
                authenticator_data: B64.encode(&data),
                signature: B64.encode(self.key.sign(&msg).to_bytes()),
            }
        }
    }

    fn push(log: &mut Log, e: Event) {
        log.events.push(e);
    }

    /// k1 added at 1000; k2 added by k1 at 2000; k1 removed by k2 at 3000.
    fn log() -> (Log, Pk, Pk) {
        let (k1, k2) = (Pk::new(1), Pk::new(2));
        let mut log = Log { account: "acct".into(), ..Default::default() };
        push(&mut log, k1.draft(1000, "signup").after(None));
        let e = k2.draft(2000, "passkey").after(log.events.last());
        let proof = k1.sign(&e.challenge(), UP | UV, RP, json!({}));
        push(&mut log, e.with_proof(proof));
        let added = log.events[0].clone();
        let e = added.removal(3000, "passkey").placed_after(log.events.last());
        let proof = k2.sign(&e.challenge(), UP | UV, RP, json!({}));
        push(&mut log, e.with_proof(proof));
        (log, k1, k2)
    }

    #[test]
    fn a_key_counts_while_it_is_in_the_log() {
        let (log, k1, k2) = log();
        let o = origins();
        let ch = B64.encode(Sha256::digest(b"approval"));
        assert_eq!(check_chain(&log), Ok(()));
        let v = |k: &Pk, at| verify(&log, "acct", &k.sign(&ch, UP | UV, RP, json!({})), &ch, at, &o).map(|v| v.key.seq);
        assert_eq!(v(&k1, 999), Err(Refusal::NotLive), "before it was added");
        assert_eq!(v(&k1, 1000), Ok(0));
        assert_eq!(v(&k1, 2999), Ok(0));
        assert_eq!(v(&k1, 3000), Err(Refusal::NotLive), "removed before at");
        assert_eq!(v(&k2, 1999), Err(Refusal::NotLive));
        assert_eq!(v(&k2, 5000), Ok(1));
    }

    #[test]
    fn every_way_an_assertion_misses() {
        let (log, _, k2) = log();
        let o = origins();
        let ch = B64.encode(Sha256::digest(b"approval"));
        let v = |a: Assertion| verify(&log, "acct", &a, &ch, 5000, &o).map(|_| ());
        assert_eq!(v(k2.sign("other", UP | UV, RP, json!({}))), Err(Refusal::Challenge));
        assert_eq!(v(k2.sign(&ch, UP, RP, json!({}))), Err(Refusal::User));
        assert_eq!(v(k2.sign(&ch, UV, RP, json!({}))), Err(Refusal::User));
        assert_eq!(v(k2.sign(&ch, UP | UV, "other.test", json!({}))), Err(Refusal::Rp));
        assert_eq!(v(k2.sign(&ch, UP | UV, RP, json!({ "origin": "https://evil.test" }))), Err(Refusal::Origin));
        assert_eq!(v(k2.sign(&ch, UP | UV, RP, json!({ "type": "webauthn.create" }))), Err(Refusal::Ceremony));
        assert_eq!(v(k2.sign(&ch, UP | UV, RP, json!({ "crossOrigin": true }))), Err(Refusal::Framed));
        assert_eq!(v(k2.sign(&ch, UP | UV, RP, json!({ "topOrigin": "https://evil.test" }))), Err(Refusal::Framed));
        let mut forged = k2.sign(&ch, UP | UV, RP, json!({}));
        forged.signature = B64.encode([0u8; 64]);
        assert_eq!(v(forged), Err(Refusal::Signature));
        // Padded base64url reads the same.
        let mut padded = k2.sign(&ch, UP | UV, RP, json!({}));
        padded.client_data_json.push_str("==");
        assert!(v(padded).is_ok());
    }

    #[test]
    fn a_tampered_log_fails_where_it_was_changed() {
        let (log, _, _) = log();
        let bad = |f: &dyn Fn(&mut Log)| {
            let mut l = log.clone();
            f(&mut l);
            check_chain(&l)
        };
        assert_eq!(bad(&|l| l.events[0].key = l.events[1].key.clone()), Err(Refusal::Chain(0)), "a swapped key");
        assert_eq!(
            bad(&|l| {
                l.events[0].key = l.events[1].key.clone();
                l.events[0].hash = l.events[0].compute_hash();
            }),
            Err(Refusal::Chain(1)),
            "rehashed, so the next prev breaks"
        );
        assert_eq!(bad(&|l| l.events[2].account = "other".into()), Err(Refusal::Chain(2)));
        assert_eq!(
            bad(&|l| {
                l.events.remove(1);
            }),
            Err(Refusal::Chain(1)),
            "an event cut out"
        );
        assert_eq!(bad(&|l| l.events[1].proof = None), Err(Refusal::Chain(1)), "a passkey event with no proof");
        assert_eq!(
            bad(&|l| {
                l.events[0].by = "github\nby signup".into();
                l.events[0].hash = l.events[0].compute_hash();
            }),
            Err(Refusal::Chain(0)),
            "a line smuggled into a field"
        );
    }

    #[test]
    fn by_reads_back_what_it_writes() {
        for b in [
            By::Passkey,
            By::Signup,
            By::Github,
            By::Device("d1"),
            By::Session,
            By::Reset,
            By::Deleted,
            By::Backfill(17),
        ] {
            assert_eq!(By::parse(&b.text()), Some(b));
        }
        for s in ["", "device", "device ", "backfill 01", "backfill x", "passkey x", "other"] {
            assert_eq!(By::parse(s), None, "{s:?}");
        }
    }

    #[test]
    fn sites_give_origins_by_host() {
        let o = origins_of_sites(["https://Control.Example.com/", "http://127.0.0.1:7690", "https://old.example"]);
        assert_eq!(o["control.example.com"], ["https://control.example.com"]);
        assert_eq!(o["127.0.0.1"], ["http://127.0.0.1:7690"]);
        assert_eq!(o["old.example"], ["https://old.example"]);
    }

    /// The log's own proofs are checked against what the log says, so a
    /// step-up made at a site the verifier doesn't list (an old URL, since
    /// dropped) leaves the log whole for it; only the assertion being
    /// checked must come from an origin the verifier allows.
    #[test]
    fn a_proof_from_another_site_keeps_the_log_whole() {
        let (k1, k2) = (Pk::new(1), Pk::new(2));
        let mut log = Log { account: "acct".into(), ..Default::default() };
        let d = Draft { rp: "old.test control.test".into(), ..k1.draft(1000, "backfill 1000") };
        push(&mut log, d.after(None));
        let e = k2.draft(2000, "passkey").after(log.events.last());
        let e = Event { rp: "old.test".into(), ..e }.placed_after(log.events.last());
        let proof = k1.sign(&e.challenge(), UP | UV, "old.test", json!({ "origin": "https://old.test" }));
        push(&mut log, e.with_proof(proof));
        assert_eq!(check_chain(&log), Ok(()));
        let ch = B64.encode(Sha256::digest(b"approval"));
        let here = k1.sign(&ch, UP | UV, RP, json!({}));
        assert!(verify(&log, "acct", &here, &ch, 5000, &origins()).is_ok(), "only control.test known");
        assert_eq!(verify(&log, "other", &here, &ch, 5000, &origins()).err(), Some(Refusal::Account));
        // A proof from an origin that isn't its RP's own breaks it for all.
        let mut bad = log.clone();
        let e = bad.events.pop().unwrap();
        let e = Event { proof: None, ..e };
        let proof = k1.sign(&e.challenge(), UP | UV, "old.test", json!({ "origin": "https://evil.test" }));
        bad.events.push(e.with_proof(proof));
        assert_eq!(check_chain(&bad), Err(Refusal::Chain(1)));
        for (origin, rp, ok) in [
            ("https://old.test", "old.test", true),
            ("http://old.test:7690", "old.test", true),
            ("https://old.test.evil", "old.test", false),
            ("https://x.old.test", "old.test", false),
            ("old.test", "old.test", false),
        ] {
            assert_eq!(own_origin(origin, rp), ok, "{origin}");
        }
    }
}
