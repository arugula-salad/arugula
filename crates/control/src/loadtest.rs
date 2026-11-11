//! A staging control for the load test (#530), in a `loadtest` build
//! only: `ARUGULA_LOADTEST_EXEMPT_IPS` names the generators' addresses
//! (comma-separated), which then skip the per-address limits (10 accounts
//! and 30 joins an hour), so one host can sign up thousands of accounts and
//! enrol their machines through the real routes. Nothing else changes.
//!
//! It can't reach production: a release builds without the feature (the
//! code isn't in the binary), and a `loadtest` build refuses to start
//! unless every URL it answers at is a loopback one or a host whose first
//! label ends in `-loadtest` (`arugula-control-loadtest.fly.dev`).
//! docs/control-ops.md, "Load testing", has the staging copy.

use std::net::IpAddr;

use anyhow::{Context as _, bail};

use crate::Site;

/// The variable naming the generators' addresses.
pub const EXEMPT: &str = "ARUGULA_LOADTEST_EXEMPT_IPS";

/// Whether `site` is one a load-test build may answer at.
fn staging(site: &Site) -> bool {
    let host = site.host();
    let loopback = host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    loopback || host.split('.').next().is_some_and(|l| l.ends_with("-loadtest"))
}

/// Check the sites, and read the generators' addresses from `value` (the
/// variable's, if set).
pub fn exempt(sites: &[Site], value: Option<&str>) -> anyhow::Result<Vec<IpAddr>> {
    if let Some(s) = sites.iter().find(|s| !staging(s)) {
        bail!(
            "this is a load-test build (#530): it runs only at a loopback URL or a host whose first label ends in \
             `-loadtest`, not {}",
            s.url
        );
    }
    let ips = value
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<IpAddr>().with_context(|| format!("{EXEMPT}: {s} isn't an IP address")))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(ips)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sites(urls: &[&str]) -> Vec<Site> {
        urls.iter().map(|u| Site::new(u).unwrap()).collect()
    }

    #[test]
    fn only_staging_urls() {
        let ok = sites(&["https://arugula-control-loadtest.fly.dev", "http://127.0.0.1:7690"]);
        assert_eq!(exempt(&ok, Some("203.0.113.7, ::1")).unwrap().len(), 2);
        assert!(exempt(&sites(&["http://localhost:7690"]), None).unwrap().is_empty());
        for prod in ["https://control.arugula.io", "https://illogical-control.fly.dev", "https://loadtest.arugula.io"] {
            let e = exempt(&sites(&["https://arugula-control-loadtest.fly.dev", prod]), None).unwrap_err();
            assert!(e.to_string().contains(prod), "{e}");
        }
        assert!(exempt(&ok, Some("not-an-ip")).is_err());
    }
}
