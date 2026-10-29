//! A project's work items (boards, the picker and the inbox):
//! its native issues, its mirrored GitHub issues and PRs, and its runs, as
//! pointers a row takes as they are ([`wire::PROJECT_WORK`]). Read by the
//! members who may read the project, behind the `boards` flag, as boards
//! are.
//!
//! **Per viewer:** a mirrored item shows only from a linked repo the reader
//! may read now ([`viewer::check`], as the project's items list does).
//! **Untriaged** is per project: an open issue that no row of any board of
//! this project points at; another project's board doesn't triage it.

use std::{collections::HashSet, sync::Arc};

use arugula_control_common::ApiError;
use arugula_control_wire::boards as wire;
use axum::{
    Json,
    extract::{Path, Query, State},
};

use super::{J, bad, place};
use crate::{
    Platform,
    auth::Person,
    db::{
        forge::ItemFilter,
        issues::{Actor, Filter},
        runs::{Run, RunFilter},
    },
    forge::{
        items::names,
        viewer::{self, Viewer},
    },
    issues::{ISSUE_SEARCHES, words},
};

const DEFAULT: u32 = 30;
const MOST: u32 = 100;

/// `GET /api/projects/{project}/work`.
pub async fn route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Query(q): Query<wire::WorkQuery>,
) -> J<wire::WorkItems> {
    Ok(Json(list(&app, &s.account, &project, &q).await?))
}

/// A run as a picker lists it: labelled as a board labels it, and as
/// recent as its newest moment.
fn run_item(r: &Run) -> wire::WorkItem {
    wire::WorkItem {
        pointer: wire::Pointer::Run { id: r.id.clone() },
        source: "run".to_owned(),
        kind: "run".to_owned(),
        repo: None,
        number: None,
        title: format!("{} run", r.trigger.as_str()),
        state: r.state.as_str().to_owned(),
        assignees: vec![],
        updated_at: r.ended_at.max(r.started_at).unwrap_or(0).max(r.created_at),
    }
}

/// The project's work as `account` may read it, newest first.
pub async fn list(
    app: &Platform,
    account: &str,
    asked: &str,
    q: &wire::WorkQuery,
) -> Result<wire::WorkItems, ApiError> {
    let project = place(app, Actor::person(account), asked, false).await?;
    let kind = q.kind.as_deref().map(str::trim).filter(|k| !k.is_empty());
    if let Some(k) = kind
        && !matches!(k, "issue" | "pr" | "run")
    {
        return Err(bad(&format!("kind is issue, pr or run, not {k:?}")));
    }
    let limit = q.limit.unwrap_or(DEFAULT).clamp(1, MOST);
    let text = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    // Untriaged is issues only: no PRs, no runs.
    let (issues, prs, runs) = match kind {
        None => (true, !q.untriaged, !q.untriaged),
        Some(k) => (k == "issue", k == "pr" && !q.untriaged, k == "run" && !q.untriaged),
    };
    // Rows already on a board drop out after the read, so read that many more.
    let on_boards: HashSet<String> = if q.untriaged { app.db.project_board_targets(&project)? } else { HashSet::new() };
    let read = limit.saturating_add(u32::try_from(on_boards.len()).unwrap_or(u32::MAX));
    let mut out: Vec<wire::WorkItem> = Vec::new();
    // The (repo, number) of each native issue read that was exported to
    // GitHub: its mirrored copy is the same work.
    let mut exported: HashSet<(u64, u64)> = HashSet::new();

    if issues {
        let words = words(text.unwrap_or(""));
        if !words.is_empty() {
            app.limits.check_account(ISSUE_SEARCHES, account)?;
        }
        let f = Filter { open: q.untriaged.then_some(true), words: &words, ..Default::default() };
        // Native issues are read by number, newest first, not by `updated_at`.
        for i in app.db.issues(&project, &f, None, read)?.0 {
            if let Some(g) = &i.github {
                exported.insert((g.repo_id, g.number));
            }
            if on_boards.contains(&i.id) {
                continue;
            }
            out.push(wire::WorkItem {
                pointer: wire::Pointer::Issue { id: i.id },
                source: "native".to_owned(),
                kind: "issue".to_owned(),
                repo: None,
                number: Some(i.number),
                title: i.title,
                state: i.state.as_str().to_owned(),
                assignees: i.assignees,
                updated_at: i.updated_at,
            });
        }
    }

    if issues || prs {
        let repos = app.db.linked_repos(&project)?;
        let checked = viewer::check(app, &Viewer::of(app, account).await?, &repos).await?;
        let f = ItemFilter {
            kind: match (issues, prs) {
                (true, true) => None,
                (true, false) => Some("issue"),
                _ => Some("pr"),
            },
            state: q.untriaged.then_some("open"),
            text,
            limit: read,
            ..Default::default()
        };
        for i in app.db.forge_items(&checked.readable, &f)? {
            if on_boards.contains(&i.id) || exported.contains(&(i.repo_id, i.number)) {
                continue;
            }
            let repo = repos.iter().find(|r| r.repo_id == i.repo_id).map(|r| r.full_name.clone());
            out.push(wire::WorkItem {
                pointer: wire::Pointer::ForgeItem { id: i.id },
                source: "github".to_owned(),
                kind: i.kind,
                repo,
                number: Some(i.number),
                title: i.title,
                state: i.state,
                assignees: names(&i.assignees),
                updated_at: i.updated_at,
            });
        }
    }

    if runs {
        // A run matches `q` by its trigger's name, so a search reads as
        // many as a list may and keeps those.
        let scan = if text.is_some() { MOST } else { limit };
        let needle = text.map(str::to_ascii_lowercase);
        for r in app.db.runs_of(&project, &RunFilter::default(), scan)? {
            if needle.as_deref().is_none_or(|n| r.trigger.as_str().contains(n)) {
                out.push(run_item(&r));
            }
        }
    }

    out.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    out.truncate(limit as usize);
    name_assignees(app, &mut out).await?;
    Ok(wire::WorkItems { project, items: out })
}

/// Name the accounts a native issue is assigned to, as a board card does:
/// their login, else their name, else `someone who left`. One read of the
/// hub's accounts for the whole list, and none when nobody is assigned.
/// Mirrored items keep the GitHub logins they came with.
async fn name_assignees(app: &Platform, items: &mut [wire::WorkItem]) -> Result<(), ApiError> {
    let native = |i: &wire::WorkItem| matches!(i.pointer, wire::Pointer::Issue { .. });
    let mut ids: Vec<String> = items.iter().filter(|i| native(i)).flat_map(|i| i.assignees.iter().cloned()).collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(());
    }
    let accounts = app.cache.accounts(&*app.hub, &ids).await?;
    for i in items.iter_mut().filter(|i| native(i)) {
        for a in &mut i.assignees {
            *a = match accounts.get(a.as_str()) {
                Some(acc) if !acc.login.is_empty() => acc.login.clone(),
                Some(acc) if !acc.name.is_empty() => acc.name.clone(),
                _ => "someone who left".to_owned(),
            };
        }
    }
    Ok(())
}
