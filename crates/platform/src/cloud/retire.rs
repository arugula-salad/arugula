//! `arugula-platform managoat-retire`: the deletion of beta Cloud
//! workspaces' Managoat resources, 14 days after the notice (M189, #1250).
//! An operator runs it once, by hand: `--dry-run` lists every workspace,
//! vault and environment in the platform's Managoat account, and what it
//! would delete; `--execute` deletes them, workspaces first (each holds a
//! vault and an environment), then vaults, then environments.
//!
//! Only what control made is deleted, unless `--all`: a workspace named
//! after a Cloud workspace (`cw_…`), and a vault or environment whose
//! metadata names a Cloud workspace or a run (`arugula_workspace`,
//! `arugula_run`). A delete Managoat answers 404 is already done, so it can
//! run again after a failure. It never touches the platform's database:
//! the rows go with the code, in the PR after.

use std::{fmt::Write as _, time::Duration};

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use reqwest::Method;
use serde_json::Value;

use super::provider::Provider;

#[derive(Parser, Debug)]
#[command(
    name = "managoat-retire",
    about = "Delete every Managoat workspace, vault and environment beta Cloud workspaces left (M189)"
)]
pub struct Args {
    /// List what it would delete, and delete nothing.
    #[arg(long, required_unless_present = "execute", conflicts_with = "execute")]
    pub dry_run: bool,
    /// Delete them.
    #[arg(long)]
    pub execute: bool,
    /// Also delete what control didn't make.
    #[arg(long)]
    pub all: bool,
    #[arg(long, env = "ARUGULA_MANAGOAT_URL", default_value = "https://managoat.com")]
    pub url: String,
    #[arg(long, env = "ARUGULA_MANAGOAT_API_KEY", hide_env_values = true)]
    pub key: String,
}

/// The order they're deleted in.
pub const KINDS: [&str; 3] = ["workspaces", "vaults", "environments"];

/// One Managoat resource, and what's done with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    /// Control made it.
    pub ours: bool,
    /// `listed` (`--dry-run`, or not ours without `--all`), `deleted`, or
    /// `failed: …`.
    pub outcome: String,
}

/// Whether control made `v`, a resource of `kind`.
fn ours(kind: &str, v: &Value) -> bool {
    match kind {
        "workspaces" => v["name"].as_str().is_some_and(|n| n.starts_with("cw_")),
        _ => v["metadata"]["arugula_workspace"].is_string() || v["metadata"]["arugula_run"].is_string(),
    }
}

/// List every workspace, vault and environment, and with `execute`, delete
/// the ones control made (every one, with `all`).
pub async fn retire(p: &Provider, execute: bool, all: bool) -> Result<Vec<Resource>> {
    let mut found = Vec::new();
    for kind in KINDS {
        let listed =
            p.query(Method::GET, &["api", kind], &[], None).await.with_context(|| format!("listing {kind}"))?;
        let listed = listed.as_array().with_context(|| format!("Managoat's {kind} aren't a list"))?;
        for v in listed {
            let id = v["id"].as_str().with_context(|| format!("one of Managoat's {kind} has no id"))?;
            found.push(Resource {
                kind,
                id: id.to_owned(),
                name: v["name"].as_str().unwrap_or_default().to_owned(),
                ours: ours(kind, v),
                outcome: "listed".into(),
            });
        }
    }
    if execute {
        for r in found.iter_mut().filter(|r| all || r.ours) {
            r.outcome = match p.remove(r.kind, &r.id).await {
                Ok(_) => "deleted".into(),
                Err(e) => format!("failed: {e:#}"),
            };
        }
    }
    Ok(found)
}

/// What `retire` did, for the operator.
pub fn render(found: &[Resource], execute: bool, all: bool) -> String {
    let mut out = String::new();
    let would = |r: &Resource| all || r.ours;
    for kind in KINDS {
        let of: Vec<&Resource> = found.iter().filter(|r| r.kind == kind).collect();
        let _ = writeln!(out, "{kind}: {}", of.len());
        for r in of {
            let action = match (execute, would(r)) {
                (_, false) => "kept (not control's; --all deletes it)".to_owned(),
                (false, true) => "would delete".to_owned(),
                (true, true) => r.outcome.clone(),
            };
            let _ = writeln!(out, "  {}  {}  {action}", r.id, if r.name.is_empty() { "-" } else { &r.name });
        }
    }
    let n = found.iter().filter(|r| would(r)).count();
    let failed = found.iter().filter(|r| r.outcome.starts_with("failed")).count();
    let _ = match (execute, failed) {
        (false, _) => writeln!(out, "\n{n} to delete. Nothing was deleted: --execute deletes them."),
        (true, 0) => writeln!(out, "\ndeleted {n}"),
        (true, f) => writeln!(out, "\ndeleted {} of {n}; {f} failed: run it again", n - f),
    };
    out
}

/// `arugula-platform managoat-retire …`.
pub fn run(prog: &str, args: impl Iterator<Item = String>) -> Result<()> {
    let a = Args::parse_from(std::iter::once(prog.to_owned()).chain(args));
    let p = Provider::new(&a.url, a.key)?;
    let found = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async { tokio::time::timeout(Duration::from_secs(600), retire(&p, a.execute, a.all)).await })
        .context("Managoat took more than ten minutes")??;
    print!("{}", render(&found, a.execute, a.all));
    if found.iter().any(|r| r.outcome.starts_with("failed")) {
        bail!("some deletes failed");
    }
    Ok(())
}
