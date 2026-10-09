//! `arugula wait`: for a command, an exit, a match, or an agent.

use super::Ctx;
use crate::http::call_raw;
use crate::util::{Pane, here, print_json, snake};
use arugula_proto::{
    api::{WaitRequest, WaitResult},
    op::ops::PaneWait,
};

#[derive(clap::Args)]
pub struct Args {
    pane: Option<Pane>,
    #[arg(long, group = "until")]
    command_end: bool,
    #[arg(long, group = "until")]
    exit: bool,
    /// A regular expression to wait for in the output.
    #[arg(long = "match", group = "until")]
    matching: Option<String>,
    /// Until it's no longer working (an agent's turn ended, or it needs
    /// you): any block.
    #[arg(long, group = "until")]
    idle: bool,
    /// Until it needs you (an agent asks to run something).
    #[arg(long, group = "until")]
    needs_input: bool,
    /// Seconds.
    #[arg(long)]
    timeout: Option<f64>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { pane, command_end, exit, matching, idle, needs_input, timeout } = args;
    let pane = here(pane)?;
    let (until, re) = match (command_end, exit, matching) {
        _ if idle => ("idle", None),
        _ if needs_input => ("needs-input", None),
        (_, true, _) => ("exit", None),
        (_, _, Some(re)) => ("match", Some(re)),
        _ => ("command-end", None),
    };
    let req = WaitRequest { until: until.to_owned(), re, timeout };
    let (w, v) = call_raw::<PaneWait>(&sock, &pane, &req)?;
    if json_out {
        print_json(&v);
    }
    Ok(match w {
        WaitResult::Timeout => {
            if !json_out {
                eprintln!("arugula: timed out");
            }
            124
        }
        WaitResult::CommandEnd { text, exit, .. } => {
            if !json_out {
                let exit = exit.map_or("null".to_owned(), |e| e.to_string());
                println!("{} exited {exit}", text.as_deref().unwrap_or("command"));
            }
            exit.unwrap_or(0)
        }
        WaitResult::Exit { code } => code.unwrap_or(0),
        WaitResult::Attention { state, ask } => {
            if json_out {
            } else if ask.is_some() {
                // What it asks, for a script (or another agent) to
                // answer with `call %N answer`.
                print_json(&v["ask"]);
            } else {
                println!("{}", snake(&state).replace('_', "-"));
            }
            0
        }
        WaitResult::Match { text, .. } => {
            if !json_out {
                println!("{text}");
            }
            0
        }
    })
}
