//! A merged PR closes the native issues it names (Arugula #432) and moves the
//! Plane items it names to Done (#437). The link a
//! PR makes is the one the boards already read ([`crate::boards::refs`],
//! [`crate::boards::auto`]): `Arugula #N`, `Closes #N`, `(#N)` or a branch
//! `feat/N-…` in its title, branch or body; and the issue of any row a person
//! or an agent attached it to as work. Nobody writes a keyword or tends a
//! board. `part of`, `refs` or `see` before `Arugula #N` or `KEY-N` only links.
//!
//! It runs wherever the mirror learns of a PR: a webhook ([`webhook`]) and
//! the poller's turns ([`merged`]). Only an issue of the project the PR's
//! repo is linked to is closed, board or no board, as completed, and only an
//! open one: an issue closed already (for any reason) stays as it is, a PR
//! closed without merging closes nothing, and an issue is never reopened, so
//! a redelivery or a second pass over the same PR adds nothing. It is closed
//! as the person who merged the PR when they are in the project, else as the
//! App; the event names the PR.
//!
//! A Plane item is not moved here: after the native closes, this queues a job
//! for each ([`crate::trackers::done`]), so a slow or failing Plane never
//! delays or fails the webhook, and the job retries and sends once.

use arugula_e2e::now_ms;
use serde_json::Value;
use tracing::warn;

use crate::{
    Platform,
    boards::refs::{Refs, references},
    db::{
        forge::MergedPr,
        issues::{Actor, CloseReason, Patch, State},
    },
    trackers::done,
};

/// Who an issue is closed as when no member of its project merged the PR:
/// the App, named as GitHub's text is in a thread (`github:<login>`).
pub const APP: &str = "github:arugula";

/// A `pull_request` webhook, after the mirror wrote it: the issues the PR
/// closes if it is merged now. Its body is read from the content the same
/// delivery wrote, so this runs after [`super::ingest::webhook`].
pub async fn webhook(p: &Platform, event: &str, v: &Value) {
    if event != "pull_request" {
        return;
    }
    if let (Some(repo), Some(number)) = (v["repository"]["id"].as_u64(), v["pull_request"]["number"].as_u64()) {
        merged(p, repo, &[number]).await;
    }
}

/// The PRs of `repo` among these `numbers` that are merged: each closes the
/// open native issues of the repo's project that it names.
pub async fn merged(p: &Platform, repo: u64, numbers: &[u64]) {
    let link = match p.db.linked_repo(repo) {
        Ok(Some(l)) if l.unlinked_at.is_none() => l,
        Ok(_) => return,
        Err(e) => return warn!(error = %e, "closing issues: finding the project of a repo"),
    };
    let prs = match p.db.merged_prs(repo, numbers) {
        Ok(prs) => prs,
        Err(e) => return warn!(error = %e, "closing issues: reading merged PRs"),
    };
    for pr in prs {
        if let Err(e) = close_for(p, &link.project_id, (repo, &link.full_name), &pr).await {
            warn!(repo, number = pr.number, error = format!("{e:#}"), "closing the issues a merged PR names");
        }
        if let Err(e) = queue_plane(p, &link.project_id, &link.full_name, &pr) {
            warn!(repo, number = pr.number, error = format!("{e:#}"), "queueing the Plane items a merged PR names");
        }
    }
}

/// What a merged PR names, by the boards' own rule ([`references`] over its
/// title, head branch and body): native issues in `numbers` and `native`
/// (`native` less the softly written, [`Refs::closing_native`]) and Plane
/// items in `plane`. One place for every thing a merge moves; Plane's Done
/// reads `plane` of the same result.
pub fn named_by(pr: &MergedPr) -> Refs {
    references(&pr.title, pr.head_ref.as_deref().unwrap_or(""), pr.body.as_deref().unwrap_or(""), Some(pr.number))
}

async fn close_for(p: &Platform, project: &str, repo: (u64, &str), pr: &MergedPr) -> anyhow::Result<()> {
    let refs = named_by(pr);
    let mut issues = vec![];
    for id in p.db.issues_closed_by(project, repo.0, &pr.id, &refs)? {
        if let Some(i) = p.db.issue(&id)?.filter(|i| i.state == State::Open) {
            issues.push(i);
        }
    }
    if issues.is_empty() {
        return Ok(());
    }
    let merger = merger(p, project, pr.merged_by).await;
    let by = Actor::person(merger.as_deref().unwrap_or(APP));
    let reference = format!("{}#{}", repo.1, pr.number);
    for i in issues {
        let patch =
            Patch { state: Some(State::Closed(CloseReason::Completed)), by_pr: Some(&reference), ..Patch::default() };
        let Some((issue, events)) = p.db.patch_issue(&i.id, &patch, by, now_ms())? else { continue };
        if !events.is_empty() {
            // As `issues::change` tells those who show it.
            p.chat.object_changed(&p.db, &format!("project:{}", issue.project), &issue.id);
            crate::boards::issue_changed(p, &issue.id);
        }
    }
    Ok(())
}

/// A job for each Plane item of `project` that `pr` names plainly, to move it
/// to Done. One job to a PR and item, so a redelivery or a second pass adds
/// none.
fn queue_plane(p: &Platform, project: &str, repo: &str, pr: &MergedPr) -> anyhow::Result<()> {
    let refs = named_by(pr);
    let closing: Vec<_> = refs.closing_plane().cloned().collect();
    if closing.is_empty() {
        return Ok(());
    }
    for (c, number) in crate::boards::auto::plane_items(p, project, &closing)? {
        let to = done::Move {
            project: project.to_owned(),
            connection: c.id,
            number,
            pr: format!("{repo}#{}", pr.number),
            merged_by: pr.merged_by,
        };
        done::queue(&p.db, &pr.id, &to)?;
    }
    Ok(())
}

/// The account of whoever merged (by GitHub's id), if they are in `project`.
pub(crate) async fn merger(p: &Platform, project: &str, github: Option<u64>) -> Option<String> {
    let account = p.hub.github_account(github?).await.ok().flatten()?;
    let project = p.db.issue_project(project).ok().flatten()?;
    let member = match (&project.account, &project.team) {
        (Some(a), _) => *a == account,
        (None, Some(t)) => p.cache.team(&*p.hub, t).await.ok().flatten().is_some_and(|t| t.role(&account).is_some()),
        _ => false,
    };
    member.then_some(account)
}
