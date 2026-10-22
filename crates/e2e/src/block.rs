//! Block channels (M50, #150): what lets a block's frame, on an origin of
//! its own that holds no device key, open a channel to one block.
//!
//! **The grant.** Control's page holds the device key. For a block it makes
//! a one-off X25519 key (the block key) and signs a grant for it with the
//! device's Ed25519 key. The frame does the Noise handshake with the block
//! key as its static key and sends the grant in message 1's payload (as
//! [`Hello`]), so the daemon decides before it answers: one round trip, no
//! transport message first. What's signed is a fixed line format, as
//! certificates are, so WebCrypto and Rust sign the same bytes:
//!
//! ```text
//! arugula block grant v1
//! daemon <the daemon's device id>
//! block <the block's id on that daemon>
//! key <the block key's X25519 public half, hex>
//! expires <ms since the epoch, by the daemon's clock>
//! by <the signing device's id>
//! ```
//!
//! The daemon lets a channel in ([`Grant::check`]) only when the grant is
//! for this daemon, for the key doing the handshake, signed by a device it
//! trusts to open channels, not expired by its own clock and not good for
//! more than [`MAX_LIFE_MS`] from now, for a block it has. The page mints
//! `expires` from the daemon's clock, which it learns over its own device
//! channel, never from its own clock (a device a day off couldn't open
//! blocks) and never from the frame (block code could stretch its grant).
//!
//! Expiry gates new channels only: an open channel lasts until it closes,
//! the block closes or the device stops being trusted.
//!
//! **The answer** is message 2's payload, an [`Answer`]: `{"ok":true}`, or
//! `{"ok":false,"refused":"expired","message":"…"}`, and then the daemon
//! closes. It's encrypted to the block key, so only the grant's holder
//! learns why. The refusal and the close can arrive together, so a client
//! reads message 2 before it treats a close as "closed during the
//! handshake": a refused `expired` means "ask for a new grant".
//!
//! **Replay.** Message 1 is encrypted to the daemon's static key only, so
//! anyone who saw it can send it again, and the daemon answers. But the
//! answer and every transport message after it need the block key's
//! private half (Noise's `se`), so a replay opens nothing.

use std::fmt;

use anyhow::Context;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::keys::{DeviceKeys, hex32};

/// The longest a grant may be good for, from the daemon's now. Grants are
/// meant to be short and renewed: one copied out of a block's storage
/// works only this long.
pub const MAX_LIFE_MS: u64 = 10 * 60 * 1000;

/// What a block channel's message 1 carries. A device's own channel sends
/// an empty payload instead, which is how the daemon tells them apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub grant: Grant,
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize hello")
    }

    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(payload).context("a block channel's hello")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub daemon: String,
    pub block: String,
    /// The block key's X25519 public half, hex.
    pub key: String,
    /// Ms since the epoch, by the daemon's clock.
    pub expires: u64,
    /// The signing device's id.
    pub by: String,
    /// Ed25519 signature by `by` over [`Grant::body`], hex.
    pub sig: String,
}

/// Why a grant didn't let a channel in. The code is what [`Answer`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refused {
    /// Message 1's payload isn't a [`Hello`].
    Malformed,
    /// For another daemon.
    OtherDaemon,
    /// For another key than the one doing the handshake.
    OtherKey,
    /// Signed by a device this daemon doesn't trust (or that may not open
    /// channels).
    Untrusted,
    /// The signature doesn't match the grant.
    Signature,
    /// Expired by the daemon's clock: ask for a new grant.
    Expired,
    /// Good for longer than [`MAX_LIFE_MS`].
    TooLong,
    /// This daemon has no such block (or it isn't one a channel can show).
    NoBlock,
    /// A public preview's port isn't public under that id now (M187).
    NotPublic,
}

impl Refused {
    pub fn code(self) -> &'static str {
        match self {
            Refused::Malformed => "malformed",
            Refused::OtherDaemon => "other-daemon",
            Refused::OtherKey => "other-key",
            Refused::Untrusted => "untrusted",
            Refused::Signature => "signature",
            Refused::Expired => "expired",
            Refused::TooLong => "too-long",
            Refused::NoBlock => "no-block",
            Refused::NotPublic => "not-public",
        }
    }

    /// For a person reading a log or an error.
    pub fn message(self) -> &'static str {
        match self {
            Refused::Malformed => "not a block grant",
            Refused::OtherDaemon => "the grant is for another machine",
            Refused::OtherKey => "the grant is for another key",
            Refused::Untrusted => "the grant is signed by a device this machine doesn't trust",
            Refused::Signature => "the grant's signature doesn't match",
            Refused::Expired => "the grant expired",
            Refused::TooLong => "the grant is good for too long",
            Refused::NoBlock => "this machine has no such block",
            Refused::NotPublic => "that port isn't public",
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Refused {}

/// What the daemon checks a grant against.
pub struct Expect<'a> {
    /// This daemon's device id.
    pub daemon: &'a str,
    /// The handshake's static key (the block key, if the grant is honest).
    pub remote: &'a [u8; 32],
    /// The daemon's clock, ms since the epoch.
    pub now: u64,
}

impl Grant {
    /// A grant for `key` to open `block` on `daemon` until `expires`,
    /// signed by `signer`.
    pub fn new(signer: &DeviceKeys, daemon: &str, block: &str, key: &[u8; 32], expires: u64) -> Self {
        let mut g = Self {
            daemon: daemon.to_owned(),
            block: block.to_owned(),
            key: hex::encode(key),
            expires,
            by: signer.id(),
            sig: String::new(),
        };
        g.sig = hex::encode(signer.signature(g.body().as_bytes()));
        g
    }

    pub fn body(&self) -> String {
        // Frozen once shipped: see `frozen.rs`.
        format!(
            "arugula block grant v1\ndaemon {}\nblock {}\nkey {}\nexpires {}\nby {}\n",
            self.daemon, self.block, self.key, self.expires, self.by
        )
    }

    /// Whether this grant lets a channel in. `signer` gives the Ed25519 key
    /// of a device id this daemon trusts to open channels (`None` for any
    /// other); `has_block` says whether a block id is one it can show.
    ///
    /// Everything that needs no lookup comes first, then the signature,
    /// and only then expiry and the block, so a grant nobody trusted signed
    /// learns nothing about this daemon's clock or blocks.
    pub fn check(
        &self,
        want: &Expect<'_>,
        signer: impl FnOnce(&str) -> Option<[u8; 32]>,
        has_block: impl FnOnce(&str) -> bool,
    ) -> Result<(), Refused> {
        if self.daemon != want.daemon {
            return Err(Refused::OtherDaemon);
        }
        let key = hex32(&self.key).map_err(|_| Refused::Malformed)?;
        if key != *want.remote {
            return Err(Refused::OtherKey);
        }
        // Only what the format can carry: a newline would let one grant's
        // body read as another's.
        let line = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control);
        if !line(&self.block) || !line(&self.by) {
            return Err(Refused::Malformed);
        }
        let sign = signer(&self.by).ok_or(Refused::Untrusted)?;
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
        if !has_block(&self.block) {
            return Err(Refused::NoBlock);
        }
        Ok(())
    }
}

/// Message 2's payload on a block channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refused>,
    /// For a person; [`Refused::message`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Answer {
    pub fn ok() -> Self {
        Self { ok: true, refused: None, message: None }
    }

    pub fn refused(why: Refused) -> Self {
        Self { ok: false, refused: Some(why), message: Some(why.message().to_owned()) }
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize answer")
    }

    /// From message 2's payload. `Err` with the refusal if it is one; a
    /// refusal of a kind this build doesn't know is [`Refused::Malformed`].
    pub fn decode(payload: &[u8]) -> Result<(), Refused> {
        let v: serde_json::Value = serde_json::from_slice(payload).map_err(|_| Refused::Malformed)?;
        if v["ok"] == serde_json::Value::Bool(true) {
            return Ok(());
        }
        Err(serde_json::from_value(v["refused"].clone()).unwrap_or(Refused::Malformed))
    }
}

/// What a block channel carries once it's open (S27's wire, unchanged).
///
/// Every message is a [`crate::channel::Msg::Binary`] holding one frame:
/// `kind (1 byte) ‖ stream (u32 BE) ‖ payload`. The client opens streams,
/// with ids it picks, and the daemon only answers. Many streams share one
/// channel: a page's requests in parallel, and its WebSockets.
///
/// ```text
/// client to daemon                 daemon to client
/// 'H' request head (ReqHead)       'R' response head (ResHead)
/// 'D' body bytes                   'D' body bytes
/// 'E' end of body                  'E' end of body
/// 'O' WebSocket open (WsOpen)      'A' WebSocket accepted ({"protocol"})
/// 'M' message: 0 text / 1 binary   'M' message
/// 'C' close (WsClose)              'C' close (WsClose)
/// 'K' cancel the stream            'X' failed ({"message"})
/// ```
///
/// A request's body comes whole before the daemon sends it on ('H', any
/// 'D's, 'E'); the answer streams. Heads are JSON.
pub mod wire {
    use anyhow::bail;
    use serde::{Deserialize, Serialize};

    pub const HEAD: u8 = b'H';
    pub const DATA: u8 = b'D';
    pub const END: u8 = b'E';
    pub const OPEN: u8 = b'O';
    pub const MESSAGE: u8 = b'M';
    pub const CLOSE: u8 = b'C';
    pub const CANCEL: u8 = b'K';
    pub const RESPONSE: u8 = b'R';
    pub const ACCEPTED: u8 = b'A';
    pub const FAILED: u8 = b'X';

    /// A request, as the page made it: its path and query on the block's
    /// origin, and its headers (a name may repeat). Browsers don't show a
    /// worker `Sec-Fetch-*`, so the worker says what the request is with
    /// `sec-fetch-mode` and `sec-fetch-dest` from its `mode` and
    /// `destination`: `navigate` and `iframe` tell the daemon where the
    /// frame is (`sites.rs`) and that a page may get its head script.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ReqHead {
        pub method: String,
        pub path: String,
        #[serde(default)]
        pub headers: Vec<(String, String)>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ResHead {
        pub status: u16,
        #[serde(default)]
        pub headers: Vec<(String, String)>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct WsOpen {
        pub path: String,
        #[serde(default)]
        pub protocols: Vec<String>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct WsClose {
        #[serde(default)]
        pub code: u16,
        #[serde(default)]
        pub reason: String,
    }

    pub fn frame(kind: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let mut f = Vec::with_capacity(5 + payload.len());
        f.push(kind);
        f.extend_from_slice(&stream.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    pub fn json_frame<T: Serialize>(kind: u8, stream: u32, v: &T) -> Vec<u8> {
        frame(kind, stream, &serde_json::to_vec(v).expect("serialize"))
    }

    /// `(kind, stream, payload)`.
    pub fn parse(f: &[u8]) -> anyhow::Result<(u8, u32, &[u8])> {
        if f.len() < 5 {
            bail!("short frame");
        }
        Ok((f[0], u32::from_be_bytes([f[1], f[2], f[3], f[4]]), &f[5..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{Initiator, Responder, prologue};

    const NOW: u64 = 1_800_000_000_000;

    struct World {
        daemon: DeviceKeys,
        device: DeviceKeys,
        block_key: DeviceKeys,
    }

    impl World {
        fn new() -> Self {
            Self { daemon: DeviceKeys::generate(), device: DeviceKeys::generate(), block_key: DeviceKeys::generate() }
        }

        fn grant(&self, block: &str, expires: u64) -> Grant {
            Grant::new(&self.device, &self.daemon.id(), block, &self.block_key.noise_public, expires)
        }

        /// Check `g` as the daemon would: the device is trusted, block "7"
        /// exists, the handshake's key is `remote`.
        fn check_as(&self, g: &Grant, remote: &[u8; 32], trusted: &DeviceKeys) -> Result<(), Refused> {
            let id = trusted.id();
            let sign = trusted.sign_public();
            g.check(
                &Expect { daemon: &self.daemon.id(), remote, now: NOW },
                |by| (by == id).then_some(sign),
                |b| b == "7",
            )
        }

        fn check(&self, g: &Grant) -> Result<(), Refused> {
            self.check_as(g, &self.block_key.noise_public, &self.device)
        }
    }

    #[test]
    fn a_valid_grant_lets_in() {
        let w = World::new();
        assert_eq!(w.check(&w.grant("7", NOW + 60_000)), Ok(()));
        assert_eq!(w.check(&w.grant("7", NOW + MAX_LIFE_MS)), Ok(()));
    }

    #[test]
    fn expired_or_too_long() {
        let w = World::new();
        assert_eq!(w.check(&w.grant("7", NOW)), Err(Refused::Expired));
        assert_eq!(w.check(&w.grant("7", NOW - 1)), Err(Refused::Expired));
        assert_eq!(w.check(&w.grant("7", NOW + MAX_LIFE_MS + 1)), Err(Refused::TooLong));
        // A longer life under the same signature.
        let mut g = w.grant("7", NOW - 1);
        g.expires = NOW + 60_000;
        assert_eq!(w.check(&g), Err(Refused::Signature));
    }

    #[test]
    fn wrong_block() {
        let w = World::new();
        assert_eq!(w.check(&w.grant("8", NOW + 60_000)), Err(Refused::NoBlock));
        // Another block under the first one's signature.
        let mut g = w.grant("8", NOW + 60_000);
        g.block = "7".into();
        assert_eq!(w.check(&g), Err(Refused::Signature));
        // A block id that would add a line to the body.
        assert_eq!(w.check(&w.grant("7\nexpires 1", NOW + 60_000)), Err(Refused::Malformed));
    }

    #[test]
    fn wrong_daemon() {
        let w = World::new();
        let other = DeviceKeys::generate();
        let g = Grant::new(&w.device, &other.id(), "7", &w.block_key.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::OtherDaemon));
        let mut g = w.grant("7", NOW + 60_000);
        g.daemon = other.id();
        assert_eq!(w.check(&g), Err(Refused::OtherDaemon));
    }

    #[test]
    fn wrong_key() {
        let w = World::new();
        let g = w.grant("7", NOW + 60_000);
        let thief = DeviceKeys::generate();
        assert_eq!(w.check_as(&g, &thief.noise_public, &w.device), Err(Refused::OtherKey));
    }

    #[test]
    fn untrusted_device() {
        let w = World::new();
        let stranger = DeviceKeys::generate();
        let g = Grant::new(&stranger, &w.daemon.id(), "7", &w.block_key.noise_public, NOW + 60_000);
        assert_eq!(w.check(&g), Err(Refused::Untrusted));
        // Claiming to be the trusted device, signed by the stranger.
        let mut g = g;
        g.by = w.device.id();
        assert_eq!(w.check(&g), Err(Refused::Signature));
    }

    #[test]
    fn answers_round_trip() {
        assert_eq!(Answer::decode(&Answer::ok().encode()), Ok(()));
        assert_eq!(Answer::decode(&Answer::refused(Refused::Expired).encode()), Err(Refused::Expired));
        let json = String::from_utf8(Answer::refused(Refused::OtherKey).encode()).unwrap();
        assert_eq!(json, r#"{"ok":false,"refused":"other-key","message":"the grant is for another key"}"#);
        // From a later daemon, a reason this build doesn't know.
        assert_eq!(Answer::decode(br#"{"ok":false,"refused":"new-reason"}"#), Err(Refused::Malformed));
        assert_eq!(Answer::decode(b""), Err(Refused::Malformed));
    }

    /// The whole exchange: the grant in message 1, the answer in message 2.
    fn handshake(w: &World, g: &Grant) -> (Result<(), Refused>, Result<(), Refused>) {
        let p = prologue(&w.daemon.id());
        let hello = Hello { grant: g.clone() }.encode();
        let (init, m1) = Initiator::start_with(&w.block_key, &w.daemon.noise_public, &p, &hello).unwrap();
        let (resp, who, payload) = Responder::read(&w.daemon, &p, &m1).unwrap();
        let verdict =
            Hello::decode(&payload).map_err(|_| Refused::Malformed).and_then(|h| w.check_as(&h.grant, &who, &w.device));
        let answer = match verdict {
            Ok(()) => Answer::ok(),
            Err(why) => Answer::refused(why),
        };
        let (m2, _) = resp.finish(&answer.encode()).unwrap();
        let (got, _) = init.finish(&m2).unwrap();
        (verdict, Answer::decode(&got))
    }

    #[test]
    fn the_grant_rides_message_1_and_the_answer_message_2() {
        let w = World::new();
        assert_eq!(handshake(&w, &w.grant("7", NOW + 60_000)), (Ok(()), Ok(())));
        let (daemon, client) = handshake(&w, &w.grant("7", NOW - 1));
        assert_eq!((daemon, client), (Err(Refused::Expired), Err(Refused::Expired)));
    }

    #[test]
    fn a_refusal_that_comes_with_the_close_is_a_refusal() {
        // The daemon queues message 2 and closes; the client gets both at
        // once. What it reads first is still the refusal.
        let w = World::new();
        let p = prologue(&w.daemon.id());
        let hello = Hello { grant: w.grant("7", NOW - 1) }.encode();
        let (init, m1) = Initiator::start_with(&w.block_key, &w.daemon.noise_public, &p, &hello).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (resp, _, _) = Responder::read(&w.daemon, &p, &m1).unwrap();
        let (m2, _) = resp.finish(&Answer::refused(Refused::Expired).encode()).unwrap();
        tx.send(m2).unwrap();
        drop(tx);
        let m2 = rx.try_recv().unwrap();
        assert!(rx.try_recv().is_err(), "closed");
        let (payload, _) = init.finish(&m2).unwrap();
        assert_eq!(Answer::decode(&payload), Err(Refused::Expired));
    }

    #[test]
    fn a_replayed_message_1_opens_nothing_without_the_block_key() {
        let w = World::new();
        let p = prologue(&w.daemon.id());
        let hello = Hello { grant: w.grant("7", NOW + 60_000) }.encode();
        let (_, m1) = Initiator::start_with(&w.block_key, &w.daemon.noise_public, &p, &hello).unwrap();

        // Someone who saw message 1 sends it again: the daemon reads the
        // same grant and answers.
        let (resp, who, payload) = Responder::read(&w.daemon, &p, &m1).unwrap();
        assert_eq!(w.check_as(&Hello::decode(&payload).unwrap().grant, &who, &w.device), Ok(()));
        let (m2, daemon_end) = resp.finish(&Answer::ok().encode()).unwrap();

        // Without the block key's private half they can't read the answer:
        // message 2 answers a handshake whose ephemeral and static private
        // keys only the block's frame has.
        let thief = DeviceKeys::generate();
        let (theirs, _) = Initiator::start_with(&thief, &w.daemon.noise_public, &p, &hello).unwrap();
        assert!(theirs.finish(&m2).is_err());

        // Nor speak on the channel: anything they make up doesn't decrypt.
        let mut forged = m1.clone();
        forged.truncate(40);
        assert!(daemon_end.open(&forged).is_err());
        assert!(daemon_end.open(&[0u8; 64]).is_err());
    }

    #[test]
    fn frames() {
        let f = wire::frame(wire::DATA, 7, b"abc");
        assert_eq!(wire::parse(&f).unwrap(), (b'D', 7, &b"abc"[..]));
        assert!(wire::parse(&f[..4]).is_err());
        let h =
            wire::json_frame(wire::HEAD, 1, &wire::ReqHead { method: "GET".into(), path: "/".into(), headers: vec![] });
        let (k, s, p) = wire::parse(&h).unwrap();
        assert_eq!((k, s), (b'H', 1));
        assert_eq!(serde_json::from_slice::<wire::ReqHead>(p).unwrap().path, "/");
    }

    #[test]
    fn hello_is_json_with_the_grant() {
        let w = World::new();
        let g = w.grant("7", NOW + 1);
        let h = Hello { grant: g.clone() };
        assert_eq!(Hello::decode(&h.encode()).unwrap(), h);
        assert!(Hello::decode(b"").is_err());
        assert!(g.body().starts_with("arugula block grant v1\n"));
    }
}
