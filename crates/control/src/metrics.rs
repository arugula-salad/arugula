//! The hub's Prometheus metrics (part of Arugula #532), served by
//! `arugula_control_common::metrics` on `--metrics-listen` (the private
//! network only) for the launch dashboard at fly-metrics.net:
//!
//! - the relay: sockets by kind, its ceiling, sockets refused for it and
//!   bytes relayed;
//! - the database: accounts, machines joined and machines seen in the last
//!   day (sign-ups are the rise in accounts);
//! - product events by name (`signed up`, `install started`, `crash`, …),
//!   counted whether or not PostHog is set;
//! - whether maintenance is on, the process's resident memory and the
//!   build.
//!
//! Fly's own metrics have the rest: memory, connections and HTTP status
//! codes (the error rate). docs/launch-runbook.md has the dashboard.

use std::sync::Arc;

use arugula_control_common::metrics::Text;
use arugula_e2e::now_ms;

use crate::App;

/// The process's resident memory, from `/proc` (Linux; `None` elsewhere).
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// `/metrics`' text, now.
pub fn render(app: &App) -> String {
    let mut t = Text::new();
    let (sockets, daemons, max, refused) = app.relay.counts();
    t.gauge("arugula_relay_sockets", "Relay sockets open, by kind", &[("kind", "daemon")], daemons as f64)
        .gauge(
            "arugula_relay_sockets",
            "Relay sockets open, by kind",
            &[("kind", "client")],
            sockets.saturating_sub(daemons) as f64,
        )
        .gauge("arugula_relay_max_sockets", "The relay's ceiling (ARUGULA_RELAY_MAX_TOTAL)", &[], max as f64)
        .counter("arugula_relay_refused_total", "Relay sockets refused because the relay was full", &[], refused as f64)
        .counter("arugula_relay_bytes_total", "Bytes relayed, both ways, every socket", &[], app.relay.bytes() as f64);
    match app.db.counts(now_ms()) {
        Ok(c) => {
            t.gauge("arugula_accounts", "Accounts", &[], c.accounts as f64)
                .gauge("arugula_machines", "Machines joined to an account", &[], c.machines as f64)
                .gauge(
                    "arugula_machines_seen",
                    "Machines on the relay within the window",
                    &[("within", "1d")],
                    c.machines_seen_day as f64,
                );
        }
        Err(e) => tracing::warn!(error = %e, "metrics: counting the database"),
    }
    t.events(&app.analytics.counts());
    t.gauge(
        "arugula_maintenance",
        "1 while the maintenance switch is on",
        &[],
        f64::from(u8::from(app.maintenance.retry_after().is_some())),
    );
    if let Some(b) = resident_bytes() {
        t.gauge("arugula_process_resident_bytes", "The hub process's resident memory", &[], b as f64);
    }
    t.gauge("arugula_build_info", "The running build", &[("version", env!("CARGO_PKG_VERSION"))], 1.0);
    t.finish()
}

/// Serve them on `at` (`--metrics-listen`).
pub async fn serve(app: Arc<App>, at: std::net::SocketAddr) -> anyhow::Result<()> {
    arugula_control_common::metrics::serve(at, move || {
        let app = app.clone();
        async move { tokio::task::spawn_blocking(move || render(&app)).await.unwrap_or_default() }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_accounts_and_machines() {
        let db = crate::db::Db::memory();
        db.conn()
            .execute_batch(
                "INSERT INTO accounts (id, root, created) VALUES ('a', NULL, 1), ('b', NULL, 2);
                 INSERT INTO daemons (id, account, name, urls, last_seen)
                     VALUES ('d1', 'a', 'x', '[]', 100000000), ('d2', 'a', 'y', '[]', 1), ('d3', 'b', 'z', '[]', NULL);",
            )
            .unwrap();
        assert_eq!(
            db.counts(100_000_000 + 1000).unwrap(),
            crate::db::counts::Counts { accounts: 2, machines: 3, machines_seen_day: 1 }
        );
    }

    /// The whole text, from a real hub: the relay's counts, the database's
    /// and the product events.
    #[tokio::test]
    async fn renders_the_hub() {
        let app = App::for_tests("http://127.0.0.1:1").start();
        app.db.account_for("github", "gh-1", "someone", "acct1", now_ms()).unwrap();
        app.analytics.signed_in("acct1", "github", true);
        let t = render(&app);
        for line in [
            "arugula_relay_sockets{kind=\"daemon\"} 0",
            "arugula_relay_max_sockets 5000",
            "arugula_relay_bytes_total 0",
            "arugula_accounts 1",
            "arugula_machines 0",
            "arugula_events_total{event=\"signed up\"} 1",
            "arugula_maintenance 0",
        ] {
            assert!(t.lines().any(|l| l == line), "no {line:?} in\n{t}");
        }
        assert!(t.contains("# TYPE arugula_relay_bytes_total counter"));
    }
}
