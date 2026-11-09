//! `arugula mentions on|off`: let teammates' agents mention a session, or not.

use super::Ctx;
use crate::http::call;
use crate::util::{Pane, here};
use arugula_proto::{api::MentionsRequest, op::ops::PaneMentions};

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Switch,
}

#[derive(clap::Subcommand)]
pub enum Switch {
    /// Teammates' agents may mention the session again.
    On { pane: Option<Pane> },
    /// Silence teammates' agents for a session.
    ///
    /// A mention for it waits in the machine's inbox, and its sender is told
    /// it's silenced. It lasts as long as the pane, not across its close; the
    /// whole machine's switch is `agent_mentions` in `sharing.json`.
    Off { pane: Option<Pane> },
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let (on, pane) = match args.cmd {
        Switch::On { pane } => (true, pane),
        Switch::Off { pane } => (false, pane),
    };
    let pane = here(pane)?;
    call::<PaneMentions>(&sock, &pane, &MentionsRequest { on })?;
    println!("%{pane}: teammates' agents may{} mention this session", if on { "" } else { " not" });
    Ok(0)
}
