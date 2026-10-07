//! `arugula shell-env`: the shell environment blocks that run your tools get.

use super::Ctx;
use crate::http::call_raw;
use crate::util::print_json;
use arugula_proto::{
    api::Empty,
    op::ops::{ShellEnvGet, ShellEnvRefresh},
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    refresh: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { refresh } = args;
    let (env, v) = match refresh {
        true => call_raw::<ShellEnvRefresh>(&sock, &(), &Empty {})?,
        false => call_raw::<ShellEnvGet>(&sock, &(), &Empty {})?,
    };
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    let shell = if env.shell.is_empty() { "?" } else { &env.shell };
    match env.error.as_deref() {
        Some(e) => println!("{shell}: {e}; blocks get the daemon's environment"),
        None => {
            println!("{shell} ({} ms)", env.ms);
            match env.path.as_deref() {
                Some(p) => println!("PATH={p}"),
                None => println!("PATH is the daemon's"),
            }
            println!("also sets: {}", env.vars.iter().filter(|k| *k != "PATH").cloned().collect::<Vec<_>>().join(" "));
        }
    }
    Ok(0)
}
