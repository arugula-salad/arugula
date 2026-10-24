//! #123: team chat as a block. The block holds only where it is: the
//! messages are Arugula control's, which the page reads as the person
//! looking (`web/src/shell/blocks/team-chat.tsx`, drawing the same
//! components as the platform's chat page), so no machine keeps a copy and
//! this daemon never sees a message. What it keeps is the layout entry:
//! `{room?, pinned?, view?, q?}`.
//!
//! - `room` alone is where the block was last (the page saves it as the
//!   person moves, `route`), so a reload and a daemon restart reopen
//!   there; with no `room` it opens on the newest unread one.
//! - `pinned` is a block for one room, drawn without the sidebar: `route`
//!   leaves it where it is.
//! - `view` and `q` are Activity, or a search.
//!
//! Methods: `route {room?, view?, q?}` (what the page calls), `state`.

use std::sync::{Arc, Mutex};

use arugula_proto::BlockType;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::block::{Block, BlockCtx, Summary, no_method};

/// The longest room id, search or stored value: a room's id is
/// `ch_` and a few dozen characters.
const MAX: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// `activity` or `search`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
}

impl Config {
    /// What the page may keep: nothing but a room, and Activity or a
    /// search, none of them long. A pinned block's room is its own.
    fn checked(self) -> Result<Self, String> {
        let long = |s: &Option<String>| s.as_deref().is_some_and(|s| s.chars().count() > MAX || s.contains('\0'));
        if long(&self.room) || long(&self.q) {
            return Err("a chat block's room and search are short text".into());
        }
        if self.room.as_deref().is_some_and(str::is_empty) {
            return Err("a room is named by its id".into());
        }
        if !matches!(self.view.as_deref(), None | Some("activity" | "search")) {
            return Err("a chat block shows a room, activity or a search".into());
        }
        if self.pinned && self.room.is_none() {
            return Err("a pinned chat block needs its room".into());
        }
        Ok(self)
    }
}

pub struct TeamChat {
    ctx: BlockCtx,
    config: Mutex<Config>,
}

impl TeamChat {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("chat config: {e}"))?;
        Ok(Arc::new(Self { ctx, config: Mutex::new(config.checked()?) }))
    }

    /// The page moved: keep where, unless the block is pinned to a room.
    fn route(&self, args: &Value) -> Result<Value, String> {
        let text = |k: &str| args[k].as_str().map(str::to_owned).filter(|s| !s.is_empty());
        let mut c = self.config.lock().unwrap();
        if c.pinned {
            return Ok(json!(*c));
        }
        let next = Config { room: text("room"), pinned: false, view: text("view"), q: text("q") }.checked()?;
        if *c != next {
            *c = next;
            self.ctx.changed();
        }
        Ok(json!(*c))
    }
}

impl Block for TeamChat {
    fn kind(&self) -> BlockType {
        BlockType::TeamChat
    }

    fn config(&self) -> Value {
        json!(*self.config.lock().unwrap())
    }

    fn state(&self) -> Value {
        // The same: there is nothing here the layout entry doesn't say.
        self.config()
    }

    fn text(&self) -> String {
        let c = self.config.lock().unwrap();
        match (&c.room, c.view.as_deref()) {
            (Some(r), _) if c.pinned => format!("Team chat, pinned to {r}\n"),
            (_, Some("activity")) => "Team chat: Activity\n".into(),
            (_, Some(_)) => format!("Team chat: search {:?}\n", c.q.as_deref().unwrap_or("")),
            (Some(r), None) => format!("Team chat: {r}\n"),
            (None, None) => "Team chat\n".into(),
        }
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let out = match method {
            "route" => self.route(&args),
            "state" => Ok(self.state()),
            m => Err(no_method(BlockType::TeamChat, m)),
        };
        Box::pin(async move { out })
    }

    fn close(&self) {}

    fn summary(&self) -> Summary {
        Summary { title: Some("Chat".into()), ..Summary::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(v: Value) -> Result<Config, String> {
        serde_json::from_value::<Config>(v).map_err(|e| e.to_string())?.checked()
    }

    #[test]
    fn a_pinned_block_needs_its_room() {
        assert!(cfg(json!({ "pinned": true })).is_err());
        assert_eq!(cfg(json!({ "pinned": true, "room": "ch_ops" })).unwrap().room.as_deref(), Some("ch_ops"));
        assert!(cfg(json!({})).is_ok(), "the whole of chat, opening on the newest unread");
    }

    #[test]
    fn what_the_page_keeps_is_short_and_known() {
        assert!(cfg(json!({ "room": "" })).is_err());
        assert!(cfg(json!({ "room": "x".repeat(MAX + 1) })).is_err());
        assert!(cfg(json!({ "view": "everything" })).is_err());
        assert!(cfg(json!({ "view": "search", "q": "deploy" })).is_ok());
    }

    #[test]
    fn a_config_round_trips_without_its_empty_fields() {
        let c = cfg(json!({ "room": "ch_ops", "pinned": true })).unwrap();
        assert_eq!(json!(c), json!({ "room": "ch_ops", "pinned": true }));
        assert_eq!(json!(Config::default()), json!({}));
    }
}
