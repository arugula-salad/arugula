//! S38 prototype (#979, throwaway): a conversation's record sealed for
//! control, which stores it and can't read it.
//!
//! - **One key per conversation** (32 random bytes, AES-256-GCM). Each
//!   entry of its log, and its record, is sealed under it with a fresh
//!   nonce. The additional data names the conversation and what the bytes
//!   are, so control can't move an entry from one conversation to another,
//!   or pass a record off as a message.
//! - **Wrapped to each member device's X25519 key** (the Noise key in its
//!   certificate): an ephemeral X25519 key, HKDF-SHA256 over the shared
//!   secret, and AES-256-GCM over the conversation key. Only that device's
//!   private key unwraps it. Control keeps the wrapped copies, by device.
//!
//! This is the "per-conversation key" side of S38's question 1. A product
//! version would also rotate the key on a removal (an epoch), sign each
//! entry with the author's device key, and chain entries by hash, so
//! control can't reorder or drop one unseen: see the spike's README.

use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, aead::Payload};
use anyhow::{Context, bail};
use curve25519_dalek::MontgomeryPoint;
use hkdf::Hkdf;
use sha2::Sha256;

const SEAL: &str = "arugula conv seal v1";
const WRAP: &str = "arugula conv key wrap v1";

/// A new conversation key.
pub fn new_key() -> [u8; 32] {
    crate::random()
}

fn aad(conv: &str, kind: &str) -> Vec<u8> {
    format!("{SEAL}\nconv {conv}\nkind {kind}\n").into_bytes()
}

/// `plain` sealed under `key`, for conversation `conv`, as a `kind` (`msg`,
/// `record`): `nonce (12) ‖ ciphertext and tag`.
pub fn seal(key: &[u8; 32], conv: &str, kind: &str, plain: &[u8]) -> Vec<u8> {
    let nonce: [u8; 12] = crate::random();
    let c = Aes256Gcm::new_from_slice(key).expect("a 32-byte key");
    let a = aad(conv, kind);
    let ct = c.encrypt(&nonce.into(), Payload { msg: plain, aad: &a }).expect("aes-gcm seals");
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    out
}

/// What [`seal`] sealed, or an error if it isn't that conversation's `kind`
/// under `key`.
pub fn open(key: &[u8; 32], conv: &str, kind: &str, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
    if sealed.len() < 12 + 16 {
        bail!("too short to be sealed");
    }
    let (nonce, ct) = sealed.split_at(12);
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let c = Aes256Gcm::new_from_slice(key).expect("a 32-byte key");
    let a = aad(conv, kind);
    c.decrypt(&nonce.into(), Payload { msg: ct, aad: &a }).map_err(|_| anyhow::anyhow!("doesn't open: not this key"))
}

fn kek(shared: &[u8; 32], eph: &[u8; 32], to: &[u8; 32], conv: &str, device: &str) -> anyhow::Result<[u8; 32]> {
    if shared.iter().all(|b| *b == 0) {
        bail!("a low-order key");
    }
    let mut salt = eph.to_vec();
    salt.extend_from_slice(to);
    let info = format!("{WRAP}\nconv {conv}\ndevice {device}\n");
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&salt), shared).expand(info.as_bytes(), &mut out).map_err(|_| anyhow::anyhow!("hkdf"))?;
    Ok(out)
}

/// `key` wrapped to device `device`'s X25519 public key `to`:
/// `ephemeral public (32) ‖ nonce (12) ‖ ciphertext and tag`.
pub fn wrap(key: &[u8; 32], conv: &str, device: &str, to: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
    let e: [u8; 32] = crate::random();
    let eph = MontgomeryPoint::mul_base_clamped(e).to_bytes();
    let shared = MontgomeryPoint(*to).mul_clamped(e).to_bytes();
    let k = kek(&shared, &eph, to, conv, device)?;
    let nonce: [u8; 12] = crate::random();
    let ct = Aes256Gcm::new_from_slice(&k)
        .expect("a 32-byte key")
        .encrypt(&nonce.into(), key.as_slice())
        .map_err(|_| anyhow::anyhow!("aes-gcm"))?;
    let mut out = eph.to_vec();
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// The conversation key from what [`wrap`] made for this device.
pub fn unwrap(
    wrapped: &[u8],
    conv: &str,
    device: &str,
    private: &[u8; 32],
    public: &[u8; 32],
) -> anyhow::Result<[u8; 32]> {
    if wrapped.len() != 32 + 12 + 32 + 16 {
        bail!("not a wrapped key");
    }
    let eph: [u8; 32] = wrapped[..32].try_into().unwrap();
    let nonce: [u8; 12] = wrapped[32..44].try_into().unwrap();
    let shared = MontgomeryPoint(eph).mul_clamped(*private).to_bytes();
    let k = kek(&shared, &eph, public, conv, device)?;
    let key = Aes256Gcm::new_from_slice(&k)
        .expect("a 32-byte key")
        .decrypt(&nonce.into(), &wrapped[44..])
        .map_err(|_| anyhow::anyhow!("not wrapped for this device"))?;
    key.try_into().ok().context("a key of the wrong size")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKeys;

    #[test]
    fn a_member_device_unwraps_the_key_and_opens_what_it_sealed_and_nobody_else_does() {
        let (me, other) = (DeviceKeys::generate(), DeviceKeys::generate());
        let k = new_key();
        let w = wrap(&k, "c-1", &me.id(), &me.noise_public).unwrap();
        assert_eq!(unwrap(&w, "c-1", &me.id(), &me.noise_private, &me.noise_public).unwrap(), k);
        assert!(unwrap(&w, "c-1", &other.id(), &other.noise_private, &other.noise_public).is_err());
        assert!(unwrap(&w, "c-2", &me.id(), &me.noise_private, &me.noise_public).is_err(), "another conversation's");

        let s = seal(&k, "c-1", "msg", b"run the tests");
        assert!(!s.windows(3).any(|w| w == b"run"));
        assert_eq!(open(&k, "c-1", "msg", &s).unwrap(), b"run the tests");
        assert!(open(&k, "c-2", "msg", &s).is_err(), "moved to another conversation");
        assert!(open(&k, "c-1", "record", &s).is_err(), "passed off as a record");
        assert!(open(&new_key(), "c-1", "msg", &s).is_err());
    }
}
