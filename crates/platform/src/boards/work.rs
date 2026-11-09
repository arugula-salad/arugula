//! A project's work items (boards, the picker and the inbox):
//! its native issues, its mirrored GitHub issues and PRs, and its runs, as
//! pointers a row takes as they are ([`wire::PROJECT_WORK`]). Read by the
//! members who may read the project, as boards are.
//!
//! **Per viewer:** a mirrored item shows only from a linked repo the reader
//! may read now ([`viewer::check`], as the project's items list does).
//! A connected Plane project's items (`source: "plane"`) are read the same
//! way, by any member ([`viewer::check_connections`]).
//! **Untriaged** is per project: an open issue that no row of any board of
//! this project points at; another project's board doesn't triage it.
//! An untriaged list is read whole, then counted, filtered by label and cut
//! to a page, so its total, its labels and its pages all come from one
//! set; a page's cursor is where it ended in the order (`updated_at`, then
//! id), so an item that leaves the list between pages moves nothing.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

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
        trackers::TrackerFilter,
    },
    forge::{
        items::names,
        viewer::{self, Viewer},
    },
    issues::{ISSUE_SEARCHES, words},
    trackers,
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
fn run_item(r: &Run) -> wire::RowCandidate {
    wire::RowCandidate {
        pointer: wire::Pointer::Run { id: r.id.clone() },
        source: "run".to_owned(),
        kind: "run".to_owned(),
        repo: None,
        identifier: None,
        url: None,
        number: None,
        title: format!("{} run", r.trigger.as_str()),
        state: r.state.as_str().to_owned(),
        assignees: vec![],
        labels: vec![],
        updated_at: r.ended_at.max(r.started_at).unwrap_or(0).max(r.created_at),
    }
}

/// The most recently updated first, or the least with `order=old`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    New,
    Old,
}

/// The id a pointer in a work list carries.
fn key(p: &wire::Pointer) -> &str {
    match p {
        wire::Pointer::Issue { id } | wire::Pointer::ForgeItem { id } | wire::Pointer::Run { id } => id,
        _ => "",
    }
}

/// An item's place in a list: the sort key, so a larger one is later.
fn place_of(order: Order, updated_at: u64, id: &str) -> (u64, String) {
    match order {
        Order::New => (u64::MAX - updated_at, id.to_owned()),
        Order::Old => (updated_at, id.to_owned()),
    }
}

/// What `next` says: where the page ended.
fn cursor_of(i: &wire::RowCandidate) -> String {
    format!("{}:{}", i.updated_at, key(&i.pointer))
}

fn parse_cursor(c: &str) -> Result<(u64, &str), ApiError> {
    c.split_once(':')
        .and_then(|(at, id)| Some((at.parse().ok()?, id)))
        .ok_or_else(|| bad("that cursor is not one this list gave: start again without it"))
}

/// The project's work as `account` may read it, newest first.
pub async fn list(
    app: &Platform,
    account: &str,
    asked: &str,
    q: &wire::WorkQuery,
) -> Result<wire::WorkItems, ApiError> {
    let order = match q.order.as_deref() {
        None | Some("new") => Order::New,
        Some("old") => Order::Old,
        Some(o) => return Err(bad(&format!("order is new or old, not {o:?}"))),
    };
    let label = q.label.as_deref().map(str::trim).filter(|l| !l.is_empty());
    if !q.untriaged && (q.cursor.is_some() || label.is_some() || order == Order::Old) {
        return Err(bad("a cursor, a label and order=old go with untriaged=true"));
    }
    let after = q.cursor.as_deref().map(parse_cursor).transpose()?;
    let limit = q.limit.unwrap_or(DEFAULT).clamp(1, MOST);
    let (project, mut out) = gather(app, account, asked, q, limit).await?;
    let (mut next, mut total, mut labels) = (None, None, vec![]);
    if q.untriaged {
        // The labels count everything that matches the words, `label` aside.
        let mut counts: HashMap<&str, u64> = HashMap::new();
        for i in &out {
            for l in i.labels.iter().collect::<HashSet<_>>() {
                *counts.entry(l).or_default() += 1;
            }
        }
        let mut facet: Vec<wire::WorkLabel> =
            counts.into_iter().map(|(name, count)| wire::WorkLabel { name: name.to_owned(), count }).collect();
        facet.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        labels = facet;
        if let Some(l) = label {
            out.retain(|i| i.labels.iter().any(|x| x == l));
        }
        total = Some(out.len() as u64);
        out.sort_by_cached_key(|i| place_of(order, i.updated_at, key(&i.pointer)));
        if let Some((at, id)) = after {
            let past = place_of(order, at, id);
            out.retain(|i| place_of(order, i.updated_at, key(&i.pointer)) > past);
        }
        if out.len() > limit as usize {
            out.truncate(limit as usize);
            next = out.last().map(cursor_of);
        }
    } else {
        out.truncate(limit as usize);
    }
    name_assignees(app, &mut out).await?;
    Ok(wire::WorkItems { project, items: out, next, total, labels })
}

/// Every untriaged item that matches `text` and `label`, newest first, as
/// [`list`] pages them: what an add-many by query adds. `project` is the
/// project's id or `personal`.
pub async fn untriaged(
    app: &Platform,
    account: &str,
    project: &str,
    text: Option<&str>,
    label: Option<&str>,
) -> Result<(String, Vec<wire::RowCandidate>), ApiError> {
    let q = wire::WorkQuery { q: text.map(Into::into), untriaged: true, ..Default::default() };
    let (project, mut out) = gather(app, account, project, &q, MOST).await?;
    if let Some(l) = label.map(str::trim).filter(|l| !l.is_empty()) {
        out.retain(|i| i.labels.iter().any(|x| x == l));
    }
    out.sort_by_cached_key(|i| place_of(Order::New, i.updated_at, key(&i.pointer)));
    Ok((project, out))
}

/// The items that match `q` in every source, newest first. An untriaged
/// read has no limit (the caller cuts it); any other reads `limit` of each
/// source.
async fn gather(
    app: &Platform,
    account: &str,
    asked: &str,
    q: &wire::WorkQuery,
    limit: u32,
) -> Result<(String, Vec<wire::RowCandidate>), ApiError> {
    let project = place(app, Actor::person(account), asked, false).await?;
    let kind = q.kind.as_deref().map(str::trim).filter(|k| !k.is_empty());
    if let Some(k) = kind
        && !matches!(k, "issue" | "pr" | "run")
    {
        return Err(bad(&format!("kind is issue, pr or run, not {k:?}")));
    }
    let text = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    // Untriaged is issues only: no PRs, no runs.
    let (issues, prs, runs) = match kind {
        None => (true, !q.untriaged, !q.untriaged),
        Some(k) => (k == "issue", k == "pr" && !q.untriaged, k == "run" && !q.untriaged),
    };
    // Rows already on a board drop out after the read, so read that many
    // more; an untriaged read takes them all.
    let on_boards: HashSet<String> = if q.untriaged { app.db.project_board_targets(&project)? } else { HashSet::new() };
    // The same work on a board under another issue (the native issue and its
    // GitHub copy) drops out too.
    let worked: HashSet<String> = if q.untriaged { app.db.project_board_work_items(&project)? } else { HashSet::new() };
    let read = if q.untriaged {
        u32::MAX
    } else {
        limit.saturating_add(u32::try_from(on_boards.len() + worked.len()).unwrap_or(u32::MAX))
    };
    let mut out: Vec<wire::RowCandidate> = Vec::new();
    // The work of each native issue read: its mirrored copy is the same work,
    // and the picker lists the native one.
    let mut native: HashSet<String> = HashSet::new();

    if issues {
        let words = words(text.unwrap_or(""));
        if !words.is_empty() {
            app.limits.check_account(ISSUE_SEARCHES, account)?;
        }
        let f = Filter { open: q.untriaged.then_some(true), words: &words, ..Default::default() };
        // Native issues are read by number, newest first, not by `updated_at`.
        let read_issues = app.db.issues(&project, &f, None, read)?.0;
        let ids: Vec<&str> = read_issues.iter().map(|i| i.id.as_str()).collect();
        let work = app.db.work_identities(&ids)?;
        for i in read_issues {
            let same = work.get(&i.id).cloned().unwrap_or_default();
            native.insert(same.clone());
            if on_boards.contains(&i.id) || worked.contains(&same) {
                continue;
            }
            out.push(wire::RowCandidate {
                pointer: wire::Pointer::Issue { id: i.id },
                source: "native".to_owned(),
                kind: "issue".to_owned(),
                repo: None,
                identifier: None,
                url: None,
                number: Some(i.number),
                title: i.title,
                state: i.state.as_str().to_owned(),
                assignees: i.assignees,
                labels: i.labels,
                updated_at: i.updated_at,
            });
        }
    }

    if issues || prs {
        let repos = app.db.linked_repos(&project)?;
        let checked = viewer::check(app, &mut Viewer::of(app, account).await?, &repos).await?;
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
        let read_items = app.db.forge_items(&checked.readable, &f)?;
        let ids: Vec<&str> = read_items.iter().filter(|i| i.kind == "issue").map(|i| i.id.as_str()).collect();
        let work = app.db.work_identities(&ids)?;
        for i in read_items {
            let same = work.get(&i.id);
            if on_boards.contains(&i.id) || same.is_some_and(|w| native.contains(w) || worked.contains(w)) {
                continue;
            }
            let repo = repos.iter().find(|r| r.repo_id == i.repo_id).map(|r| r.full_name.clone());
            out.push(wire::RowCandidate {
                pointer: wire::Pointer::ForgeItem { id: i.id },
                source: "github".to_owned(),
                kind: i.kind,
                repo,
                identifier: None,
                url: None,
                number: Some(i.number),
                title: i.title,
                state: i.state,
                assignees: names(&i.assignees),
                labels: names(&i.labels),
                updated_at: i.updated_at,
            });
        }
    }

    if issues {
        // A connection's open items, by the same rule: any member of the project reads them.
        let viewer = Viewer::of(app, account).await?;
        let conns = viewer::check_connections(app, &viewer, &app.db.tracker_connections(&project)?).await?;
        let f = TrackerFilter { open: q.untriaged, text, limit: read, ..Default::default() };
        let read_items = app.db.tracker_items(&conns, &f)?;
        let ids: Vec<&str> = read_items.iter().map(|i| i.id.as_str()).collect();
        let work = app.db.work_identities(&ids)?;
        for i in read_items {
            if on_boards.contains(&i.id) || work.get(&i.id).is_some_and(|w| worked.contains(w)) {
                continue;
            }
            let Some(c) = conns.get(i.mirror_id) else { continue };
            out.push(wire::RowCandidate {
                pointer: wire::Pointer::ForgeItem { id: i.id },
                source: "plane".to_owned(),
                kind: "issue".to_owned(),
                repo: Some(c.name.clone()),
                identifier: Some(c.identifier.clone()),
                url: Some(trackers::item_url(&c.workspace_slug, &c.identifier, i.number)),
                number: Some(i.number),
                title: i.title,
                state: i.state,
                assignees: vec![],
                labels: i.labels,
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
    Ok((project, out))
}

/// Name the accounts a native issue is assigned to, as a board card does:
/// their login, else their name, else `someone who left`. One read of the
/// hub's accounts for the whole list, and none when nobody is assigned.
/// Mirrored items keep the GitHub logins they came with.
async fn name_assignees(app: &Platform, items: &mut [wire::RowCandidate]) -> Result<(), ApiError> {
    let native = |i: &wire::RowCandidate| matches!(i.pointer, wire::Pointer::Issue { .. });
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
