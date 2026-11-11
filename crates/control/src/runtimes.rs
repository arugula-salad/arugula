//! Two runtimes (#857): the relay's, and everyone else's.
//!
//! The hub's database calls block the thread they run on. Its SQLite is one
//! connection behind a lock, and each write syncs to disk, which stays
//! (Jake, 2026-11-10). In the load test (#530), a burst of joins on one
//! runtime held every relay socket behind those writes: keystroke echo p95
//! went from 25 ms to 28 s, and the health check failed.
//!
//! So the **relay's runtime** accepts every connection, carries hyper's
//! connections, and moves relayed bytes: each daemon's socket, each
//! page's channels, each block's socket. The **main runtime** runs every
//! request's handler, with its middleware and its database calls
//! ([`off_relay`]), and the background tasks. A relay pump that has to
//! touch the database (once a minute, or as a socket closes) does it on the
//! blocking pool ([`blocking`]), never on a relay thread.
//!
//! `ARUGULA_CONTROL_ONE_RUNTIME=1` puts everything back on one runtime, as
//! before. Tests don't set the handles, and run everything where they are.

use std::{future::Future, sync::OnceLock};

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::runtime::Handle;

/// The main runtime: handlers and the database.
static MAIN: OnceLock<Handle> = OnceLock::new();
/// The relay's runtime: connections and relayed bytes.
static RELAY: OnceLock<Handle> = OnceLock::new();

/// The variable that keeps everything on one runtime.
pub const ONE_RUNTIME: &str = "ARUGULA_CONTROL_ONE_RUNTIME";

/// The two runtimes' handles, set once by `main`.
pub fn set(main: Handle, relay: Handle) {
    let _ = MAIN.set(main);
    let _ = RELAY.set(relay);
}

/// The relay's runtime, when there's one of its own.
pub fn relay() -> Option<&'static Handle> {
    RELAY.get()
}

/// Middleware, outermost: the rest of the request runs on the main runtime,
/// so whatever it waits on (the database's lock, a sync to disk) holds up
/// a main thread, never a relay one. Its response comes back here.
pub async fn off_relay(req: Request, next: Next) -> Response {
    let Some(main) = MAIN.get() else { return next.run(req).await };
    match main.spawn(next.run(req)).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "a request's handler failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "control failed on that request").into_response()
        }
    }
}

/// Run a relay socket's pump on the relay's runtime (or here, with one
/// runtime).
pub fn spawn_relay<F>(f: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    match RELAY.get() {
        Some(h) => drop(h.spawn(f)),
        None => drop(tokio::spawn(f)),
    }
}

/// A database call from a relay pump: on the blocking pool, so the pump's
/// thread carries on moving bytes meanwhile.
pub async fn blocking<T, F>(f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await.ok()
}
