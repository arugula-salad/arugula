//! Who's around, live, as a block: a team's room on arugula.stream, with a
//! tile per member there (their avatar or outline while their camera is
//! on), waves and calls. The page joins it with a room token Arugula
//! control signs for the person looking
//! (`web/src/shell/blocks/people.tsx`), so this daemon never sees who's in
//! it. What it keeps is the layout entry: `{team}`.
//!
//! No Labs flag: control says it's off when it has no arugula.stream app.
//! Methods: `state`.

use std::sync::Arc;

use arugula_proto::BlockType;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::block::{Block, BlockCtx, BlockKind, Summary, no_method};

/// The longest team id kept.
const MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub team: String,
}

impl Config {
    fn checked(self) -> Result<Self, String> {
        let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        if self.team.is_empty() || self.team.len() > MAX || !self.team.chars().all(ok) {
            return Err("a People block is named by a team's id".into());
        }
        Ok(self)
    }
}

pub struct PeopleKind;

impl BlockKind for PeopleKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("people config: {e}"))?;
        let b = Arc::new(PeopleBlock { ctx, config: config.checked()? });
        // Pages already connected hear of a new block's state only when it
        // changes: say it once.
        b.ctx.changed();
        Ok(b)
    }

    fn check(&self, config: &Value) -> Result<(), String> {
        serde_json::from_value::<Config>(config.clone()).map_err(|e| format!("people config: {e}"))?.checked().map(drop)
    }
}

pub struct PeopleBlock {
    ctx: BlockCtx,
    config: Config,
}

impl Block for PeopleBlock {
    fn kind(&self) -> BlockType {
        BlockType::People
    }

    fn config(&self) -> Value {
        json!(self.config)
    }

    fn state(&self) -> Value {
        // The same: who's in the room is the relay's.
        self.config()
    }

    fn text(&self) -> String {
        format!("People of team {}\n", self.config.team)
    }

    fn call(&self, method: &str, _args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let out = match method {
            "state" => Ok(self.state()),
            m => Err(no_method(BlockType::People, m)),
        };
        Box::pin(async move { out })
    }

    fn close(&self) {
        let _ = &self.ctx;
    }

    fn summary(&self) -> Summary {
        Summary { title: Some("People".into()), ..Summary::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(v: Value) -> Result<Config, String> {
        serde_json::from_value::<Config>(v).map_err(|e| e.to_string())?.checked()
    }

    #[test]
    fn a_people_block_is_a_team_id_and_nothing_else() {
        assert!(cfg(json!({ "team": "tm_0123abcd" })).is_ok());
        assert!(cfg(json!({})).is_err());
        assert!(cfg(json!({ "team": "" })).is_err());
        assert!(cfg(json!({ "team": "a/../b" })).is_err());
        assert!(cfg(json!({ "team": "a:b" })).is_err());
        assert_eq!(json!(cfg(json!({ "team": "ab" })).unwrap()), json!({ "team": "ab" }));
    }
}
