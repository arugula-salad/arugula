//! Prometheus metrics for the launch dashboard (part of Arugula #532): a
//! listener of its own, on the private network only, that Fly's managed
//! Prometheus scrapes (`[metrics]` in each `fly.toml`) and Grafana at
//! fly-metrics.net reads. Off unless `--metrics-listen` is set, so a
//! control someone else runs serves nothing new.
//!
//! The text format is written by hand ([`Text`]): a few gauges and
//! counters don't need a metrics crate.
//!
//! Only the private network reaches it, as the hub's internal API: Fly's
//! proxy forwards only the ports a `fly.toml` declares a service for, and
//! this one never is; a request that came through the proxy anyway (with
//! `Fly-Forwarded-Port` or `Fly-Client-IP`) is refused. It says counts,
//! never an account, a name or anything typed.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
    future::Future,
    net::SocketAddr,
    sync::Arc,
};

use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use tracing::{info, warn};

/// The headers Fly's proxy adds to what it forwards from outside.
const PROXIED: [&str; 2] = ["fly-forwarded-port", "fly-client-ip"];

/// Where to serve them: both binaries flatten this into their flags.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Listen {
    /// Serve Prometheus metrics at `/metrics` on this address, on the
    /// private network only (Fly's `[metrics]` scrapes it). Off unless set.
    #[arg(long = "metrics-listen", env = "ARUGULA_METRICS_LISTEN")]
    pub at: Option<SocketAddr>,
}

/// Prometheus's text exposition format.
#[derive(Default)]
pub struct Text {
    out: String,
    described: HashSet<String>,
}

impl Text {
    pub fn new() -> Self {
        Self::default()
    }

    /// A value that goes up and down.
    pub fn gauge(&mut self, name: &str, help: &str, labels: &[(&str, &str)], v: f64) -> &mut Self {
        self.sample(name, "gauge", help, labels, v)
    }

    /// A count that only goes up (until the process restarts: Prometheus's
    /// `rate` and `increase` allow for that).
    pub fn counter(&mut self, name: &str, help: &str, labels: &[(&str, &str)], v: f64) -> &mut Self {
        self.sample(name, "counter", help, labels, v)
    }

    fn sample(&mut self, name: &str, kind: &str, help: &str, labels: &[(&str, &str)], v: f64) -> &mut Self {
        if self.described.insert(name.to_owned()) {
            let _ = writeln!(self.out, "# HELP {name} {}", help.replace('\\', "\\\\").replace('\n', " "));
            let _ = writeln!(self.out, "# TYPE {name} {kind}");
        }
        self.out.push_str(name);
        if !labels.is_empty() {
            self.out.push('{');
            for (i, (k, val)) in labels.iter().enumerate() {
                if i > 0 {
                    self.out.push(',');
                }
                let val = val.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                let _ = write!(self.out, "{k}=\"{val}\"");
            }
            self.out.push('}');
        }
        let _ = writeln!(self.out, " {v}");
        self
    }

    /// Every product event counted since start, by name
    /// ([`crate::analytics::Analytics::counts`]).
    pub fn events(&mut self, counts: &BTreeMap<String, u64>) -> &mut Self {
        for (event, n) in counts {
            self.counter(
                "arugula_events_total",
                "Product events since the process started, by name (what PostHog gets, counted here too)",
                &[("event", event)],
                *n as f64,
            );
        }
        self
    }

    pub fn finish(self) -> String {
        self.out
    }
}

/// Serve `render`'s text at `/metrics` on `at` until the process stops.
pub async fn serve<F, Fut>(at: SocketAddr, render: F) -> anyhow::Result<()>
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = String> + Send + 'static,
{
    let l = tokio::net::TcpListener::bind(at).await.map_err(|e| anyhow::anyhow!("metrics on {at}: {e}"))?;
    info!(addr = %l.local_addr()?, "Prometheus metrics at /metrics, for the private network");
    let router = router(render);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(l, router).await {
            warn!(error = %e, "the metrics listener stopped");
        }
    });
    Ok(())
}

/// The metrics router: `/metrics`, and nothing that came through the
/// public proxy.
pub fn router<F, Fut>(render: F) -> Router
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = String> + Send + 'static,
{
    let render = Arc::new(render);
    Router::new()
        .route(
            "/metrics",
            get(move || {
                let render = render.clone();
                async move {
                    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], render().await)
                        .into_response()
                }
            }),
        )
        .layer(axum::middleware::from_fn(not_proxied))
}

async fn not_proxied(req: Request, next: Next) -> Response {
    if PROXIED.iter().any(|h| req.headers().contains_key(*h)) {
        warn!(path = req.uri().path(), "refused a metrics request that came through the public proxy");
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_text_format() {
        let mut t = Text::new();
        t.gauge("a_sockets", "Open sockets", &[("kind", "daemon")], 3.0)
            .gauge("a_sockets", "Open sockets", &[("kind", "client")], 1.0)
            .counter("a_bytes_total", "Bytes", &[], 12345.0);
        let mut counts = BTreeMap::new();
        counts.insert("signed up".to_owned(), 2);
        counts.insert("say \"hi\"".to_owned(), 1);
        t.events(&counts);
        let s = t.finish();
        assert_eq!(
            s,
            "# HELP a_sockets Open sockets\n# TYPE a_sockets gauge\n\
             a_sockets{kind=\"daemon\"} 3\na_sockets{kind=\"client\"} 1\n\
             # HELP a_bytes_total Bytes\n# TYPE a_bytes_total counter\na_bytes_total 12345\n\
             # HELP arugula_events_total Product events since the process started, by name (what PostHog gets, counted here too)\n\
             # TYPE arugula_events_total counter\n\
             arugula_events_total{event=\"say \\\"hi\\\"\"} 1\narugula_events_total{event=\"signed up\"} 2\n"
        );
    }

    #[tokio::test]
    async fn serves_metrics_but_not_through_the_proxy() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, router(|| async { "x 1\n".to_owned() })).await });
        let http = reqwest::Client::new();
        let ok = http.get(format!("http://{at}/metrics")).send().await.unwrap();
        assert_eq!(ok.status(), 200);
        assert!(ok.headers()["content-type"].to_str().unwrap().starts_with("text/plain; version=0.0.4"));
        assert_eq!(ok.text().await.unwrap(), "x 1\n");
        let proxied = http.get(format!("http://{at}/metrics")).header("fly-client-ip", "1.2.3.4").send().await.unwrap();
        assert_eq!(proxied.status(), 404);
    }
}
