//! What the platform's tests share: its routes behind a gate that says
//! who's asking as the hub's would ([`router`]).

// Also the feature crates' test harness (#314, the `testing` feature): its
// helpers are `pub`, and only its own tests use the rest.
#![cfg_attr(not(test), allow(dead_code, unused_imports))]

use std::sync::Arc;

use arugula_control_common::Principal;
use axum::http::StatusCode;

use crate::{
    Platform,
    auth::{Asked, Credential},
};

pub mod hub;
pub use hub::*;

/// The platform's routes behind a gate that says who's asking as the
/// hub's would: `ilg_session=session-<account>` is that account's
/// session (with the same-origin check), and `x-test-daemon: <account>
/// <machine>` that machine's checked signature.
pub fn router(app: Arc<Platform>) -> axum::Router {
    crate::router(app).layer(axum::middleware::from_fn(gate))
}

async fn gate(mut req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let headers = req.headers();
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let session = header("cookie")
        .and_then(|c| c.split(';').find_map(|kv| kv.trim().strip_prefix("ilg_session=session-").map(str::to_owned)));
    let asked = if let Some(d) = header("x-test-daemon") {
        let (account, daemon) = d.split_once(' ').unwrap();
        Some(Asked::Principal(Principal::Daemon { account: account.into(), daemon: daemon.into() }))
    } else if let Some(account) = session {
        let foreign = header("origin").is_some_and(|o| o != "https://control.example");
        Some(if req.method() != axum::http::Method::GET && foreign {
            Asked::Refused {
                status: StatusCode::FORBIDDEN,
                error: "cross-origin request refused".into(),
                by: Credential::Session,
            }
        } else {
            Asked::Principal(Principal::Person { account })
        })
    } else {
        None
    };
    if let Some(a) = asked {
        req.extensions_mut().insert(a);
    }
    next.run(req).await
}
