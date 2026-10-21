//! The hub's internal API (M140): `HubPort` over HTTP for an
//! out-of-process platform, served by `arugula_platform::remote::serve_hub`
//! over the hub's own `hub_port::InProcess`, on a listener of its own
//! (`--internal-listen`, `[::]:7691` by default with an out-of-process
//! platform).
//!
//! Only the private network reaches it:
//! - it's never the public listener: none of these paths is on control's
//!   site, and Fly's proxy only forwards to the ports `fly.toml` declares
//!   a service for, which this one never is;
//! - a call that came through Fly's proxy anyway (one carrying
//!   `Fly-Forwarded-Port` or `Fly-Client-IP`, which the proxy adds) is
//!   refused, in case someone declares one;
//! - every call is signed with the key the hub and the platform share
//!   (`arugula_control_common::internal`).

use std::sync::Arc;

use arugula_control_common::principal::PrincipalKey;
use arugula_platform::HubPort;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::warn;

use crate::App;

/// The headers Fly's proxy adds to what it forwards from outside.
const PROXIED: [&str; 2] = ["fly-forwarded-port", "fly-client-ip"];

/// The internal API's router.
pub fn router(app: &Arc<App>, key: PrincipalKey) -> axum::Router {
    let hub: Arc<dyn HubPort> = app.hub_port.clone();
    arugula_platform::remote::serve_hub(hub, key).layer(axum::middleware::from_fn(not_proxied))
}

/// Serve it on `l` until control stops.
pub async fn serve(app: Arc<App>, l: tokio::net::TcpListener, key: PrincipalKey) {
    let l = axum::serve::ListenerExt::tap_io(l, |t| {
        let _ = t.set_nodelay(true);
    });
    if let Err(e) = axum::serve(l, router(&app, key)).await {
        warn!(error = %e, "the hub's internal API stopped");
    }
}

/// Nothing that came through the public proxy.
async fn not_proxied(req: Request, next: Next) -> Response {
    if PROXIED.iter().any(|h| req.headers().contains_key(*h)) {
        warn!(path = req.uri().path(), "refused an internal call that came through the public proxy");
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(req).await
}
