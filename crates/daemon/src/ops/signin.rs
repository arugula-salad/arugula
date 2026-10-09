//! `signin-link`: `arugula web`'s link, the local token in a sign-in URL.
//! Registered on the Unix socket's router only (`server::local_router`),
//! never over TCP, the tunnel or a channel: `authz` doesn't see a socket
//! request, so it is the socket that makes the caller the owner.

use arugula_proto::{
    api::{Empty, SigninLink},
    op::ops::SigninLinkGet,
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};

impl Handle for SigninLinkGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<SigninLink, OpError> {
        match cx.app.access.signin_link() {
            Some(url) => Ok(SigninLink { url }),
            // Plain text, as the route sent it.
            None => Err(OpError::Text(
                StatusCode::NOT_FOUND,
                "this daemon has no local token (it's reached through a tunnel)".into(),
            )),
        }
    }
}
