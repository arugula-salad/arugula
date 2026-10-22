//! Public previews (M187, aru-097): a port a project's member made public
//! in a hosted workspace, opened by anyone at `<preview id>.<preview
//! domain>` with no device key.
//!
//! A stranger's browser has nothing to sign a block grant with, so control
//! itself opens the channel: it terminates HTTPS for the preview domain and
//! brings each request to the hosted machine over its relay socket, on a
//! block channel ([`crate::block`]) whose message 1 carries a [`PublicHello`]
//! instead of a [`crate::block::Hello`]. The grant is signed with control's
//! hosted key (the one the project's delegation names, which the machine
//! already trusts for its own certificate) and names one port and the
//! preview's id, never a block:
//!
//! ```text
//! arugula public preview grant v1
//! daemon <the hosted machine's device id>
//! port <the port, 1-65535>
//! preview <the preview's id>
//! key <the one-off key's X25519 public half, hex>
//! expires <ms since the epoch>
//! ```
//!
//! The machine lets it in ([`PublicGrant::check`]) only when it's signed
//! with that key, for this machine and the key doing the handshake, not
//! expired and good for at most [`crate::block::MAX_LIFE_MS`], and when the
//! platform says now (asked through control, cached briefly) that the port
//! is public under that id. So control can't open any other port, and a
//! port made private stops serving within a minute even if control's own
//! cache lags. Control sees the page in plain text: it's public.

use anyhow::Context;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::{
    block::{Expect, MAX_LIFE_MS, Refused},
    hosted::HostedKey,
    keys::hex32,
};

/// What a public preview's channel's message 1 carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicHello {
    pub public: PublicGrant,
}

impl PublicHello {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize hello")
    }

    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(payload).context("a public preview's hello")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicGrant {
    pub daemon: String,
    pub port: u16,
    /// The preview's id: the label of its host under the preview domain.
    pub preview: String,
    /// The one-off key's X25519 public half, hex.
    pub key: String,
    pub expires: u64,
    /// Ed25519 signature with control's hosted key over [`PublicGrant::body`], hex.
    pub sig: String,
}

impl PublicGrant {
    /// A grant for `key` to open `port` on `daemon` as preview `preview`,
    /// until `expires`, signed with control's hosted key.
    pub fn new(signer: &HostedKey, daemon: &str, port: u16, preview: &str, key: &[u8; 32], expires: u64) -> Self {
        let mut g = Self {
            daemon: daemon.to_owned(),
            port,
            preview: preview.to_owned(),
            key: hex::encode(key),
            expires,
            sig: String::new(),
        };
        g.sig = hex::encode(signer.sign_bytes(g.body().as_bytes()));
        g
    }

    pub fn body(&self) -> String {
        // Frozen once shipped: see `frozen.rs`.
        format!(
            "arugula public preview grant v1\ndaemon {}\nport {}\npreview {}\nkey {}\nexpires {}\n",
            self.daemon, self.port, self.preview, self.key, self.expires
        )
    }

    /// Whether this grant lets a channel in. `hosted_key` is control's
    /// hosted key as the machine's delegation names it (hex), or `None` on
    /// a machine that isn't hosted; `public` says whether a port is public
    /// under a preview id now. As for block grants, what needs no lookup
    /// comes first, then the signature, then expiry and the port.
    pub fn check(
        &self,
        want: &Expect<'_>,
        hosted_key: Option<&str>,
        public: impl FnOnce(u16, &str) -> bool,
    ) -> Result<(), Refused> {
        if self.daemon != want.daemon {
            return Err(Refused::OtherDaemon);
        }
        let key = hex32(&self.key).map_err(|_| Refused::Malformed)?;
        if key != *want.remote {
            return Err(Refused::OtherKey);
        }
        let label = |s: &str| !s.is_empty() && s.len() <= 63 && s.bytes().all(|b| b.is_ascii_alphanumeric());
        if self.port == 0 || !label(&self.preview) {
            return Err(Refused::Malformed);
        }
        let sign = hosted_key.and_then(|k| hex32(k).ok()).ok_or(Refused::Untrusted)?;
        let sig: [u8; 64] = hex::decode(&self.sig).ok().and_then(|s| s.try_into().ok()).ok_or(Refused::Signature)?;
        VerifyingKey::from_bytes(&sign)
            .and_then(|k| k.verify(self.body().as_bytes(), &Signature::from_bytes(&sig)))
            .map_err(|_| Refused::Signature)?;
        if self.expires <= want.now {
            return Err(Refused::Expired);
        }
        if self.expires - want.now > MAX_LIFE_MS {
            return Err(Refused::TooLong);
        }
        if !public(self.port, &self.preview) {
            return Err(Refused::NotPublic);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::DeviceKeys;

    const NOW: u64 = 1_800_000_000_000;
    const ID: &str = "0123456789abcdef0123456789abcdef";

    struct World {
        control: HostedKey,
        daemon: DeviceKeys,
        key: DeviceKeys,
    }

    impl World {
        fn new() -> Self {
            Self { control: HostedKey::generate(), daemon: DeviceKeys::generate(), key: DeviceKeys::generate() }
        }

        fn grant(&self, port: u16, expires: u64) -> PublicGrant {
            PublicGrant::new(&self.control, &self.daemon.id(), port, ID, &self.key.noise_public, expires)
        }

        /// As the machine checks it: port 5173 is public as `ID`.
        fn check_with(&self, g: &PublicGrant, hosted_key: Option<&str>) -> Result<(), Refused> {
            g.check(
                &Expect { daemon: &self.daemon.id(), remote: &self.key.noise_public, now: NOW },
                hosted_key,
                |port, id| port == 5173 && id == ID,
            )
        }

        fn check(&self, g: &PublicGrant) -> Result<(), Refused> {
            self.check_with(g, Some(&self.control.public()))
        }
    }

    #[test]
    fn a_public_port_lets_in() {
        let w = World::new();
        assert_eq!(w.check(&w.grant(5173, NOW + 60_000)), Ok(()));
    }

    #[test]
    fn only_the_public_port_under_its_id() {
        let w = World::new();
        assert_eq!(w.check(&w.grant(22, NOW + 60_000)), Err(Refused::NotPublic));
        let g = PublicGrant::new(&w.control, &w.daemon.id(), 5173, "ffff", &w.key.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::NotPublic));
        // Another port under the public one's signature.
        let mut g = w.grant(22, NOW + 60_000);
        g.port = 5173;
        assert_eq!(w.check(&g), Err(Refused::Signature));
    }

    #[test]
    fn only_controls_hosted_key() {
        let w = World::new();
        let other = HostedKey::generate();
        let g = PublicGrant::new(&other, &w.daemon.id(), 5173, ID, &w.key.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::Signature));
        // A machine that isn't hosted serves no public previews.
        assert_eq!(w.check_with(&w.grant(5173, NOW + 60_000), None), Err(Refused::Untrusted));
    }

    #[test]
    fn expiry_daemon_and_key() {
        let w = World::new();
        assert_eq!(w.check(&w.grant(5173, NOW)), Err(Refused::Expired));
        assert_eq!(w.check(&w.grant(5173, NOW + MAX_LIFE_MS + 1)), Err(Refused::TooLong));
        let g = PublicGrant::new(&w.control, "other", 5173, ID, &w.key.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::OtherDaemon));
        let thief = DeviceKeys::generate();
        let g = PublicGrant::new(&w.control, &w.daemon.id(), 5173, ID, &thief.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::OtherKey));
        // An id that would add a line to the body.
        let g = PublicGrant::new(&w.control, &w.daemon.id(), 5173, "a\nport 22", &w.key.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::Malformed));
    }

    #[test]
    fn hellos_tell_apart() {
        let w = World::new();
        let h = PublicHello { public: w.grant(5173, NOW + 1) };
        assert_eq!(PublicHello::decode(&h.encode()).unwrap(), h);
        // A block channel's hello isn't a public one, nor the other way.
        assert!(crate::block::Hello::decode(&h.encode()).is_err());
        assert!(h.public.body().starts_with("arugula public preview grant v1\n"));
    }
}
