//! A hosted workspace's setup (M183): on its box's first boot, its daemon
//! reads what to set up from the hub's `GET /api/hosted/spec` (signed as
//! the hosted machine), runs it in a pane anyone on the project can watch,
//! and says how it goes at `POST /api/hosted/setup`. The hub asks the
//! platform for both (`PlatformPort::hosted_plan`, `hosted_setup`).
//!
//! - **The plan** ([`plan`]) is the spec, with what to clone each repo with
//!   and the values of the env its spec takes from its secrets. A GitHub
//!   repo linked to the project is cloned with a read-only installation
//!   token for that repo alone, minted now (an hour; M167's helper logs
//!   it). Any other repo is cloned with the workspace's `GIT_TOKEN` secret
//!   (and `GIT_USERNAME`, else `x-access-token`), when it has one. The
//!   daemon hands them to git in its environment for the clone alone:
//!   nothing is written to the box's disk.
//! - **What it's built from.** Reading the plan marks the box built from
//!   that spec, so "rebuild needed" follows the spec the box really ran.
//! - **Its status** ([`report`]) is kept on the workspace: `setting_up`
//!   with its step, `ready`, or `failed` with the step and a fixed reason
//!   code. Never the pane's text: control never stores terminal lines, and
//!   the pane is end to end like any other, so a client reads it from the
//!   box. Only the box's own machine may say it, or a box the worker hasn't
//!   seen enroll yet. A rebuild clears it ([`super::worker`]).
//!
//! Only machines enrolled for the workspace reach either: the hub reads the
//! workspace and project from the machine's certificate.

use std::collections::BTreeMap;

use arugula_control_wire::hosted::{SETUP_REASONS, SetupPlan, SetupRepo, SetupReport};
use tracing::{info, warn};

use crate::{
    Platform,
    forge::{Caller, Repo},
};

/// The secret a non-GitHub repo (or one not linked to the project) is
/// cloned with, and the user name beside it.
pub const GIT_TOKEN: &str = "GIT_TOKEN";
pub const GIT_USERNAME: &str = "GIT_USERNAME";
/// The user name an installation token (or a token alone) clones with.
const TOKEN_USER: &str = "x-access-token";

/// Whether `machine` may act for workspace `w` now: the machine it
/// enrolled as, or any while the worker hasn't seen it enroll yet.
fn its_box(w: &super::HostedWorkspace, machine: &str) -> bool {
    matches!(w.state.as_str(), "enrolling" | "ready" | "waking" | "paused")
        && w.machine.as_deref().is_none_or(|m| m == machine)
}

/// A secret's value, when it's sealed here: with the workspace's project's
/// key (M209), or the master key for one from before it.
fn secret(app: &Platform, project: &str, workspace: &str, name: &str) -> anyhow::Result<Option<String>> {
    let (Some(master), Some(sealed)) = (&app.hosted.cipher, app.db.hosted_secret_sealed(workspace, name)?) else {
        return Ok(None);
    };
    Ok(Some(crate::keys::open_hosted(&app.db.c(), master, project, workspace, name, &sealed)?))
}

/// `OWNER/NAME` of a GitHub clone URL.
pub fn github_repo(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if u.host_str()? != "github.com" {
        return None;
    }
    let mut parts = u.path_segments()?.filter(|p| !p.is_empty());
    let (owner, name) = (parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let name = name.strip_suffix(".git").unwrap_or(name);
    Some(format!("{owner}/{name}"))
}

/// A read-only installation token for `url`, when it's a GitHub repo
/// linked to `project` and the platform has the App.
async fn app_token(app: &Platform, project: &str, url: &str) -> Option<String> {
    let gh = app.github.as_ref()?;
    let name = github_repo(url)?;
    let link = app.db.linked_repo_named(&name).ok().flatten().filter(|l| l.project_id == project)?;
    match gh.hand_out(&Caller::of(app, "platform"), link.installation_id, Repo::Id(link.repo_id)).await {
        Ok(t) => Some(t.secret().to_owned()),
        Err(e) => {
            warn!(project, repo = name, error = %format!("{e:#}"), "no installation token for a hosted workspace's clone");
            None
        }
    }
}

/// What workspace `workspace` of `project` sets up, for `machine`: `None`
/// when there's no such workspace, or it isn't that machine's.
pub async fn plan(app: &Platform, project: &str, workspace: &str, machine: &str) -> anyhow::Result<Option<SetupPlan>> {
    let Some(w) = app.db.hosted_workspace(project, workspace)? else { return Ok(None) };
    if w.id != workspace || !its_box(&w, machine) {
        return Ok(None);
    }
    let token = secret(app, project, &w.id, GIT_TOKEN)?;
    let user = secret(app, project, &w.id, GIT_USERNAME)?.filter(|u| !u.is_empty());
    let mut repos = Vec::new();
    for url in &w.spec.repos {
        let r = if let Some(t) = app_token(app, project, url).await {
            SetupRepo {
                url: url.clone(),
                via: "github_app".into(),
                username: Some(TOKEN_USER.into()),
                password: Some(t),
            }
        } else if let Some(t) = &token {
            SetupRepo {
                url: url.clone(),
                via: "secret".into(),
                username: Some(user.clone().unwrap_or_else(|| TOKEN_USER.into())),
                password: Some(t.clone()),
            }
        } else {
            SetupRepo { url: url.clone(), via: "none".into(), username: None, password: None }
        };
        repos.push(r);
    }
    let (mut env, mut unset) = (BTreeMap::new(), Vec::new());
    for name in &w.spec.env {
        match secret(app, project, &w.id, name)? {
            Some(v) => {
                env.insert(name.clone(), v);
            }
            None => unset.push(name.clone()),
        }
    }
    let spec_hash = w.spec.hash();
    app.db.hosted_built_from(&w.id, &spec_hash)?;
    // Which repos, and how each is cloned; never a token or a value.
    let via: Vec<&str> = repos.iter().map(|r| r.via.as_str()).collect();
    info!(project, workspace = w.id, machine, ?via, env = env.len(), "a hosted box read its setup");
    Ok(Some(SetupPlan {
        workspace: w.id,
        spec_hash,
        repos,
        packages: w.spec.packages,
        setup: w.spec.setup,
        env,
        unset,
    }))
}

/// Keep what `machine` says of workspace `workspace`'s setup: whether it was
/// kept (`false`: no such workspace, or not that machine's box).
pub fn report(app: &Platform, project: &str, workspace: &str, machine: &str, r: &SetupReport) -> anyhow::Result<bool> {
    let Some(w) = app.db.hosted_workspace(project, workspace)? else { return Ok(false) };
    if w.id != workspace || !its_box(&w, machine) {
        return Ok(false);
    }
    let mut r = r.clone();
    r.step = r.step.chars().filter(|c| !c.is_control()).take(200).collect();
    if !SETUP_REASONS.contains(&r.reason.as_str()) {
        r.reason = String::new();
    }
    let kept = app.db.report_hosted_setup(&w.id, machine, &r, arugula_e2e::now_ms())?;
    if kept {
        info!(project, workspace = w.id, status = r.status.as_str(), step = r.step, "a hosted box's setup");
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_urls() {
        assert_eq!(github_repo("https://github.com/acme/app").as_deref(), Some("acme/app"));
        assert_eq!(github_repo("https://github.com/acme/app.git").as_deref(), Some("acme/app"));
        assert_eq!(github_repo("https://github.com/acme/app/").as_deref(), Some("acme/app"));
        assert_eq!(github_repo("https://github.com/acme"), None);
        assert_eq!(github_repo("https://github.com/acme/app/tree/main"), None);
        assert_eq!(github_repo("https://git.example/acme/app"), None);
    }
}
