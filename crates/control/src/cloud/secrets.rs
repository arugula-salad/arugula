//! Account-bound authenticated encryption for cloud credentials. The key is
//! supplied by deployment, separately from SQLite and provider credentials.
use anyhow::{Context, Result, bail};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};

pub struct SecretCipher(XChaCha20Poly1305);
impl SecretCipher {
    pub fn new(hex_key: &str) -> Result<Self> {
        let key = hex::decode(hex_key).context("cloud secret key must be 64 hex characters")?;
        if key.len() != 32 {
            bail!("cloud secret key must be 32 bytes");
        }
        Ok(Self(XChaCha20Poly1305::new_from_slice(&key).map_err(|_| anyhow::anyhow!("invalid cloud secret key"))?))
    }
    pub fn seal(&self, account: &str, name: &str, value: &str) -> Result<Vec<u8>> {
        let nonce = arugula_e2e::random::<24>();
        let aad = serde_json::to_vec(&("arugula-cloud-secret-v1", account, name))?;
        let ciphertext = self
            .0
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: value.as_bytes(), aad: &aad })
            .map_err(|_| anyhow::anyhow!("could not encrypt cloud secret"))?;
        let mut sealed = nonce.to_vec();
        sealed.extend(ciphertext);
        Ok(sealed)
    }
    pub fn open(&self, account: &str, name: &str, sealed: &[u8]) -> Result<String> {
        if sealed.len() < 40 {
            bail!("invalid encrypted cloud secret");
        }
        let aad = serde_json::to_vec(&("arugula-cloud-secret-v1", account, name))?;
        let value = self
            .0
            .decrypt(XNonce::from_slice(&sealed[..24]), Payload { msg: &sealed[24..], aad: &aad })
            .map_err(|_| anyhow::anyhow!("could not decrypt cloud secret"))?;
        String::from_utf8(value).context("invalid cloud secret encoding")
    }
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !["FOUNTAIN_", "MANAGOAT_", "ANTHROPIC_", "OPENAI_", "GOOGLE_", "ARUGULA_"]
            .iter()
            .any(|p| name.starts_with(p))
        && !matches!(name, "PATH" | "HOME" | "LD_PRELOAD" | "LD_LIBRARY_PATH" | "NODE_OPTIONS" | "BASH_ENV" | "ENV")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_values_are_bound_to_account_and_name() {
        let cipher = SecretCipher::new(&"01".repeat(32)).unwrap();
        let mut encrypted = cipher.seal("a", "GITHUB_TOKEN", "sensitive-value").unwrap();
        assert!(!encrypted.windows(15).any(|w| w == b"sensitive-value"));
        assert_eq!(cipher.open("a", "GITHUB_TOKEN", &encrypted).unwrap(), "sensitive-value");
        assert!(cipher.open("b", "GITHUB_TOKEN", &encrypted).is_err());
        assert!(cipher.open("a", "OTHER_TOKEN", &encrypted).is_err());
        encrypted[30] ^= 1;
        assert!(cipher.open("a", "GITHUB_TOKEN", &encrypted).is_err());
    }
}
