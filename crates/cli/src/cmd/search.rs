//! `arugula search`: the output of every pane.

use super::Ctx;
use crate::http::{enc, request};
use crate::util::{duration, print_json};
use arugula_proto::api::SearchHit;

#[derive(clap::Args)]
pub struct Args {
    re: String,
    #[arg(long)]
    since: Option<String>,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    /// Another host's history, as synced here (`all`: every host's).
    #[arg(long, value_name = "HOST")]
    synced: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, gone, .. } = ctx;
    let synced_q = |flag| super::synced_q(flag, &gone);
    let Args { re, since, limit, synced } = args;
    let mut q = vec![format!("re={}", enc(&re)), format!("limit={limit}")];
    q.extend(synced_q(synced));
    if let Some(s) = since {
        q.push(format!("since={}", duration(&s)?));
    }
    let (hits, v) =
        request(&sock, "GET", &format!("/api/search?{}", q.join("&")), None)?.parse_raw::<Vec<SearchHit>>()?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    for h in &hits {
        let cmd = h.command.as_ref().map(|c| format!("  ({c})")).unwrap_or_default();
        let host = h.host.as_ref().map(|h| format!("{h}:")).unwrap_or_default();
        println!("{host}%{}@{}: {}{cmd}", h.pane, h.offset, h.line);
    }
    Ok(0)
}
