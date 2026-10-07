//! `arugula ide`: arugulad as Claude Code's IDE.

use super::Ctx;
use crate::http::{request, request_as};
use crate::util::print_json;
use arugula_proto::api::{IdeDiffs, IdeDiffsRequest, IdeInfo};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    diffs: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { diffs } = args;
    if let Some(d) = diffs {
        request_as(&sock, "PUT", "/api/ide", &IdeDiffsRequest { diffs: d })?.parse::<IdeDiffs>()?;
    }
    let (ide, v) = request(&sock, "GET", "/api/ide", None)?.parse_raw::<IdeInfo>()?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    if !ide.on {
        println!("arugulad isn't Claude Code's IDE (--no-claude-ide)");
        return Ok(0);
    }
    // `port` is printed as the JSON it is: `null` when the daemon sent none.
    let port = ide.port.map_or("null".to_owned(), |p| p.to_string());
    let lock_dir = ide.lock_dir.as_deref().map(|d| d.display().to_string()).unwrap_or_default();
    println!("Claude Code's IDE on port {port} ({lock_dir})");
    println!("diffs go to: {}", ide.diffs.as_deref().unwrap_or(""));
    for o in ide.others.iter().flatten() {
        let alive = if o.alive { "" } else { "  (gone)" };
        println!("  also: {:<24} port {}  {}{alive}", o.name, o.port, o.folders.join(", "));
    }
    Ok(0)
}
