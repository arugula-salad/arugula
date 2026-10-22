//! Who control trusts for outgoing TLS (GitHub, Stripe, Managoat,
//! E2B), in both processes. The
//! platform's roots first. On a machine with none (a bare container image
//! without `ca-certificates`, where someone runs the static binary
//! themselves) reqwest's client won't build, and control used to stop at
//! start with "No CA certificates were loaded from the system". Then the
//! bundled Mozilla roots (`webpki-root-certs`) stand in, with one warning,
//! as they do for the daemon.

use std::time::Duration;

use tracing::warn;

/// What control says it is to the services it calls. GitHub refuses a
/// request without a User-Agent (403), and the forge client leaves it to
/// the caller (`arugula-forge-github`), so every request carries this one.
pub const USER_AGENT: &str = concat!("arugula-control/", env!("CARGO_PKG_VERSION"));

/// An HTTP client with this timeout and [`USER_AGENT`], verifying with the
/// platform's roots, or the bundled ones when there are none.
pub fn client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    build(|| reqwest::Client::builder().timeout(timeout).user_agent(USER_AGENT))
}

/// The client `builder` makes, with the platform's roots, or the bundled
/// ones when there are none. The calls between the hub and the platform
/// are plain HTTP on the private network, but a client won't build
/// without roots either way.
pub fn build(builder: impl Fn() -> reqwest::ClientBuilder) -> anyhow::Result<reqwest::Client> {
    match builder().build() {
        Ok(c) => Ok(c),
        Err(e) => {
            warn!(error = %e, "no system CA certificates: trusting the bundled Mozilla roots (install ca-certificates to use the system's)");
            bundled(builder())
        }
    }
}

fn bundled(builder: reqwest::ClientBuilder) -> anyhow::Result<reqwest::Client> {
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().filter_map(|c| reqwest::Certificate::from_der(c).ok());
    Ok(builder.tls_certs_only(roots).build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_roots_make_a_client() {
        assert!(webpki_root_certs::TLS_SERVER_ROOT_CERTS.len() > 100);
        bundled(reqwest::Client::builder().timeout(Duration::from_secs(1))).unwrap();
    }

    /// Every request says who it is: GitHub's API answers 403 without it,
    /// which the mirror's poller met live (#1210).
    #[tokio::test]
    async fn requests_carry_a_user_agent() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", l.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0; 4096];
            let n = s.read(&mut buf).await.unwrap();
            s.write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n").await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase()
        });
        client(Duration::from_secs(5)).unwrap().get(url).send().await.unwrap();
        let req = server.await.unwrap();
        assert!(req.contains(&format!("user-agent: {}", USER_AGENT.to_ascii_lowercase())), "{req}");
    }
}
