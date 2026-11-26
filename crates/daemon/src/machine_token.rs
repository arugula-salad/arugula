//! Machine tokens (aru-141): the daemon's side. A channel that opens with
//! a [`TokenHello`] is let in by what the hub's token says, checked here:
//! signed by a key the hub publishes, by this hub, for this machine, a
//! machine token, within its lifetime, issued since this daemon started,
//! and not used before.
//!
//! The hub's keys come from its `/.well-known/jwks.json`, fetched once and
//! again when a token names a key this daemon hasn't seen (at most every
//! [`REFETCH`]): that's how a rotated key is picked up.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use arugula_control_wire::{
    ladder::{START_USE, StartClaims},
    machine_token::{BLOCK_USE, BlockClaims, CLOCK_SKEW_SECS, Jwk, Jwks, MAX_LIFETIME_SECS, MachineClaims, USE},
};
use base64::Engine as _;
use serde::Deserialize;

/// The soonest the keys are fetched again for a `kid` this daemon doesn't
/// know: a stream of made-up `kid`s costs control one fetch a minute.
const REFETCH: Duration = Duration::from_secs(60);

/// The longest a token may be, in bytes.
const MAX_TOKEN: usize = 8192;

/// Why a token was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Malformed,
    Signature,
    Issuer,
    Audience,
    NotMachine,
    /// Not a block grant (a machine token, say) where one was wanted.
    NotBlock,
    /// Not a start ticket (aru-143) where one was wanted.
    NotStart,
    Expired,
    Lifetime,
    Replayed,
    /// Issued before this daemon started: it may have been taken before a
    /// restart, which the in-memory replay check doesn't remember.
    BeforeStart,
    NoKeys,
    /// This machine lets nobody in: a deploy's box.
    Closed,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::Malformed => "not a machine token",
            Refused::Signature => "not signed by a key control publishes",
            Refused::Issuer => "issued by another control",
            Refused::Audience => "issued for another machine",
            Refused::NotMachine => "not a machine token",
            Refused::NotBlock => "not a block grant",
            Refused::NotStart => "not a start ticket",
            Refused::Expired => "expired: ask control for a new one",
            Refused::Lifetime => "outside the lifetime control gives",
            Refused::Replayed => "already used",
            Refused::BeforeStart => "issued before this machine started: ask control for a new one",
            Refused::NoKeys => "this machine couldn't fetch control's keys",
            Refused::Closed => "this machine lets nobody in",
        })
    }
}

impl std::error::Error for Refused {}

fn unb64(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return None;
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()
}

fn key_of(jwk: &Jwk) -> Option<ed25519_dalek::VerifyingKey> {
    if jwk.kty != "OKP" || jwk.crv != "Ed25519" {
        return None;
    }
    let x: [u8; 32] = unb64(&jwk.x)?.try_into().ok()?;
    ed25519_dalek::VerifyingKey::from_bytes(&x).ok()
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
    /// Extensions a verifier must understand (RFC 7515 4.1.11): none are.
    #[serde(default)]
    crit: Option<serde_json::Value>,
}

/// The header's `kid`, or Malformed.
fn kid_of(token: &str) -> Result<String, Refused> {
    let h = token.split('.').next().ok_or(Refused::Malformed)?;
    let header: Header = unb64(h).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(Refused::Malformed)?;
    Ok(header.kid)
}

/// `token`'s claims, once its signature by a key in `jwks` holds, if they
/// carry `use`: what every token the hub signs for a machine starts with.
fn signed(token: &str, jwks: &Jwks, use_: &str, wrong_use: Refused) -> Result<serde_json::Value, Refused> {
    if token.len() > MAX_TOKEN {
        return Err(Refused::Malformed);
    }
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(Refused::Malformed);
    };
    let header: Header = unb64(h).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(Refused::Malformed)?;
    if header.alg != "EdDSA" || header.crit.is_some() {
        return Err(Refused::Malformed);
    }
    let jwk = jwks.find(&header.kid).ok_or(Refused::Signature)?;
    let key = key_of(jwk).ok_or(Refused::Signature)?;
    let sig: [u8; 64] = unb64(s).and_then(|s| s.try_into().ok()).ok_or(Refused::Malformed)?;
    key.verify_strict(format!("{h}.{p}").as_bytes(), &ed25519_dalek::Signature::from_bytes(&sig))
        .map_err(|_| Refused::Signature)?;
    // Claims are read only once the signature holds.
    let v: serde_json::Value = unb64(p).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(Refused::Malformed)?;
    if v.get("use").and_then(|u| u.as_str()) != Some(use_) {
        return Err(wrong_use);
    }
    Ok(v)
}

/// The claims every token for a machine has, checked the same way.
struct Common<'a> {
    iss: &'a str,
    aud: &'a str,
    sub: &'a str,
    iat: u64,
    exp: u64,
    jti: &'a str,
}

impl Common<'_> {
    /// From an issuer `iss_ok` takes, for machine `aud`, live at `now`.
    fn check(&self, iss_ok: impl Fn(&str) -> bool, aud: &str, now: u64) -> Result<(), Refused> {
        if !iss_ok(self.iss) {
            return Err(Refused::Issuer);
        }
        if self.aud != aud {
            return Err(Refused::Audience);
        }
        if self.exp <= self.iat || self.exp - self.iat > MAX_LIFETIME_SECS || self.iat > now + CLOCK_SKEW_SECS {
            return Err(Refused::Lifetime);
        }
        if self.exp <= now {
            return Err(Refused::Expired);
        }
        if self.jti.is_empty() || self.sub.is_empty() {
            return Err(Refused::Malformed);
        }
        Ok(())
    }
}

/// Checks `token` against `jwks`: issued by an issuer `iss_ok` takes, for
/// machine `aud`, a machine token, live at `now` (seconds). Pure: the
/// replay check is [`Verifier`]'s.
pub fn verify(
    token: &str,
    jwks: &Jwks,
    iss_ok: impl Fn(&str) -> bool,
    aud: &str,
    now: u64,
) -> Result<MachineClaims, Refused> {
    let v = signed(token, jwks, USE, Refused::NotMachine)?;
    let c: MachineClaims = serde_json::from_value(v).map_err(|_| Refused::Malformed)?;
    Common { iss: &c.iss, aud: &c.aud, sub: &c.sub, iat: c.iat, exp: c.exp, jti: &c.jti }.check(iss_ok, aud, now)?;
    Ok(c)
}

/// Checks a block grant (#1001) as [`verify`] checks a machine token, but
/// for `use: "block"`. Whether its key, its block and its role let a
/// channel in is the block channel's to say (`block_channel.rs`).
///
/// No replay check: a frame's worker and its pages open their channels
/// with the same grant until it expires, as with a device's grant. It's
/// bound to the one-off key that does the handshake, and a channel needs
/// that key's private half, so a copied grant opens nothing.
pub fn verify_block(
    token: &str,
    jwks: &Jwks,
    iss_ok: impl Fn(&str) -> bool,
    aud: &str,
    now: u64,
) -> Result<BlockClaims, Refused> {
    let v = signed(token, jwks, BLOCK_USE, Refused::NotBlock)?;
    let c: BlockClaims = serde_json::from_value(v).map_err(|_| Refused::Malformed)?;
    Common { iss: &c.iss, aud: &c.aud, sub: &c.sub, iat: c.iat, exp: c.exp, jti: &c.jti }.check(iss_ok, aud, now)?;
    Ok(c)
}

/// Checks a start ticket (aru-143) as [`verify`] checks a machine token, but
/// for `use: "start"`. Whether it starts what it's sent with is the ladder's
/// to say (`ladder.rs`); [`Verifier::take_start`] takes it once.
pub fn verify_start(
    token: &str,
    jwks: &Jwks,
    iss_ok: impl Fn(&str) -> bool,
    aud: &str,
    now: u64,
) -> Result<StartClaims, Refused> {
    let v = signed(token, jwks, START_USE, Refused::NotStart)?;
    let c: StartClaims = serde_json::from_value(v).map_err(|_| Refused::Malformed)?;
    Common { iss: &c.iss, aud: &c.aud, sub: &c.sub, iat: c.iat, exp: c.exp, jti: &c.jti }.check(iss_ok, aud, now)?;
    if c.from.is_empty() || c.board.is_empty() {
        return Err(Refused::Malformed);
    }
    Ok(c)
}

/// The hub's keys as last fetched, and the tokens already taken.
pub struct Verifier {
    keys: Mutex<Option<(Jwks, Instant)>>,
    /// Each token's `jti` until it expires.
    seen: Mutex<HashMap<String, u64>>,
    /// When this daemon started (seconds since the epoch): a token issued
    /// earlier is refused. `seen` lives only in memory, so this is what
    /// keeps a token taken before a restart from being taken again after
    /// it, without writing anything down.
    started: u64,
}

impl Default for Verifier {
    fn default() -> Self {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        Self::started_at(now)
    }
}

impl Verifier {
    /// A verifier for a daemon that started at `started` (seconds). The
    /// daemon makes one, with its control state, once per process.
    pub fn started_at(started: u64) -> Self {
        Self { keys: Mutex::new(None), seen: Mutex::new(HashMap::new()), started }
    }

    /// The keys held now, if any.
    pub fn keys(&self) -> Option<Jwks> {
        self.keys.lock().unwrap().as_ref().map(|(k, _)| k.clone())
    }

    /// Whether to fetch the keys: none held, or `kid` isn't among them and
    /// the last fetch was long enough ago.
    pub fn should_fetch(&self, kid: &str) -> bool {
        match &*self.keys.lock().unwrap() {
            None => true,
            Some((k, at)) => k.find(kid).is_none() && at.elapsed() >= REFETCH,
        }
    }

    pub fn set_keys(&self, k: Jwks) {
        *self.keys.lock().unwrap() = Some((k, Instant::now()));
    }

    /// Take `claims`' token, once: Replayed if it was taken before, and
    /// BeforeStart if it was issued before this daemon started.
    ///
    /// No allowance for clock skew here: if control's clock is behind this
    /// machine's, a token it issued just after a restart can carry an `iat`
    /// before `started` and be refused. The client asks for a new one on a
    /// `machine-token` refusal, so that costs a few seconds; widening the
    /// window by CLOCK_SKEW_SECS would let a token taken just before the
    /// restart through again. `iat` is whole seconds, so a token issued in
    /// the second this daemon started is taken.
    pub fn take(&self, claims: &MachineClaims, now: u64) -> Result<(), Refused> {
        self.take_once(claims.iat, &claims.jti, claims.exp, now)
    }

    /// [`take`](Self::take), for a start ticket (aru-143). Its `jti` is the
    /// hub's, unique among every token it signs, so one set holds both.
    pub fn take_start(&self, claims: &StartClaims, now: u64) -> Result<(), Refused> {
        self.take_once(claims.iat, &claims.jti, claims.exp, now)
    }

    fn take_once(&self, iat: u64, jti: &str, exp: u64, now: u64) -> Result<(), Refused> {
        if iat < self.started {
            return Err(Refused::BeforeStart);
        }
        let mut seen = self.seen.lock().unwrap();
        seen.retain(|_, exp| *exp > now);
        if seen.contains_key(jti) {
            return Err(Refused::Replayed);
        }
        seen.insert(jti.to_owned(), exp);
        Ok(())
    }

    /// The `kid` a token names, to decide whether to fetch first.
    pub fn kid(token: &str) -> Result<String, Refused> {
        kid_of(token)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use arugula_control_wire::machine_token::TokenRole;
    use ed25519_dalek::{Signer as _, SigningKey};

    use super::*;

    fn b64(b: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
    }

    pub(crate) fn key() -> (SigningKey, Jwks) {
        let k = SigningKey::from_bytes(&[7; 32]);
        let jwk = Jwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: b64(k.verifying_key().as_bytes()),
            kid: "k1".into(),
            alg: Some("EdDSA".into()),
            use_: Some("sig".into()),
        };
        (k, Jwks { keys: vec![jwk] })
    }

    /// A token as the hub signs one (its `sign_id_token`, with these claims).
    pub(crate) fn sign(k: &SigningKey, kid: &str, claims: &serde_json::Value) -> String {
        let h = b64(&serde_json::to_vec(&serde_json::json!({ "alg": "EdDSA", "typ": "JWT", "kid": kid })).unwrap());
        let p = b64(&serde_json::to_vec(claims).unwrap());
        let s = b64(&k.sign(format!("{h}.{p}").as_bytes()).to_bytes());
        format!("{h}.{p}.{s}")
    }

    pub(crate) fn claims(role: &str, aud: &str, iat: u64) -> serde_json::Value {
        serde_json::json!({
            "iss": "https://control.example", "sub": "acct1", "aud": aud, "use": "machine",
            "role": role, "name": "someone", "iat": iat, "exp": iat + 300, "jti": "j1",
        })
    }

    const ISS: fn(&str) -> bool = |i| i == "https://control.example";

    #[test]
    fn a_good_token_says_who_and_what() {
        let (k, jwks) = key();
        let t = sign(&k, "k1", &claims("editor", "m1", 1000));
        let c = verify(&t, &jwks, ISS, "m1", 1010).unwrap();
        assert_eq!((c.sub.as_str(), c.role, c.name.as_str()), ("acct1", TokenRole::Editor, "someone"));
    }

    #[test]
    fn refusals() {
        let (k, jwks) = key();
        let good = claims("owner", "m1", 1000);
        let t = sign(&k, "k1", &good);
        assert_eq!(verify(&t, &jwks, ISS, "m2", 1010), Err(Refused::Audience));
        assert_eq!(verify(&t, &jwks, |_| false, "m1", 1010), Err(Refused::Issuer));
        assert_eq!(verify(&t, &jwks, ISS, "m1", 1300), Err(Refused::Expired));
        assert_eq!(verify(&t, &jwks, ISS, "m1", 900), Err(Refused::Lifetime));
        // Another key, or a key the set doesn't have.
        let other = SigningKey::from_bytes(&[8; 32]);
        assert_eq!(verify(&sign(&other, "k1", &good), &jwks, ISS, "m1", 1010), Err(Refused::Signature));
        assert_eq!(verify(&sign(&k, "k9", &good), &jwks, ISS, "m1", 1010), Err(Refused::Signature));
        // An ID token (no `use`) from the same keys.
        let mut id = good.clone();
        id.as_object_mut().unwrap().remove("use");
        assert_eq!(verify(&sign(&k, "k1", &id), &jwks, ISS, "m1", 1010), Err(Refused::NotMachine));
        // Too long a life.
        let mut long = good.clone();
        long["exp"] = serde_json::json!(1000 + 3600);
        assert_eq!(verify(&sign(&k, "k1", &long), &jwks, ISS, "m1", 1010), Err(Refused::Lifetime));
        // A changed payload under the old signature.
        let mut parts: Vec<String> = t.split('.').map(str::to_owned).collect();
        parts[1] = b64(&serde_json::to_vec(&claims("owner", "m2", 1000)).unwrap());
        assert_eq!(verify(&parts.join("."), &jwks, ISS, "m2", 1010), Err(Refused::Signature));
        assert_eq!(verify("a.b", &jwks, ISS, "m1", 1010), Err(Refused::Malformed));
    }

    pub(crate) fn block_claims(role: &str, aud: &str, iat: u64) -> serde_json::Value {
        serde_json::json!({
            "iss": "https://control.example", "sub": "acct1", "aud": aud, "use": "block",
            "role": role, "name": "someone", "block": "7", "key": "ab".repeat(32),
            "iat": iat, "exp": iat + 300, "jti": "j1",
        })
    }

    #[test]
    fn a_block_grant_is_never_a_machine_token() {
        let (k, jwks) = key();
        let b = sign(&k, "k1", &block_claims("owner", "m1", 1000));
        let c = verify_block(&b, &jwks, ISS, "m1", 1010).unwrap();
        assert_eq!((c.block.as_str(), c.role, c.key.len()), ("7", TokenRole::Owner, 64));
        assert_eq!(verify(&b, &jwks, ISS, "m1", 1010), Err(Refused::NotMachine));
        let m = sign(&k, "k1", &claims("owner", "m1", 1000));
        assert_eq!(verify_block(&m, &jwks, ISS, "m1", 1010), Err(Refused::NotBlock));
        // The checks a machine token has.
        assert_eq!(verify_block(&b, &jwks, ISS, "m2", 1010), Err(Refused::Audience));
        assert_eq!(verify_block(&b, &jwks, |_| false, "m1", 1010), Err(Refused::Issuer));
        assert_eq!(verify_block(&b, &jwks, ISS, "m1", 1300), Err(Refused::Expired));
        let other = SigningKey::from_bytes(&[8; 32]);
        assert_eq!(
            verify_block(&sign(&other, "k1", &block_claims("owner", "m1", 1000)), &jwks, ISS, "m1", 1010),
            Err(Refused::Signature)
        );
        let mut long = block_claims("owner", "m1", 1000);
        long["exp"] = serde_json::json!(1000 + 3600);
        assert_eq!(verify_block(&sign(&k, "k1", &long), &jwks, ISS, "m1", 1010), Err(Refused::Lifetime));
        let mut keyless = block_claims("owner", "m1", 1000);
        keyless.as_object_mut().unwrap().remove("key");
        assert_eq!(verify_block(&sign(&k, "k1", &keyless), &jwks, ISS, "m1", 1010), Err(Refused::Malformed));
    }

    #[test]
    fn a_token_is_taken_once() {
        let (k, jwks) = key();
        let c = verify(&sign(&k, "k1", &claims("viewer", "m1", 1000)), &jwks, ISS, "m1", 1010).unwrap();
        let v = Verifier::started_at(1000);
        assert_eq!(v.take(&c, 1010), Ok(()));
        assert_eq!(v.take(&c, 1011), Err(Refused::Replayed));
        // Forgotten once it can't be used anyway.
        assert_eq!(v.take(&c, 1301), Ok(()));
    }

    #[test]
    fn a_token_from_before_the_daemon_started_is_refused() {
        let (k, jwks) = key();
        let c = verify(&sign(&k, "k1", &claims("viewer", "m1", 1000)), &jwks, ISS, "m1", 1010).unwrap();
        // Taken, then the daemon restarts: the new one has no `seen`.
        let before = Verifier::started_at(990);
        assert_eq!(before.take(&c, 1010), Ok(()));
        let after = Verifier::started_at(1005);
        assert_eq!(after.take(&c, 1011), Err(Refused::BeforeStart));
        // A token issued since then is taken.
        let mut fresh = claims("viewer", "m1", 1006);
        fresh["jti"] = serde_json::json!("j2");
        let c = verify(&sign(&k, "k1", &fresh), &jwks, ISS, "m1", 1010).unwrap();
        assert_eq!(after.take(&c, 1011), Ok(()));
        // One issued in the second it started is taken too.
        assert_eq!(Verifier::started_at(1006).take(&c, 1011), Ok(()));
    }

    #[test]
    fn keys_are_fetched_for_an_unknown_kid_at_most_every_minute() {
        let v = Verifier::default();
        assert!(v.should_fetch("k1"));
        v.set_keys(key().1);
        assert!(!v.should_fetch("k1"));
        assert!(!v.should_fetch("k2"), "just fetched");
    }
}
