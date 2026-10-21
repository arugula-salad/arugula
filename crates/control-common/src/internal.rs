//! Calls between control's two processes once the platform is its own
//! (M140): the platform asking the hub (`HubPort` over HTTP) and the hub
//! asking the platform (`PlatformPort` over HTTP). Neither has a public
//! address for these: the hub serves `HubPort` on a listener of its own,
//! and the platform has no public address at all. Both travel Fly's
//! private network (6PN, which Fly carries over WireGuard between hosts),
//! as plain HTTP.
//!
//! Each call is signed with the key the two share (the principal key,
//! `ARUGULA_PLATFORM_KEY`), not sent with it:
//!
//! `x-arugula-internal: v1 <ms> <nonce> <mac>`
//!
//! `<mac>` is an HMAC-SHA256 (base64url, no padding) over which process
//! is called, the method, the path and query, the time, the nonce and the
//! body's SHA-256. So a captured call is good once ([`Nonces`]), within
//! [`SKEW_MS`](crate::principal::SKEW_MS), for that one request to that
//! one process, and the key itself never crosses the network or lands in
//! a log. Its first line differs from the principal header's, so neither
//! passes for the other.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hmac::Mac as _;
use sha2::Digest as _;

use crate::principal::{BadPrincipal, PrincipalKey, SKEW_MS};

/// The header an internal call is signed in.
pub const HEADER: &str = "x-arugula-internal";

/// Which process a call is to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callee {
    /// The hub's internal API: `HubPort`.
    Hub,
    /// The platform's: `PlatformPort`.
    Platform,
}

impl Callee {
    fn as_str(self) -> &'static str {
        match self {
            Callee::Hub => "hub",
            Callee::Platform => "platform",
        }
    }
}

fn message(to: Callee, method: &str, path_and_query: &str, ms: u64, nonce: &str, body: &[u8]) -> String {
    let body = hex(&sha2::Sha256::digest(body));
    format!("arugula internal v1\n{}\n{method}\n{path_and_query}\n{ms}\n{nonce}\n{body}\n", to.as_str())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The header for a call to `to`, at `ms`.
pub fn sign(key: &PrincipalKey, to: Callee, method: &str, path_and_query: &str, body: &[u8], ms: u64) -> String {
    let nonce = hex(&arugula_e2e::random::<16>());
    let tag = key.hmac(message(to, method, path_and_query, ms, &nonce, body).as_bytes()).finalize().into_bytes();
    format!("v1 {ms} {nonce} {}", B64.encode(tag))
}

/// One call, as the callee sees it.
pub struct Call<'a> {
    pub to: Callee,
    pub method: &'a str,
    pub path_and_query: &'a str,
    pub body: &'a [u8],
}

/// Whether `header` signs this call with this key, within [`SKEW_MS`] of
/// `now_ms`, and its nonce hasn't been taken before.
pub fn verify(key: &PrincipalKey, nonces: &Nonces, header: &str, call: &Call, now_ms: u64) -> Result<(), BadPrincipal> {
    let Call { to, method, path_and_query, body } = *call;
    let f: Vec<&str> = header.split(' ').collect();
    let [v, ms, nonce, tag] = f.as_slice() else { return Err(BadPrincipal::Malformed) };
    if *v != "v1" {
        return Err(BadPrincipal::Version);
    }
    let ms: u64 = ms.parse().map_err(|_| BadPrincipal::Malformed)?;
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(BadPrincipal::Malformed);
    }
    let tag = B64.decode(tag).map_err(|_| BadPrincipal::Malformed)?;
    key.hmac(message(to, method, path_and_query, ms, nonce, body).as_bytes())
        .verify_slice(&tag)
        .map_err(|_| BadPrincipal::Signature)?;
    if ms.abs_diff(now_ms) > SKEW_MS {
        return Err(BadPrincipal::Stale);
    }
    if !nonces.first(nonce, now_ms) {
        return Err(BadPrincipal::Replayed);
    }
    Ok(())
}

/// Nonces already taken, each kept as long as its call could still be
/// fresh.
#[derive(Default)]
pub struct Nonces(std::sync::Mutex<std::collections::HashMap<String, u64>>);

impl Nonces {
    fn first(&self, nonce: &str, now_ms: u64) -> bool {
        let mut m = self.0.lock().unwrap();
        if m.len() > 10_000 {
            m.retain(|_, until| *until > now_ms);
        }
        match m.get(nonce) {
            Some(until) if *until > now_ms => false,
            _ => {
                m.insert(nonce.to_owned(), now_ms + 2 * SKEW_MS);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_000_000_000;

    fn key() -> PrincipalKey {
        PrincipalKey::new(&[7; 32]).unwrap()
    }

    #[test]
    fn a_call_is_good_once_for_itself() {
        let n = Nonces::default();
        let h = sign(&key(), Callee::Hub, "POST", "/internal/hub/v1/teams", b"{}", NOW);
        let check = |h: &str, to, path_and_query: &str, body: &[u8], now| {
            verify(&key(), &n, h, &Call { to, method: "POST", path_and_query, body }, now)
        };
        assert_eq!(check(&h, Callee::Platform, "/internal/hub/v1/teams", b"{}", NOW), Err(BadPrincipal::Signature));
        assert_eq!(check(&h, Callee::Hub, "/internal/hub/v1/daemons", b"{}", NOW), Err(BadPrincipal::Signature));
        assert_eq!(check(&h, Callee::Hub, "/internal/hub/v1/teams", b"{ }", NOW), Err(BadPrincipal::Signature));
        assert_eq!(
            check(&h, Callee::Hub, "/internal/hub/v1/teams", b"{}", NOW + SKEW_MS + 1),
            Err(BadPrincipal::Stale)
        );
        assert_eq!(check(&h, Callee::Hub, "/internal/hub/v1/teams", b"{}", NOW + 1), Ok(()));
        assert_eq!(check(&h, Callee::Hub, "/internal/hub/v1/teams", b"{}", NOW + 2), Err(BadPrincipal::Replayed));
        let other = PrincipalKey::new(&[8; 32]).unwrap();
        let h = sign(&other, Callee::Hub, "POST", "/internal/hub/v1/teams", b"{}", NOW);
        assert_eq!(check(&h, Callee::Hub, "/internal/hub/v1/teams", b"{}", NOW), Err(BadPrincipal::Signature));
        for bad in ["", "v1", "v2 1 00000000000000000000000000000000 x", "v1 x y z"] {
            assert!(check(bad, Callee::Hub, "/internal/hub/v1/teams", b"{}", NOW).is_err(), "{bad:?}");
        }
    }

    /// A principal header's MAC is no internal call's, under the same key.
    #[test]
    fn a_principal_header_is_no_call() {
        let p = crate::Principal::Person { account: "a".into() };
        let h = key().sign(&p, "POST", "/internal/hub/v1/teams", "req-1", NOW);
        let mac = h.rsplit(' ').next().unwrap();
        let forged = format!("v1 {NOW} {} {mac}", "0".repeat(32));
        let n = Nonces::default();
        assert_eq!(
            verify(
                &key(),
                &n,
                &forged,
                &Call { to: Callee::Hub, method: "POST", path_and_query: "/internal/hub/v1/teams", body: b"" },
                NOW
            ),
            Err(BadPrincipal::Signature)
        );
    }
}
