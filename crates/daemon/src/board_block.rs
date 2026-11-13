//! A board as a block (boards, step 2): a pin of one project
//! board beside the panes. The block holds only which board it is: the
//! board is Arugula control's, which the page reads as the person looking
//! (`web/src/shell/blocks/board.tsx`, drawing the same view as the
//! platform's board page), so no machine keeps a copy and this daemon
//! never sees a row. What it keeps is the layout entry: `{board}`.
//!
//! Boards are on for everyone (native #191). Methods: `state`.

use std::sync::Arc;

use arugula_control_wire::boards::{BoardView, agent_board, text as board_text};
use arugula_proto::BlockType;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::block::{Block, BlockCtx, BlockKind, Summary, no_method};
use crate::server::App;

/// The longest board id kept: a board's id is `brd_` and 32 hex characters.
const MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub board: String,
}

impl Config {
    fn checked(self) -> Result<Self, String> {
        let rest = self.board.strip_prefix("brd_").unwrap_or_default();
        if self.board.len() > MAX || rest.is_empty() || !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("a board block is named by a board's id, brd_…".into());
        }
        Ok(self)
    }
}

/// What `capture` of a board block returns: the board's text, as
/// `board_read` prints it, read from control as this machine's agent (the
/// same call, so the same wait). A board that can't be read (offline, no
/// grant, not the owner's to read) is one line saying why.
pub async fn captured(app: &App, board: &str, owner: bool) -> String {
    if !owner {
        return format!("Board {board}: a board is the owner's to read on this machine\n");
    }
    let read = app.control.cloud(reqwest::Method::GET, &agent_board(board), None).await;
    let view = read.map_err(|e| e.to_string()).and_then(|v| {
        serde_json::from_value::<BoardView>(v).map_err(|e| format!("Arugula cloud sent a board I can't read: {e}"))
    });
    match view {
        Ok(v) => board_text(&v),
        Err(why) => format!("Board {board}: can't read it: {}\n", why.split_whitespace().collect::<Vec<_>>().join(" ")),
    }
}

pub struct BoardKind;

impl BlockKind for BoardKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("board config: {e}"))?;
        let b = Arc::new(BoardBlock { ctx, config: config.checked()? });
        // Pages already connected hear of a new block's state only when it
        // changes: say it once.
        b.ctx.changed();
        Ok(b)
    }

    fn check(&self, config: &Value) -> Result<(), String> {
        serde_json::from_value::<Config>(config.clone()).map_err(|e| format!("board config: {e}"))?.checked().map(drop)
    }
}

pub struct BoardBlock {
    ctx: BlockCtx,
    config: Config,
}

impl Block for BoardBlock {
    fn kind(&self) -> BlockType {
        BlockType::Board
    }

    fn config(&self) -> Value {
        json!(self.config)
    }

    fn state(&self) -> Value {
        // The same: there is nothing here the layout entry doesn't say.
        self.config()
    }

    fn text(&self) -> String {
        format!("Board {}\n", self.config.board)
    }

    fn call(&self, method: &str, _args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let out = match method {
            "state" => Ok(self.state()),
            m => Err(no_method(BlockType::Board, m)),
        };
        Box::pin(async move { out })
    }

    fn close(&self) {
        let _ = &self.ctx;
    }

    fn summary(&self) -> Summary {
        Summary { title: Some("Board".into()), ..Summary::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(v: Value) -> Result<Config, String> {
        serde_json::from_value::<Config>(v).map_err(|e| e.to_string())?.checked()
    }

    #[test]
    fn a_board_block_is_a_board_id_and_nothing_else() {
        assert!(cfg(json!({ "board": "brd_0123abcd" })).is_ok());
        assert!(cfg(json!({})).is_err());
        assert!(cfg(json!({ "board": "" })).is_err());
        assert!(cfg(json!({ "board": "iss_0123" })).is_err());
        assert!(cfg(json!({ "board": "brd_x/../y" })).is_err());
        assert_eq!(json!(cfg(json!({ "board": "brd_ab" })).unwrap()), json!({ "board": "brd_ab" }));
    }
}
