//! A merged PR closes the native issues it names (Arugula #432) and moves the
//! Plane items it names to Done (#437). The link a
//! PR makes is the one the boards already read ([`crate::boards::refs`],
//! [`crate::boards::auto`]): `Arugula #N` or `Closes #N` in its title or body,
//! `(#N)` in its title, or a branch `feat/N-…`; and the issue of any row a
//! person or an agent attached it to as work. Nobody writes a keyword or
//! tends a board. `part of`, `refs` or `see` before `Arugula #N` or `KEY-N`
//! only links, and so does any other `#N` in the body, such as `(#N)` in
//! passing (Arugula #743); a branch's number the text names softly too
//! (#755). The `closed` event keeps the words that closed it (`by`).
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
//! **Not finished** (Arugula #841): a ticket marked so ([`crate::db::item_holds`])
//! isn't closed by a merge, native or Plane; the PR still attaches to its
//! row. The hold remembers the merge it kept from closing the ticket
//! ([`Skipped`]); [`released`] runs when the mark comes off, and closes the
//! ticket now, naming that PR. A hold set after the merge remembers none: the
//! release finds the PR as a merge does, by its references, among the
//! project's merged PRs that mention the ticket and those attached to its
//! row; there a PR whose body the mirror doesn't keep is read by its title
//! and branch only.
//!
//! A Plane item is not moved here: after the native closes, this queues a job
//! for each ([`crate::trackers::done`]), so a slow or failing Plane never
//! delays or fails the webhook, and the job retries and sends once.
//!
//! The body is read from the content the mirror keeps, which is kept only
//! while the project mirrors content (`db/content.rs`). With that off a PR
//! has no body here, so a merge brings its own: the webhook's payload has it,
//! and the poller reads it once from GitHub for a PR it just saw merged
//! ([`merged_with`]). It is used for this pass only: never stored, never
//! logged. One limit: a PR the poller saw merged whose one read failed (the
//! budget, GitHub's error) is named by its title and branch only, as the
//! next turn does not look at it again.

use std::collections::HashMap;

use arugula_e2e::now_ms;
use serde_json::Value;
use tracing::warn;

use crate::{
    Platform,
    boards::refs::{Refs, references},
    db::{
        boards::ClosedBy,
        forge::MergedPr,
        issues::{Actor, CloseReason, Patch, State},
        item_holds::Skipped,
    },
    trackers::done,
};

/// Who an issue is closed as when no member of its project merged the PR:
/// the App, named as GitHub's text is in a thread (`github:<login>`).
pub const APP: &str = "github:arugula";

/// The words of a close by a PR attached to the issue's row, naming nothing.
pub const ATTACHED: &str = "attached to its board row as work";

/// A `pull_request` webhook, after the mirror wrote it: the issues the PR
/// closes if it is merged now. Its body is the mirror's, which the same
/// delivery wrote, so this runs after [`super::ingest::webhook`]; the
/// delivery's own stands in when the mirror keeps none.
pub async fn webhook(p: &Platform, event: &str, v: &Value) {
    if event != "pull_request" {
        return;
    }
    if let (Some(repo), Some(number)) = (v["repository"]["id"].as_u64(), v["pull_request"]["number"].as_u64()) {
        let bodies: HashMap<u64, String> =
            v["pull_request"]["body"].as_str().map(|b| (number, b.to_owned())).into_iter().collect();
        merged_with(p, repo, &[number], &bodies).await;
    }
}

/// The PRs of `repo` among these `numbers` that are merged: each closes the
/// open native issues of the repo's project that it names.
pub async fn merged(p: &Platform, repo: u64, numbers: &[u64]) {
    merged_with(p, repo, numbers, &HashMap::new()).await;
}

/// [`merged`], with the body of some PRs (by number) to read where the
/// mirror keeps none.
pub async fn merged_with(p: &Platform, repo: u64, numbers: &[u64], bodies: &HashMap<u64, String>) {
    let link = match p.db.linked_repo(repo) {
        Ok(Some(l)) if l.unlinked_at.is_none() => l,
        Ok(_) => return,
        Err(e) => return warn!(error = %e, "closing issues: finding the project of a repo"),
    };
    let prs = match p.db.merged_prs(repo, numbers) {
        Ok(prs) => prs,
        Err(e) => return warn!(error = %e, "closing issues: reading merged PRs"),
    };
    for mut pr in prs {
        if pr.body.is_none() {
            pr.body = bodies.get(&pr.number).cloned();
        }
        if let Err(e) = close_for(p, &link.project_id, (repo, &link.full_name), &pr, None).await {
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

/// Close the open native issues `pr` names, but for a held one; `only`, an
/// issue's id, leaves out the others (a release closes just its own).
async fn close_for(
    p: &Platform,
    project: &str,
    repo: (u64, &str),
    pr: &MergedPr,
    only: Option<&str>,
) -> anyhow::Result<()> {
    let refs = named_by(pr);
    let mut issues = vec![];
    for (id, by) in p.db.issues_closed_by(project, repo.0, &pr.id, &refs)? {
        if only.is_some_and(|o| o != id) {
            continue;
        }
        let Some(i) = p.db.issue(&id)?.filter(|i| i.state == State::Open) else { continue };
        let words = match by {
            ClosedBy::Native(n) => refs.words(true, n),
            ClosedBy::Number(n) => refs.words(false, n),
            ClosedBy::Row => None,
        };
        let words = words.unwrap_or(ATTACHED).to_owned();
        if p.db.hold_of(project, &id)?.is_some() {
            let skipped = Skipped {
                pr: pr.id.clone(),
                reference: format!("{}#{}", repo.1, pr.number),
                words: Some(words),
                merged_by: pr.merged_by,
            };
            p.db.hold_skipped(project, &id, &skipped)?;
            continue;
        }
        issues.push((i, words));
    }
    if issues.is_empty() {
        return Ok(());
    }
    let reference = format!("{}#{}", repo.1, pr.number);
    close_issues(p, project, issues, &reference, pr.merged_by).await
}

/// Close `issues`, with the words that named each, as completed by the
/// merger of `reference`'s PR (GitHub's id) or the App.
async fn close_issues(
    p: &Platform,
    project: &str,
    issues: Vec<(crate::db::issues::Issue, String)>,
    reference: &str,
    merged_by: Option<u64>,
) -> anyhow::Result<()> {
    let merger = merger(p, project, merged_by).await;
    let by = Actor::person(merger.as_deref().unwrap_or(APP));
    for (i, words) in issues {
        let patch = Patch {
            state: Some(State::Closed(CloseReason::Completed)),
            by_pr: Some(reference),
            by_words: Some(&words),
            ..Patch::default()
        };
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
        if let Some(t) = p.db.tracker_target(project, &format!("{}-{number}", c.identifier))?
            && p.db.hold_of(project, &t.id)?.is_some()
        {
            let skipped = Skipped {
                pr: pr.id.clone(),
                reference: format!("{repo}#{}", pr.number),
                words: None,
                merged_by: pr.merged_by,
            };
            p.db.hold_skipped(project, &t.id, &skipped)?;
            continue;
        }
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

/// The mark Not finished came off `item` (an `iss_` or `fi_` id) of
/// `project`: if a merged PR that would have closed it exists, close it now,
/// as a merge does, naming that PR. A native issue closes here; a Plane
/// item's move to Done is queued, as a merge queues it. Nothing merged, or
/// the ticket closed already: nothing happens.
pub async fn released(p: &Platform, project: &str, item: &str, skipped: Option<Skipped>) {
    if let Err(e) = release(p, project, item, skipped).await {
        warn!(project, item, error = format!("{e:#}"), "closing a ticket whose Not finished mark came off");
    }
}

async fn release(p: &Platform, project: &str, item: &str, skipped: Option<Skipped>) -> anyhow::Result<()> {
    if item.starts_with("iss_") {
        let Some(issue) = p.db.issue(item)?.filter(|i| i.project == project && i.state == State::Open) else {
            return Ok(());
        };
        if let Some(s) = skipped {
            let words = s.words.unwrap_or_else(|| ATTACHED.to_owned());
            return close_issues(p, project, vec![(issue, words)], &s.reference, s.merged_by).await;
        }
        let mut found = p.db.merged_prs_attached_to(project, item)?;
        for needle in std::iter::once(issue.number).chain(issue.github.as_ref().map(|g| g.number)) {
            found.extend(p.db.merged_prs_mentioning(project, &needle.to_string())?);
        }
        found.sort_by_key(|m| std::cmp::Reverse(m.pr.number));
        let mut seen = vec![];
        for m in found {
            if seen.contains(&m.pr.id) {
                continue;
            }
            seen.push(m.pr.id.clone());
            close_for(p, project, (m.repo, &m.full_name), &m.pr, Some(item)).await?;
            if p.db.issue(item)?.is_none_or(|i| i.state != State::Open) {
                break;
            }
        }
        return Ok(());
    }
    let Some(t) = p.db.tracker_target(project, item)? else { return Ok(()) };
    let Some(c) = p.db.tracker_connection(project, &t.connection_id)?.filter(|c| c.unlinked_at.is_none()) else {
        return Ok(());
    };
    let found = match skipped {
        Some(s) => Some((s.pr, s.reference, s.merged_by)),
        None => {
            let key = format!("{}-{}", t.identifier, t.number);
            let mut found = None;
            for m in p.db.merged_prs_mentioning(project, &key)? {
                let named = named_by(&m.pr);
                if named.closing_plane().any(|(k, n)| *n == t.number && k.eq_ignore_ascii_case(&t.identifier)) {
                    found = Some((m.pr.id.clone(), format!("{}#{}", m.full_name, m.pr.number), m.pr.merged_by));
                    break;
                }
            }
            found
        }
    };
    if let Some((pr, reference, merged_by)) = found {
        let to =
            done::Move { project: project.to_owned(), connection: c.id, number: t.number, pr: reference, merged_by };
        done::queue_released(&p.db, &pr, &to, now_ms())?;
    }
    Ok(())
}
