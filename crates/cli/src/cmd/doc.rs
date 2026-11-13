//! `arugula doc move`: Docs on this machine given to a team (Arugula #863).

use std::io::{IsTerminal, Write};

use super::Ctx;
use crate::http::call;
use crate::util::print_json;
use anyhow::bail;
use arugula_proto::{
    api::{DocMoveRequest, Empty},
    op::ops::{DocMove, DocsTeam},
};

#[derive(clap::Subcommand)]
pub enum DocCmd {
    /// Give Docs on this machine to a team's project.
    ///
    /// The team owns each one from then on: it leaves this machine's Docs,
    /// and a block showing it and `doc open` of its id go on with the team's
    /// Doc. A link shared before the move keeps working,
    /// frozen as it was. Asks once, for all of them; `--yes` doesn't ask.
    Move {
        /// The Docs' ids (the Docs page and MCP's `doc list` have them).
        #[arg(required = true)]
        ids: Vec<String>,
        /// The team's project (a prj_ id) [default: the current project, which must be a team's].
        #[arg(long)]
        project: Option<String>,
        /// Don't ask.
        #[arg(long, short)]
        yes: bool,
    },
}

pub fn run(cmd: DocCmd, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out } = ctx;
    let DocCmd::Move { ids, project, yes } = cmd;
    // Where they go, said before anything moves.
    let (project, team) = match project {
        Some(p) => (p.clone(), format!("the project {p}")),
        None => {
            let t = call::<DocsTeam>(&sock, &(), &Empty {})?;
            match t.team {
                Some(t) => (t.project, t.name),
                None => bail!(
                    "{}: name a team's project with --project",
                    t.why.unwrap_or_else(|| "the current project isn't a team's".into())
                ),
            }
        }
    };
    if !yes && !confirm(ids.len(), &team)? {
        return Ok(1);
    }
    let (mut out, mut failed) = (Vec::new(), false);
    for id in &ids {
        match call::<DocMove>(&sock, id, &DocMoveRequest { project: Some(project.clone()) }) {
            Ok(m) => {
                if !json_out {
                    println!(
                        "{id}: moved to {}, Doc {}{}",
                        m.name,
                        m.id,
                        m.link.as_ref().map(|l| format!(" ({l})")).unwrap_or_default()
                    );
                }
                out.push(serde_json::json!({ "doc": id, "moved": m }));
            }
            Err(e) => {
                failed = true;
                if !json_out {
                    eprintln!("{id}: {e:#}");
                }
                out.push(serde_json::json!({ "doc": id, "error": format!("{e:#}") }));
            }
        }
    }
    if json_out {
        print_json(&serde_json::json!(out));
    }
    Ok(i32::from(failed))
}

/// Ask once whether the team should own them; anywhere but a terminal,
/// stop and say `--yes`.
fn confirm(n: usize, team: &str) -> anyhow::Result<bool> {
    let what = if n == 1 { "this Doc".to_owned() } else { format!("these {n} Docs") };
    eprintln!("{team} will own {what}: they leave this machine's Docs. A link shared before keeps working, as it was.");
    if !std::io::stdin().is_terminal() {
        eprintln!("Not moving: run again with --yes.");
        return Ok(false);
    }
    eprint!("Move {what} to {team}? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    if matches!(line.trim(), "y" | "Y" | "yes") {
        return Ok(true);
    }
    eprintln!("Not moving.");
    Ok(false)
}
