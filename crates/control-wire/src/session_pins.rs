//! The sessions a person pinned (Arugula
//! iss_c0fd27ac196647fa7c857dc3659a0098): a Pinned group in the Sessions
//! sidebar, below Needs you, and first on the Panes page and the phone's
//! sheet. Kept per account on the platform, as the chat sidebar is
//! ([`crate::chat_sidebar`]): pinning never changes a teammate's view of a
//! shared session, and the phone draws the same pins as the computer. `GET`
//! and `PUT /api/sessions/pins`; a change is told to the account's other
//! pages as [`ChatLive::Pins`](crate::chat::ChatLive::Pins).
//!
//! A pin is a machine's id and a session's id there, which the machine
//! never reuses (`next_session` is in its `layout.json`). A pin whose
//! session isn't on its machine any more is never drawn, and a page drops
//! it the next time it writes the list. A machine not joined to an account
//! has no pins.

use serde::{Deserialize, Serialize};

/// Pins an account may keep.
pub const MAX_PINS: usize = 100;

/// One pinned session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPin {
    /// The machine's id in control's directory.
    pub machine: String,
    /// The session's id on that machine.
    pub session: u32,
}

/// `GET /api/sessions/pins`, and the body of `PUT` (which replaces it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionPins {
    /// In the order they were pinned, oldest first; each once, at most
    /// [`MAX_PINS`].
    #[serde(default)]
    pub pins: Vec<SessionPin>,
}
