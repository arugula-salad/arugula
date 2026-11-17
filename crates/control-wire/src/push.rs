//! Web push through control (M21): the subscriptions a daemon serves, and
//! the encrypted notifications it has control relay. Control sees only
//! ciphertext.

use std::collections::BTreeMap;

use arugula_e2e::push::PushSub;
use serde::{Deserialize, Serialize};

use crate::machine_token::TokenRole;

/// `GET /api/daemon/push-subs`: the subscriptions of the people the daemon
/// serves. Control keeps each as the browser sent it, so it answers with
/// `PushSubs<serde_json::Value>`: the stored bodies, verbatim.
///
/// A subscription signed by a device the daemon trusts is that device's
/// person's. Since machines trust control (aru-141), one nobody signed is
/// taken too, as the person `roles` names for its account: control lists
/// there only the accounts that may reach this machine, each with the role
/// a machine token would give it. A control from before `roles` leaves it
/// out, and then only signed subscriptions count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PushSubs<S = PushSub> {
    pub subs: Vec<S>,
    /// The accounts of `subs` that may reach this machine, and their role on
    /// it, as control has them now.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<String, TokenRole>,
}

/// `POST /api/daemon/push`: one notification, encrypted for one subscription.
/// A daemon sends only `endpoint` and `body`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PushRequest {
    /// The subscription's push-service URL.
    pub endpoint: String,
    /// The encrypted notification (aes128gcm), standard base64.
    pub body: String,
    /// Seconds the push service keeps it; control takes an hour when it's
    /// left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
    /// `very-low`, `low`, `normal` or `high`; control takes `high` when it's
    /// left out or isn't one of those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,
}

/// Control's answer to a push: what the push service said.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PushAnswer {
    pub status: u16,
}
