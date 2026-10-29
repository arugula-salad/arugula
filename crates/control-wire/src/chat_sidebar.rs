//! How an account arranges its chat sidebar (#120): the rooms it starred,
//! sections of its own with rooms dragged into them, which groups it
//! collapsed and how each is sorted. Kept per account on the platform, so
//! the page on a computer and the phone's list draw the same one. `GET` and
//! `PUT /api/chat/sidebar`; a change is told to the account's other pages
//! as [`ChatLive::Sidebar`](crate::chat::ChatLive::Sidebar).
//!
//! A room is in one place at most: starred, or in a section, or in neither
//! (where its kind puts it). The platform makes it so on the way in
//! (`chat::sidebar::tidy`), and a page skips an id it doesn't list.

use serde::{Deserialize, Serialize};

/// Sections an account may make, and rooms it may place.
pub const MAX_SECTIONS: usize = 24;
pub const MAX_PLACED: usize = 1000;
/// A section's name, in characters.
pub const MAX_SECTION_NAME: usize = 40;

/// How a group's rooms are ordered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ChatSort {
    /// By name.
    #[default]
    Name,
    /// Newest message first.
    Recent,
}

/// A section the account made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSection {
    /// Made by the page that creates it (`s_` and hex), stable under rename.
    pub id: String,
    pub name: String,
    /// Room ids, in the order the person dragged them.
    #[serde(default)]
    pub rooms: Vec<String>,
}

/// `GET /api/chat/sidebar`, and the body of `PUT` (which replaces it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatSidebar {
    /// Starred rooms, in the order they were starred.
    #[serde(default)]
    pub starred: Vec<String>,
    /// The account's own sections, top to bottom, below Starred.
    #[serde(default)]
    pub sections: Vec<ChatSection>,
    /// Groups shown shut, by key: `starred`, a section's id, or a built-in
    /// group's (`<team>/channels`, `<team>/dms`, `machine/<id>`, `archives`).
    #[serde(default)]
    pub collapsed: Vec<String>,
    /// Groups not sorted by name, by the same keys.
    #[serde(default)]
    pub sorts: std::collections::BTreeMap<String, ChatSort>,
}
