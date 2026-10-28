//! A hosted machine's setup and where its repos stand (M183), for
//! `arugula cloud rebuild` to warn with. HTTP and the CLI only:
//! `hosted_setup.get` and the tarball of what a rebuild would lose
//! (`GET /api/hosted/setup/archive`, hand-written: its answer is bytes).

use std::sync::Arc;

use arugula_proto::{
    api::{Empty, HostedSetup},
    op::ops::HostedSetupGet,
};
use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::stream;

use arugulad_base::ops::{Cx, Handle, OpError};

use super::HostedBox;
use crate::{
    api::{ApiError, Res},
    server::App,
};

/// What this box's setup did, and each repo's changed files and commits on
/// no remote branch. Empty on a machine that isn't hosted.
impl Handle<HostedBox> for HostedSetupGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<HostedSetup, OpError> {
        Ok(super::setup::status(cx.app.control.state_dir()).await)
    }
}

/// `GET /api/hosted/setup/archive` (M183): a gzipped tarball of a hosted
/// box's repos that hold work a rebuild would lose ([`super::setup::archive`]),
/// streamed as `tar` writes it. 404 when none does.
pub(super) async fn setup_archive(State(app): State<Arc<App>>) -> Res<Response> {
    let child = super::setup::archive(app.control.state_dir())
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "no repo here has work that isn't pushed".into()))?;
    let out = stream::unfold(Some(child), |child| async move {
        use tokio::io::AsyncReadExt as _;
        let mut child = child?;
        let mut buf = vec![0u8; 64 * 1024];
        let n = match child.stdout.as_mut()?.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => return Some((Err(e), None)),
        };
        if n == 0 {
            let _ = child.wait().await;
            return None;
        }
        buf.truncate(n);
        Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)), Some(child)))
    });
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"unpushed.tar.gz\""),
        ],
        axum::body::Body::from_stream(out),
    )
        .into_response())
}
