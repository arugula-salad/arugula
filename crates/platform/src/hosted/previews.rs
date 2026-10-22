//! Public previews (M187, aru-097): a port of a hosted workspace that an
//! owner or editor made public, served to anyone at `<id>.<preview
//! domain>` (`ARUGULA_PREVIEW_DOMAIN`). The hub serves it through the
//! workspace's machine's relay socket (`crates/control/src/previews.rs`),
//! asking here which port an id is ([`crate::PlatformPort::public_preview`]),
//! and the machine asks here, through the hub, which of its ports are
//! public before it serves one ([`crate::PlatformPort::public_ports`]).
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/projects/{project}/workspaces/{id}/ports` | members |
//! | `PUT /api/projects/{project}/workspaces/{id}/ports/{port}/public` (`{"renew":true}` only renews) | owners and editors |
//! | `DELETE /api/projects/{project}/workspaces/{id}/ports/{port}/public` | owners and editors |
//!
//! A port is public for [`PUBLIC_FOR_MS`] (7 days) from when it was made
//! public or last renewed, and says who did it. Its id is 32 random hex
//! characters, new each time it's made public, never the workspace's. The
//! operator lists and takes down any of them with `arugula-platform
//! previews` (docs/control-ops.md, "Public previews").

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

pub use crate::db::previews::{PUBLIC_FOR_MS, PublicPreview};
use crate::{Platform, auth::Person, cloud::CloudDriver, forge::links::membership};

/// A preview domain as `ARUGULA_PREVIEW_DOMAIN` takes it: lower case, no
/// dots at the ends. The hub checks it's a site of its own.
pub fn domain(raw: &str) -> anyhow::Result<Option<String>> {
    let d = raw.trim().trim_matches('.').to_ascii_lowercase();
    if d.is_empty() {
        return Ok(None);
    }
    if d.contains(['/', ':', ' ']) || !d.contains('.') {
        anyhow::bail!("ARUGULA_PREVIEW_DOMAIN {raw:?}: a domain name, like arugula.site");
    }
    Ok(Some(d))
}

/// A new preview's id: a DNS label nobody can guess.
pub fn new_id() -> String {
    hex::encode(arugula_e2e::random::<16>())
}

/// A preview's URL: `<id>.<domain>`, with control's scheme and port (as
/// block origins take the page's).
pub fn url(public_url: &str, domain: &str, id: &str) -> String {
    let (scheme, port) = match url::Url::parse(public_url) {
        Ok(u) => (u.scheme().to_owned(), u.port()),
        Err(_) => ("https".to_owned(), None),
    };
    match port {
        Some(p) => format!("{scheme}://{id}.{domain}:{p}"),
        None => format!("{scheme}://{id}.{domain}"),
    }
}

/// One port of a workspace, as the routes answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub port: u16,
    #[serde(default)]
    pub label: String,
    pub public: bool,
    /// Its public URL, while it's public.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Who made it public or last renewed it, and until when (kept after
    /// it expires, until it's turned off or made public again).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_until: Option<u64>,
}

/// `GET .../ports`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Ports {
    pub workspace: String,
    /// Whether public previews are on here.
    pub previews: bool,
    pub ports: Vec<Port>,
}

/// `PUT .../ports/{port}/public`.
#[derive(Debug, Default, Deserialize)]
pub struct MakePublic {
    /// Only renew a port that's public now (409 if it isn't).
    #[serde(default)]
    pub renew: bool,
}

fn on(app: &Platform) -> Result<&str, ApiError> {
    if app.compute.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "hosted workspaces aren't set up on this control"));
    }
    app.hosted
        .preview_domain
        .as_deref()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "public previews aren't set up on this control"))
}

fn port_of(n: u32) -> Result<u16, ApiError> {
    u16::try_from(n).ok().filter(|p| *p > 0).ok_or_else(|| err(StatusCode::BAD_REQUEST, "a port is 1 to 65535"))
}

/// The workspace by id or name in a project `account` is in, with whether
/// they may change its previews (an owner or editor).
async fn workspace(
    app: &Platform,
    account: &str,
    project: &str,
    id: &str,
    change: bool,
) -> Result<(String, super::HostedWorkspace), ApiError> {
    let (project, role) = membership(app, account, project, change).await?;
    if change && role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't make a port public"));
    }
    let w =
        app.db.hosted_workspace(&project, id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such hosted workspace"))?;
    if w.state == "deleting" || w.state == "deleted" {
        return Err(err(StatusCode::CONFLICT, "this hosted workspace is being deleted"));
    }
    Ok((project, w))
}

fn ports(app: &Platform, workspace: &str, now: u64) -> anyhow::Result<Vec<Port>> {
    let public: std::collections::HashMap<u16, String> = app.db.public_ports(workspace, now)?.into_iter().collect();
    let domain = app.hosted.preview_domain.as_deref();
    Ok(app
        .db
        .hosted_ports(workspace)?
        .into_iter()
        .filter_map(|(port, label, by, until)| {
            let port = u16::try_from(port).ok()?;
            let id = public.get(&port);
            Some(Port {
                port,
                label,
                public: id.is_some(),
                url: id.zip(domain).map(|(id, d)| url(&app.cfg.public_url, d, id)),
                public_by: by,
                public_until: until,
            })
        })
        .collect())
}

pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<Ports>, ApiError> {
    if app.compute.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "hosted workspaces aren't set up on this control"));
    }
    let (_, w) = workspace(&app, &account, &project, &id, false).await?;
    let ports = ports(&app, &w.id, now_ms())?;
    Ok(Json(Ports { workspace: w.id, previews: app.hosted.preview_domain.is_some(), ports }))
}

/// Make a port public, or renew it: for 7 days from now, as this account.
pub async fn make_public(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id, port)): Path<(String, String, u32)>,
    body: Option<Json<MakePublic>>,
) -> Result<Json<Port>, ApiError> {
    let domain = on(&app)?.to_owned();
    let port = port_of(port)?;
    let (project, w) = workspace(&app, &account, &project, &id, true).await?;
    let now = now_ms();
    let until = now + PUBLIC_FOR_MS;
    let renew = body.is_some_and(|b| b.renew);
    let public = if renew {
        app.db
            .renew_port_public(&w.id, port, &account, until, now)?
            .ok_or_else(|| err(StatusCode::CONFLICT, "that port isn't public: make it public first"))?
    } else {
        app.db.make_port_public(&w.id, port, &account, &new_id(), until, now)?
    };
    info!(%account, %project, workspace = %w.id, port, renew, "made a port public");
    app.analytics.capture(&account, "preview made public", json!({ "renew": renew }));
    Ok(Json(Port {
        port,
        label: String::new(),
        public: true,
        url: Some(url(&app.cfg.public_url, &domain, &public)),
        public_by: Some(account),
        public_until: Some(until),
    }))
}

/// Turn a port's public preview off: it stops within a minute (the hub's
/// and the machine's caches).
pub async fn make_private(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id, port)): Path<(String, String, u32)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    on(&app)?;
    let port = port_of(port)?;
    let (project, w) = workspace(&app, &account, &project, &id, true).await?;
    let was = app.db.make_port_private(&w.id, port)?;
    info!(%account, %project, workspace = %w.id, port, was, "turned a public preview off");
    Ok(Json(json!({ "port": port, "public": false, "was_public": was })))
}

/// For the hub: the preview with this id, if it's public now.
pub fn lookup(app: &Platform, id: &str) -> anyhow::Result<Option<PublicPreview>> {
    if app.hosted.preview_domain.is_none() || id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    app.db.public_preview(id, now_ms())
}

/// For a hosted machine, through the hub: its workspace's ports that are
/// public now, with their ids. None while previews are off.
pub fn public_ports(app: &Platform, workspace: &str) -> anyhow::Result<Vec<(u16, String)>> {
    if app.hosted.preview_domain.is_none() {
        return Ok(vec![]);
    }
    app.db.public_ports(workspace, now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_domains() {
        assert_eq!(url("https://control.arugula.io", "arugula.site", "ab"), "https://ab.arugula.site");
        assert_eq!(url("http://127.0.0.1:24880", "previews.localhost", "ab"), "http://ab.previews.localhost:24880");
        assert_eq!(domain(" Arugula.Site. ").unwrap().as_deref(), Some("arugula.site"));
        assert_eq!(domain("").unwrap(), None);
        assert!(domain("arugula").is_err());
        assert!(domain("https://arugula.site").is_err());
        let id = new_id();
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(id, new_id());
    }
}
