//! `arugula app URL`: an app's box as a block, by its address. The box signs
//! you in with your Arugula account (native #206: the studio login that
//! listed apps by name is retired).

use super::Ctx;
use crate::util::{Pane, env_pane, here, open_block, print_json};
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {
    /// The app's address: its box, e.g. `https://pinboard.example`.
    url: String,
    /// What to call the block (default: the box's host).
    #[arg(long)]
    title: Option<String>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { url, title, split, session } = args;
    let split = match split.as_deref() {
        None => None,
        Some("right") => Some(here(None)?),
        Some(p) => Some(p.parse::<Pane>().map_err(anyhow::Error::msg)?.0),
    };
    let mut config = json!({ "box_url": url });
    if let Some(t) = title {
        config["title"] = json!(t);
    }
    let body =
        OpenRequest { kind: BlockType::App, config, split, session, from_pane: env_pane(), ..Default::default() };
    let (opened, v) = open_block(&sock, &body)?;
    if json_out {
        print_json(&v);
    } else {
        println!("%{}", opened.block);
    }
    Ok(0)
}
