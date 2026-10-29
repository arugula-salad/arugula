//! Who is asking, as the hub found out and forwards to the platform in its
//! principal header (`arugula_control_common::principal`, which signs and
//! checks the header and re-exports these).

use serde::{Deserialize, Serialize};

/// Who is asking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Principal {
    /// A signed-in person, by their session cookie.
    Person { account: String },
    /// A CLI device, by its signature: a session for its account (M49).
    Cli { account: String, device: String },
    /// An enrolled daemon, by its signature: it acts as its account.
    Daemon { account: String, daemon: String },
}

impl Principal {
    /// The account it acts as.
    pub fn account(&self) -> &str {
        match self {
            Principal::Person { account } | Principal::Cli { account, .. } | Principal::Daemon { account, .. } => {
                account
            }
        }
    }

    /// The device or machine that signed, if one did.
    pub fn device(&self) -> Option<&str> {
        match self {
            Principal::Person { .. } => None,
            Principal::Cli { device, .. } => Some(device),
            Principal::Daemon { daemon, .. } => Some(daemon),
        }
    }
}

/// What the hub vouches for when its gateway forwards a call to the
/// platform (M140): every forwarded call carries one, so the platform
/// refuses anything without it. Its JSON is the principal's for a
/// principal (so `PrincipalKey::verify` reads it too), else
/// `{"kind": "refused", ...}` or `{"kind": "nobody"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Vouch {
    /// Who's asking.
    Principal(Principal),
    /// No credentials, or credentials the hub refused.
    Not(NotVouched),
}

/// A call the hub forwards without a principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotVouched {
    /// The request's credentials were refused: the status and the error
    /// the hub would have answered with, and which credentials they were.
    /// The platform's extractors answer with them, as the hub's do.
    Refused { status: u16, error: String, by: Credential },
    /// No session and no signature (a webhook, or a page signed out).
    Nobody,
}

/// What a request proved who it was with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Credential {
    /// A session cookie.
    Session,
    /// A device's or daemon's signature (`x-arugula-auth`).
    Signature,
}
