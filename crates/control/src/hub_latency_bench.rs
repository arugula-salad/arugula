//! What a hub call and a database round trip cost a platform request when
//! the hub is in another region and the database is on the network
//! (platform step 2, #1150): the platform out of process behind the hub's
//! gateway, as in `chat_bench`, with every `HubPort` call over `HttpHub`
//! held back by [`DELAY`] first, about one ewr↔iad round trip. Not a CI
//! check (timings flake): run it by hand, on this commit and on the one
//! before a change to how the platform asks the hub or its database, and
//! compare.
//!
//! Three columns per route: **warm** (requests back to back), **every
//! 30 s** (the cache's clock moved on 30 s before each request, with the
//! change feed read throughout, as on a quiet control), and **cache
//! dropped** (everything asked again: what every 30 s cost before the
//! cache kept what the feed covers). Each also says what the last request's
//! `Server-Timing` counted (`arugula_platform::timing`): statements (each a
//! round trip on Postgres) and hub calls.
//!
//! The platform's database is SQLite in memory, or, with
//! `ARUGULA_TEST_POSTGRES_URL`, a schema of its own on that Postgres,
//! reached through a proxy here that holds every read back
//! `ARUGULA_BENCH_DB_RTT_US / 2` (1000 µs a round trip unless said), about
//! a round trip to Fly Managed Postgres in the same region.
//!
//! ```sh
//! cargo test -p arugula-control --release -- --ignored --nocapture hub_latency_bench
//! ARUGULA_TEST_POSTGRES_URL=postgres://postgres:platform@127.0.0.1:5432/postgres \
//!   cargo test -p arugula-control --release -- --ignored --nocapture hub_latency_bench
//! ```

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_common::Principal;
use arugula_e2e::{now_ms, team::TeamRole};
use arugula_platform::{
    HubPort,
    db::chat::Room,
    hub::{Account, ChangePage, Daemon, Flags, HubResult, Meter, PushOptions, Pushed, Resolved, Team},
};
use futures_util::future::BoxFuture;

use crate::{
    db,
    routing_wire::{daemon, member, person, roster, session},
};

/// Added to every hub call: about what ewr↔iad adds over 6PN.
const DELAY: Duration = Duration::from_millis(7);
const ROUNDS: usize = 200;
const TEAM: &str = "0123456789abcdef";
const MACHINES: usize = 5;
const CHANNELS: usize = 10;
const THREADS_PER_MACHINE: usize = 6;

/// A TCP proxy to `to` that holds every read back `one_way` before passing
/// it on, both ways: a round trip through it costs about twice that more.
/// Threads, not tasks: a thread's sleep keeps to tens of µs, Tokio's timer
/// to a millisecond.
fn slow_proxy(to: std::net::SocketAddr, one_way: Duration) -> std::net::SocketAddr {
    use std::{
        io::{Read as _, Write as _},
        net::{Shutdown, TcpListener, TcpStream},
    };
    fn pipe(mut from: TcpStream, mut to: TcpStream, one_way: Duration) {
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            while let Ok(n @ 1..) = from.read(&mut buf) {
                std::thread::sleep(one_way);
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            let _ = to.shutdown(Shutdown::Write);
        });
    }
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let at = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let Ok(s) = TcpStream::connect(to) else { continue };
            let _ = (c.set_nodelay(true), s.set_nodelay(true));
            pipe(c.try_clone().unwrap(), s.try_clone().unwrap(), one_way);
            pipe(s, c, one_way);
        }
    });
    at
}

/// The platform's database for the bench, and what it is.
fn database() -> (arugula_platform::db::Db, String) {
    let Some(url) = std::env::var("ARUGULA_TEST_POSTGRES_URL").ok().filter(|u| !u.is_empty()) else {
        return (arugula_platform::db::Db::memory(), "SQLite in memory".into());
    };
    let rtt: u64 = std::env::var("ARUGULA_BENCH_DB_RTT_US").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let mut u = url::Url::parse(&url).unwrap();
    let to = std::net::ToSocketAddrs::to_socket_addrs(&(u.host_str().unwrap(), u.port().unwrap_or(5432)))
        .unwrap()
        .next()
        .unwrap();
    let via = slow_proxy(to, Duration::from_micros(rtt / 2));
    u.set_host(Some(&via.ip().to_string())).unwrap();
    u.set_port(Some(via.port())).unwrap();
    (arugula_platform::db::Db::test_on(u.as_str()), format!("Postgres, {rtt} µs added to every round trip"))
}

/// One `Server-Timing` entry's number (`arugula_platform::timing`).
fn entry(header: &str, name: &str) -> String {
    header
        .split(',')
        .map(str::trim)
        .find_map(|e| {
            let mut parts = e.split(';');
            (parts.next()? == name).then(|| parts.find_map(|p| p.split_once('=').map(|(_, v)| v.to_owned())))?
        })
        .unwrap_or_else(|| "-".into())
}

/// A hub port that waits [`DELAY`] before each call.
struct Far(Arc<dyn HubPort>);

macro_rules! far {
    ($call:expr) => {
        Box::pin(async move {
            tokio::time::sleep(DELAY).await;
            $call.await
        })
    };
}

impl HubPort for Far {
    fn resolve<'a>(&'a self, p: &'a Principal) -> BoxFuture<'a, HubResult<Option<Resolved>>> {
        far!(self.0.resolve(p))
    }
    fn accounts<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        far!(self.0.accounts(ids))
    }
    fn accounts_called<'a>(&'a self, word: &'a str) -> BoxFuture<'a, HubResult<Vec<Account>>> {
        far!(self.0.accounts_called(word))
    }
    fn teams_of<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        far!(self.0.teams_of(account))
    }
    fn teams<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Team>>> {
        far!(self.0.teams(ids))
    }
    fn daemons<'a>(&'a self, ids: &'a [String]) -> BoxFuture<'a, HubResult<Vec<Daemon>>> {
        far!(self.0.daemons(ids))
    }
    fn machines_for<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Vec<String>>> {
        far!(self.0.machines_for(account))
    }
    fn may_reach<'a>(&'a self, account: &'a str, machines: &'a [String]) -> BoxFuture<'a, HubResult<Vec<String>>> {
        far!(self.0.may_reach(account, machines))
    }
    fn push<'a>(&'a self, account: &'a str, message: &'a serde_json::Value) -> BoxFuture<'a, HubResult<Pushed>> {
        far!(self.0.push(account, message))
    }
    fn push_with<'a>(
        &'a self,
        account: &'a str,
        message: &'a serde_json::Value,
        options: PushOptions,
    ) -> BoxFuture<'a, HubResult<Pushed>> {
        far!(self.0.push_with(account, message, options))
    }
    fn labs_flags<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, HubResult<BTreeMap<String, Flags>>> {
        far!(self.0.labs_flags(owners))
    }
    fn meters<'a>(&'a self, accounts: &'a [String], since: u64, until: u64) -> BoxFuture<'a, HubResult<Vec<Meter>>> {
        far!(self.0.meters(accounts, since, until))
    }
    fn metered_accounts(&self) -> BoxFuture<'_, HubResult<Vec<String>>> {
        far!(self.0.metered_accounts())
    }
    fn changes(&self, after: u64, limit: u32) -> BoxFuture<'_, HubResult<ChangePage>> {
        far!(self.0.changes(after, limit))
    }
    fn personal_project<'a>(&'a self, account: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        far!(self.0.personal_project(account))
    }
    fn can_drive_cloud<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, HubResult<bool>> {
        far!(self.0.can_drive_cloud(daemon, account))
    }
    fn team_project<'a>(&'a self, team: &'a str) -> BoxFuture<'a, HubResult<Option<String>>> {
        far!(self.0.team_project(team))
    }
}

#[tokio::test]
#[ignore = "a measurement, by hand: see the top of the file"]
async fn hub_latency_bench() {
    let (db, on) = database();
    let s = crate::gateway_wire::split_on(db, |hub| Arc::new(Far(hub))).await;
    // a0 owns team acme (five mates) and its five machines, each with six
    // pane threads; the team has ten channels; chat is on.
    let keys: Vec<_> = (0..5).map(|i| person(&s.hub, &format!("mate{i}"), &format!("a{i}"))).collect();
    let members = (0..5)
        .map(|i| member(&format!("a{i}"), &keys[i], if i == 0 { TeamRole::Owner } else { TeamRole::Editor }))
        .collect();
    let r = roster(TEAM, 1, members, &keys[0]);
    let t = db::Team {
        id: TEAM.into(),
        name: "acme".into(),
        founder: "a0".into(),
        founder_root: keys[0].id(),
        locked: false,
    };
    s.hub.db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), now_ms()).unwrap();
    s.hub.db.set_labs_flag(&format!("team:{TEAM}"), "chat", Some(true)).unwrap();
    let room = |id: String, scope: String, kind: &str, machine: Option<String>, target: Option<String>| Room {
        name: id.clone(),
        id,
        scope,
        kind: kind.into(),
        machine,
        target,
        created_by: "a0".into(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    for m in 0..MACHINES {
        let id = daemon(&s.hub, "a0", &format!("box-{m}")).id();
        s.hub.db.set_daemon_team(&id, TEAM).unwrap();
        for p in 0..THREADS_PER_MACHINE {
            let r = room(format!("th_{m}_{p}"), format!("machine:{id}"), "pane", Some(id.clone()), Some(p.to_string()));
            s.platform.db.add_chat_room(&r, &[], None).unwrap();
        }
    }
    for i in 0..CHANNELS {
        s.platform
            .db
            .add_chat_room(&room(format!("ch_{i}"), format!("team:{TEAM}"), "channel", None, None), &[], None)
            .unwrap();
    }
    // The platform's change-feed follower, as `arugula-platform` runs it.
    arugula_platform::chat::start(&s.platform);
    arugula_platform::cache::catch_up(&s.platform).await.unwrap();
    let cookie = session(&s.hub, "a0");
    let http = reqwest::Client::new();
    let get = |path: &'static str| {
        let (http, cookie, base) = (&http, &cookie, &s.base);
        async move {
            let r = http.get(format!("{base}{path}")).header("cookie", cookie).send().await.unwrap();
            assert_eq!(r.status(), 200, "{path}");
            let timing = r.headers().get("server-timing").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
            r.bytes().await.unwrap();
            timing
        }
    };
    let mut lines = vec![format!(
        "hub_latency_bench: {} ms added to every hub call; the platform on {on}; {ROUNDS} rounds",
        DELAY.as_millis()
    )];
    for path in ["/api/me", "/api/grants", "/api/chat", "/api/billing", "/api/workspaces"] {
        for _ in 0..10 {
            get(path).await;
        }
        let mut said = format!("{path:24}");
        for (column, how) in [("warm", 0), ("every 30 s", 1), ("cache dropped", 2)] {
            let mut took = Vec::with_capacity(ROUNDS);
            let mut timing = String::new();
            for _ in 0..ROUNDS {
                match how {
                    1 => {
                        // 30 s on, and the feed read just now, as the
                        // follower does every second.
                        s.platform.cache.advance(Duration::from_secs(30));
                        arugula_platform::cache::catch_up(&s.platform).await.unwrap();
                    }
                    2 => s.platform.cache.drop_all(),
                    _ => {}
                }
                let t = Instant::now();
                timing = get(path).await;
                took.push(t.elapsed());
            }
            took.sort();
            let at = |q: f64| took[((took.len() as f64 * q) as usize).min(took.len() - 1)].as_secs_f64() * 1e3;
            said += &format!(
                "  | {column}: p50 {:6.2} p95 {:6.2} ms, {} stmts, {} hub",
                at(0.5),
                at(0.95),
                entry(&timing, "dbq"),
                entry(&timing, "hubn")
            );
        }
        lines.push(said);
    }
    println!("{}", lines.join("\n"));
}
