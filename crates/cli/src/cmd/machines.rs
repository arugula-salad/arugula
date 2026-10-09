//! `arugula machines`: the machines panes run on.

use super::Ctx;
use crate::http::call_raw;
use crate::util::{print_json, snake};
use arugula_proto::{Owner, api::Empty, op::ops::MachinesList};

pub fn run(ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let (machines, v) = call_raw::<MachinesList>(&sock, &(), &Empty {})?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    for m in &machines {
        let image = m.image.as_deref().unwrap_or("default image");
        let owner = match m.owner {
            Owner::Tab(t) => format!("@{t}"),
            Owner::Pane(p) => format!("%{p}"),
        };
        let (state, sprite, provider, name) =
            (snake(&m.state), &m.sprite, &m.provider, m.name.as_deref().unwrap_or(""));
        println!("m{} {owner:<5} {state:<9} {name:<18} {sprite:<34} {provider} ({image})", m.id);
    }
    Ok(0)
}
