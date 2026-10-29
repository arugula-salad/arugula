//! Who's around as a block (outline presence, S43 spike): live camera
//! outlines of the people who share a team with the person looking. The
//! frames are Arugula control's, passed on in memory and read by the page
//! as that person (`web/src/shell/blocks/people.tsx`), so this daemon never
//! sees one. What it keeps is the layout entry: `{}`.
//!
//! No Labs flag while it's a spike. Methods: `state`.

use std::sync::Arc;

use arugula_proto::BlockType;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use crate::block::{Block, BlockCtx, BlockKind, Summary, no_method};

pub struct PeopleKind;

impl BlockKind for PeopleKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        self.check(&config)?;
        let b = Arc::new(PeopleBlock { ctx });
        // Pages already connected hear of a new block's state only when it
        // changes: say it once.
        b.ctx.changed();
        Ok(b)
    }

    fn check(&self, config: &Value) -> Result<(), String> {
        if config.is_object() || config.is_null() { Ok(()) } else { Err("a People block's config is {}".into()) }
    }
}

pub struct PeopleBlock {
    ctx: BlockCtx,
}

impl Block for PeopleBlock {
    fn kind(&self) -> BlockType {
        BlockType::People
    }

    fn config(&self) -> Value {
        json!({})
    }

    fn state(&self) -> Value {
        self.config()
    }

    fn text(&self) -> String {
        "People\n".into()
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

    #[test]
    fn a_people_block_keeps_nothing() {
        assert!(PeopleKind.check(&json!({})).is_ok());
        assert!(PeopleKind.check(&json!({ "anything": 1 })).is_ok());
        assert!(PeopleKind.check(&json!("people")).is_err());
    }
}
