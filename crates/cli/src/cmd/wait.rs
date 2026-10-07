//! `arugula wait`: for a command, an exit, a match, or an agent.

use super::Ctx;
use crate::http::{enc, request};
use crate::util::{Pane, here, print_json, snake};
use arugula_proto::api::WaitResult;

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
    let mut q = match (command_end, exit, &matching) {
        _ if idle => "until=idle".to_owned(),
        _ if needs_input => "until=needs-input".to_owned(),
        (_, true, _) => "until=exit".to_owned(),
        (_, _, Some(re)) => format!("until=match&re={}", enc(re)),
        _ => "until=command-end".to_owned(),
    };
    if let Some(t) = timeout {
        q.push_str(&format!("&timeout={t}"));
    }
    let (w, v) = request(&sock, "GET", &format!("/api/panes/{pane}/wait?{q}"), None)?.parse_raw::<WaitResult>()?;
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
