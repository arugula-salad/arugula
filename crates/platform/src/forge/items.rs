//! Reading the forge mirror (M165): a project's PRs and issues, one item,
//! its checks and its events. Every handler takes a [`Viewer`] and reads
//! through [`viewer::check`]'s [`Readable`](viewer::Readable): a list
//! leaves out each repo the viewer may not read (and says why), and an
//! item they may not read is no item at all (404).

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects as wire;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::Value;

use arugula_control_wire::boards::IssueRef;

use super::viewer::{self, ReadableConnection, Viewer};
use crate::{
    Platform,
    db::{
        forge::{ForgeItem, ItemFilter},
        trackers::{TrackerFilter, TrackerItem},
    },
    trackers,
};

/// How many items a list holds at most, and when it doesn't say.
const MOST: u32 = 200;
const DEFAULT: u32 = 100;

/// A JSON list of names or logins as stored; anything else is none.
pub(crate) fn names(json: &str) -> Vec<String> {
    let v: Vec<Value> = serde_json::from_str(json).unwrap_or_default();
    v.iter()
        .filter_map(|x| x.as_str().or_else(|| x["login"].as_str()).or_else(|| x["name"].as_str()))
        .map(Into::into)
        .collect()
}

fn wire_item(i: ForgeItem, repo: &str) -> wire::ForgeItem {
    wire::ForgeItem {
        id: i.id,
        repo: Some(repo.to_owned()),
        number: i.number,
        provider: "github".to_owned(),
        identifier: None,
        state_name: None,
        state_group: None,
        url: None,
        kind: i.kind,
        state: i.state,
        title: i.title,
        labels: names(&i.labels),
        author: i.author_login,
        assignees: names(&i.assignees),
        reviewers: names(&i.reviewers),
        draft: i.draft,
        merge_state: i.merge_state,
        head_ref: i.head_ref,
        head_sha: i.head_sha,
        base_ref: i.base_ref,
        created_at: i.created_at,
        updated_at: i.updated_at,
    }
}

fn no_item() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such item")
}

/// `GET /api/projects/{project}/items`'s query (`wire::PROJECT_ITEMS`).
#[derive(Debug, Default, Deserialize)]
pub struct ItemsQuery {
    pub repo: Option<String>,
    pub kind: Option<String>,
    pub state: Option<String>,
    pub author: Option<String>,
    pub review_requested: Option<String>,
    /// Text in the title, or in a mirrored body or comment (M166): the
    /// list's search.
    pub q: Option<String>,
    pub limit: Option<u32>,
    /// `github` or `plane`.
    pub provider: Option<String>,
    /// A Plane connection's key.
    pub tracker: Option<String>,
}

/// `GET /api/projects/{project}/items`: the project's items the viewer
/// may read, last updated first.
pub async fn list(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(project): Path<String>,
    Query(q): Query<ItemsQuery>,
) -> Result<Json<wire::ForgeItems>, ApiError> {
    let (project, _) = super::links::membership(&app, &v.account, &project, false).await?;
    let provider = q.provider.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_ascii_lowercase);
    if let Some(p) = &provider
        && !matches!(p.as_str(), "github" | "plane")
    {
        return Err(err(StatusCode::BAD_REQUEST, &format!("provider is github or plane, not {p:?}")));
    }
    let tracker = q.tracker.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let limit = q.limit.unwrap_or(DEFAULT).clamp(1, MOST);
    // A tracker narrows to Plane as a repo narrows to GitHub; a GitHub-only filter leaves Plane out.
    let github = provider.as_deref() != Some("plane") && tracker.is_none();
    let plane = provider.as_deref() != Some("github")
        && q.repo.is_none()
        && q.author.is_none()
        && q.review_requested.is_none()
        && matches!(q.kind.as_deref(), None | Some("issue"))
        && matches!(q.state.as_deref(), None | Some("open" | "closed"));
    let (mut items, mut withheld) = (Vec::new(), Vec::new());
    if github {
        let (found, held) = github_items(&app, &v, &project, &q, limit).await?;
        items = found;
        withheld = held;
    }
    if plane {
        items.extend(plane_items(&app, &v, &project, &q, tracker, limit).await?);
    }
    // The exact `IDENT-N` a search names stays in front of the rest.
    let exact = exact_ref(&q);
    items.sort_by_key(|i| {
        let named = exact.as_ref().is_some_and(|(key, n)| {
            i.number == *n && i.identifier.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(key))
        });
        (!named, std::cmp::Reverse(i.updated_at))
    });
    items.truncate(limit as usize);
    Ok(Json(wire::ForgeItems { project, items, withheld }))
}

/// The `IDENT-N` a search writes whole.
fn exact_ref(q: &ItemsQuery) -> Option<(String, u64)> {
    match IssueRef::parse(q.q.as_deref()?) {
        Some(IssueRef::Plane { key, number }) => Some((key, number)),
        _ => None,
    }
}

/// The project's linked repos' items the viewer may read, and the repos they may not.
async fn github_items(
    app: &Platform,
    v: &Viewer,
    project: &str,
    q: &ItemsQuery,
    limit: u32,
) -> Result<(Vec<wire::ForgeItem>, Vec<wire::WithheldRepo>), ApiError> {
    let mut repos = app.db.linked_repos(project)?;
    if let Some(name) = &q.repo {
        repos.retain(|r| r.full_name.eq_ignore_ascii_case(name.trim()));
        if repos.is_empty() {
            return Err(err(StatusCode::NOT_FOUND, &format!("{name} isn't linked to this project")));
        }
    }
    let checked = viewer::check(app, v, &repos).await?;
    let f = ItemFilter {
        kind: q.kind.as_deref(),
        state: q.state.as_deref(),
        author: q.author.as_deref(),
        review_requested: q.review_requested.as_deref(),
        text: q.q.as_deref(),
        number: None,
        limit,
    };
    let name = |id: u64| repos.iter().find(|r| r.repo_id == id).map_or("", |r| r.full_name.as_str());
    let items = app
        .db
        .forge_items(&checked.readable, &f)?
        .into_iter()
        .map(|i| {
            let repo = name(i.repo_id).to_owned();
            wire_item(i, &repo)
        })
        .collect();
    let withheld = checked
        .withheld
        .iter()
        // An unlinked repo is listed as one with the project's repos.
        .filter(|(_, why)| *why != viewer::Withheld::Unlinked)
        .map(|(id, why)| wire::WithheldRepo { repo: name(*id).to_owned(), why: why.as_str().to_owned() })
        .collect();
    Ok((items, withheld))
}

/// A Plane item as an agent reads it.
fn plane_item(i: TrackerItem, c: &ReadableConnection) -> wire::ForgeItem {
    wire::ForgeItem {
        id: i.id,
        repo: None,
        number: i.number,
        provider: "plane".to_owned(),
        identifier: Some(c.identifier.clone()),
        state_name: Some(i.state_name),
        state_group: Some(i.state_group),
        url: Some(trackers::item_url(&c.workspace_slug, &c.identifier, i.number)),
        kind: "issue".to_owned(),
        state: i.state,
        title: i.title,
        labels: vec![],
        author: None,
        assignees: vec![],
        reviewers: vec![],
        draft: false,
        merge_state: None,
        head_ref: None,
        head_sha: None,
        base_ref: None,
        created_at: i.created_at,
        updated_at: i.updated_at,
    }
}

/// The project's connections' items the viewer may read, by the rule the
/// board's picker reads them by ([`viewer::check_connections`]): a
/// connection they may not read is left out, unnamed. `q` as `IDENT-N`
/// finds that item, and as text the titles.
async fn plane_items(
    app: &Platform,
    v: &Viewer,
    project: &str,
    q: &ItemsQuery,
    tracker: Option<&str>,
    limit: u32,
) -> Result<Vec<wire::ForgeItem>, ApiError> {
    let mut all = app.db.tracker_connections(project)?;
    if let Some(key) = tracker {
        all.retain(|c| c.identifier.eq_ignore_ascii_case(key));
    }
    let conns = viewer::check_connections(app, v, &all).await?;
    let text = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let (closed, open) = (q.state.as_deref() == Some("closed"), q.state.as_deref() == Some("open"));
    let mut out = Vec::new();
    let mut read = |f: &TrackerFilter<'_>, only: Option<&str>| -> Result<(), ApiError> {
        for i in app.db.tracker_items(&conns, f)? {
            let Some(c) = conns.get(i.mirror_id) else { continue };
            if only.is_some_and(|k| !c.identifier.eq_ignore_ascii_case(k))
                || out.iter().any(|o: &wire::ForgeItem| o.id == i.id)
            {
                continue;
            }
            out.push(plane_item(i, c));
        }
        Ok(())
    };
    if let Some((key, number)) = exact_ref(q) {
        read(&TrackerFilter { open, closed, number: Some(number), limit: MOST, ..Default::default() }, Some(&key))?;
    }
    read(&TrackerFilter { open, closed, text, limit, ..Default::default() }, None)?;
    Ok(out)
}

/// One Plane item by its key and number, when the viewer may read its
/// connection (`tracker.get`): no item at all to anyone else.
pub async fn plane_get(
    app: &Platform,
    v: &Viewer,
    project: &str,
    key: &str,
    number: u64,
) -> Result<wire::ForgeItem, ApiError> {
    let (project, _) = super::links::membership(app, &v.account, project, false).await?;
    let mut all = app.db.tracker_connections(&project)?;
    all.retain(|c| c.identifier.eq_ignore_ascii_case(key));
    let conns = viewer::check_connections(app, v, &all).await?;
    let f = TrackerFilter { number: Some(number), limit: 1, ..Default::default() };
    let found = app.db.tracker_items(&conns, &f)?.into_iter().next();
    let item = found.and_then(|i| conns.get(i.mirror_id).map(|c| plane_item(i, c)));
    item.ok_or_else(|| err(StatusCode::NOT_FOUND, &format!("{key}-{number} isn't in the mirror of this project (yet)")))
}

/// What a Plane project offers a work item (`tracker.fields`), when the
/// viewer may read its connection: the one `connection` or `tracker` names,
/// else the project's only one. The kept fields while they're fresh.
pub async fn tracker_fields(
    app: &Platform,
    v: &Viewer,
    project: &str,
    tracker: Option<&str>,
    connection: Option<&str>,
) -> Result<wire::TrackerFields, ApiError> {
    let (project, _) = super::links::membership(app, &v.account, project, false).await?;
    let all: Vec<_> = app.db.tracker_connections(&project)?.into_iter().filter(|c| c.unlinked_at.is_none()).collect();
    let ask = |t: &str| t.trim().to_owned();
    let (tracker, connection) = (tracker.map(ask).filter(|t| !t.is_empty()), connection.map(ask));
    let named: Vec<_> = all
        .iter()
        .filter(|c| connection.as_ref().is_none_or(|id| *id == c.id))
        .filter(|c| tracker.as_ref().is_none_or(|t| c.identifier.eq_ignore_ascii_case(t)))
        .collect();
    let c = match named.as_slice() {
        [] => return Err(err(StatusCode::NOT_FOUND, "that Plane project isn't connected to this project")),
        [only] => (*only).clone(),
        many => {
            let list: Vec<&str> = many.iter().map(|c| c.identifier.as_str()).collect();
            let list = list.join(", ");
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("this project has {} Plane connections: say which with tracker: {list}", many.len()),
            ));
        }
    };
    // Only a connection the viewer may read.
    if viewer::check_connections(app, v, std::slice::from_ref(&c)).await?.is_empty() {
        return Err(err(StatusCode::NOT_FOUND, "that Plane project isn't connected to this project"));
    }
    let provider = crate::trackers::writes::provider(app, &project, &c)?;
    let (fields, _) = crate::trackers::fields::load(app, &c, &*provider).await?;
    let read_at = app.db.tracker_fields(&c.id)?.map_or(0, |(_, at)| at);
    Ok(crate::trackers::fields::wire(&c, &fields, read_at))
}

/// `GET /api/forge/items/{id}`.
pub async fn get(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
) -> Result<Json<wire::ForgeItem>, ApiError> {
    let (readable, repo) = viewer::item(&app, &v, &id).await?.ok_or_else(no_item)?;
    let i = app.db.forge_item(&readable, &id)?.ok_or_else(no_item)?;
    Ok(Json(wire_item(i, &repo.full_name)))
}

/// `GET /api/forge/items/{id}/checks`: its head commit's.
pub async fn checks(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
) -> Result<Json<wire::ForgeChecks>, ApiError> {
    let (readable, repo) = viewer::item(&app, &v, &id).await?.ok_or_else(no_item)?;
    let i = app.db.forge_item(&readable, &id)?.ok_or_else(no_item)?;
    let checks = match &i.head_sha {
        Some(sha) => app.db.forge_checks(&readable, repo.repo_id, sha)?,
        None => vec![],
    };
    let checks = checks
        .into_iter()
        .map(|c| wire::ForgeCheck {
            kind: c.kind,
            name: c.name,
            status: c.status,
            conclusion: c.conclusion,
            url: c.url,
            updated_at: c.updated_at,
        })
        .collect();
    Ok(Json(wire::ForgeChecks { item: i.id, head_sha: i.head_sha, checks }))
}

/// `GET /api/forge/items/{id}/events`: oldest first.
pub async fn events(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
) -> Result<Json<wire::ForgeEvents>, ApiError> {
    let (readable, repo) = viewer::item(&app, &v, &id).await?.ok_or_else(no_item)?;
    let i = app.db.forge_item(&readable, &id)?.ok_or_else(no_item)?;
    let events = app
        .db
        .forge_events(&readable, repo.repo_id, i.number)?
        .into_iter()
        .map(|e| wire::ForgeEvent {
            id: e.id,
            kind: e.kind,
            actor: e.actor_login,
            review_state: e.review_state,
            at: e.at,
        })
        .collect();
    Ok(Json(wire::ForgeEvents { item: i.id, events }))
}
