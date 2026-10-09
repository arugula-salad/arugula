//! `arugula process`: a pane's foreground process.

use super::Ctx;
use crate::http::call_raw;
use crate::util::{Pane, here, print_json};
use arugula_proto::{api::Empty, op::ops::PaneProcess};

#[derive(clap::Args)]
pub struct Args {
    pub pane: Option<Pane>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { pane } = args;
    let (p, v) = call_raw::<PaneProcess>(&sock, &here(pane)?, &Empty {})?;
    if json_out {
        print_json(&v);
    } else {
        println!("{} {}  (cwd {})", p.foreground, p.argv.join(" "), p.cwd.as_deref().unwrap_or("?"));
    }
    Ok(0)
}
