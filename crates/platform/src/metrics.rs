//! The platform's Prometheus metrics (part of Arugula #532), served by
//! `arugula_control_common::metrics` on `--metrics-listen` (the private
//! network only) for the launch dashboard at fly-metrics.net:
//!
//! - hosted workspaces by state and purpose, and the boxes running now
//!   (an open `hosted_usage` interval);
//! - what this month's boxes cost on E2B so far, as `arugula-platform
//!   usage` reckons it (`hosted::usage::PRICES`);
//! - product events by name, counted whether or not PostHog is set.
//!
//! Hosted control runs two platform machines on one database: both say the
//! same database counts, so the dashboard takes their `max`, and sums only
//! the events.

use arugula_control_common::metrics::Text;
use arugula_e2e::now_ms;

use crate::{
    Platform,
    db::{
        Db,
        sql::{Sql, params},
    },
};

/// What the database says.
#[derive(Debug, Default, PartialEq)]
pub struct Counts {
    /// Workspaces not deleted, by (state, purpose).
    pub workspaces: Vec<(String, String, u64)>,
    /// Boxes running now: open usage intervals.
    pub running: u64,
    /// What this UTC month's box time cost so far, in dollars.
    pub month_cost: f64,
}

pub fn counts(db: &Db, now: u64) -> anyhow::Result<Counts> {
    let c = db.c();
    let workspaces = c.query_map(
        "SELECT state, COALESCE(purpose, 'workspace'), count(*) FROM hosted_workspaces
         WHERE deleted_at IS NULL GROUP BY state, COALESCE(purpose, 'workspace') ORDER BY 1, 2",
        params![],
        |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?, r.get::<i64>(2)? as u64)),
    )?;
    let running =
        c.query_row("SELECT count(*) FROM hosted_usage WHERE ended_at IS NULL", params![], |r| r.get::<i64>(0))? as u64;
    let from = crate::hosted::broker::prices::month_start(now);
    let month = crate::hosted::usage::report(&crate::db::hosted_usage::intervals(&c, from, now)?, vec![], from, now);
    Ok(Counts { workspaces, running, month_cost: month.all.cost() })
}

/// `/metrics`' text, now.
pub fn render(p: &Platform) -> String {
    let mut t = Text::new();
    match counts(&p.db, now_ms()) {
        Ok(c) => {
            for (state, purpose, n) in &c.workspaces {
                t.gauge(
                    "arugula_hosted_workspaces",
                    "Hosted workspaces not deleted, by state and purpose",
                    &[("state", state), ("purpose", purpose)],
                    *n as f64,
                );
            }
            t.gauge("arugula_hosted_boxes_running", "Hosted boxes running now (billed by E2B)", &[], c.running as f64)
                .gauge(
                    "arugula_hosted_cost_usd",
                    "What hosted boxes cost on E2B so far in the period, at hosted::usage::PRICES",
                    &[("period", "month")],
                    c.month_cost,
                );
        }
        Err(e) => tracing::warn!(error = %e, "metrics: counting the database"),
    }
    t.events(&p.analytics.counts());
    t.gauge(
        "arugula_platform_paused",
        "1 while this machine is paused (ARUGULA_PLATFORM_PAUSED)",
        &[],
        f64::from(u8::from(p.paused.load(std::sync::atomic::Ordering::Relaxed))),
    );
    t.gauge("arugula_build_info", "The running build", &[("version", env!("CARGO_PKG_VERSION"))], 1.0);
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_workspaces_running_boxes_and_the_month() {
        let db = Db::test();
        let day = 86_400_000u64;
        let now = 1_793_534_400_000; // 2026-11-01T12:00Z
        let start = crate::hosted::broker::prices::month_start(now);
        db.c()
            .execute(
                "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, created_by, created_at, updated_at)
                 VALUES ('hw_a', 'prj_a', 'a', '{}', 'h', 'ready', 'u', 1, 1),
                        ('hw_b', 'prj_a', 'b', '{}', 'h', 'paused', 'u', 1, 1),
                        ('hw_c', 'prj_a', 'c', '{}', 'h', 'ready', 'u', 1, 1)",
                params![],
            )
            .unwrap();
        // hw_a has run 2 hours this month and still runs; hw_b ran an hour
        // last month, which this month doesn't count.
        db.c()
            .execute(
                "INSERT INTO hosted_usage (workspace_id, project_id, sandbox_id, cpus, memory_mb, started_at, ended_at)
                 VALUES ('hw_a', 'prj_a', 's1', 2, 2048, ?1, NULL),
                        ('hw_b', 'prj_a', 's2', 2, 2048, ?2, ?3)",
                params![now - 2 * 3_600_000, start - day, start - day + 3_600_000],
            )
            .unwrap();
        let c = counts(&db, now).unwrap();
        assert_eq!(c.workspaces, [("paused".into(), "workspace".into(), 1), ("ready".into(), "workspace".into(), 2)]);
        assert_eq!(c.running, 1);
        let two_hours =
            crate::hosted::usage::PRICES.cost(crate::hosted::usage::Class { cpus: 2, memory_mb: 2048 }, 2 * 3_600_000);
        assert!((c.month_cost - two_hours).abs() < 1e-9, "{} vs {two_hours}", c.month_cost);
        assert!(c.month_cost > 0.0);
    }
}
