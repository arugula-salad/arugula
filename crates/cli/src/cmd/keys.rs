//! `arugula keys`: press named keys in a pane.

use super::Ctx;
use crate::http::request_as;
use crate::util::Pane;
use arugula_proto::api::{Empty, KeysRequest};

#[derive(clap::Args)]
pub struct Args {
    pane: Pane,
    #[arg(required = true)]
    keys: Vec<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { pane, keys } = args;
    request_as(&sock, "POST", &format!("/api/panes/{}/keys", pane.0), &KeysRequest { keys })?.parse::<Empty>()?;
    Ok(0)
}
