//! `arugula process`: a pane's foreground process.

use super::Ctx;
use crate::http::request;
use crate::util::{Pane, here, print_json};
use arugula_proto::api::Process;

#[derive(clap::Args)]
pub struct Args {
    pub pane: Option<Pane>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { pane } = args;
    let (p, v) = request(&sock, "GET", &format!("/api/panes/{}/process", here(pane)?), None)?.parse_raw::<Process>()?;
    if json_out {
        print_json(&v);
    } else {
        println!("{} {}  (cwd {})", p.foreground, p.argv.join(" "), p.cwd.as_deref().unwrap_or("?"));
    }
    Ok(0)
}
