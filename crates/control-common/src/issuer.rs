//! The hub as an issuer (M228): the tokens it signs for other products,
//! and how anyone checks them. Pure: no state, no clock but the one passed
//! in, so the hub signs with it and anything else (the platform's link API,
//! M229; a test client) verifies with it.
//!
//! Two formats from the same Ed25519 keys:
//!
//! - **ID tokens**: JWTs (RFC 7519) signed `EdDSA` (RFC 8037), with the
//!   signing key's `kid` in the header. A client checks one against the
//!   keys at the hub's `/.well-known/jwks.json` ([`verify_id_token`]).
//! - **hud's forge assertions**: `hudfs1.<payload>.<signature>`, exactly
//!   as hud's `proxy/forge-signin.ts` reads them, so a box checks them with
//!   the key's SPKI in `HUD_FORGE_SIGNIN_KEY` and no change on hud's side
//!   ([`sign_hud`], [`verify_hud`]).
//!
//! What a token says is who: the account, its numeric GitHub id, and when.
//! Never a project or a role: those are read from the platform with the
//! access token, so a change there takes effect at once rather than when a
//! token runs out ([`IdClaims`] has no field for them).

use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long an ID token lasts, in seconds.
pub const ID_TOKEN_SECS: u64 = 300;
/// How long an access token lasts, in seconds.
pub const ACCESS_TOKEN_SECS: u64 = 600;
/// How long a hud assertion lasts, in seconds (hud takes at most 600).
pub const HUD_SECS: u64 = 300;
/// The longest a token this module verifies may live, issue to expiry.
pub const MAX_LIFETIME_SECS: u64 = 600;
/// How far ahead of the verifier's clock a token's `iat` may be.
pub const CLOCK_SKEW_SECS: u64 = 60;
/// hud's assertion prefix (`ASSERTION_PREFIX` in hud).
pub const HUD_PREFIX: &str = "hudfs1";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return None;
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()
}

/// One of the issuer's signing keys.
#[derive(Clone)]
pub struct IssuerKey {
    key: SigningKey,
    kid: String,
}

impl std::fmt::Debug for IssuerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuerKey").field("kid", &self.kid).finish_non_exhaustive()
    }
}

impl PartialEq for IssuerKey {
    fn eq(&self, o: &Self) -> bool {
        self.kid == o.kid
    }
}

impl IssuerKey {
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let key = SigningKey::from_bytes(seed);
        let kid = thumbprint(&key.verifying_key());
        IssuerKey { key, kid }
    }

    /// A key from its 32-byte seed in hex (`openssl rand -hex 32`, or
    /// `arugula-control issuer key new`).
    pub fn from_hex(s: &str) -> anyhow::Result<Self> {
        let seed: [u8; 32] = hex_decode(s.trim())
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| anyhow::anyhow!("an issuer key is 32 bytes of hex (64 characters)"))?;
        Ok(Self::from_seed(&seed))
    }

    pub fn generate() -> Self {
        Self::from_seed(&arugula_e2e::random::<32>())
    }

    /// The seed, in hex: the secret.
    pub fn seed_hex(&self) -> String {
        self.key.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Its id: the RFC 7638 thumbprint of its public key, so anyone with
    /// the public key can work it out, and it never says anything of the
    /// secret.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Its public half as a JWK, for the JWKS.
    pub fn jwk(&self) -> Jwk {
        Jwk::of(&self.key.verifying_key(), &self.kid)
    }

    /// Its public half as hud takes it in `HUD_FORGE_SIGNIN_KEY`: the
    /// base64 of its DER SubjectPublicKeyInfo.
    pub fn spki_base64(&self) -> String {
        let mut der = SPKI_PREFIX.to_vec();
        der.extend_from_slice(self.key.verifying_key().as_bytes());
        base64::engine::general_purpose::STANDARD.encode(der)
    }

    fn sign(&self, msg: &[u8]) -> String {
        b64(&self.key.sign(msg).to_bytes())
    }
}

/// An Ed25519 SubjectPublicKeyInfo's DER, before the 32 key bytes.
const SPKI_PREFIX: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// RFC 7638's thumbprint of an Ed25519 key (RFC 8037's members, in order).
fn thumbprint(k: &VerifyingKey) -> String {
    let canon = format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#, b64(k.as_bytes()));
    b64(&Sha256::digest(canon.as_bytes()))
}

/// One public key, as a JWKS lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub kid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>,
}

impl Jwk {
    fn of(k: &VerifyingKey, kid: &str) -> Self {
        Jwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: b64(k.as_bytes()),
            kid: kid.into(),
            alg: Some("EdDSA".into()),
            use_: Some("sig".into()),
        }
    }

    /// The Ed25519 key it holds, when it is one.
    pub fn key(&self) -> Option<VerifyingKey> {
        if self.kty != "OKP" || self.crv != "Ed25519" {
            return None;
        }
        let x: [u8; 32] = unb64(&self.x)?.try_into().ok()?;
        VerifyingKey::from_bytes(&x).ok()
    }
}

/// `/.well-known/jwks.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

impl Jwks {
    pub fn find(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|k| k.kid == kid)
    }
}

/// What an ID token says. No project or role, ever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdClaims {
    /// The hub's public URL.
    pub iss: String,
    /// The account's id on the hub.
    pub sub: String,
    /// The client it was issued to.
    pub aud: String,
    pub iat: u64,
    pub exp: u64,
    /// The account's numeric GitHub id, when it has signed in with GitHub.
    /// Accounts link by this, never by a login (logins change hands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_id: Option<u64>,
    /// The client's `nonce` from its authorize request, when it sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Header {
    alg: String,
    #[serde(default)]
    typ: Option<String>,
    kid: String,
}

/// Signs an ID token.
pub fn sign_id_token(key: &IssuerKey, claims: &IdClaims) -> String {
    let header = Header { alg: "EdDSA".into(), typ: Some("JWT".into()), kid: key.kid.clone() };
    let signed = format!(
        "{}.{}",
        b64(&serde_json::to_vec(&header).expect("a header")),
        b64(&serde_json::to_vec(claims).expect("claims"))
    );
    let sig = key.sign(signed.as_bytes());
    format!("{signed}.{sig}")
}

/// Why a token was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Not a token of this shape.
    Malformed,
    /// Signed by no key in the set (an unknown `kid`, or a bad signature).
    Signature,
    /// Issued by another issuer.
    Issuer,
    /// Issued for another client (or box).
    Audience,
    /// Ran out.
    Expired,
    /// Lives longer than [`MAX_LIFETIME_SECS`], or issued in the future.
    Lifetime,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::Malformed => "not a token this issuer signs",
            Refused::Signature => "not signed by a key the issuer publishes",
            Refused::Issuer => "issued by another issuer",
            Refused::Audience => "issued for another client",
            Refused::Expired => "expired: sign in again",
            Refused::Lifetime => "outside the lifetime the issuer gives",
        })
    }
}

impl std::error::Error for Refused {}

fn lifetime(iat: u64, exp: u64, now: u64) -> Result<(), Refused> {
    if exp <= iat || exp - iat > MAX_LIFETIME_SECS || iat > now + CLOCK_SKEW_SECS {
        return Err(Refused::Lifetime);
    }
    if exp <= now {
        return Err(Refused::Expired);
    }
    Ok(())
}

fn verify_sig(jwk: &Jwk, msg: &[u8], sig: &str) -> Result<(), Refused> {
    let key = jwk.key().ok_or(Refused::Signature)?;
    let sig: [u8; 64] = unb64(sig).and_then(|s| s.try_into().ok()).ok_or(Refused::Malformed)?;
    key.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(&sig)).map_err(|_| Refused::Signature)
}

/// Checks an ID token against a JWKS (the issuer's, fetched from its
/// `jwks_uri`): its signature by the key its `kid` names, its issuer, its
/// audience and its lifetime, at `now` (seconds since the epoch). A client
/// that meets an unknown `kid` fetches the JWKS again once and retries:
/// that's how it picks up a new key.
pub fn verify_id_token(token: &str, jwks: &Jwks, iss: &str, aud: &str, now: u64) -> Result<IdClaims, Refused> {
    if token.len() > 8192 {
        return Err(Refused::Malformed);
    }
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(Refused::Malformed);
    };
    let header: Header = unb64(h).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(Refused::Malformed)?;
    if header.alg != "EdDSA" {
        return Err(Refused::Malformed);
    }
    let jwk = jwks.find(&header.kid).ok_or(Refused::Signature)?;
    verify_sig(jwk, format!("{h}.{p}").as_bytes(), s)?;
    let claims: IdClaims = unb64(p).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(Refused::Malformed)?;
    if claims.iss != iss {
        return Err(Refused::Issuer);
    }
    if claims.aud != aud {
        return Err(Refused::Audience);
    }
    lifetime(claims.iat, claims.exp, now)?;
    Ok(claims)
}

/// hud's forge assertion's payload (hud's `docs/forge-signin.md`). hud
/// reads `sub`, `aud`, `iat`, `exp` and `jti` and ignores the rest; the
/// rest says who in the platform's terms, for hud's roster from project
/// members later (M237).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HudClaims {
    /// `github:<login>`, chant's form of a forge identity.
    pub sub: String,
    /// The box: its `HUD_FORGE_SIGNIN_AUDIENCE`, the client id here.
    pub aud: String,
    pub iat: u64,
    pub exp: u64,
    /// A nonce: hud takes each once.
    pub jti: String,
    /// The account's id on the hub.
    pub account: String,
    /// Its numeric GitHub id.
    pub github_id: u64,
    /// The key that signed it, for whoever reads a log.
    pub kid: String,
}

/// Signs a hud assertion: `hudfs1.<payload>.<signature>`, Ed25519 over the
/// ASCII of `hudfs1.<payload>`.
pub fn sign_hud(key: &IssuerKey, claims: &HudClaims) -> String {
    let signed = format!("{HUD_PREFIX}.{}", b64(&serde_json::to_vec(claims).expect("claims")));
    let sig = key.sign(signed.as_bytes());
    format!("{signed}.{sig}")
}

/// Checks a hud assertion the way hud does (`checkForgeAssertion`),
/// against every key in a JWKS (hud itself holds one). Replay is the
/// caller's, as in hud.
pub fn verify_hud(token: &str, jwks: &Jwks, aud: &str, now: u64) -> Result<HudClaims, Refused> {
    let parts: Vec<&str> = token.split('.').collect();
    let [prefix, p, s] = parts[..] else { return Err(Refused::Malformed) };
    if prefix != HUD_PREFIX || token.len() > 4096 {
        return Err(Refused::Malformed);
    }
    let msg = format!("{prefix}.{p}");
    if !jwks.keys.iter().any(|k| verify_sig(k, msg.as_bytes(), s).is_ok()) {
        return Err(Refused::Signature);
    }
    let claims: HudClaims = unb64(p).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(Refused::Malformed)?;
    if claims.aud != aud {
        return Err(Refused::Audience);
    }
    if !(8..=200).contains(&claims.jti.len()) {
        return Err(Refused::Malformed);
    }
    lifetime(claims.iat, claims.exp, now)?;
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(now: u64) -> IdClaims {
        IdClaims {
            iss: "https://control.example".into(),
            sub: "acct1".into(),
            aud: "studio".into(),
            iat: now,
            exp: now + ID_TOKEN_SECS,
            github_id: Some(42),
            nonce: None,
        }
    }

    #[test]
    fn an_id_token_verifies_against_its_jwks_only() {
        let (a, b) = (IssuerKey::generate(), IssuerKey::generate());
        let now = 1_800_000_000;
        let t = sign_id_token(&a, &claims(now));
        let jwks = Jwks { keys: vec![b.jwk(), a.jwk()] };
        assert_eq!(verify_id_token(&t, &jwks, "https://control.example", "studio", now).unwrap(), claims(now));
        // Another key's set, another issuer, another client.
        let only_b = Jwks { keys: vec![b.jwk()] };
        assert_eq!(verify_id_token(&t, &only_b, "https://control.example", "studio", now), Err(Refused::Signature));
        assert_eq!(verify_id_token(&t, &jwks, "https://other", "studio", now), Err(Refused::Issuer));
        assert_eq!(verify_id_token(&t, &jwks, "https://control.example", "hud", now), Err(Refused::Audience));
        // Ran out; and a kid swapped onto another key's signature.
        let late = now + ID_TOKEN_SECS;
        assert_eq!(verify_id_token(&t, &jwks, "https://control.example", "studio", late), Err(Refused::Expired));
        let header = b64(format!(r#"{{"alg":"EdDSA","kid":"{}"}}"#, a.kid()).as_bytes());
        let signed = format!("{header}.{}", b64(&serde_json::to_vec(&claims(now)).unwrap()));
        let forged = format!("{signed}.{}", b.sign(signed.as_bytes()));
        assert_eq!(verify_id_token(&forged, &jwks, "https://control.example", "studio", now), Err(Refused::Signature));
        // A payload changed after signing.
        let mut parts: Vec<String> = t.split('.').map(str::to_owned).collect();
        let mut c = claims(now);
        c.sub = "someone-else".into();
        parts[1] = b64(&serde_json::to_vec(&c).unwrap());
        assert_eq!(
            verify_id_token(&parts.join("."), &jwks, "https://control.example", "studio", now),
            Err(Refused::Signature)
        );
    }

    #[test]
    fn a_token_lives_minutes_and_says_who_only() {
        let k = IssuerKey::generate();
        let now = 1_800_000_000;
        let mut c = claims(now);
        c.exp = now + MAX_LIFETIME_SECS + 1;
        let jwks = Jwks { keys: vec![k.jwk()] };
        assert_eq!(verify_id_token(&sign_id_token(&k, &c), &jwks, &c.iss, &c.aud, now), Err(Refused::Lifetime));
        // Who, never what they may do: no project or role in an ID token.
        let json = serde_json::to_value(claims(now)).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["aud", "exp", "github_id", "iat", "iss", "sub"]);
    }

    #[test]
    fn hud_assertions_in_huds_own_shape() {
        let k = IssuerKey::generate();
        let now = 1_800_000_000;
        let c = HudClaims {
            sub: "github:jake-gh".into(),
            aud: "box1".into(),
            iat: now,
            exp: now + HUD_SECS,
            jti: "0123456789abcdef".into(),
            account: "acct1".into(),
            github_id: 7,
            kid: k.kid().into(),
        };
        let t = sign_hud(&k, &c);
        let (signed, sig) = t.rsplit_once('.').unwrap();
        assert!(signed.starts_with("hudfs1."));
        // Ed25519 over the ASCII of `hudfs1.<payload>`, by the key whose
        // SPKI hud is given.
        let spki = base64::engine::general_purpose::STANDARD.decode(k.spki_base64()).unwrap();
        assert_eq!(spki.len(), 44);
        let vk = VerifyingKey::from_bytes(spki[12..].try_into().unwrap()).unwrap();
        let sig: [u8; 64] = unb64(sig).unwrap().try_into().unwrap();
        vk.verify_strict(signed.as_bytes(), &ed25519_dalek::Signature::from_bytes(&sig)).unwrap();
        let jwks = Jwks { keys: vec![k.jwk()] };
        assert_eq!(verify_hud(&t, &jwks, "box1", now).unwrap(), c);
        assert_eq!(verify_hud(&t, &jwks, "box2", now), Err(Refused::Audience));
        assert_eq!(verify_hud(&t, &jwks, "box1", now + HUD_SECS), Err(Refused::Expired));
    }

    #[test]
    fn keys_from_hex_and_their_ids() {
        let k = IssuerKey::from_hex(&"07".repeat(32)).unwrap();
        assert_eq!(IssuerKey::from_hex(&k.seed_hex()).unwrap().kid(), k.kid());
        assert_eq!(k.kid().len(), 43);
        assert!(IssuerKey::from_hex("abcd").is_err());
        assert!(IssuerKey::from_hex(&"zz".repeat(32)).is_err());
        assert_eq!(k.jwk().key().unwrap().as_bytes(), k.key.verifying_key().as_bytes());
    }
}
