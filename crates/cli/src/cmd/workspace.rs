//! `arugula workspace`: a chant workspace as a block.

use super::Ctx;
use crate::util::{absolute, env_pane, loaded, open_block, print_json, split_of};
use anyhow::{Context, bail};
use arugula_proto::{BlockType, api::OpenRequest};
use serde_json::json;

#[derive(clap::Args)]
pub struct Args {
    /// The workspace root, holding chant.workspace.json [default: here].
    dir: Option<String>,
    /// The environment whose gates and releases to read.
    #[arg(long, default_value = "local")]
    env: String,
    /// Your chant principal, as gate approvals name you (a forge
    /// identity such as `github:<login>`, or a signer): chant runs, and
    /// signs, as you. Default: your Arugula name.
    #[arg(long)]
    actor: Option<String>,
    /// An editor's chant principal, `NAME=PRINCIPAL` by their Arugula
    /// name (`friend@example.com=github:friend`). Repeatable.
    #[arg(long = "principal", value_name = "NAME=PRINCIPAL")]
    principals: Vec<String>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { dir, env, actor, principals, split, session } = args;
    let root = absolute(dir.as_deref().unwrap_or("."))?;
    let mut named = serde_json::Map::new();
    for p in &principals {
        let (name, principal) =
            p.split_once('=').filter(|(n, v)| !n.is_empty() && !v.is_empty()).context("--principal NAME=PRINCIPAL")?;
        named.insert(name.trim().to_owned(), principal.trim().into());
    }
    let body = OpenRequest {
        kind: BlockType::Workspace,
        config: json!({ "root": root, "env": env, "actor": actor, "principals": named }),
        split: split_of(split.as_deref())?,
        local: true,
        session,
        from_pane: env_pane(),
        ..Default::default()
    };
    let block = open_block(&sock, &body)?.0.block;
    // Its first read: four chant processes, a second or two.
    let v = loaded(&sock, block)?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    println!("%{block}");
    let st = &v["state"];
    if let Some(e) = st["error"].as_str() {
        // chant's reason code, when it gave one (declaration-missing, ...).
        match st["error_code"].as_str() {
            Some(code) => bail!("{e} ({code})"),
            None => bail!("{e}"),
        }
    }
    let members = st["members"].as_array().map_or(0, Vec::len);
    let gates = st["gates"].as_array().cloned().unwrap_or_default();
    // A decision point's open question (#621) waits beside the gates.
    let (points, gates): (Vec<_>, Vec<_>) = gates.into_iter().partition(|g| g["source"]["kind"] == "point");
    let open = match points.len() {
        0 => String::new(),
        1 => ", 1 decision open".to_owned(),
        n => format!(", {n} decisions open"),
    };
    println!(
        "{}: {members} members, {} records, {} waiting at a gate{open}",
        st["name"].as_str().unwrap_or("workspace"),
        st["records"].as_array().map_or(0, Vec::len),
        gates.len()
    );
    for g in gates {
        let s = |k: &str| g[k].as_str().unwrap_or("").to_owned();
        println!("  {}: {} waits at gate {}", s("member"), s("op"), s("gate"));
    }
    for p in points {
        println!("  {}", point_line(&p));
    }
    Ok(0)
}

/// A decision point's question as a line: what it asks, and its choices.
fn point_line(g: &serde_json::Value) -> String {
    let src = &g["source"];
    let member = g["member"].as_str().filter(|m| !m.is_empty()).map(|m| format!("{m}: ")).unwrap_or_default();
    let choices: Vec<&str> =
        src["choices"].as_array().into_iter().flatten().filter_map(|c| c["label"].as_str()).collect();
    let pick = if choices.is_empty() { String::new() } else { format!(" ({})", choices.join(", ")) };
    format!("{member}{}{pick}  [{}]", src["question"].as_str().unwrap_or(""), src["id"].as_str().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_decision_points_line() {
        let g = serde_json::json!({ "member": "", "source": { "kind": "point", "id": "slice-tier-1",
            "question": "Which builder tier (W-001)", "choices": [{ "label": "small" }, { "label": "large" }] } });
        assert_eq!(super::point_line(&g), "Which builder tier (W-001) (small, large)  [slice-tier-1]");
    }
}
