//! Device certificates and revocations, and deciding which devices an
//! account trusts.
//!
//! Control stores and hands these out, but it holds no signing key, so it
//! can't make one that checks out: every certificate is signed by a device
//! the account already trusts, back to the account's first device (the
//! root, self-signed). A daemon pins that root when it joins and evaluates
//! whatever control sends against it ([`Trust::evaluate`]).
//!
//! What's signed is a fixed line format, so browsers (WebCrypto Ed25519)
//! and Rust produce the same bytes without agreeing on JSON:
//!
//! ```text
//! illogical device v1
//! account <id>
//! device <id>
//! kind browser|cli|daemon|recovery|app
//! name <text: no control characters, at most 64 chars>
//! noise <hex>
//! sign <hex>
//! created <ms>
//! approver <device id>
//! ```

use std::collections::HashMap;

use anyhow::{bail, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::keys::{DeviceKeys, device_id, hex32};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Browser,
    Cli,
    Daemon,
    /// A recovery code: approves a new device when every other is lost.
    Recovery,
    /// The desktop app's window. Its keys reach it through whatever
    /// listens on the computer's loopback port, so, like a CLI's, they may
    /// be a program's on that computer, not the person's. It connects and
    /// approves machines, CLIs and other apps, but never a browser (see
    /// [`Kind::certifies`]). Older readers don't know this word: control
    /// leaves `app` certificates out of what it answers a client that
    /// doesn't say it knows them (`app-devices`, see control's `compat.rs`).
    App,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Browser => "browser",
            Kind::Cli => "cli",
            Kind::Daemon => "daemon",
            Kind::Recovery => "recovery",
            Kind::App => "app",
        }
    }

    /// May sign other devices' certificates (of the kinds
    /// [`Kind::certifies`] says), revocations and team papers.
    pub fn approves(self) -> bool {
        matches!(self, Kind::Browser | Kind::Cli | Kind::Recovery | Kind::App)
    }

    /// May sign a certificate of `kind`. A browser signs any. A recovery
    /// code signs any but a machine's. A CLI and the desktop app hold keys
    /// a program on their computer can read, so neither makes a browser,
    /// nor a recovery code (which would make one): a CLI signs CLIs and
    /// machines, and the app signs apps too. A machine signs none.
    pub fn certifies(self, kind: Kind) -> bool {
        match self {
            Kind::Browser => true,
            Kind::Recovery => kind != Kind::Daemon,
            Kind::Cli => matches!(kind, Kind::Cli | Kind::Daemon),
            Kind::App => matches!(kind, Kind::App | Kind::Cli | Kind::Daemon),
            Kind::Daemon => false,
        }
    }

    /// May be an account's first device (self-signed): a browser, or a CLI.
    /// Not the app, which couldn't approve the browsers to come.
    pub fn roots(self) -> bool {
        matches!(self, Kind::Browser | Kind::Cli)
    }

    /// May open channels to daemons.
    pub fn connects(self) -> bool {
        matches!(self, Kind::Browser | Kind::Cli | Kind::App)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cert {
    pub v: u32,
    pub account: String,
    pub device: String,
    pub kind: Kind,
    pub name: String,
    /// X25519 public key, hex (zeros for a recovery code).
    pub noise: String,
    /// Ed25519 public key, hex.
    pub sign: String,
    pub created: u64,
    /// The device that signed this; the device itself for the root.
    pub approver: String,
    /// Ed25519 signature by the approver over [`Cert::body`], hex; empty
    /// until approved.
    #[serde(default)]
    pub sig: String,
}

fn plain(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Cert {
    /// An unsigned certificate for `keys`.
    pub fn new(keys: &DeviceKeys, account: &str, kind: Kind, name: &str) -> Self {
        Self {
            v: 1,
            account: account.to_owned(),
            device: keys.id(),
            kind,
            name: name.to_owned(),
            noise: hex::encode(keys.noise_public),
            sign: hex::encode(keys.sign_public()),
            created: crate::now_ms(),
            approver: String::new(),
            sig: String::new(),
        }
    }

    pub fn body(&self) -> String {
        // Frozen (#504): signed into every certificate; see `frozen.rs`.
        format!(
            "illogical device v1\naccount {}\ndevice {}\nkind {}\nname {}\nnoise {}\nsign {}\ncreated {}\napprover {}\n",
            self.account,
            self.device,
            self.kind.as_str(),
            self.name,
            self.noise,
            self.sign,
            self.created,
            self.approver
        )
    }

    /// Well formed, with an id that matches its keys. Says nothing about
    /// the signature.
    pub fn check_form(&self) -> anyhow::Result<()> {
        ensure!(self.v == 1, "unknown certificate version {}", self.v);
        ensure!(token(&self.account), "bad account id");
        ensure!(token(&self.approver), "no approver");
        ensure!(plain(&self.name, 64), "a device name is 1 to 64 characters, none of them control characters");
        let (noise, sign) = (hex32(&self.noise)?, hex32(&self.sign)?);
        ensure!(device_id(&noise, &sign) == self.device, "device id doesn't match its keys");
        Ok(())
    }

    /// A request to be approved: well formed apart from the account and
    /// approver, which the approver fills in.
    pub fn check_request(&self) -> anyhow::Result<()> {
        ensure!(self.v == 1, "unknown certificate version {}", self.v);
        ensure!(plain(&self.name, 64), "a device name is 1 to 64 characters, none of them control characters");
        let (noise, sign) = (hex32(&self.noise)?, hex32(&self.sign)?);
        ensure!(device_id(&noise, &sign) == self.device, "device id doesn't match its keys");
        Ok(())
    }

    /// The same device, kind and name: what an approval may not change.
    pub fn same_request(&self, other: &Cert) -> bool {
        (&self.device, &self.noise, &self.sign, self.kind, &self.name)
            == (&other.device, &other.noise, &other.sign, other.kind, &other.name)
    }

    pub fn noise_key(&self) -> [u8; 32] {
        hex32(&self.noise).unwrap_or_default()
    }

    fn verifying_key(&self) -> anyhow::Result<VerifyingKey> {
        Ok(VerifyingKey::from_bytes(&hex32(&self.sign)?)?)
    }

    /// Sign as `approver` (setting `approver` first).
    pub fn sign_with(&mut self, approver: &DeviceKeys) {
        self.approver = approver.id();
        self.sig = hex::encode(approver.signature(self.body().as_bytes()));
    }

    /// The signature checks out against `approver`'s key.
    pub fn signed_by(&self, approver: &Cert) -> bool {
        approver.device == self.approver && verify(&approver.verifying_key(), self.body().as_bytes(), &self.sig)
    }

    /// The desktop app's window: an `app` certificate, or a `browser` one
    /// from before the app had a kind of its own, which the app named
    /// ("Arugula app on jake-air", "illogical app" before the rename). No
    /// browser names itself so. Control treats both alike: they don't
    /// approve browsers, and they don't approve deploys.
    pub fn from_app(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        self.kind == Kind::App
            || (self.kind == Kind::Browser && (name.starts_with("arugula app") || name.starts_with("illogical app")))
    }

    /// A browser's or a phone's own device: `browser`, and not the desktop
    /// app's from before it had a kind ([`Cert::from_app`]).
    pub fn is_browser(&self) -> bool {
        self.kind == Kind::Browser && !self.from_app()
    }
}

/// `sig_hex` is `sign_hex`'s Ed25519 signature of `msg`.
pub fn verify_hex(sign_hex: &str, msg: &[u8], sig_hex: &str) -> bool {
    let key = hex32(sign_hex).and_then(|k| Ok(VerifyingKey::from_bytes(&k)?));
    verify(&key, msg, sig_hex)
}

fn verify(key: &anyhow::Result<VerifyingKey>, msg: &[u8], sig_hex: &str) -> bool {
    let Ok(key) = key else { return false };
    let Ok(sig) = hex::decode(sig_hex) else { return false };
    let Ok(sig) = <[u8; 64]>::try_from(sig.as_slice()) else { return false };
    key.verify_strict(msg, &Signature::from_bytes(&sig)).is_ok()
}

/// A device taken off the account, signed by one of its devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revocation {
    pub v: u32,
    pub account: String,
    pub device: String,
    pub at: u64,
    pub by: String,
    pub sig: String,
}

impl Revocation {
    pub fn new(account: &str, device: &str, by: &DeviceKeys) -> Self {
        let mut r = Self {
            v: 1,
            account: account.into(),
            device: device.into(),
            at: crate::now_ms(),
            by: by.id(),
            sig: String::new(),
        };
        r.sig = hex::encode(by.signature(r.body().as_bytes()));
        r
    }

    pub fn signed_by(&self, signer: &Cert) -> bool {
        signer.device == self.by && verify(&signer.verifying_key(), self.body().as_bytes(), &self.sig)
    }

    pub fn body(&self) -> String {
        format!(
            "illogical revoke v1\naccount {}\ndevice {}\nat {}\nby {}\n",
            self.account, self.device, self.at, self.by
        )
    }
}

/// What a daemon pinned when it joined: the account and its root device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trust {
    pub account: String,
    pub root: String,
}

/// The devices an account trusts now.
#[derive(Debug, Default, Clone)]
pub struct Trusted {
    pub devices: HashMap<String, Cert>,
}

impl Trusted {
    pub fn get(&self, id: &str) -> Option<&Cert> {
        self.devices.get(id)
    }

    /// The device whose Noise key this is.
    pub fn by_noise(&self, key: &[u8]) -> Option<&Cert> {
        let hex_key = hex::encode(key);
        self.devices.values().find(|c| c.noise == hex_key)
    }
}

impl Trust {
    /// Which of `certs` chain back to the pinned root, net of
    /// `revocations`. A device revoked at time T no longer counts, nor
    /// does anything it approved after T; what it approved before T stays.
    /// Anything that doesn't check out is ignored, not an error: control
    /// may send junk, and it can only ever shrink the set.
    pub fn evaluate(&self, certs: &[Cert], revocations: &[Revocation]) -> Trusted {
        let mut by_id: HashMap<&str, Vec<&Cert>> = HashMap::new();
        for c in certs {
            if c.account == self.account && c.check_form().is_ok() {
                by_id.entry(c.device.as_str()).or_default().push(c);
            }
        }
        // Revocations count when their signer is valid; twice, so a
        // revoked device's own revocations stop counting.
        let mut revoked: HashMap<String, u64> = HashMap::new();
        for _ in 0..2 {
            let mut next = HashMap::new();
            for r in revocations {
                if r.v != 1 || r.account != self.account {
                    continue;
                }
                let signer = self.valid_cert(&by_id, &revoked, &r.by, r.at, 0);
                // A browser or a phone is removed only by a device that's
                // a browser all the way back to the root: never by a CLI
                // or the app, whose keys a program on their computer holds.
                // (A recovery code too: revoking one backdated, before the
                // browser it let in was made, would take that browser out.)
                let of_browser = by_id
                    .get(r.device.as_str())
                    .is_some_and(|cs| cs.iter().any(|c| matches!(c.kind, Kind::Browser | Kind::Recovery)));
                if signer.is_some_and(|s| s.kind.approves() && verify(&s.verifying_key(), r.body().as_bytes(), &r.sig))
                    && (!of_browser
                        || self
                            .chained_in(&by_id, &r.by, &|c: &Cert| browser_link(c, &r.by, |c| c.kind == Kind::Browser)))
                {
                    let at = next.entry(r.device.clone()).or_insert(r.at);
                    *at = (*at).min(r.at);
                }
            }
            revoked = next;
        }
        let now = u64::MAX;
        let mut out = Trusted::default();
        for id in by_id.keys() {
            if let Some(c) = self.valid_cert(&by_id, &revoked, id, now, 0) {
                out.devices.insert(c.device.clone(), c.clone());
            }
        }
        out
    }

    /// Whether `device`, and every device that certified it back to the
    /// root (the root included), passes `link`. Every certificate kept
    /// for an id on the way is looked at, not only the one that made it
    /// trusted, so one more from control can only refuse; a link with
    /// none, a self-signed one that isn't the root, or a chain longer than
    /// 64 refuses. Signatures aren't checked here: ask [`Trust::evaluate`]
    /// whether `device` is trusted at all.
    pub fn chained(&self, certs: &[Cert], device: &str, link: impl Fn(&Cert) -> bool) -> bool {
        let mut by_id: HashMap<&str, Vec<&Cert>> = HashMap::new();
        for c in certs {
            if c.account == self.account && c.check_form().is_ok() {
                by_id.entry(c.device.as_str()).or_default().push(c);
            }
        }
        self.chained_in(&by_id, device, &link)
    }

    /// [`Trust::chained`] with a browser on every link: what may add or
    /// promote a team's owner or editor, or remove a browser or a phone.
    /// `device` is a browser; the links back may be recovery codes too
    /// (the person's own, on paper, and made only by browsers), so a
    /// browser a recovery code let in after every other was lost counts.
    /// The desktop app's devices from before it had a kind of its own pass
    /// here (they're `browser` certificates); control refuses new papers
    /// from them with [`Cert::is_browser`] on every link.
    pub fn browser_chained(&self, certs: &[Cert], device: &str) -> bool {
        self.chained(certs, device, |c| browser_link(c, device, |c| c.kind == Kind::Browser))
    }

    fn chained_in(&self, by_id: &HashMap<&str, Vec<&Cert>>, device: &str, link: &dyn Fn(&Cert) -> bool) -> bool {
        let mut next = vec![device.to_owned()];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = next.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if seen.len() > 64 {
                return false;
            }
            let Some(these) = by_id.get(id.as_str()) else { return false };
            for c in these {
                if !link(c) {
                    return false;
                }
                if c.approver == c.device {
                    if c.device != self.root {
                        return false;
                    }
                } else {
                    next.push(c.approver.clone());
                }
            }
        }
        true
    }

    /// Why `cert` wouldn't count with the account's `certs` (#327), or
    /// `None` when it does.
    pub fn refusal(&self, certs: &[Cert], revocations: &[Revocation], cert: &Cert) -> Option<Refusal> {
        let mut all = certs.to_vec();
        all.push(cert.clone());
        if self.evaluate(&all, revocations).get(&cert.device) == Some(cert) {
            return None;
        }
        all.pop();
        let now = self.evaluate(&all, revocations);
        // A revocation stands for good; one signed by a device trusted now.
        let revoked = revocations.iter().any(|r| {
            r.device == cert.device && r.account == self.account && now.get(&r.by).is_some_and(|by| r.signed_by(by))
        });
        Some(match now.get(&cert.approver) {
            _ if revoked => Refusal::Revoked,
            None => Refusal::ApproverUntrusted,
            Some(a) if !cert.signed_by(a) => Refusal::BadSignature,
            Some(a) if !a.kind.approves() => Refusal::CantApprove,
            Some(a) if a.kind == Kind::Recovery && cert.kind == Kind::Daemon => Refusal::RecoveryForMachine,
            Some(a) if !a.kind.certifies(cert.kind) => Refusal::NeedsBrowser,
            Some(_) => Refusal::NoChain,
        })
    }

    /// `id`'s certificate, if it was valid at time `at`: not revoked by
    /// then, and signed by a device that was valid when this was created.
    fn valid_cert<'a>(
        &self,
        by_id: &HashMap<&str, Vec<&'a Cert>>,
        revoked: &HashMap<String, u64>,
        id: &str,
        at: u64,
        depth: usize,
    ) -> Option<&'a Cert> {
        if depth > 64 || revoked.get(id).is_some_and(|&t| t <= at) {
            return None;
        }
        for c in by_id.get(id)? {
            if id == self.root {
                if c.approver == c.device && c.kind.roots() && c.signed_by(c) {
                    return Some(c);
                }
                continue;
            }
            if c.approver == c.device {
                continue;
            }
            let Some(approver) = self.valid_cert(by_id, revoked, &c.approver, c.created, depth + 1) else { continue };
            // Recovery codes approve people's devices, not daemons; a CLI
            // or the app never a browser.
            if approver.kind.certifies(c.kind) && c.signed_by(approver) {
                return Some(c);
            }
        }
        None
    }
}

/// A link of a browser chain to `device`: `device` itself as `browser`
/// says, a link before it as that or a recovery code.
pub fn browser_link(c: &Cert, device: &str, browser: impl Fn(&Cert) -> bool) -> bool {
    browser(c) || (c.kind == Kind::Recovery && c.device != device)
}

/// Which check an approval failed (#327).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The device was revoked: its key can't be approved again.
    Revoked,
    /// The approving device isn't one the account trusts (now).
    ApproverUntrusted,
    /// The signature isn't the approving device's.
    BadSignature,
    /// The approving device is a machine, which approves nothing.
    CantApprove,
    /// A recovery code approving a machine.
    RecoveryForMachine,
    /// A CLI or the desktop app approving a browser, a phone or a recovery
    /// code: only a browser does.
    NeedsBrowser,
    /// Anything else: its form, its kind, or when it was made.
    NoChain,
}

impl Refusal {
    /// A code for clients to act on.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Revoked => "revoked",
            Refusal::ApproverUntrusted => "approver_untrusted",
            Refusal::BadSignature => "bad_signature",
            Refusal::CantApprove => "cant_approve",
            Refusal::RecoveryForMachine => "recovery_for_machine",
            Refusal::NeedsBrowser => "needs_browser",
            Refusal::NoChain => "no_chain",
        }
    }

    /// The check that failed, as a clause.
    pub fn check(self) -> &'static str {
        match self {
            Refusal::Revoked => {
                "its key was removed from the account, so it can't be approved again: it needs a new key"
            }
            Refusal::ApproverUntrusted => "the approving device isn't one the account trusts",
            Refusal::BadSignature => "the signature doesn't verify with the approving device's key",
            Refusal::CantApprove => "the approving device can't approve others",
            Refusal::RecoveryForMachine => "a recovery code approves browsers and phones, not machines",
            Refusal::NeedsBrowser => {
                "a CLI or the desktop app approves machines, CLIs and apps, not a browser, a phone or a recovery code"
            }
            Refusal::NoChain => "it doesn't chain to the account's first device (form, kind or time)",
        }
    }
}

/// What a daemon signs with its own key when it asks to join (0.17 and
/// newer): that whoever asks holds the key, not just its certificate.
pub fn join_proof_body(cert: &Cert, ms: u64) -> String {
    format!("illogical join proof v1\n{ms}\n{}", cert.body())
}

/// The letters join codes and join secrets are written in: base32 without
/// I, L, O and U.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ0123456789";

/// The code a daemon shows when joining, derived from its keys: the device
/// approving it recomputes it from the certificate control shows, so
/// control can't swap in a key of its own. Ten base32 characters, as
/// `XXXXX-XXXXX`.
pub fn join_code(cert: &Cert) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(format!("illogical join\n{}\n{}\n", cert.noise, cert.sign).as_bytes());
    let mut bits: u64 = u64::from_be_bytes(h[..8].try_into().unwrap());
    let mut s = String::new();
    for i in 0..10 {
        if i == 5 {
            s.push('-');
        }
        s.push(ALPHABET[(bits >> 59) as usize] as char);
        bits <<= 5;
    }
    s
}

/// A signed request to control, as the `x-arugula-auth` header's value:
/// `v2 <device id> <ms> <nonce> <sig>`, signed over the method, the path
/// and query, the time, a fresh nonce and the body's SHA-256. Daemons sign
/// every request this way, and so does the CLI (M49).
pub fn request_auth(keys: &DeviceKeys, method: &str, path_and_query: &str, body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let ms = crate::now_ms();
    let nonce = hex::encode(crate::random::<16>());
    let digest = hex::encode(Sha256::digest(body));
    let msg = format!("illogical daemon auth v2\n{method}\n{path_and_query}\n{ms}\n{nonce}\n{digest}\n");
    format!("v2 {} {ms} {nonce} {}", keys.id(), hex::encode(keys.signature(msg.as_bytes())))
}

/// Parse a code as typed: case, spaces and dashes don't matter.
pub fn normalize_code(s: &str) -> anyhow::Result<String> {
    let c: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    if c.len() != 10 {
        bail!("a join code is ten letters and digits");
    }
    Ok(format!("{}-{}", &c[..5], &c[5..]))
}

// ---------------------------------------------------------------- the join secret (#760)
//
// A machine (or the CLI) joining makes a secret of its own and puts it in
// the approval link after the code: `#join=<code>.<secret>`. The fragment
// never reaches control's server, and the page sends control only the
// code. The approving device answers with a MAC under the secret over the
// account it approves into and the machine's keys ([`join_mac`]); the
// machine checks it and pins that root without asking anyone to compare
// fingerprints. Control sees the MAC but not the secret, so it can't make
// one for an account of its own, nor for a key of its own.
//
// Control does see a MAC, so it could try secrets offline against it: the
// secret is 16 characters (80 bits), not the code's 10, which puts that out
// of reach in the ten minutes a join lasts. A wrong MAC ends the join
// (the machine drops its key), so the online guess is one per code.

/// The feature a daemon (or the CLI) names when it asks to join with a
/// secret, so the page knows to send a MAC (`features=join-secret`).
pub const JOIN_SECRET: &str = "join-secret";

/// How many characters a join secret has (5 bits each).
const SECRET_LEN: usize = 16;

/// A new join secret, as `XXXX-XXXX-XXXX-XXXX`.
pub fn join_secret() -> String {
    let raw: String = crate::random::<SECRET_LEN>().iter().map(|b| ALPHABET[usize::from(b & 31)] as char).collect();
    group_secret(&raw)
}

fn group_secret(raw: &str) -> String {
    raw.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect::<Vec<_>>().join("-")
}

/// A join secret as typed: 16 letters and digits, any case, dashes and
/// spaces ignored. `None` if it isn't one.
pub fn normalize_secret(s: &str) -> Option<String> {
    let c: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    (c.len() == SECRET_LEN && c.bytes().all(|b| ALPHABET.contains(&b))).then(|| group_secret(&c))
}

/// A whole join code as the page or a person has it, `CODE.SECRET` or
/// just `CODE`: the code (normalized) and the secret, if there is one.
pub fn split_join_code(s: &str) -> anyhow::Result<(String, Option<String>)> {
    match s.split_once('.') {
        Some((code, secret)) => {
            let secret = normalize_secret(secret)
                .ok_or_else(|| anyhow::anyhow!("a join code's second half is 16 letters and digits"))?;
            Ok((normalize_code(code)?, Some(secret)))
        }
        None => Ok((normalize_code(s)?, None)),
    }
}

/// What the approving device MACs under the secret: the account it
/// approves the machine into (its id and root device) and the machine's
/// keys. Domain-separated from every other use of these keys.
fn join_mac_body(trust: &Trust, cert: &Cert) -> String {
    format!(
        "arugula join secret v1\naccount {}\nroot {}\ndevice {}\nnoise {}\nsign {}\n",
        trust.account, trust.root, cert.device, cert.noise, cert.sign
    )
}

fn join_hmac(secret: &str, trust: &Trust, cert: &Cert) -> hkdf::hmac::Hmac<sha2::Sha256> {
    use hkdf::hmac::{KeyInit, Mac};
    // The key is the secret's 16 characters, without the dashes.
    let key: String = secret.chars().filter(|c| *c != '-').collect();
    let mut m = <hkdf::hmac::Hmac<sha2::Sha256> as KeyInit>::new_from_slice(key.as_bytes()).expect("any key length");
    m.update(join_mac_body(trust, cert).as_bytes());
    m
}

/// The approving device's answer (hex HMAC-SHA256): `secret` binds the
/// account `trust` names to the machine whose keys are in `cert`.
pub fn join_mac(secret: &str, trust: &Trust, cert: &Cert) -> String {
    use hkdf::hmac::Mac;
    hex::encode(join_hmac(secret, trust, cert).finalize().into_bytes())
}

/// What a device signs to pin its account's root (#764), when its keys
/// come from a passkey's PRF: another browser that derives the same keys
/// checks this, made by those keys, rather than take control's word for
/// the root. The page's `devicePinBody` is the same text.
pub fn device_pin_body(trust: &Trust, device: &str) -> String {
    format!("arugula device pin v1\naccount {}\nroot {}\ndevice {}\n", trust.account, trust.root, device)
}

/// Whether `pin` is `cert`'s own signature over [`device_pin_body`].
pub fn device_pin_ok(trust: &Trust, cert: &Cert, pin: &str) -> bool {
    verify_hex(&cert.sign, device_pin_body(trust, &cert.device).as_bytes(), pin)
}

/// Whether `mac` (hex) is [`join_mac`]'s, compared in constant time.
pub fn join_mac_ok(secret: &str, trust: &Trust, cert: &Cert, mac: &str) -> bool {
    use hkdf::hmac::Mac;
    let Ok(mac) = hex::decode(mac.trim()) else { return false };
    join_hmac(secret, trust, cert).verify_slice(&mac).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(account: &str) -> (DeviceKeys, Cert) {
        let k = DeviceKeys::generate();
        let mut c = Cert::new(&k, account, Kind::Browser, "laptop");
        c.sign_with(&k);
        (k, c)
    }

    fn approved(by: &DeviceKeys, account: &str, kind: Kind, name: &str) -> (DeviceKeys, Cert) {
        let k = DeviceKeys::generate();
        let mut c = Cert::new(&k, account, kind, name);
        c.sign_with(by);
        (k, c)
    }

    fn trust(c: &Cert) -> Trust {
        Trust { account: c.account.clone(), root: c.device.clone() }
    }

    #[test]
    fn chain_back_to_the_root() {
        let (rk, rc) = root("acct");
        let (pk, pc) = approved(&rk, "acct", Kind::Browser, "phone");
        let (_, dc) = approved(&pk, "acct", Kind::Daemon, "geek");
        let t = trust(&rc).evaluate(&[rc.clone(), pc.clone(), dc.clone()], &[]);
        assert_eq!(t.devices.len(), 3);
        assert_eq!(t.by_noise(&pk.noise_public).unwrap().name, "phone");
    }

    #[test]
    fn control_cannot_add_a_device() {
        let (_, rc) = root("acct");
        // Control makes its own "root" and a device under it.
        let (fk, mut fc) = root("acct");
        let (_, mut sneaky) = approved(&fk, "acct", Kind::Browser, "control");
        let t = trust(&rc).evaluate(&[rc.clone(), fc.clone(), sneaky.clone()], &[]);
        assert_eq!(t.devices.len(), 1);
        // Or claims the real root approved it.
        sneaky.approver = rc.device.clone();
        fc.approver = rc.device.clone();
        let t = trust(&rc).evaluate(&[rc.clone(), fc, sneaky], &[]);
        assert_eq!(t.devices.len(), 1);
    }

    #[test]
    fn tampered_names_and_keys_fail() {
        let (rk, rc) = root("acct");
        let (_, mut pc) = approved(&rk, "acct", Kind::Browser, "phone");
        pc.name = "laptop".into();
        let t = trust(&rc).evaluate(&[rc.clone(), pc.clone()], &[]);
        assert_eq!(t.devices.len(), 1);
        let (other, _) = root("acct");
        let (_, mut pc) = approved(&rk, "acct", Kind::Browser, "phone");
        pc.noise = hex::encode(other.noise_public);
        assert!(pc.check_form().is_err());
    }

    #[test]
    fn revoking_keeps_earlier_approvals() {
        let (rk, rc) = root("acct");
        let (pk, mut pc) = approved(&rk, "acct", Kind::Browser, "phone");
        pc.created = 1;
        pc.sign_with(&rk);
        let (_, mut before) = approved(&pk, "acct", Kind::Daemon, "before");
        before.created = 10;
        before.sign_with(&pk);
        let (_, mut after) = approved(&pk, "acct", Kind::Daemon, "after");
        after.created = 30;
        after.sign_with(&pk);
        let mut rev = Revocation::new("acct", &pc.device, &rk);
        rev.at = 20;
        rev.sig = hex::encode(rk.signature(rev.body().as_bytes()));
        let t = trust(&rc).evaluate(&[rc.clone(), pc.clone(), before.clone(), after.clone()], &[rev]);
        let names: std::collections::BTreeSet<_> = t.devices.values().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["before", "laptop"].into());
    }

    #[test]
    fn a_revoked_device_cannot_revoke() {
        let (rk, rc) = root("acct");
        let (pk, pc) = approved(&rk, "acct", Kind::Browser, "phone");
        let (_, qc) = approved(&rk, "acct", Kind::Browser, "tablet");
        let mut kill_phone = Revocation::new("acct", &pc.device, &rk);
        kill_phone.at = 1;
        kill_phone.sig = hex::encode(rk.signature(kill_phone.body().as_bytes()));
        let spite = Revocation::new("acct", &qc.device, &pk);
        let t = trust(&rc).evaluate(&[rc.clone(), pc, qc], &[kill_phone, spite]);
        let names: std::collections::BTreeSet<_> = t.devices.values().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["laptop", "tablet"].into());
    }

    #[test]
    fn recovery_codes_approve_people_not_daemons() {
        let (rk, rc) = root("acct");
        let (ck, cc) = approved(&rk, "acct", Kind::Recovery, "recovery 1");
        let (_, newc) = approved(&ck, "acct", Kind::Browser, "new laptop");
        let (_, dc) = approved(&ck, "acct", Kind::Daemon, "box");
        let t = trust(&rc).evaluate(&[rc.clone(), cc, newc, dc], &[]);
        assert!(t.devices.values().any(|c| c.name == "new laptop"));
        assert!(!t.devices.values().any(|c| c.name == "box"));
    }

    /// #327: which check an approval failed.
    #[test]
    fn refusals_name_the_check() {
        let (rk, rc) = root("acct");
        let t = trust(&rc);
        let (ck, cc) = approved(&rk, "acct", Kind::Recovery, "recovery 1");
        let (dk, dc) = approved(&rk, "acct", Kind::Daemon, "box");
        let have = [rc.clone(), cc.clone(), dc.clone()];
        let new = |by: &DeviceKeys, kind: Kind| {
            let mut c = Cert::new(&DeviceKeys::generate(), "acct", kind, "new");
            c.sign_with(by);
            c
        };
        assert_eq!(t.refusal(&have, &[], &new(&rk, Kind::Browser)), None);
        let (stranger, _) = root("acct");
        assert_eq!(t.refusal(&have, &[], &new(&stranger, Kind::Browser)), Some(Refusal::ApproverUntrusted));
        let mut forged = new(&rk, Kind::Browser);
        forged.name = "renamed".into();
        assert_eq!(t.refusal(&have, &[], &forged), Some(Refusal::BadSignature));
        assert_eq!(t.refusal(&have, &[], &new(&dk, Kind::Browser)), Some(Refusal::CantApprove));
        assert_eq!(t.refusal(&have, &[], &new(&ck, Kind::Daemon)), Some(Refusal::RecoveryForMachine));
        // A removed machine, approved again with its old key.
        let mut again = Cert::new(&dk, "acct", Kind::Daemon, "box");
        again.created = dc.created + 1;
        again.sign_with(&rk);
        let gone = Revocation::new("acct", &dc.device, &rk);
        assert_eq!(t.refusal(&have, &[gone], &again), Some(Refusal::Revoked));
        let mut other = new(&rk, Kind::Browser);
        other.account = "elsewhere".into();
        other.sign_with(&rk);
        assert_eq!(t.refusal(&have, &[], &other), Some(Refusal::NoChain));
    }

    /// A CLI's or the desktop app's key may be a program's on its computer:
    /// neither makes a browser, nor a recovery code (which would make one).
    /// The app is never an account's first device.
    #[test]
    fn a_cli_or_the_app_never_makes_a_browser() {
        let (rk, rc) = root("acct");
        let (ck, cc) = approved(&rk, "acct", Kind::Cli, "terminal");
        let (ak, ac) = approved(&rk, "acct", Kind::App, "Arugula app on box");
        let t = trust(&rc);
        let made = |by: &DeviceKeys, kind: Kind| approved(by, "acct", kind, &format!("by {}", by.id())).1;
        for (by, ok) in [
            (&ck, &[Kind::Cli, Kind::Daemon][..]),
            (&ak, &[Kind::App, Kind::Cli, Kind::Daemon][..]),
            (&rk, &[Kind::Browser, Kind::Cli, Kind::App, Kind::Daemon, Kind::Recovery][..]),
        ] {
            for kind in [Kind::Browser, Kind::Cli, Kind::App, Kind::Daemon, Kind::Recovery] {
                let c = made(by, kind);
                let got = t.evaluate(&[rc.clone(), cc.clone(), ac.clone(), c.clone()], &[]).get(&c.device).is_some();
                assert_eq!(got, ok.contains(&kind), "{} signing a {} certificate", by.id(), kind.as_str());
                let want = if ok.contains(&kind) { None } else { Some(Refusal::NeedsBrowser) };
                assert_eq!(t.refusal(&[rc.clone(), cc.clone(), ac.clone()], &[], &c), want);
            }
        }
        // An app that signed itself in as the root is no root.
        let k = DeviceKeys::generate();
        let mut app_root = Cert::new(&k, "acct", Kind::App, "Arugula app");
        app_root.sign_with(&k);
        assert!(trust(&app_root).evaluate(&[app_root.clone()], &[]).devices.is_empty());
        assert!(!Kind::App.roots() && Kind::Browser.roots());
        assert!(Kind::App.connects() && Kind::App.approves());
    }

    /// A browser or a phone is removed only by a browser all the way back
    /// to the root; a CLI or the app still removes machines, CLIs and apps.
    #[test]
    fn only_a_browser_removes_a_browser() {
        let (rk, rc) = root("acct");
        let (_, phone) = approved(&rk, "acct", Kind::Browser, "phone");
        let (ck, cli) = approved(&rk, "acct", Kind::Cli, "terminal");
        let (ak, app) = approved(&rk, "acct", Kind::App, "Arugula app");
        let (_, machine) = approved(&rk, "acct", Kind::Daemon, "box");
        let (_, other_cli) = approved(&rk, "acct", Kind::Cli, "terminal 2");
        let t = trust(&rc);
        let all = [rc.clone(), phone.clone(), cli.clone(), app.clone(), machine.clone(), other_cli.clone()];
        let gone = |r: Revocation, id: &str| t.evaluate(&all, &[r]).get(id).is_none();
        for by in [&ck, &ak] {
            assert!(
                !gone(Revocation::new("acct", &phone.device, by), &phone.device),
                "a CLI or the app removing a phone"
            );
            assert!(!gone(Revocation::new("acct", &rc.device, by), &rc.device), "or the root");
            assert!(gone(Revocation::new("acct", &machine.device, by), &machine.device), "a machine, it may");
            assert!(gone(Revocation::new("acct", &other_cli.device, by), &other_cli.device), "a CLI, it may");
        }
        assert!(gone(Revocation::new("acct", &phone.device, &rk), &phone.device), "the root browser may");
        assert!(t.browser_chained(&all, &phone.device) && !t.browser_chained(&all, &cli.device));
        // A browser a recovery code let in counts as one all the way back;
        // the code itself doesn't stand for a browser.
        let (code_k, code) = approved(&rk, "acct", Kind::Recovery, "recovery 1");
        let (nk, new) = approved(&code_k, "acct", Kind::Browser, "new laptop");
        let all = [all.to_vec(), vec![code.clone(), new.clone()]].concat();
        assert!(t.browser_chained(&all, &new.device) && !t.browser_chained(&all, &code.device));
        let r = Revocation::new("acct", &phone.device, &nk);
        assert!(t.evaluate(&all, &[r]).get(&phone.device).is_none(), "the recovered browser removes the lost phone");
        // The code that let it in, revoked backdated (before the browser was
        // made) by a CLI or the app: doesn't count, and the browser stays.
        // A browser of the account's may (it could remove the browser too).
        let backdated = |by: &DeviceKeys| {
            let mut r = Revocation::new("acct", &code.device, by);
            r.at = 1;
            r.sig = hex::encode(by.signature(r.body().as_bytes()));
            r
        };
        for by in [&ck, &ak] {
            let now = t.evaluate(&all, &[backdated(by)]);
            assert!(now.get(&new.device).is_some() && now.get(&code.device).is_some(), "a CLI or the app can't");
        }
        assert!(t.evaluate(&all, &[backdated(&rk)]).get(&new.device).is_none());
    }

    /// The app's devices from before it had a kind are `browser` ones it
    /// named; control treats them as the app's.
    #[test]
    fn the_apps_devices_by_kind_or_by_their_old_name() {
        let c = |kind: Kind, name: &str| Cert::new(&DeviceKeys::generate(), "acct", kind, name);
        assert!(c(Kind::App, "anything").from_app());
        assert!(c(Kind::Browser, "Arugula app on jake-air").from_app());
        assert!(c(Kind::Browser, "Arugula app (AppImage) on geek").from_app());
        assert!(c(Kind::Browser, "illogical app on geek").from_app());
        assert!(!c(Kind::Browser, "Safari on iPhone").from_app());
        assert!(!c(Kind::Cli, "Arugula app").from_app(), "only a browser's name counts");
        assert!(c(Kind::Browser, "Chrome on Mac").is_browser());
        assert!(!c(Kind::Browser, "Arugula app").is_browser());
        assert!(!c(Kind::App, "Chrome on Mac").is_browser());
    }

    #[test]
    fn join_secrets() {
        let (_, root) = root("acct");
        let k = DeviceKeys::generate();
        let machine = Cert::new(&k, "", Kind::Daemon, "box");
        let s = join_secret();
        assert_eq!(s.len(), 19);
        assert_eq!(normalize_secret(&s.to_lowercase().replace('-', " ")).as_deref(), Some(s.as_str()));
        assert!(normalize_secret("ILOU-ILOU-ILOU-ILOU").is_none(), "not in the alphabet");
        let code = join_code(&machine);
        assert_eq!(split_join_code(&format!("{code}.{s}")).unwrap(), (code.clone(), Some(s.clone())));
        assert_eq!(split_join_code(&code.to_lowercase()).unwrap(), (code.clone(), None));
        assert!(split_join_code(&format!("{code}.ABCD")).is_err());

        let trust = Trust { account: "acct".into(), root: root.device.clone() };
        let mac = join_mac(&s, &trust, &machine);
        assert!(join_mac_ok(&s, &trust, &machine, &mac));
        // Another account, another key, another secret: none check out.
        let theirs = Trust { account: "acct".into(), root: DeviceKeys::generate().id() };
        assert!(!join_mac_ok(&s, &theirs, &machine, &mac));
        let other = Cert::new(&DeviceKeys::generate(), "", Kind::Daemon, "box");
        assert!(!join_mac_ok(&s, &trust, &other, &mac));
        assert!(!join_mac_ok(&join_secret(), &trust, &machine, &mac));
        assert!(!join_mac_ok(&s, &trust, &machine, "zz"));
    }

    /// The page computes the same MAC (`web/src/shell/e2e/cert.ts`'s `joinMac`):
    /// fixed inputs, fixed answer.
    #[test]
    fn the_join_mac_is_fixed() {
        let trust = Trust { account: "a1".into(), root: "0123456789abcdef".into() };
        let c = Cert {
            v: 1,
            account: String::new(),
            device: "fedcba9876543210".into(),
            kind: Kind::Daemon,
            name: "box".into(),
            noise: "aa".into(),
            sign: "bb".into(),
            created: 0,
            approver: String::new(),
            sig: String::new(),
        };
        let mac = join_mac("ABCD-EFGH-JKMN-PQRS", &trust, &c);
        assert_eq!(mac, "21ec725fbc58f0e2aac0c21b84b6c9f99ca6f4a15bf48f5cf2d98598eaab135b");
        assert_eq!(mac, join_mac("ABCDEFGHJKMNPQRS", &trust, &c), "dashes aside");
    }

    /// #764: a device's pin is its own signature; another device's, or one
    /// over another root, isn't.
    #[test]
    fn device_pins() {
        let (k, c) = root("acct");
        let trust = Trust { account: "acct".into(), root: c.device.clone() };
        assert_eq!(
            device_pin_body(&trust, "d1"),
            format!("arugula device pin v1\naccount acct\nroot {}\ndevice d1\n", c.device)
        );
        let pin = hex::encode(k.signature(device_pin_body(&trust, &c.device).as_bytes()));
        assert!(device_pin_ok(&trust, &c, &pin));
        let other = Trust { account: "acct".into(), root: "0123456789abcdef".into() };
        assert!(!device_pin_ok(&other, &c, &pin));
        let (_, c2) = root("acct");
        assert!(!device_pin_ok(&trust, &c2, &pin));
    }

    #[test]
    fn join_codes() {
        let (_, c) = root("acct");
        let code = join_code(&c);
        assert_eq!(code.len(), 11);
        assert_eq!(normalize_code(&code.to_lowercase().replace('-', " ")).unwrap(), code);
    }
}
