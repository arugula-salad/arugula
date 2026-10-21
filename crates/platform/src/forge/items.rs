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

use super::viewer::{self, Viewer};
use crate::{
    Platform,
    db::forge::{ForgeItem, ItemFilter},
};

/// How many items a list holds at most, and when it doesn't say.
const MOST: u32 = 200;
const DEFAULT: u32 = 100;

/// A JSON list of names or logins as stored; anything else is none.
fn names(json: &str) -> Vec<String> {
    let v: Vec<Value> = serde_json::from_str(json).unwrap_or_default();
    v.iter()
        .filter_map(|x| x.as_str().or_else(|| x["login"].as_str()).or_else(|| x["name"].as_str()))
        .map(Into::into)
        .collect()
}

fn wire_item(i: ForgeItem, repo: &str) -> wire::ForgeItem {
    wire::ForgeItem {
        id: i.id,
        repo: repo.to_owned(),
        number: i.number,
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
    let mut repos = app.db.linked_repos(&project)?;
    if let Some(name) = &q.repo {
        repos.retain(|r| r.full_name.eq_ignore_ascii_case(name.trim()));
        if repos.is_empty() {
            return Err(err(StatusCode::NOT_FOUND, &format!("{name} isn't linked to this project")));
        }
    }
    let checked = viewer::check(&app, &v, &repos).await?;
    let f = ItemFilter {
        kind: q.kind.as_deref(),
        state: q.state.as_deref(),
        author: q.author.as_deref(),
        review_requested: q.review_requested.as_deref(),
        text: q.q.as_deref(),
        limit: q.limit.unwrap_or(DEFAULT).clamp(1, MOST),
    };
    let name = |id: u64| repos.iter().find(|r| r.repo_id == id).map_or("", |r| r.full_name.as_str());
    let items = app.db.forge_items(&checked.readable, &f)?.into_iter().map(|i| {
        let repo = name(i.repo_id).to_owned();
        wire_item(i, &repo)
    });
    let withheld = checked
        .withheld
        .iter()
        // An unlinked repo is listed as one with the project's repos.
        .filter(|(_, why)| *why != viewer::Withheld::Unlinked)
        .map(|(id, why)| wire::WithheldRepo { repo: name(*id).to_owned(), why: why.as_str().to_owned() })
        .collect();
    Ok(Json(wire::ForgeItems { project, items: items.collect(), withheld }))
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
