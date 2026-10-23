//! Rate limits, counted in memory (a restart forgets; that's fine for
//! abuse limits): per client address, per account, per daemon, per install
//! or for everyone. Each process keeps its own [`Limits`] and its own
//! limits (`(name, how many)`), and a limit is counted by one process only,
//! so the hub and the platform never need each other's counts.
//!
//! The client address is the TCP peer, or behind a proxy that says so
//! (`--trust-proxy-header Fly-Client-IP`, say) that header. An IPv6 client
//! counts by its /64: one host usually has the whole prefix to pick from.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::http::{HeaderMap, StatusCode};

use crate::{ApiError, err};

pub struct Limits {
    /// Per (what, who): when each recent use happened.
    seen: Mutex<HashMap<(&'static str, String), Vec<Instant>>>,
    pub proxy_header: Option<String>,
}

const WINDOW: Duration = Duration::from_secs(3600);
const MINUTE: Duration = Duration::from_secs(60);

/// What a client address counts as: itself, or for IPv6 its /64.
pub fn bucket(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
    }
}

impl Limits {
    pub fn new(proxy_header: Option<String>) -> Self {
        Self { seen: Mutex::new(HashMap::new()), proxy_header }
    }

    pub fn client_ip(&self, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
        self.proxy_header
            .as_deref()
            .and_then(|h| headers.get(h))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(peer.ip())
    }

    /// Count one use from a client address; refuse once over the limit.
    pub fn check(&self, what: (&'static str, usize), ip: IpAddr) -> Result<(), ApiError> {
        self.count(what, format!("ip:{}", bucket(ip)), "too many of those from here; try again later")
    }

    /// Count one use by an account; refuse once over the limit.
    pub fn check_account(&self, what: (&'static str, usize), account: &str) -> Result<(), ApiError> {
        self.count(what, format!("account:{account}"), "too many of those lately; try again later")
    }

    /// Take back the last use [`Limits::check_account`] counted for
    /// `account`: for what turned out not to happen (M218: an ask that was
    /// refused).
    pub fn uncount_account(&self, what: (&'static str, usize), account: &str) {
        if let Some(v) = self.seen.lock().unwrap().get_mut(&(what.0, format!("account:{account}"))) {
            v.pop();
        }
    }

    /// Count one use by a daemon; refuse once over the limit.
    pub fn check_daemon(&self, what: (&'static str, usize), daemon: &str) -> Result<(), ApiError> {
        self.count(what, format!("daemon:{daemon}"), "too many of those from this machine lately; try again later")
    }

    /// Count one use by an issuer client (M229: a product's server, by its
    /// client id); refuse once over the limit.
    pub fn check_client(&self, what: (&'static str, usize), client: &str) -> Result<(), ApiError> {
        self.count(what, format!("client:{client}"), "too many of those from this client lately; try again later")
    }

    /// Count one use by an install (its telemetry id); refuse once over the limit.
    pub fn check_install(&self, what: (&'static str, usize), install: &str) -> Result<(), ApiError> {
        self.count(what, format!("install:{install}"), "too many reports from this install lately; try again later")
    }

    /// Count one use of something with one limit for everyone.
    pub fn check_all(&self, what: (&'static str, usize)) -> Result<(), ApiError> {
        self.count(what, String::new(), "too many of those lately; try again later")
    }

    /// Count one use by an account, `what.1` a minute; refuse once over.
    pub fn check_account_per_minute(&self, what: (&'static str, usize), account: &str) -> Result<(), ApiError> {
        self.count_in(what, format!("account:{account}"), "too many of those lately; try again in a minute", MINUTE)
    }

    /// Count one use of something with one limit for everyone, `what.1` a
    /// minute.
    pub fn check_all_per_minute(&self, what: (&'static str, usize)) -> Result<(), ApiError> {
        self.count_in(what, String::new(), "too many of those lately; try again in a minute", MINUTE)
    }

    fn count(&self, what: (&'static str, usize), who: String, msg: &str) -> Result<(), ApiError> {
        self.count_in(what, who, msg, WINDOW)
    }

    fn count_in(
        &self,
        (what, most): (&'static str, usize),
        who: String,
        msg: &str,
        window: Duration,
    ) -> Result<(), ApiError> {
        let now = Instant::now();
        let mut seen = self.seen.lock().unwrap();
        if seen.len() > 100_000 {
            seen.retain(|_, v| v.last().is_some_and(|t| now.duration_since(*t) < WINDOW));
        }
        let v = seen.entry((what, who)).or_default();
        v.retain(|t| now.duration_since(*t) < window);
        if v.len() >= most {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, msg));
        }
        v.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_per_ip() {
        let l = Limits::new(Some("fly-client-ip".into()));
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..3 {
            l.check(("t", 3), a).unwrap();
        }
        assert!(l.check(("t", 3), a).is_err());
        assert!(l.check(("t", 3), b).is_ok());
        let mut h = HeaderMap::new();
        h.insert("fly-client-ip", "203.0.113.9".parse().unwrap());
        assert_eq!(l.client_ip("127.0.0.1:1".parse().unwrap(), &h), "203.0.113.9".parse::<IpAddr>().unwrap());
        let plain = Limits::new(None);
        assert_eq!(plain.client_ip("127.0.0.1:1".parse().unwrap(), &h), "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn ipv6_counts_by_its_64() {
        let l = Limits::new(None);
        for i in 0..3 {
            let ip: IpAddr = format!("2001:db8:1:2::{i:x}").parse().unwrap();
            l.check(("t", 3), ip).unwrap();
        }
        let next: IpAddr = "2001:db8:1:2:ffff:1:2:3".parse().unwrap();
        assert!(l.check(("t", 3), next).is_err(), "another address in the same /64");
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert!(l.check(("t", 3), other).is_ok(), "the next /64 is someone else");
        let mapped: IpAddr = "::ffff:10.0.0.9".parse().unwrap();
        assert_eq!(bucket(mapped), "10.0.0.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn accounts_and_daemons_count_apart() {
        let l = Limits::new(None);
        l.check_account(("t", 1), "a").unwrap();
        assert!(l.check_account(("t", 1), "a").is_err());
        assert!(l.check_account(("t", 1), "b").is_ok());
        assert!(l.check_daemon(("t", 1), "a").is_ok(), "a daemon with the same id is another bucket");
        l.uncount_account(("t", 1), "a");
        assert!(l.check_account(("t", 1), "a").is_ok(), "a use taken back is free again");
        l.check_all(("u", 1)).unwrap();
        assert!(l.check_all(("u", 1)).is_err());
    }
}
