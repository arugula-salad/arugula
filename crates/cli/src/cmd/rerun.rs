//! `arugula rerun`: type a pane's failed command again.

use super::Ctx;
use crate::http::request_as;
use crate::util::{Pane, here};
use anyhow::bail;
use arugula_proto::{
    Action,
    api::{ActRequest, ActResponse},
};

#[derive(clap::Args)]
pub struct Args {
    pub pane: Option<Pane>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { pane } = args;
    let pane = here(pane)?;
    let body = ActRequest {
        action: Action::Rerun,
        pane: Some(pane),
        panes: vec![],
        id: None,
        content: None,
        option: None,
        suggestion: None,
        message: None,
        text: None,
    };
    let v: ActResponse = request_as(&sock, "POST", "/api/attention/act", &body)?.parse()?;
    if let Some(e) = v.results.first().and_then(|r| r.error.as_deref()) {
        bail!("{e}");
    }
    Ok(0)
}
