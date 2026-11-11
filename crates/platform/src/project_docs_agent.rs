//! Project Docs for a machine's agents (Arugula #862): the routes the
//! daemon's `doc` tool and Doc block call, signed by the machine, for a
//! team's Docs. In a team's context a Doc lives in the team's project (aru-139);
//! its machines' agents make and write it there as their owner's, labelled
//! with the machine, as they do native issues ([`crate::issues::agent`]).
//!
//! Each is one of [`crate::project_docs`]'s functions with the agent as
//! [`By`], so who may do what is checked in one place and a write's counts,
//! revs and live push are the person's: the owner's role in the project is
//! the ceiling (checked from the hub's roster, so someone demoted to a
//! viewer is refused though the machine's grant row is still there), and the
//! machine needs `docs.write` over the project too, asked again on every
//! call. Reading a Doc needs only that the owner reads the project.
//!
//! A machine's own Docs are not these: they're copied up by
//! [`project_docs_machine`](crate::project_docs_machine) until #865 removes
//! the copies.

use std::sync::Arc;

use arugula_control_common::ApiError;
use arugula_control_wire::project_docs as wire;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
};

use crate::{
    Platform,
    auth::Machine,
    project_docs::{By, create, get, put_asset, write},
};

/// Reads a machine's agents make, per hour (a Doc).
const AGENT_DOC_READS: (&str, usize) = ("doc-agent-read", 3_600);
/// Writes a machine's agents make, per hour (a new Doc, a change). Each
/// reaches the people reading the Doc at once, so fewer than reads.
const AGENT_DOC_WRITES: (&str, usize) = ("doc-agent-write", 600);
/// Pictures, per hour: a few to a Doc.
const AGENT_DOC_ASSETS: (&str, usize) = ("doc-agent-asset", 300);

type J<T> = Result<Json<T>, ApiError>;

fn by(d: &Machine) -> By<'_> {
    By::Agent { owner: &d.account, machine: &d.daemon }
}

/// `POST /api/daemon/projects/{project}/docs`: a Doc made at once, as the
/// owner's, labelled with the machine (`docs.write`).
pub async fn create_route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Json(new): Json<wire::NewDoc>,
) -> J<wire::DocView> {
    app.limits.check_daemon(AGENT_DOC_WRITES, &d.daemon)?;
    Ok(Json(create(&app, by(&d), &project, &new).await?))
}

/// `GET /api/daemon/projects/{project}/docs/{doc}`: a Doc and its sections,
/// as the owner may read it (no power).
pub async fn get_route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((project, doc)): Path<(String, String)>,
) -> J<wire::DocView> {
    app.limits.check_daemon(AGENT_DOC_READS, &d.daemon)?;
    Ok(Json(get(&app, by(&d), &project, &doc).await?))
}

/// `PUT /api/daemon/projects/{project}/docs/{doc}`: rename it, or write or
/// add a section, as the person's `PUT` does (`docs.write`).
pub async fn put_route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((project, doc)): Path<(String, String)>,
    Json(w): Json<wire::DocWrite>,
) -> J<wire::DocView> {
    app.limits.check_daemon(AGENT_DOC_WRITES, &d.daemon)?;
    Ok(Json(write(&app, by(&d), &project, &doc, &w).await?))
}

/// `PUT /api/daemon/projects/{project}/docs/{doc}/assets/{name}`: a
/// picture's bytes, with a machine Doc's limits (`docs.write`).
pub async fn asset_route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((project, doc, name)): Path<(String, String, String)>,
    body: Bytes,
) -> J<wire::Asset> {
    app.limits.check_daemon(AGENT_DOC_ASSETS, &d.daemon)?;
    Ok(Json(put_asset(&app, by(&d), &project, &doc, &name, &body).await?))
}

#[cfg(test)]
#[path = "project_docs_agent_tests.rs"]
mod tests;
