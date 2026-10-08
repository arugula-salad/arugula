//! `arugula ls`: the panes.

use super::Ctx;
use crate::http::call_raw;
use crate::util::print_json;
use arugula_proto::{Attention, api::Empty, op::ops::ListPanes};

pub fn run(ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let (panes, v) = call_raw::<ListPanes>(&sock, &(), &Empty {})?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    for p in panes {
        let i = &p.info;
        let tab = p.tab_name.clone().unwrap_or_else(|| format!("@{}", p.tab));
        let what = if !i.running {
            "(waiting)".to_owned()
        } else {
            i.current.as_ref().and_then(|c| c.text.as_deref()).or(i.command.as_deref()).unwrap_or("").to_owned()
        };
        let attention = match i.attention {
            Attention::Idle => String::new(),
            Attention::Working => "  [working]".to_owned(),
            Attention::NeedsInput => "  [needs_input]".to_owned(),
            Attention::Done => "  [done]".to_owned(),
        };
        let host = i.host.map(|m| format!("  (vm m{m})")).unwrap_or_default();
        // The id isn't padded: it never was (it was a JSON value, whose `Display` ignores the width).
        println!(
            "%{} {:<12} {:<14} {:<36} {what}{attention}{host}",
            i.id,
            p.session_name,
            tab,
            i.cwd.as_deref().unwrap_or("")
        );
    }
    Ok(0)
}
