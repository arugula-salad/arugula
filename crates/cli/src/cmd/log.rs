//! `arugula log`: a pane's commands and who ran each.

use super::Ctx;
use crate::http::request;
use crate::util::{Pane, here, print_json, time};
use arugula_proto::api::{DriverEntry, HistoryEntry};

#[derive(clap::Args)]
pub struct Args {
    pane: Option<Pane>,
    #[arg(long)]
    who: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { pane, who } = args;
    let id = here(pane)?;
    if !who {
        let (entries, v) = request(&sock, "GET", &format!("/api/history?pane={id}&limit=1000"), None)?
            .parse_raw::<Vec<HistoryEntry>>()?;
        if json_out {
            print_json(&v);
            return Ok(0);
        }
        for c in &entries {
            println!(
                "{:>8}  {:<16} {}",
                time(c.started_ms),
                c.by.as_deref().unwrap_or("-"),
                c.text.as_deref().unwrap_or("?")
            );
        }
        return Ok(0);
    }
    let (drivers, v) =
        request(&sock, "GET", &format!("/api/panes/{id}/drivers"), None)?.parse_raw::<Vec<DriverEntry>>()?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    for d in &drivers {
        println!("{:>8}  @{} {}", time(d.at_ms), d.offset, d.who);
    }
    Ok(0)
}
