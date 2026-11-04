//! The client half of the block sites' proxy (`sites`): an HTTP/1
//! connection over a stream of its own, and a switched exchange (a
//! WebSocket) joined to the caller's.

use std::error::Error;

use hyper::{
    Response,
    body::{Body, Incoming},
    client::conn::http1::SendRequest,
    upgrade::{OnUpgrade, Upgraded},
};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};

/// An HTTP/1 connection over `io`. Its task carries upgrades, so a
/// switched response's bytes come out of `hyper::upgrade::on`.
pub(crate) async fn connect<B, T>(io: T) -> hyper::Result<SendRequest<B>>
where
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn Error + Send + Sync>>,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io)).await?;
    crate::task::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    Ok(sender)
}

type Ends = (TokioIo<Upgraded>, TokioIo<Upgraded>);

/// Both ends of a switched exchange, once both upgrades are through.
fn ends(
    client: OnUpgrade,
    res: &mut Response<Incoming>,
) -> impl Future<Output = Result<Ends, (Option<hyper::Error>, Option<hyper::Error>)>> + Send + 'static {
    let server = hyper::upgrade::on(res);
    async move {
        match tokio::join!(client, server) {
            (Ok(c), Ok(s)) => Ok((TokioIo::new(c), TokioIo::new(s))),
            (c, s) => Err((c.err(), s.err())),
        }
    }
}

/// Pass a switched exchange's bytes both ways until either end closes.
pub(crate) fn splice(client: OnUpgrade, res: &mut Response<Incoming>) {
    let ends = ends(client, res);
    crate::task::spawn(async move {
        let Ok((mut a, mut b)) = ends.await else { return };
        let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
    });
}
