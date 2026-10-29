//! Who is asking, as the hub found out ("Platform paths stay on
//! control.arugula.io forever, behind the hub's gateway"). The hub checks
//! the session cookie or the daemon's signature; the platform never sees
//! either ("The hub stays the identity provider"), only the [`Principal`]
//! the hub hands it.
//!
//! In process the hub puts the principal in the request's extensions. Once
//! the platform is its own process (M140) the hub forwards it in the
//! [`HEADER`], signed with a key both share:
//!
//! `x-arugula-principal: v1 <ms> <request id> <principal> <mac>`
//!
//! `<principal>` is the principal's JSON and `<mac>` an HMAC-SHA256, both
//! base64url without padding. The MAC covers the method, the path and
//! query, the time, the request id and the principal's JSON, so a header
//! can't be moved to another request. The platform takes a header only
//! within [`SKEW_MS`] of its own clock. The hub drops any principal header
//! a client sent before it adds its own.
//!
//! The gateway forwards every call with one, even one with no principal:
//! then the JSON is a [`Vouch`] that says the hub refused the credentials
//! (and how it would have answered) or that there were none, and the
//! platform refuses a call with no valid header at all.

pub use arugula_control_wire::internal::{Credential, NotVouched, Principal, Vouch};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hmac::{KeyInit as _, Mac as _};

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// The header the hub forwards a principal in.
pub const HEADER: &str = "x-arugula-principal";

/// How far a header's time may be from the platform's clock: both are on
/// one private network, so a header older than this was held back.
pub const SKEW_MS: u64 = 60_000;

/// A request id: what the hub logs the request as, so both sides' logs
/// line up. At most this long, of `[A-Za-z0-9_-]`.
pub const MAX_REQUEST_ID: usize = 64;

/// The key the hub signs principals with and the platform checks them
/// with: a secret both processes are given (a Fly secret on both apps).
#[derive(Clone)]
pub struct PrincipalKey(Vec<u8>);

impl std::fmt::Debug for PrincipalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrincipalKey(..)")
    }
}

impl PrincipalKey {
    /// At least 32 bytes.
    pub fn new(secret: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(secret.len() >= 32, "the principal key is at least 32 bytes");
        Ok(Self(secret.to_vec()))
    }

    /// From hex, as it's kept in a secret.
    pub fn from_hex(hex: &str) -> anyhow::Result<Self> {
        let hex = hex.trim();
        anyhow::ensure!(hex.len().is_multiple_of(2) && hex.is_ascii(), "the principal key is hex");
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| anyhow::anyhow!("the principal key is hex"))?;
        Self::new(&bytes)
    }

    /// An HMAC-SHA256 under this key, for `message`: what the principal
    /// header and the processes' internal calls (`internal`) are signed
    /// with, each with its own first line, so one can't pass for the other.
    pub(crate) fn hmac(&self, message: &[u8]) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC takes any key");
        mac.update(message);
        mac
    }

    fn mac(&self, method: &str, path_and_query: &str, ms: u64, request_id: &str, principal: &str) -> HmacSha256 {
        self.hmac(message(method, path_and_query, ms, request_id, principal).as_bytes())
    }

    /// The header for a request: `method` and `path_and_query` as the
    /// platform will see them, at `ms`.
    pub fn sign(&self, principal: &Principal, method: &str, path_and_query: &str, request_id: &str, ms: u64) -> String {
        let json = serde_json::to_string(principal).expect("a principal serializes");
        self.sign_json(&json, method, path_and_query, request_id, ms)
    }

    /// The header for a request the gateway forwards (M140): who's asking,
    /// a refusal of their credentials, or nobody.
    pub fn sign_vouch(&self, vouch: &Vouch, method: &str, path_and_query: &str, request_id: &str, ms: u64) -> String {
        let json = serde_json::to_string(vouch).expect("a vouch serializes");
        self.sign_json(&json, method, path_and_query, request_id, ms)
    }

    fn sign_json(&self, json: &str, method: &str, path_and_query: &str, request_id: &str, ms: u64) -> String {
        assert!(valid_request_id(request_id), "a request id is 1 to 64 of [A-Za-z0-9_-]");
        let tag = self.mac(method, path_and_query, ms, request_id, json).finalize().into_bytes();
        format!("v1 {ms} {request_id} {} {}", B64.encode(json), B64.encode(tag))
    }

    /// The principal in a header, if this key signed it for this request
    /// within [`SKEW_MS`] of `now_ms`.
    pub fn verify(
        &self,
        header: &str,
        method: &str,
        path_and_query: &str,
        now_ms: u64,
    ) -> Result<Verified, BadPrincipal> {
        let (json, request_id, ms) = self.verify_json(header, method, path_and_query, now_ms)?;
        let principal = serde_json::from_str(&json).map_err(|_| BadPrincipal::Malformed)?;
        Ok(Verified { principal, request_id, ms })
    }

    /// What a forwarded request's header vouches for, if this key signed
    /// it for this request within [`SKEW_MS`] of `now_ms`: a principal (as
    /// [`verify`](Self::verify) reads it), a refusal, or nobody.
    pub fn verify_vouch(
        &self,
        header: &str,
        method: &str,
        path_and_query: &str,
        now_ms: u64,
    ) -> Result<(Vouch, String), BadPrincipal> {
        let (json, request_id, _) = self.verify_json(header, method, path_and_query, now_ms)?;
        let vouch = serde_json::from_str(&json).map_err(|_| BadPrincipal::Malformed)?;
        Ok((vouch, request_id))
    }

    /// The signed JSON, the request id and the time.
    fn verify_json(
        &self,
        header: &str,
        method: &str,
        path_and_query: &str,
        now_ms: u64,
    ) -> Result<(String, String, u64), BadPrincipal> {
        let f: Vec<&str> = header.split(' ').collect();
        let [v, ms, request_id, json, tag] = f.as_slice() else { return Err(BadPrincipal::Malformed) };
        if *v != "v1" {
            return Err(BadPrincipal::Version);
        }
        let ms: u64 = ms.parse().map_err(|_| BadPrincipal::Malformed)?;
        if !valid_request_id(request_id) {
            return Err(BadPrincipal::Malformed);
        }
        let json = String::from_utf8(B64.decode(json).map_err(|_| BadPrincipal::Malformed)?)
            .map_err(|_| BadPrincipal::Malformed)?;
        let tag = B64.decode(tag).map_err(|_| BadPrincipal::Malformed)?;
        self.mac(method, path_and_query, ms, request_id, &json)
            .verify_slice(&tag)
            .map_err(|_| BadPrincipal::Signature)?;
        if ms.abs_diff(now_ms) > SKEW_MS {
            return Err(BadPrincipal::Stale);
        }
        Ok((json, (*request_id).to_owned(), ms))
    }
}

/// What the MAC covers.
fn message(method: &str, path_and_query: &str, ms: u64, request_id: &str, principal: &str) -> String {
    format!("arugula principal v1\n{method}\n{path_and_query}\n{ms}\n{request_id}\n{principal}\n")
}

fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REQUEST_ID
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A principal header the platform took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub principal: Principal,
    pub request_id: String,
    /// When the hub signed it.
    pub ms: u64,
}

/// Why a principal header was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadPrincipal {
    Malformed,
    /// Not `v1`.
    Version,
    /// Not signed with this key, or not for this request.
    Signature,
    /// Signed longer than [`SKEW_MS`] ago, or ahead.
    Stale,
    /// An internal call whose nonce was already taken (`internal`).
    Replayed,
}

impl std::fmt::Display for BadPrincipal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BadPrincipal::Malformed => "a malformed principal header",
            BadPrincipal::Version => "a principal header of another version",
            BadPrincipal::Signature => "a principal header not signed for this request",
            BadPrincipal::Stale => "a principal header too old or too new",
            BadPrincipal::Replayed => "an internal call already taken",
        })
    }
}

impl std::error::Error for BadPrincipal {}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_000_000_000;

    fn key() -> PrincipalKey {
        PrincipalKey::new(&[7; 32]).unwrap()
    }

    fn daemon() -> Principal {
        Principal::Daemon { account: "acct-a".into(), daemon: "dmn-1".into() }
    }

    #[test]
    fn round_trips() {
        for p in [
            Principal::Person { account: "acct-a".into() },
            Principal::Cli { account: "acct-a".into(), device: "cli-1".into() },
            daemon(),
        ] {
            let h = key().sign(&p, "POST", "/api/chat/rooms/r1/messages?x=1", "req-1", NOW);
            let v = key().verify(&h, "POST", "/api/chat/rooms/r1/messages?x=1", NOW + 1_000).unwrap();
            assert_eq!(v, Verified { principal: p, request_id: "req-1".into(), ms: NOW });
        }
        assert_eq!(daemon().account(), "acct-a");
        assert_eq!(daemon().device(), Some("dmn-1"));
        assert_eq!(Principal::Person { account: "a".into() }.device(), None);
    }

    /// The JSON is part of the contract with an older or newer platform.
    #[test]
    fn json_shape() {
        assert_eq!(
            serde_json::to_value(daemon()).unwrap(),
            serde_json::json!({ "kind": "daemon", "account": "acct-a", "daemon": "dmn-1" })
        );
        assert_eq!(
            serde_json::to_value(Principal::Cli { account: "a".into(), device: "d".into() }).unwrap(),
            serde_json::json!({ "kind": "cli", "account": "a", "device": "d" })
        );
    }

    #[test]
    fn refuses_another_request() {
        let h = key().sign(&daemon(), "POST", "/api/chat", "req-1", NOW);
        assert_eq!(key().verify(&h, "GET", "/api/chat", NOW), Err(BadPrincipal::Signature), "another method");
        assert_eq!(key().verify(&h, "POST", "/api/chat?x", NOW), Err(BadPrincipal::Signature), "another path");
        let other = PrincipalKey::new(&[8; 32]).unwrap();
        assert_eq!(other.verify(&h, "POST", "/api/chat", NOW), Err(BadPrincipal::Signature), "another key");
    }

    #[test]
    fn refuses_a_changed_header() {
        let h = key().sign(&daemon(), "POST", "/api/chat", "req-1", NOW);
        let f: Vec<&str> = h.split(' ').collect();
        let person = B64.encode(r#"{"kind":"person","account":"acct-b"}"#);
        let swapped = format!("{} {} {} {person} {}", f[0], f[1], f[2], f[4]);
        assert_eq!(key().verify(&swapped, "POST", "/api/chat", NOW), Err(BadPrincipal::Signature), "another principal");
        let later = format!("{} {} {} {} {}", f[0], NOW + 1, f[2], f[3], f[4]);
        assert_eq!(key().verify(&later, "POST", "/api/chat", NOW), Err(BadPrincipal::Signature), "another time");
        let renamed = format!("{} {} req-2 {} {}", f[0], f[1], f[3], f[4]);
        assert_eq!(key().verify(&renamed, "POST", "/api/chat", NOW), Err(BadPrincipal::Signature), "another id");
        let v2 = h.replacen("v1", "v2", 1);
        assert_eq!(key().verify(&v2, "POST", "/api/chat", NOW), Err(BadPrincipal::Version));
        for bad in ["", "v1", "v1 x req-1 a b", &format!("{h} extra"), &h.replace("req-1", "req 1")] {
            assert!(key().verify(bad, "POST", "/api/chat", NOW).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_a_stale_header() {
        let h = key().sign(&daemon(), "GET", "/api/chat", "req-1", NOW);
        assert!(key().verify(&h, "GET", "/api/chat", NOW + SKEW_MS).is_ok());
        assert_eq!(key().verify(&h, "GET", "/api/chat", NOW + SKEW_MS + 1), Err(BadPrincipal::Stale));
        assert_eq!(key().verify(&h, "GET", "/api/chat", NOW - SKEW_MS - 1), Err(BadPrincipal::Stale), "from ahead");
    }

    /// A forwarded call's header carries a principal, a refusal or nobody,
    /// and a principal's reads as `verify` reads it.
    #[test]
    fn vouches() {
        let refused = Vouch::Not(NotVouched::Refused {
            status: 403,
            error: "cross-origin request refused".into(),
            by: Credential::Session,
        });
        for v in [Vouch::Principal(daemon()), refused.clone(), Vouch::Not(NotVouched::Nobody)] {
            let h = key().sign_vouch(&v, "GET", "/api/chat", "req-1", NOW);
            assert_eq!(key().verify_vouch(&h, "GET", "/api/chat", NOW).unwrap(), (v, "req-1".into()));
            assert_eq!(key().verify_vouch(&h, "POST", "/api/chat", NOW), Err(BadPrincipal::Signature));
        }
        let h = key().sign_vouch(&Vouch::Principal(daemon()), "GET", "/api/chat", "req-1", NOW);
        assert_eq!(key().verify(&h, "GET", "/api/chat", NOW).unwrap().principal, daemon());
        let h = key().sign_vouch(&refused, "GET", "/api/chat", "req-1", NOW);
        assert_eq!(key().verify(&h, "GET", "/api/chat", NOW), Err(BadPrincipal::Malformed), "no principal in it");
        assert_eq!(
            serde_json::to_value(&refused).unwrap(),
            serde_json::json!({ "kind": "refused", "status": 403, "error": "cross-origin request refused", "by": "session" })
        );
        assert_eq!(
            serde_json::to_value(Vouch::Not(NotVouched::Nobody)).unwrap(),
            serde_json::json!({ "kind": "nobody" })
        );
    }

    #[test]
    fn keys() {
        assert!(PrincipalKey::new(&[1; 31]).is_err(), "too short");
        let k = PrincipalKey::from_hex(&"ab".repeat(32)).unwrap();
        assert_eq!(k.0, vec![0xab; 32]);
        assert!(PrincipalKey::from_hex("zz").is_err());
        assert!(PrincipalKey::from_hex(&"é".repeat(32)).is_err());
        assert_eq!(format!("{k:?}"), "PrincipalKey(..)", "never printed");
    }
}
