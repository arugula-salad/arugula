//! Machine tokens (aru-141): who may reach a machine, said by the hub.
//!
//! A signed-in person asks the hub for a token naming their account, one
//! machine and one role (`POST /api/machines/{id}/token`), and presents it
//! in the first message of a channel to that machine, on the relay or a
//! direct path ([`TokenHello`]). The token is a JWT signed `EdDSA` with the
//! hub's issuer keys, the ones `/.well-known/jwks.json` publishes, and the
//! daemon checks it itself: the signature, the issuer, that it's for this
//! machine, that it's a machine token, its lifetime and that it hasn't been
//! used before.

use serde::{Deserialize, Serialize};

/// The `use` claim every machine token carries, so no other token the hub
/// signs (an ID token, say) passes for one.
pub const USE: &str = "machine";

/// How long the hub makes a machine token last, in seconds.
pub const TOKEN_SECS: u64 = 300;

/// The longest a machine token a daemon accepts may live, issue to expiry.
pub const MAX_LIFETIME_SECS: u64 = 600;

/// How far ahead of the daemon's clock a token's `iat` may be.
pub const CLOCK_SKEW_SECS: u64 = 60;

/// Where the hub publishes the keys it signs with.
pub const JWKS_PATH: &str = "/.well-known/jwks.json";

/// `POST` here for a token to reach machine `id`.
pub fn token_path(id: &str) -> String {
    format!("/api/machines/{id}/token")
}

/// `POST` here, signed as a daemon, for a token for that daemon to reach
/// machine `id` (Arugula #1002): [`TokenRole::Agents`] or
/// [`TokenRole::Drives`], answered [`MachineTokenResponse`]. A daemon older
/// than these roles refuses such a token, since it can't read its role.
pub fn machine_token_path(id: &str) -> String {
    format!("/api/machines/{id}/machine-token")
}

/// What the token lets its holder do on the machine, as the hub saw it when
/// it made the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TokenRole {
    /// The machine's owner, or an owner of its team.
    Owner,
    /// An editor in the machine's team.
    Editor,
    /// A viewer in the machine's team.
    Viewer,
    /// Anyone else signed in: they reach only sessions shared with them.
    Guest,
    /// Another machine of a person who may call this one's agents (#399,
    /// Arugula #1002): requests under `/api/a2a/` and nothing else, as
    /// their account. Asked for by a daemon ([`machine_token_path`]), never
    /// a person.
    Agents,
    /// Another machine of a person with full use of this hosted one (M184,
    /// Arugula #1002): every request, as its owner. Only a hosted machine
    /// takes it; anywhere else it's [`TokenRole::Agents`].
    Drives,
}

impl TokenRole {
    /// A token another machine asked for, not a person.
    pub fn is_machine(self) -> bool {
        matches!(self, TokenRole::Agents | TokenRole::Drives)
    }
}

/// A machine token's claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineClaims {
    /// The hub's public URL.
    pub iss: String,
    /// The account's id.
    pub sub: String,
    /// The machine's id.
    pub aud: String,
    /// Always [`USE`].
    #[serde(rename = "use")]
    pub use_: String,
    pub role: TokenRole,
    /// The account's login, to show.
    pub name: String,
    pub iat: u64,
    pub exp: u64,
    /// Unique to this token: a daemon takes each one once.
    pub jti: String,
    /// The machine that asked, for a machine's token
    /// ([`TokenRole::is_machine`]): its id, to show who called.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// The hub's answer to `POST /api/machines/{id}/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineTokenResponse {
    pub token: String,
    /// When it runs out, in seconds since the epoch.
    pub expires: u64,
    pub role: TokenRole,
}

/// The payload of a channel's first message when it opens with a machine
/// token rather than a device key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TokenHello {
    pub machine_token: String,
}

impl TokenHello {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize a token hello")
    }

    /// `Some` when the payload is a token hello and nothing else.
    pub fn decode(payload: &[u8]) -> Option<Self> {
        serde_json::from_slice(payload).ok()
    }
}

/// The `use` claim every block grant carries (#1001): never [`USE`], so a
/// block grant can't open a machine's channel and a machine token can't
/// open a block.
pub const BLOCK_USE: &str = "block";

/// `POST` here, with a [`BlockGrantRequest`], for a block grant on machine
/// `id`.
pub fn block_grant_path(id: &str) -> String {
    format!("/api/machines/{id}/block-grant")
}

/// What a page asks the hub for a block grant with: the block, and the
/// one-off key its frame will open the channel with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BlockGrantRequest {
    /// The block's pane id on the machine, in decimal.
    pub block: String,
    /// The frame's one-off X25519 key, the public half, in hex: the static
    /// key of the channel's handshake.
    pub key: String,
}

/// A block grant's claims (#1001): a machine token's, narrowed to one
/// block and bound to one key. A frame on a block's origin shows that
/// block, and nothing else on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BlockClaims {
    /// The hub's public URL.
    pub iss: String,
    /// The account's id.
    pub sub: String,
    /// The machine's id.
    pub aud: String,
    /// Always [`BLOCK_USE`].
    #[serde(rename = "use")]
    pub use_: String,
    /// The account's role on the machine when the grant was made. A block
    /// shows whatever its port does, so the daemon lets in only a role it
    /// would let in as its owner.
    pub role: TokenRole,
    /// The account's login, to show.
    pub name: String,
    /// The block's pane id, in decimal.
    pub block: String,
    /// The one-off key (X25519, hex) the grant is for: the daemon refuses
    /// it from a handshake with any other static key.
    pub key: String,
    pub iat: u64,
    pub exp: u64,
    pub jti: String,
}

/// The hub's answer to a block grant request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BlockGrantResponse {
    pub token: String,
    /// When it runs out, in seconds since the epoch.
    pub expires: u64,
}

/// The payload of a block channel's first message when its grant is the
/// hub's rather than one a device signed (`{"grant": …}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlockTokenHello {
    pub block_token: String,
}

impl BlockTokenHello {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize a block token hello")
    }

    /// `Some` when the payload is a block token hello and nothing else.
    pub fn decode(payload: &[u8]) -> Option<Self> {
        serde_json::from_slice(payload).ok()
    }
}

/// One public key, as a JWKS lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// `/.well-known/jwks.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

impl Jwks {
    pub fn find(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|k| k.kid == kid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_hello_is_nothing_else() {
        let h = TokenHello { machine_token: "a.b.c".into() };
        assert_eq!(TokenHello::decode(&h.encode()), Some(h));
        // A public preview's hello or a block's grant isn't one.
        assert_eq!(TokenHello::decode(br#"{"machine_token":"a.b.c","port":80}"#), None);
        assert_eq!(TokenHello::decode(br#"{"daemon":"d","port":80}"#), None);
        assert_eq!(TokenHello::decode(b""), None);
    }

    #[test]
    fn hellos_are_never_each_other() {
        let b = BlockTokenHello { block_token: "a.b.c".into() };
        assert_eq!(BlockTokenHello::decode(&b.encode()), Some(b.clone()));
        assert_eq!(TokenHello::decode(&b.encode()), None);
        assert_eq!(BlockTokenHello::decode(&TokenHello { machine_token: "a.b.c".into() }.encode()), None);
        assert_eq!(BlockTokenHello::decode(br#"{"grant":{}}"#), None);
        assert_eq!(block_grant_path("m1"), "/api/machines/m1/block-grant");
        assert_ne!(BLOCK_USE, USE);
    }

    #[test]
    fn claims_spell_use_and_roles_as_the_hub_does() {
        let c = MachineClaims {
            iss: "https://control.arugula.io".into(),
            sub: "acct".into(),
            aud: "machine".into(),
            use_: USE.into(),
            role: TokenRole::Editor,
            name: "someone".into(),
            iat: 1,
            exp: 301,
            jti: "j".into(),
            from: None,
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["use"], "machine");
        assert_eq!(v["role"], "editor");
        assert_eq!(serde_json::from_value::<MachineClaims>(v.clone()).unwrap(), c);
        assert!(v.get("from").is_none(), "a person's token names no machine");
        for (r, spelt) in [(TokenRole::Agents, "agents"), (TokenRole::Drives, "drives")] {
            assert_eq!(serde_json::to_value(r).unwrap(), spelt);
            assert!(r.is_machine());
        }
        assert!(!TokenRole::Owner.is_machine() && !TokenRole::Guest.is_machine());
        assert_eq!(machine_token_path("m1"), "/api/machines/m1/machine-token");
    }
}
