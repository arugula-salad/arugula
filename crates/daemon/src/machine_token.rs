//! Machine tokens (aru-141): the daemon's side. A channel that opens with
//! a [`TokenHello`] is let in by what the hub's token says, checked here:
//! signed by a key the hub publishes, by this hub, for this machine, a
//! machine token, within its lifetime, and not used before.
//!
//! The hub's keys come from its `/.well-known/jwks.json`, fetched once and
//! again when a token names a key this daemon hasn't seen (at most every
//! [`REFETCH`]): that's how a rotated key is picked up.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use arugula_control_wire::machine_token::{CLOCK_SKEW_SECS, Jwk, Jwks, MAX_LIFETIME_SECS, MachineClaims, USE};
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
    Expired,
    Lifetime,
    Replayed,
    NoKeys,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::Malformed => "not a machine token",
            Refused::Signature => "not signed by a key control publishes",
            Refused::Issuer => "issued by another control",
            Refused::Audience => "issued for another machine",
            Refused::NotMachine => "not a machine token",
            Refused::Expired => "expired: ask control for a new one",
            Refused::Lifetime => "outside the lifetime control gives",
            Refused::Replayed => "already used",
            Refused::NoKeys => "this machine couldn't fetch control's keys",
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
    if v.get("use").and_then(|u| u.as_str()) != Some(USE) {
        return Err(Refused::NotMachine);
    }
    let claims: MachineClaims = serde_json::from_value(v).map_err(|_| Refused::Malformed)?;
    if !iss_ok(&claims.iss) {
        return Err(Refused::Issuer);
    }
    if claims.aud != aud {
        return Err(Refused::Audience);
    }
    if claims.exp <= claims.iat || claims.exp - claims.iat > MAX_LIFETIME_SECS || claims.iat > now + CLOCK_SKEW_SECS {
        return Err(Refused::Lifetime);
    }
    if claims.exp <= now {
        return Err(Refused::Expired);
    }
    if claims.jti.is_empty() || claims.sub.is_empty() {
        return Err(Refused::Malformed);
    }
    Ok(claims)
}

/// The hub's keys as last fetched, and the tokens already taken.
#[derive(Default)]
pub struct Verifier {
    keys: Mutex<Option<(Jwks, Instant)>>,
    /// Each token's `jti` until it expires.
    seen: Mutex<HashMap<String, u64>>,
}

impl Verifier {
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

    /// Take `claims`' token, once: Replayed if it was taken before.
    pub fn take(&self, claims: &MachineClaims, now: u64) -> Result<(), Refused> {
        let mut seen = self.seen.lock().unwrap();
        seen.retain(|_, exp| *exp > now);
        if seen.contains_key(&claims.jti) {
            return Err(Refused::Replayed);
        }
        seen.insert(claims.jti.clone(), claims.exp);
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

    #[test]
    fn a_token_is_taken_once() {
        let (k, jwks) = key();
        let c = verify(&sign(&k, "k1", &claims("viewer", "m1", 1000)), &jwks, ISS, "m1", 1010).unwrap();
        let v = Verifier::default();
        assert_eq!(v.take(&c, 1010), Ok(()));
        assert_eq!(v.take(&c, 1011), Err(Refused::Replayed));
        // Forgotten once it can't be used anyway.
        assert_eq!(v.take(&c, 1301), Ok(()));
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
