//! `POST /plane/webhook/{connection}`: a Plane project's changes, as they
//! happen (Arugula #358).
//!
//! - **Signed** by the connection's own webhook secret, sealed at connect
//!   time: `X-Plane-Signature` is the hex HMAC-SHA256 of the raw body
//!   ([`signed_hex`]). A connection that is unknown, disconnected, or kept no
//!   secret is a 404, the same answer for all three; a bad or missing
//!   signature is a 401, and it comes before anything is read or written,
//!   the delivery's id included. Behind the boards Labs flag of whoever
//!   connected it.
//! - **Deduplicated** by `X-Plane-Delivery` in the GitHub webhook's table
//!   ([`Db::first_delivery`], key `plane:<delivery>`). A failure after that
//!   forgets it, so Plane's retry is taken.
//! - **Per workspace**: Plane sends every project's events to one webhook,
//!   so a payload for another project is ignored.
//! - **A change is re-read**: the payload's fields aren't trusted (its state
//!   may be an id only), so a created or updated work item is read with
//!   `get` and written through [`ingest_page`]. A deleted one's row goes.
//!
//! Only the connection, event and delivery are logged, never the payload,
//! the signature or the secret.
//!
//! [`Db::first_delivery`]: crate::db::Db::first_delivery

use std::sync::Arc;

use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::TrackerError;
use crate::{
    Platform,
    db::trackers::{self as rows, Connection, key_name, webhook_name},
    forge::signed_hex,
    keys,
};

/// The same answer for a connection that isn't there, isn't live, or has no
/// secret to check with.
fn nothing() -> Response {
    arugula_control_common::err(StatusCode::NOT_FOUND, "no such webhook").into_response()
}

/// What a delivery asks for.
#[derive(Debug, PartialEq, Eq)]
enum Change<'a> {
    /// Read the item again and write it.
    Read(&'a str),
    /// Remove its row.
    Remove(&'a str),
}

/// What a payload asks for, and the project it is about. `None` for an event
/// that isn't a work item's change. Plane's v2 payloads name `workitem.*`
/// events with `entity_id` and `data.project_id` (a delete's `data` is empty,
/// the record in `previous_attributes`); v1's are `issue` with an `action`.
fn change<'a>(event: &str, v: &'a Value) -> Option<(Change<'a>, Option<&'a str>)> {
    let (kind, project) = match event {
        "workitem.created" | "workitem.updated" | "workitem.archived" | "workitem.deleted" => {
            let project = v["data"]["project_id"].as_str().or_else(|| v["previous_attributes"]["project_id"].as_str());
            (event.rsplit('.').next().unwrap_or_default(), project)
        }
        "issue" => (v["action"].as_str().unwrap_or_default(), v["data"]["project"].as_str()),
        _ => return None,
    };
    let id = v["entity_id"].as_str().or_else(|| v["data"]["id"].as_str())?;
    match kind {
        "created" | "updated" | "archived" => Some((Change::Read(id), project)),
        "deleted" => Some((Change::Remove(id), project)),
        _ => None,
    }
}

/// `POST /plane/webhook/{connection}`: see the module docs.
pub async fn webhook(
    State(app): State<Arc<Platform>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    let delivery = h("x-plane-delivery").unwrap_or_default().chars().take(64).collect::<String>();
    let event = h("x-plane-event").unwrap_or_default().chars().take(64).collect::<String>();
    let (c, secret) = match checked(&app, &id).await {
        Ok(Some(found)) => found,
        Ok(None) => return nothing(),
        Err(e) => {
            warn!(connection = %id, error = format!("{e:#}"), "checking a Plane webhook's connection");
            return arugula_control_common::err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't check the webhook")
                .into_response();
        }
    };
    if !signed_hex(&secret, h("x-plane-signature"), &body) {
        warn!(connection = %id, %event, %delivery, "a Plane webhook with a bad or missing signature");
        return arugula_control_common::err(StatusCode::UNAUTHORIZED, "bad signature").into_response();
    }
    // A signed delivery, whatever it asks for: the connection's row says so.
    if let Err(e) = app.db.tracker_delivered(&c.id, now_ms()) {
        warn!(connection = %id, error = %e, "noting a Plane webhook's delivery");
    }
    let key = format!("plane:{delivery}");
    if !delivery.is_empty() {
        match app.db.first_delivery(&key, &event, now_ms()) {
            Ok(true) => {}
            Ok(false) => return Json(json!({ "duplicate": true })).into_response(),
            Err(e) => {
                warn!(connection = %id, %event, %delivery, error = %e, "recording a Plane webhook's delivery");
                return arugula_control_common::err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't record the delivery")
                    .into_response();
            }
        }
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    let event = if event.is_empty() { v["event"].as_str().unwrap_or_default().to_owned() } else { event };
    let Some((what, project)) = change(&event, &v) else {
        return Json(json!({ "ignored": true })).into_response();
    };
    if project != Some(c.source_project_id.as_str()) {
        return Json(json!({ "ignored": true })).into_response();
    }
    match apply(&app, &c, &what).await {
        Ok(changed) => {
            info!(connection = %id, %event, %delivery, changed, "a Plane webhook was taken");
            Json(json!({ "changed": changed })).into_response()
        }
        Err(e) => {
            // Forgotten, so Plane's retry is taken.
            warn!(connection = %id, %event, %delivery, error = format!("{e:#}"), "a Plane webhook couldn't be applied");
            let _ = app.db.forget_delivery(&key);
            arugula_control_common::err(StatusCode::BAD_GATEWAY, "couldn't apply the change").into_response()
        }
    }
}

/// The live connection `id` and its webhook secret, opened; `None` unless
/// the connection is live, it kept a secret, and boards are on for whoever
/// connected it.
async fn checked(app: &Platform, id: &str) -> anyhow::Result<Option<(Connection, String)>> {
    let Some(c) = app.db.live_tracker_connection(id)? else { return Ok(None) };
    let Some(who) = &c.linked_by else { return Ok(None) };
    if !crate::chat::flag_on(app, who, crate::boards::FLAG).await? {
        return Ok(None);
    }
    let Some(master) = app.secret_key.as_deref() else { return Ok(None) };
    let secret = keys::open(&app.db, master, &c.project_id, &webhook_name(&c.id))?;
    Ok(secret.map(|s| (c, s)))
}

/// Do what a delivery asks: how many rows changed.
async fn apply(app: &Platform, c: &Connection, what: &Change<'_>) -> anyhow::Result<usize> {
    let now = now_ms();
    match what {
        Change::Remove(item) => remove(app, c, item),
        Change::Read(item) => {
            let master = app.secret_key.as_deref().ok_or_else(|| anyhow::anyhow!("this platform keeps no secrets"))?;
            let key = keys::open(&app.db, master, &c.project_id, &key_name(&c.id))?
                .ok_or_else(|| anyhow::anyhow!("the connection's key isn't kept"))?;
            let provider = app.trackers.plane(&key);
            match provider.get(&c.workspace_slug, &c.source_project_id, item).await {
                Ok(it) => {
                    let changed = super::connect::ingest_page(app, &c.id, c.mirror_id, None, &[it], now)?;
                    let changed = changed.unwrap_or_default();
                    crate::boards::items_changed(app, &changed);
                    Ok(changed.len())
                }
                // Gone by the time it was read: the same as deleted.
                Err(TrackerError::NotFound) => remove(app, c, item),
                Err(e) => Err(e.into()),
            }
        }
    }
}

/// Remove the row for `item`, and tell the boards that pointed at it.
fn remove(app: &Platform, c: &Connection, item: &str) -> anyhow::Result<usize> {
    let Some(row) = app.db.tracker_item_id(&c.id, item)? else { return Ok(0) };
    // The boards are found before the row goes: they join on it.
    let boards = app.db.boards_on_item_ids(std::slice::from_ref(&row))?;
    let gone = app.db.tracker_mirror(&c.id, None, |tx| rows::delete_item(tx, &c.id, item))?.flatten();
    if gone.is_some() {
        crate::boards::boards_changed(app, &boards);
    }
    Ok(usize::from(gone.is_some()))
}
