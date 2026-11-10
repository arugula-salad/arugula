//! Keys kept in the state file, so a later run reuses its accounts and
//! machines instead of signing up and enrolling them again.

use arugula_e2e::{DeviceKeys, keys::hex32};

pub fn to_text(k: &DeviceKeys) -> String {
    format!("{} {} {}", hex::encode(k.noise_private), hex::encode(k.noise_public), hex::encode(k.sign.to_bytes()))
}

pub fn from_text(s: &str) -> anyhow::Result<DeviceKeys> {
    let f: Vec<&str> = s.split_whitespace().collect();
    let [private, public, seed] = f.as_slice() else { anyhow::bail!("bad keys") };
    Ok(DeviceKeys {
        noise_private: hex32(private)?,
        noise_public: hex32(public)?,
        sign: ed25519_dalek::SigningKey::from_bytes(&hex32(seed)?),
    })
}
