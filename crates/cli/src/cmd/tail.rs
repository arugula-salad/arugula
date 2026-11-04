//! `arugula tail`: a pane's output, live or from its history.

use super::Ctx;
use crate::http::request;
use crate::util::{Pane, here};
use std::io::{Read, Write};

#[derive(clap::Args)]
pub struct Args {
    pane: Option<Pane>,
    /// Keep printing new output.
    #[arg(short, long)]
    follow: bool,
    /// Start at this stream offset.
    #[arg(long, conflicts_with = "last_command")]
    from: Option<u64>,
    /// The output of the last (or current) command.
    #[arg(long)]
    last_command: bool,
    /// Strip colors and other escape sequences.
    #[arg(long)]
    text: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { pane, follow, from, last_command, text } = args;
    let pane = here(pane)?;
    let mut q: Vec<String> = Vec::new();
    if let Some(f) = from {
        q.push(format!("from={f}"));
    }
    if last_command {
        q.push("from=last-command".into());
    }
    if follow {
        q.push("follow=1".into());
    }
    if text {
        q.push("text=1".into());
    }
    let mut res = request(&sock, "GET", &format!("/api/panes/{pane}/tail?{}", q.join("&")), None)?.ok()?;
    let mut out = std::io::stdout().lock();
    let mut buf = [0u8; 65536];
    loop {
        let n = res.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        out.flush()?;
    }
    Ok(0)
}
