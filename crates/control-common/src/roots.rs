//! Who control trusts for outgoing TLS (GitHub, Stripe, Sprites,
//! Managoat), in both processes. The
//! platform's roots first. On a machine with none (a bare container image
//! without `ca-certificates`, where someone runs the static binary
//! themselves) reqwest's client won't build, and control used to stop at
//! start with "No CA certificates were loaded from the system". Then the
//! bundled Mozilla roots (`webpki-root-certs`) stand in, with one warning,
//! as they do for the daemon.

use std::time::Duration;

use tracing::warn;

/// An HTTP client with this timeout, verifying with the platform's roots,
/// or the bundled ones when there are none.
pub fn client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    build(|| reqwest::Client::builder().timeout(timeout))
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
}
