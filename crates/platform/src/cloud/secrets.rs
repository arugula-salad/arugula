//! The platform's master key, `ARUGULA_CLOUD_SECRET_KEY`, supplied by
//! deployment separately from the database and provider credentials.
//!
//! Since M209 it seals one thing: each project's data key ([`wrap`]), and
//! the data key seals the project's secrets ([`crate::keys`]). Before
//! M209 it sealed secrets itself, bound to an owner and a name
//! ([`SecretCipher::seal`]); such rows are still opened, and re-sealed
//! with their project's key ([`crate::keys::reseal`]).
//!
//! While the master key is rotated (docs/control-ops.md) the server has
//! the old one too, as `ARUGULA_CLOUD_SECRET_KEY_PREVIOUS`: it opens with
//! either and seals only with the new one.
//!
//! [`wrap`]: SecretCipher::wrap
use anyhow::{Context, Result, bail};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use sha2::{Digest as _, Sha256};

/// One master key and its id ([`SecretCipher::id`]).
#[derive(Clone)]
struct Master {
    aead: XChaCha20Poly1305,
    id: String,
}

impl Master {
    fn new(hex_key: &str) -> Result<Self> {
        let key = hex::decode(hex_key.trim()).context("cloud secret key must be 64 hex characters")?;
        if key.len() != 32 {
            bail!("cloud secret key must be 32 bytes");
        }
        let id = hex::encode(&Sha256::digest([b"arugula-master-key-id-v1\0".as_slice(), &key].concat())[..8]);
        let aead = XChaCha20Poly1305::new_from_slice(&key).map_err(|_| anyhow::anyhow!("invalid cloud secret key"))?;
        Ok(Self { aead, id })
    }
}

pub struct SecretCipher {
    current: Master,
    previous: Option<Master>,
}

impl SecretCipher {
    pub fn new(hex_key: &str) -> Result<Self> {
        Ok(Self { current: Master::new(hex_key)?, previous: None })
    }

    /// This key, opening with `previous` too (a rotation's old key) when
    /// it's given and not empty.
    pub fn with_previous(self, previous: Option<&str>) -> Result<Self> {
        let previous = previous.filter(|k| !k.trim().is_empty()).map(Master::new).transpose()?;
        anyhow::ensure!(
            previous.as_ref().is_none_or(|p| p.id != self.current.id),
            "ARUGULA_CLOUD_SECRET_KEY_PREVIOUS is the same key as ARUGULA_CLOUD_SECRET_KEY"
        );
        Ok(Self { previous, ..self })
    }

    /// The current key alone, without the previous one.
    pub fn without_previous(&self) -> Self {
        Self { current: self.current.clone(), previous: None }
    }

    /// The current key's id: 16 hex characters of a hash of it, which
    /// says which key wrapped a data key without saying anything of the
    /// key.
    pub fn id(&self) -> &str {
        &self.current.id
    }

    /// The previous key's id, while there is one.
    pub fn previous_id(&self) -> Option<&str> {
        self.previous.as_ref().map(|p| p.id.as_str())
    }

    /// Seal `data_key`, version `version` of `project`'s, with the current
    /// key (bound to both).
    pub(crate) fn wrap(&self, project: &str, version: i64, data_key: &[u8; 32]) -> Result<Vec<u8>> {
        let aad = serde_json::to_vec(&("arugula-project-key-v1", project, version))?;
        seal_with(&self.current.aead, &aad, data_key)
    }

    /// Open a data key that the master key with id `master` wrapped:
    /// the current key or the previous one.
    pub(crate) fn unwrap(&self, master: &str, project: &str, version: i64, wrapped: &[u8]) -> Result<[u8; 32]> {
        let key = std::iter::once(&self.current)
            .chain(&self.previous)
            .find(|k| k.id == master)
            .with_context(|| format!("project {project}'s key is wrapped by master key {master}, which this isn't"))?;
        let aad = serde_json::to_vec(&("arugula-project-key-v1", project, version))?;
        let key = open_with(&key.aead, &aad, wrapped).context("could not open a project's key")?;
        key.try_into().map_err(|_| anyhow::anyhow!("a project's key isn't 32 bytes"))
    }

    /// Seal a secret with the master key itself: the scheme before M209,
    /// which nothing writes now but tests of the re-seal.
    pub fn seal(&self, account: &str, name: &str, value: &str) -> Result<Vec<u8>> {
        let aad = serde_json::to_vec(&("arugula-cloud-secret-v1", account, name))?;
        seal_with(&self.current.aead, &aad, value.as_bytes()).context("could not encrypt cloud secret")
    }

    /// Open a secret sealed with the master key itself ([`seal`](Self::seal)),
    /// with the current key or the previous one.
    pub fn open(&self, account: &str, name: &str, sealed: &[u8]) -> Result<String> {
        let aad = serde_json::to_vec(&("arugula-cloud-secret-v1", account, name))?;
        let value = std::iter::once(&self.current)
            .chain(&self.previous)
            .find_map(|k| open_with(&k.aead, &aad, sealed).ok())
            .context("could not decrypt cloud secret")?;
        String::from_utf8(value).context("invalid cloud secret encoding")
    }
}

/// A random nonce, then `msg` sealed with `aead` and bound to `aad`.
pub(crate) fn seal_with(aead: &XChaCha20Poly1305, aad: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
    let nonce = arugula_e2e::random::<24>();
    let ciphertext = aead
        .encrypt(XNonce::from_slice(&nonce), Payload { msg, aad })
        .map_err(|_| anyhow::anyhow!("could not encrypt"))?;
    let mut sealed = nonce.to_vec();
    sealed.extend(ciphertext);
    Ok(sealed)
}

/// What [`seal_with`] sealed.
pub(crate) fn open_with(aead: &XChaCha20Poly1305, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < 40 {
        bail!("invalid encrypted value");
    }
    aead.decrypt(XNonce::from_slice(&sealed[..24]), Payload { msg: &sealed[24..], aad })
        .map_err(|_| anyhow::anyhow!("could not decrypt"))
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

    #[test]
    fn a_previous_key_still_opens_and_the_current_one_seals() {
        let (old, new) = ("01".repeat(32), "02".repeat(32));
        let sealed = SecretCipher::new(&old).unwrap().seal("a", "T", "v").unwrap();
        let both = SecretCipher::new(&new).unwrap().with_previous(Some(&old)).unwrap();
        assert_eq!(both.open("a", "T", &sealed).unwrap(), "v");
        assert!(SecretCipher::new(&new).unwrap().open("a", "T", &sealed).is_err());
        assert_eq!(both.id(), SecretCipher::new(&new).unwrap().id());
        assert_eq!(both.previous_id(), Some(SecretCipher::new(&old).unwrap().id()));
        assert_eq!(both.id().len(), 16);
        assert!(SecretCipher::new(&new).unwrap().with_previous(Some(&new)).is_err());
        let wrapped = both.wrap("prj_a", 1, &[7; 32]).unwrap();
        assert_eq!(both.unwrap(both.id(), "prj_a", 1, &wrapped).unwrap(), [7; 32]);
        assert!(both.unwrap(both.id(), "prj_b", 1, &wrapped).is_err(), "bound to its project");
        assert!(both.unwrap(both.id(), "prj_a", 2, &wrapped).is_err(), "and its version");
        assert!(both.unwrap("0000000000000000", "prj_a", 1, &wrapped).is_err(), "and its master key");
    }
}
