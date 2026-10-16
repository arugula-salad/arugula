//! `arugula rules`: the standing permission rules for agent blocks.

use super::Ctx;
use crate::http::{call, call_raw};
use crate::util::print_json;
use arugula_proto::{
    api::Empty,
    op::ops::{RuleForget, RulesForgetAll, RulesList},
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, conflicts_with = "forget_all")]
    forget: Option<usize>,
    #[arg(long)]
    forget_all: bool,
    /// Default approvals for new Managoat conversations on this machine.
    #[arg(long, value_parser = ["ask", "auto_allow"])]
    managoat_approvals: Option<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { forget, forget_all, managoat_approvals } = args;
    if let Some(policy) = managoat_approvals {
        let managoat = if policy == "auto_allow" {
            arugula_proto::api::ManagoatApproval::AutoAllow
        } else {
            arugula_proto::api::ManagoatApproval::Ask
        };
        call::<arugula_proto::op::ops::AgentApprovalsSet>(
            &sock,
            &(),
            &arugula_proto::api::AgentApprovals { managoat },
        )?;
    }
    if forget_all {
        call::<RulesForgetAll>(&sock, &(), &Empty {})?;
    } else if let Some(i) = forget {
        call::<RuleForget>(&sock, &i, &Empty {})?;
    }
    let (rules, v) = call_raw::<RulesList>(&sock, &(), &Empty {})?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    let prefs = call::<arugula_proto::op::ops::AgentApprovalsGet>(&sock, &(), &Empty {})?;
    println!("New Managoat conversations: {} (this machine)", prefs.managoat.as_str());
    if rules.rules.is_empty() {
        println!("No standing tool rules (\"Always\" for a directory or everywhere makes one)");
    }
    for r in &rules.rules {
        println!("{}  {}", r.index, r.text);
    }
    Ok(0)
}
