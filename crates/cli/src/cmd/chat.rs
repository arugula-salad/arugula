//! `arugula chat`: team chat as a block, beside the terminal it's run in.

use super::Ctx;
use crate::util::{env_pane, open_block, print_json, split_of};
use anyhow::bail;
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {
    /// A room, by its id (`ch_…`), to open on [default: the newest unread].
    room: Option<String>,
    /// Only that room, without the sidebar, kept there after a reload and a
    /// daemon restart.
    #[arg(long, requires = "room")]
    pin: bool,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { room, pin, split, session } = args;
    let room = room.map(|r| r.trim().to_owned());
    if room.as_deref() == Some("") {
        bail!("a room is named by its id");
    }
    let body = OpenRequest {
        kind: BlockType::TeamChat,
        config: json!({ "room": room, "pinned": pin }),
        split: split_of(split.as_deref())?,
        local: true,
        session,
        from_pane: env_pane(),
        ..Default::default()
    };
    let (opened, v) = open_block(&sock, &body)?;
    if json_out {
        print_json(&v);
    } else {
        println!("%{}", opened.block);
    }
    Ok(0)
}
