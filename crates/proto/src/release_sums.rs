//! A daemon release's `SHA256SUMS` is signed: `SHA256SUMS.sig`, beside it
//! in the release, is the Ed25519 signature of its bytes, base64 on one
//! line. release.yml's publish job makes it (`scripts/release sign-sums`)
//! with the key only that job's environment holds, so a release's files
//! are checked against sums no one else could have written.
//!
//! What checks it: `arugulad update` and the web notice's Update now
//! (`selfupdate::fetch`), install.sh
//! (with openssl, where it has Ed25519) and `scripts/release sidecars`
//! (the daemon the desktop app carries). The public key is below, and the
//! same key, as PEM, is in scripts/install.sh and scripts/release; a test
//! checks they agree.

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};

/// The signature's file in a release.
pub const SIG_FILE: &str = "SHA256SUMS.sig";

/// The key that signs releases' `SHA256SUMS`: Ed25519, raw, base64.
pub const PUBLIC_KEY: &str = "sD4YB+GbXgEt8eueJnob85euFZyEZYmLeTfOJhrdsVo=";

/// The same key as the PEM (SubjectPublicKeyInfo) the scripts give
/// openssl.
pub const PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAsD4YB+GbXgEt8eueJnob85euFZyEZYmLeTfOJhrdsVo=\n-----END PUBLIC KEY-----\n";

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// [`PUBLIC_KEY`]'s bytes.
pub fn public_key() -> [u8; 32] {
    let bytes = b64().decode(PUBLIC_KEY).expect("PUBLIC_KEY is base64");
    bytes.try_into().expect("PUBLIC_KEY is 32 bytes")
}

/// Whether `sig` (a `SHA256SUMS.sig` as downloaded) is the release key's
/// signature of `sums`; why not, if it isn't.
pub fn verify(sums: &[u8], sig: &[u8]) -> Result<(), String> {
    verify_with(&public_key(), sums, sig)
}

/// [`verify`] with another key (tests).
pub fn verify_with(key: &[u8; 32], sums: &[u8], sig: &[u8]) -> Result<(), String> {
    let key = VerifyingKey::from_bytes(key).map_err(|_| "the release key isn't an Ed25519 key".to_owned())?;
    let text = std::str::from_utf8(sig).map_err(|_| format!("{SIG_FILE} isn't text"))?;
    let raw = b64().decode(text.trim()).map_err(|_| format!("{SIG_FILE} isn't base64"))?;
    let raw: [u8; 64] = raw.try_into().map_err(|_| format!("{SIG_FILE} isn't an Ed25519 signature"))?;
    key.verify_strict(sums, &Signature::from_bytes(&raw))
        .map_err(|_| format!("SHA256SUMS isn't signed by the release key ({SIG_FILE} doesn't match)"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn sig(k: &SigningKey, msg: &[u8]) -> String {
        format!("{}\n", b64().encode(k.sign(msg).to_bytes()))
    }

    #[test]
    fn accepts_the_keys_signature_and_nothing_else() {
        let k = key(7);
        let pk = k.verifying_key().to_bytes();
        let sums = b"abc  arugula-1.0.0-x86_64-unknown-linux-musl.tar.gz\n";
        assert_eq!(verify_with(&pk, sums, sig(&k, sums).as_bytes()), Ok(()));
        // Other sums, another key, a mangled or missing signature.
        assert!(
            verify_with(&pk, b"abd  arugula-1.0.0-x86_64-unknown-linux-musl.tar.gz\n", sig(&k, sums).as_bytes())
                .is_err()
        );
        assert!(verify_with(&pk, sums, sig(&key(8), sums).as_bytes()).is_err());
        assert!(verify_with(&pk, sums, b"not base64!").is_err());
        assert!(verify_with(&pk, sums, b"").is_err());
        assert!(verify_with(&pk, sums, b64().encode([0u8; 63]).as_bytes()).is_err());
    }

    #[test]
    fn the_release_key_refuses_a_signature_by_another() {
        let sums = b"abc  f\n";
        assert!(verify(sums, sig(&key(9), sums).as_bytes()).is_err());
    }

    /// A signature made as release.yml's publish makes one (`scripts/release
    /// sign-sums`: node's crypto, base64, a newline) with the release key.
    #[test]
    fn verifies_the_release_jobs_signature() {
        let sums = b"abc  arugula-1.0.0-x86_64-unknown-linux-musl.tar.gz\n";
        let sig = b"AMK/HvcPqMkVvxAVDuQV/BO3e2bwNNCfM6C/HyinECgBjYUPxddbDVTn6ZR7TMtposixOB8+scbaV14LKk/bCA==\n";
        assert_eq!(verify(sums, sig), Ok(()));
        assert!(verify(b"abd  arugula-1.0.0-x86_64-unknown-linux-musl.tar.gz\n", sig).is_err());
    }

    /// The PEM the scripts use is the same key as [`PUBLIC_KEY`]: an
    /// Ed25519 SubjectPublicKeyInfo is a fixed 12-byte prefix and the key.
    #[test]
    fn the_pem_is_the_same_key() {
        let body: String = PUBLIC_KEY_PEM.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = b64().decode(body).unwrap();
        assert_eq!(der.len(), 44);
        assert_eq!(&der[..12], &[0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00]);
        assert_eq!(der[12..], public_key());
    }

    /// install.sh and scripts/release carry the key as PEM, for openssl and
    /// node: the same one.
    #[test]
    fn the_scripts_carry_the_same_key() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        for script in ["scripts/install.sh", "scripts/release"] {
            let text = std::fs::read_to_string(format!("{root}/{script}")).unwrap();
            assert!(text.contains(PUBLIC_KEY_PEM.trim_end()), "{script} doesn't carry the release key");
        }
    }
}
