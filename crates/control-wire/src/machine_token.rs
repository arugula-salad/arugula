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
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["use"], "machine");
        assert_eq!(v["role"], "editor");
        assert_eq!(serde_json::from_value::<MachineClaims>(v).unwrap(), c);
    }
}
