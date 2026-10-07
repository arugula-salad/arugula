//! `arugula mouse`: a click, press, release or drag at a cell.

use super::Ctx;
use crate::http::request;
use crate::util::Pane;
use arugula_proto::api::Empty;
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {
    pane: Pane,
    x: u16,
    y: u16,
    /// left, middle, right, wheel_up, wheel_down
    #[arg(long, default_value = "left")]
    button: String,
    /// click, press, release, drag
    #[arg(long, default_value = "click")]
    action: String,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, .. } = ctx;
    let Args { pane, x, y, button, action } = args;
    let body = json!({"x": x, "y": y, "button": button, "action": action});
    // The button and action stay the strings typed here: the daemon names
    // the ones it doesn't know, as it always did.
    request(&sock, "POST", &format!("/api/panes/{}/mouse", pane.0), Some(&body))?.parse::<Empty>()?;
    Ok(0)
}
