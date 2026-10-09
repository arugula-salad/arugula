//! `arugula send`: type text into a pane, or prompt the agent there and wait.

use super::Ctx;
use crate::http::{call, send_op};
use crate::util::Pane;
use arugula_proto::{
    api::{PromptRequest, PromptResult, SendRequest},
    op::ops::{PanePrompt, PaneSend},
};
use std::io::Read;

/// What `send --wait` came to, for people, and its exit code.
fn prompted(pane: u32, r: &PromptResult) -> (String, i32) {
    let q = |question: &Option<String>| question.as_deref().unwrap_or("a question").to_owned();
    match r {
        PromptResult::Done => (format!("%{pane} finished its turn"), 0),
        PromptResult::NeedsInput { question, .. } => (format!("%{pane} asks: {}", q(question)), 2),
        PromptResult::Blocked { question, .. } => (
            format!(
                "%{pane} was already waiting on someone ({}); nothing was typed (--answering to answer it)",
                q(question)
            ),
            2,
        ),
        PromptResult::Stalled { why, screen } => (format!("%{pane} stalled: {why}\n{screen}"), 3),
        PromptResult::StillRunning => (format!("%{pane} is still working (arugula wait %{pane} --idle)"), 4),
    }
}

#[derive(clap::Args)]
pub struct Args {
    pane: Pane,
    #[arg(required = true)]
    text: Vec<String>,
    /// Press Enter afterwards.
    #[arg(short, long)]
    enter: bool,
    /// Prompt the agent there and wait for its turn.
    #[arg(long)]
    wait: bool,
    /// With --wait: it's waiting on a question and this answers it.
    #[arg(long, requires = "wait")]
    answering: bool,
    /// With --wait: seconds before giving up waiting (default 100).
    #[arg(long, requires = "wait")]
    timeout: Option<f64>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { pane, text, enter, wait, answering, timeout } = args;
    let mut text = text.join(" ");
    if text == "-" {
        text.clear();
        std::io::stdin().read_to_string(&mut text)?;
    }
    if wait {
        let body = PromptRequest { text, answering, stall: None, timeout };
        let r: PromptResult = send_op::<PanePrompt>(&sock, &pane.0, &body)?.parse()?;
        let (line, code) = prompted(pane.0, &r);
        println!("{line}");
        return Ok(code);
    }
    call::<PaneSend>(&sock, &pane.0, &SendRequest { text, enter })?;
    Ok(0)
}
