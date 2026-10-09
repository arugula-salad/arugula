//! `arugula keys`: press named keys in a pane.

use super::Ctx;
use crate::http::call;
use crate::util::Pane;
use arugula_proto::{api::KeysRequest, op::ops::PaneKeys};

#[derive(clap::Args)]
pub struct Args {
    pane: Pane,
    #[arg(required = true)]
    keys: Vec<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { pane, keys } = args;
    call::<PaneKeys>(&sock, &pane.0, &KeysRequest { keys })?;
    Ok(0)
}
