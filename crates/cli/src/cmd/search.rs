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
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { re, since, limit } = args;
    let mut q = vec![format!("re={}", enc(&re)), format!("limit={limit}")];
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
        match &h.doc {
            Some(doc) => println!("doc {doc}@{}: {}", h.offset, h.line),
            None => println!("%{}@{}: {}{cmd}", h.pane, h.offset, h.line),
        }
    }
    Ok(0)
}
