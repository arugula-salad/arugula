//! `arugula app`: a studio app's box as a block, or the list of apps.

use super::Ctx;
use crate::http::call_raw;
use crate::util::{Pane, env_pane, here, open_block, print_json};
use arugula_proto::{
    BlockType,
    api::{Empty, OpenRequest},
    op::ops::StudioAppsList,
};
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {
    /// The app's name in studio.
    name: Option<String>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
    /// Follow the box with the app's follower link (`arugula studio
    /// follower`), so hud is told who answered. The default whenever a
    /// link is kept for the app.
    #[arg(long)]
    follower: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { name: None, .. } => {
            let (list, v) = call_raw::<StudioAppsList>(&sock, &(), &Empty {})?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            for row in &list.apps {
                let a = &row.app;
                let blocks: Vec<String> = row.blocks.iter().map(|b| format!("%{b}")).collect();
                let open = if blocks.is_empty() { String::new() } else { format!("  [{}]", blocks.join(", ")) };
                let status =
                    a.status.as_deref().filter(|s| !s.is_empty()).map(|s| format!(" ({s})")).unwrap_or_default();
                println!("{:<24} {}{status}{open}", a.name, a.url);
            }
        }
        Args { name: Some(name), split, session, follower } => {
            let split = match split.as_deref() {
                None => None,
                Some("right") => Some(here(None)?),
                Some(p) => Some(p.parse::<Pane>().map_err(anyhow::Error::msg)?.0),
            };
            let body = OpenRequest {
                kind: BlockType::App,
                config: if follower { json!({ "app": name, "follower": true }) } else { json!({ "app": name }) },
                split,
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
        }
    }
    Ok(0)
}
