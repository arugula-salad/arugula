//! Connecting a Plane project to a project (Arugula #356), and reading its
//! items in.
//!
//! - **Behind the `boards` Labs flag** ([`crate::boards::gate`]): every
//!   route is a 404 while boards are off for the account.
//! - **Who connects.** A project's owner or editor, through a person's
//!   route (not a machine's or an agent's): the key is theirs to give and
//!   nobody's to read back.
//! - **Connecting** reads the project from Plane with the key first. A key
//!   Plane refuses, or a project it doesn't know, is a 422 and nothing is
//!   kept. Then the key (and the optional webhook secret) are sealed with
//!   the project's key ([`keys`]), the connection is kept, and the backfill
//!   starts.
//! - **The backfill** ([`tick`]) is a linked repo's poller's way
//!   ([`crate::forge::poller`]): each machine claims a connection that
//!   isn't backfilled (`Db::claim_backfill`, a lease, `FOR UPDATE SKIP
//!   LOCKED` on Postgres), reads at most [`PAGES`] pages of the project's
//!   items a turn, keeps a cursor when there are more, and renews the lease
//!   between reads. A turn that fails waits [`BACKOFF_MS`], doubled for each
//!   failure in a row.
//! - **The reconcile** ([`reconcile`]) is the same lease once a connection
//!   is backfilled: every [`rows::RECONCILE_MS`] it reads the items updated since
//!   the newest it holds (less a minute) and writes what a webhook missed.
//!   The webhooks ([`super::webhook`]) write through [`ingest_page`] too.
//! - **Disconnecting** sets `unlinked_at` and `purge_after` (ingest stops
//!   at the next write) and removes the sealed secrets at once; the purge
//!   ([`start`]) takes the rows after [`PURGE_GRACE_MS`].
//!
//! [`keys`]: crate::keys

use std::{sync::Arc, time::Duration};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects::{ConnectTracker, TrackerConnection, TrackerConnections, TrackerSettings};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use tracing::{info, warn};

use super::TrackerError;
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::trackers::{self as rows, Backfill, Connection, Finish, NewConnection, key_name, webhook_name},
    forge::links::{self, membership},
    keys,
};

/// Most pages of items a backfill turn reads.
pub const PAGES: usize = 10;
/// How long a connection waits after its first failure.
pub const BACKOFF_MS: u64 = 60_000;
/// Most pages a reconcile reads. It stops by itself at the first page with
/// nothing newer than where it started, so this is only a safety cap.
const RECONCILE_PAGES: usize = 200;
/// How far a reconcile reads back from the newest item it holds.
const OVERLAP_MS: u64 = 60_000;
/// How often each machine looks for a connection to backfill.
const TICK: Duration = Duration::from_secs(5);

/// What a person may give as a key or a webhook secret: printable ASCII,
/// as a header carries it.
fn good_secret(v: &str) -> bool {
    (8..=512).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_graphic())
}

/// A workspace slug or a project's id or identifier.
fn good_name(v: &str) -> bool {
    (1..=128).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn wire(app: &Platform, c: &Connection) -> Result<TrackerConnection, ApiError> {
    Ok(TrackerConnection {
        id: c.id.clone(),
        provider: c.provider.clone(),
        workspace_slug: c.workspace_slug.clone(),
        source_project_id: c.source_project_id.clone(),
        identifier: c.identifier.clone(),
        name: c.name.clone(),
        agent_writes: c.agent_writes.clone(),
        has_webhook_secret: c.has_webhook_secret,
        linked_by: c.linked_by.clone(),
        created_at: c.created_at,
        unlinked_at: c.unlinked_at,
        purge_after: c.purge_after,
        backfilled_at: c.backfilled_at,
        items: app.db.tracker_item_count(c.mirror_id)?,
    })
}

/// A member who may change what's connected: not a team's viewer.
async fn changer(app: &Platform, account: &str, asked: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, asked, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't change a project's connected trackers"));
    }
    Ok(project)
}

/// `GET /api/projects/{project}/trackers`: the project's connections, and
/// those waiting for the purge, for its members. Never a key.
pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
) -> Result<Json<TrackerConnections>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let connections = app.db.tracker_connections(&project)?.iter().map(|c| wire(&app, c)).collect::<Result<_, _>>()?;
    Ok(Json(TrackerConnections { project, connections }))
}

/// `POST /api/projects/{project}/trackers`: connect a Plane project
/// (module docs).
pub async fn connect(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<ConnectTracker>,
) -> Result<Json<TrackerConnection>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = changer(&app, &account, &project).await?;
    let (workspace, asked) = (b.workspace_slug.trim(), b.project.trim());
    if !good_name(workspace) || !good_name(asked) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "workspace: the slug in Plane's address; project: its id or identifier",
        ));
    }
    let key = b.api_key.trim();
    if !good_secret(key) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "that doesn't look like a Plane API key"));
    }
    let webhook = b.webhook_secret.as_deref().map(str::trim).filter(|w| !w.is_empty());
    if webhook.is_some_and(|w| !good_secret(w)) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "that doesn't look like a webhook secret"));
    }
    let Some(master) = app.secret_key.as_deref() else {
        return Err(err(
            StatusCode::NOT_IMPLEMENTED,
            "this control can't keep a tracker's key (it has no key to seal it)",
        ));
    };
    let provider = app.trackers.plane(key);
    let found = provider.project(workspace, asked).await.map_err(|e| match e {
        TrackerError::Unauthorized => err(StatusCode::UNPROCESSABLE_ENTITY, "Plane refused that API key"),
        TrackerError::NotFound => {
            err(StatusCode::UNPROCESSABLE_ENTITY, &format!("Plane has no project {asked} in the workspace {workspace}"))
        }
        e => {
            warn!(%account, %project, error = %e, "asking Plane for a project");
            err(StatusCode::BAD_GATEWAY, &format!("couldn't ask Plane about that project: {e}"))
        }
    })?;
    let id = format!("tc_{}", hex::encode(arugula_e2e::random::<16>()));
    let forget = |app: &Platform| {
        for name in [key_name(&id), webhook_name(&id)] {
            if let Err(e) = keys::remove(&app.db, &project, &name) {
                warn!(error = %e, "removing a tracker secret that wasn't kept");
            }
        }
    };
    let sealed = keys::seal(&app.db, master, &project, &key_name(&id), key)
        .and_then(|()| webhook.map_or(Ok(()), |w| keys::seal(&app.db, master, &project, &webhook_name(&id), w)));
    if let Err(e) = sealed {
        forget(&app);
        return Err(e.into());
    }
    let new = NewConnection {
        id: id.clone(),
        project_id: project.clone(),
        workspace_slug: workspace.to_owned(),
        source_project_id: found.id,
        identifier: found.identifier,
        name: found.name,
        linked_by: account.clone(),
    };
    let made = match app.db.connect_tracker(&new, now_ms()) {
        Ok(Some(c)) => c,
        Ok(None) => {
            forget(&app);
            return Err(err(StatusCode::CONFLICT, &format!("{} is connected to a project already", new.identifier)));
        }
        Err(e) => {
            forget(&app);
            return Err(e.into());
        }
    };
    info!(%account, %project, connection = %made.id, "connected a Plane project");
    // The first read, now: the ticker would find it within [`TICK`].
    let again = app.clone();
    tokio::spawn(async move { drain(&again).await });
    Ok(Json(wire(&app, &made)?))
}

/// `PATCH /api/projects/{project}/trackers/{connection}`: whether agents'
/// writes wait for a person.
pub async fn settings(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, id)): Path<(String, String)>,
    Json(b): Json<TrackerSettings>,
) -> Result<Json<TrackerConnection>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = changer(&app, &account, &project).await?;
    if !matches!(b.agent_writes.as_str(), "approve" | "allow") {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "agent_writes: approve or allow"));
    }
    if !app.db.set_tracker_agent_writes(&project, &id, &b.agent_writes)? {
        return Err(err(StatusCode::NOT_FOUND, "that Plane project isn't connected to this project"));
    }
    info!(%account, %project, connection = %id, agent_writes = %b.agent_writes, "changed what agents may write to Plane");
    let c =
        app.db.tracker_connection(&project, &id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such connection"))?;
    Ok(Json(wire(&app, &c)?))
}

/// `DELETE /api/projects/{project}/trackers/{connection}`: disconnect it.
/// Nothing is read from Plane for it from now on, its sealed secrets go
/// now, and its rows at the purge.
pub async fn disconnect(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<TrackerConnection>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = changer(&app, &account, &project).await?;
    let gone = app
        .db
        .disconnect_tracker(&project, &id, now_ms())?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "that Plane project isn't connected to this project"))?;
    for name in [key_name(&gone.id), webhook_name(&gone.id)] {
        keys::remove(&app.db, &project, &name)?;
    }
    info!(%account, %project, connection = %gone.id, "disconnected a Plane project");
    // The webhook secret is gone with the rest: the answer says so.
    let mut shown = wire(&app, &gone)?;
    shown.has_webhook_secret = false;
    Ok(Json(shown))
}

/// Write a page of items as `connection`'s rows, in one transaction, if the
/// connection is live and (`lease`) the claim still holds: the `fi_` ids of
/// the rows that changed, or `None` when it doesn't. The one place a
/// tracker's items become mirror rows: the reconcile and the webhooks call
/// it too, the webhooks with no lease.
pub fn ingest_page(
    app: &Platform,
    connection: &str,
    mirror_id: i64,
    lease: Option<&str>,
    items: &[super::Item],
    now: u64,
) -> anyhow::Result<Option<Vec<String>>> {
    app.db.tracker_mirror(connection, lease, |tx| {
        let mut changed = vec![];
        for it in items {
            changed.extend(rows::upsert_item(tx, connection, mirror_id, it, now)?);
        }
        Ok(changed)
    })
}

/// What one backfill turn did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    /// Backfilled: this many rows written in the turn.
    Done { connection: String, written: usize },
    /// Ran out of pages: the next turn carries on.
    More { connection: String, written: usize },
    /// Failed, and waits its back-off.
    Failed { connection: String },
    /// The connection was disconnected or the claim lost mid-turn.
    Lost { connection: String },
}

/// Claim a connection whose backfill is due and run a turn on it. `None`
/// when none is due.
pub async fn tick(app: &Platform, now: u64) -> anyhow::Result<Option<Turn>> {
    let Some(job) = app.db.claim_backfill(now)? else { return Ok(None) };
    let connection = job.id.clone();
    match turn(app, &job, now).await {
        Ok(Some(Finish::Done)) => {
            let written = app.db.tracker_item_count(job.mirror_id)? as usize;
            Ok(Some(Turn::Done { connection, written }))
        }
        Ok(Some(Finish::Cursor(_))) => {
            let written = app.db.tracker_item_count(job.mirror_id)? as usize;
            Ok(Some(Turn::More { connection, written }))
        }
        Ok(None) => Ok(Some(Turn::Lost { connection })),
        Err(e) => {
            // Never the key, and never Plane's answer: `e` is a
            // `TrackerError` or ours.
            let wait = app.db.fail_backfill(&job, BACKOFF_MS, now)?;
            warn!(connection = %job.id, wait_ms = wait, error = format!("{e:#}"), "a Plane backfill turn failed");
            Ok(Some(Turn::Failed { connection }))
        }
    }
}

/// One turn: up to [`PAGES`] pages from the cursor. `None` when the claim
/// was lost.
async fn turn(app: &Platform, job: &Backfill, now: u64) -> anyhow::Result<Option<Finish>> {
    let master = app.secret_key.as_deref().ok_or_else(|| anyhow::anyhow!("this platform keeps no secrets"))?;
    let key = keys::open(&app.db, master, &job.project_id, &key_name(&job.id))?
        .ok_or_else(|| anyhow::anyhow!("the connection's key isn't kept"))?;
    let provider = app.trackers.plane(&key);
    let mut cursor = job.cursor.clone();
    for _ in 0..PAGES {
        if !app.db.renew_backfill(job, now_ms())? {
            return Ok(None);
        }
        let page = provider.list(&job.workspace_slug, &job.source_project_id, None, cursor.as_deref()).await?;
        let Some(changed) = ingest_page(app, &job.id, job.mirror_id, Some(&job.lease), &page.items, now)? else {
            return Ok(None);
        };
        crate::boards::items_changed(app, &changed);
        cursor = page.next;
        if cursor.is_none() {
            let done = app.db.finish_backfill(job, &Finish::Done, now_ms())?;
            return Ok(done.then_some(Finish::Done));
        }
    }
    let more = Finish::Cursor(cursor.unwrap_or_default());
    Ok(app.db.finish_backfill(job, &more, now_ms())?.then_some(more))
}

/// Run backfill turns until none is due, or one fails, then the reconciles
/// that are due.
async fn drain(app: &Platform) {
    backfill(app).await;
    while !app.is_paused() && !app.is_stopping() {
        match reconcile(app, now_ms()).await {
            Ok(Some(Reconciled::Failed { .. })) | Ok(None) => break,
            Ok(Some(_)) => continue,
            Err(e) => {
                warn!(error = format!("{e:#}"), "a Plane reconcile couldn't start");
                break;
            }
        }
    }
}

async fn backfill(app: &Platform) {
    while !app.is_paused() && !app.is_stopping() {
        match tick(app, now_ms()).await {
            Ok(Some(Turn::Failed { .. })) | Ok(None) => break,
            Ok(Some(_)) => continue,
            Err(e) => {
                warn!(error = format!("{e:#}"), "a Plane backfill turn couldn't start");
                break;
            }
        }
    }
}

/// What one reconcile turn did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciled {
    /// Read: this many rows changed.
    Done { connection: String, changed: usize },
    /// Failed, and waits its back-off.
    Failed { connection: String },
    /// The connection was disconnected or the claim lost mid-turn.
    Lost { connection: String },
}

/// Claim a backfilled connection whose reconcile is due and run it. `None`
/// when none is due.
pub async fn reconcile(app: &Platform, now: u64) -> anyhow::Result<Option<Reconciled>> {
    let Some(job) = app.db.claim_reconcile(now)? else { return Ok(None) };
    let connection = job.id.clone();
    match reconcile_turn(app, &job, now).await {
        Ok(Some(changed)) => Ok(Some(Reconciled::Done { connection, changed })),
        Ok(None) => Ok(Some(Reconciled::Lost { connection })),
        Err(e) => {
            let wait = app.db.fail_backfill(&job, BACKOFF_MS, now)?;
            // Once per back-off. Never the key, and never Plane's answer.
            if matches!(e.downcast_ref::<TrackerError>(), Some(TrackerError::Unauthorized)) {
                warn!(connection = %job.id, wait_ms = wait, "Plane refused a connection's key; its reconcile waits");
            } else {
                warn!(connection = %job.id, wait_ms = wait, error = format!("{e:#}"), "a Plane reconcile failed");
            }
            Ok(Some(Reconciled::Failed { connection }))
        }
    }
}

/// Read what changed since the newest item held, to the first page with
/// nothing newer (a turn must reach it: the next one starts above what it
/// skipped), and write it. How many rows changed, or `None` when the claim was lost.
async fn reconcile_turn(app: &Platform, job: &Backfill, now: u64) -> anyhow::Result<Option<usize>> {
    let master = app.secret_key.as_deref().ok_or_else(|| anyhow::anyhow!("this platform keeps no secrets"))?;
    let key = keys::open(&app.db, master, &job.project_id, &key_name(&job.id))?
        .ok_or_else(|| anyhow::anyhow!("the connection's key isn't kept"))?;
    let provider = app.trackers.plane(&key);
    let since = app.db.tracker_newest(job.mirror_id)?.map(|t| t.saturating_sub(OVERLAP_MS));
    let (mut cursor, mut total) = (None::<String>, 0);
    for _ in 0..RECONCILE_PAGES {
        if !app.db.renew_backfill(job, now_ms())? {
            return Ok(None);
        }
        let page = provider.list(&job.workspace_slug, &job.source_project_id, since, cursor.as_deref()).await?;
        let Some(changed) = ingest_page(app, &job.id, job.mirror_id, Some(&job.lease), &page.items, now)? else {
            return Ok(None);
        };
        crate::boards::items_changed(app, &changed);
        total += changed.len();
        // A page with nothing newer than `since` is the end of what changed.
        let newer = since.is_none_or(|t| page.items.iter().any(|i| i.updated_at > t));
        cursor = page.next.filter(|_| newer);
        if cursor.is_none() {
            break;
        }
    }
    if cursor.is_some() {
        warn!(connection = %job.id, pages = RECONCILE_PAGES, "a Plane reconcile hit its page cap with more changed");
    }
    Ok(app.db.finish_reconcile(job, now_ms())?.then_some(total))
}

/// Purge every disconnected connection that's due: how many went.
pub fn purge(app: &Platform, now: u64) -> anyhow::Result<usize> {
    let mut n = 0;
    while let Some(id) = app.db.purge_tracker(now)? {
        info!(connection = %id, "purged a disconnected Plane project's rows");
        n += 1;
    }
    Ok(n)
}

/// This machine's backfill, every [`TICK`], and the purge, every
/// [`links::PURGE_EVERY`], while background work isn't held.
pub fn start(app: &Arc<Platform>) {
    let weak = Arc::downgrade(app);
    tokio::spawn(async move {
        loop {
            let Some(p) = weak.upgrade() else { return };
            drain(&p).await;
            drop(p);
            tokio::time::sleep(TICK).await;
        }
    });
    let weak = Arc::downgrade(app);
    tokio::spawn(async move {
        loop {
            let Some(p) = weak.upgrade() else { return };
            if !p.is_paused()
                && !p.is_stopping()
                && let Err(e) = purge(&p, now_ms())
            {
                warn!(error = %e, "purging disconnected Plane projects");
            }
            drop(p);
            tokio::time::sleep(links::PURGE_EVERY).await;
        }
    });
}
