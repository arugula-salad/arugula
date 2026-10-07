//! `arugula rules`: the standing permission rules for agent blocks.

use super::Ctx;
use crate::http::request;
use crate::util::print_json;
use arugula_proto::api::{Empty, Rules};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, conflicts_with = "forget_all")]
    forget: Option<usize>,
    #[arg(long)]
    forget_all: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { forget, forget_all } = args;
    if forget_all {
        request(&sock, "DELETE", "/api/rules", None)?.parse::<Empty>()?;
    } else if let Some(i) = forget {
        request(&sock, "DELETE", &format!("/api/rules/{i}"), None)?.parse::<Empty>()?;
    }
    let (rules, v) = request(&sock, "GET", "/api/rules", None)?.parse_raw::<Rules>()?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    if rules.rules.is_empty() {
        println!("No standing rules: agent blocks ask (\"Always\" for a directory or everywhere makes one)");
    }
    for r in &rules.rules {
        println!("{}  {}", r.index, r.text);
    }
    Ok(0)
}
